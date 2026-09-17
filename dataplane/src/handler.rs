use std::sync::Arc;

use dataplane_sdk::core::{
    db::tx::TransactionalContext,
    error::{HandlerError, HandlerResult},
    handler::DataFlowHandler,
    model::{
        data_address::{DataAddress, EndpointProperty},
        data_flow::{DataFlow, DataFlowState},
        messages::DataFlowStatusMessage,
    },
};

use config_graph::{ConfigGraph, FileOffer};

use crate::tokens::TokenStore;

/// Implements the Data Plane Signaling state machine's provider side for
/// a single file offer, per this project's "START + TERMINATE only" MVP
/// scope (see `../ARCHITECTURE.md`): START mints a bearer token
/// unconditionally (contract negotiation already gated the agreement one
/// layer up); the actual ODRL policy check happens per public-endpoint
/// request, not here — see `crate::public::get_file`.
pub struct FileOfferHandler<T> {
    graph: Arc<ConfigGraph>,
    tokens: Arc<TokenStore>,
    public_base_url: String,
    /// Template offer this handler was configured with (see
    /// `Config::file_offer`) — cloned, with `dataset_id` overridden, to
    /// auto-seed an offer for an unrecognized dataset id when `tck_mode`
    /// is set. Unused when `tck_mode` is `false`.
    default_offer: FileOffer,
    /// See `Config::tck_mode`'s doc comment.
    tck_mode: bool,
    _marker: std::marker::PhantomData<T>,
}

impl<T> FileOfferHandler<T> {
    pub fn new(
        graph: Arc<ConfigGraph>,
        tokens: Arc<TokenStore>,
        public_base_url: String,
        default_offer: FileOffer,
        tck_mode: bool,
    ) -> Self {
        Self {
            graph,
            tokens,
            public_base_url,
            default_offer,
            tck_mode,
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<T: TransactionalContext> DataFlowHandler for FileOfferHandler<T>
where
    T::Transaction: Send,
{
    type Transaction = T::Transaction;

    async fn can_handle(&self, _flow: &DataFlow) -> HandlerResult<bool> {
        Ok(true)
    }

    async fn on_start(
        &self,
        _tx: &mut Self::Transaction,
        flow: &DataFlow,
    ) -> HandlerResult<DataFlowStatusMessage> {
        // Fail fast if this dataset isn't one the configuration graph
        // actually holds, rather than minting a token for a file that
        // will 404 on every subsequent GET — unless TCK_MODE is on, in
        // which case an unrecognized dataset id gets a fresh offer
        // auto-seeded on the spot (see `Config::tck_mode`'s doc comment
        // for why the official TCK needs this).
        if let Err(e) = self.graph.file_path(&flow.dataset_id) {
            if !self.tck_mode {
                return Err(HandlerError::NotSupported(format!(
                    "unknown dataset '{}': {e}",
                    flow.dataset_id
                )));
            }
            let offer = FileOffer {
                dataset_id: flow.dataset_id.clone(),
                ..self.default_offer.clone()
            };
            self.graph.seed_offer(&offer).map_err(|e| {
                HandlerError::NotSupported(format!(
                    "TCK_MODE: failed to auto-seed offer for dataset '{}': {e}",
                    flow.dataset_id
                ))
            })?;
            tracing::info!(
                dataset_id = %flow.dataset_id,
                "TCK_MODE: auto-seeded offer for a dataset id not already in the configuration graph"
            );
        }

        let token = self.tokens.issue(&flow.id, &flow.dataset_id);
        let endpoint = format!("{}/public/{}", self.public_base_url, flow.dataset_id);

        let data_address = DataAddress::builder()
            .endpoint(endpoint.clone())
            .endpoint_type("HTTP")
            .endpoint_properties(vec![
                EndpointProperty::builder()
                    .name("endpoint")
                    .value(endpoint)
                    .build(),
                EndpointProperty::builder()
                    .name("authorization")
                    .value(token)
                    .build(),
                EndpointProperty::builder()
                    .name("authType")
                    .value("bearer")
                    .build(),
            ])
            .build();

        Ok(DataFlowStatusMessage::builder()
            .data_flow_id(flow.id.clone())
            .state(DataFlowState::Started)
            .data_address(data_address)
            .build())
    }

    async fn on_prepare(
        &self,
        _tx: &mut Self::Transaction,
        _flow: &DataFlow,
    ) -> HandlerResult<DataFlowStatusMessage> {
        // `prepare` is the DPS *consumer*-role operation; this data plane
        // only ever plays the provider role for a single local file.
        Err(HandlerError::NotSupported(
            "this data plane only implements the provider role (START/TERMINATE)".into(),
        ))
    }

    async fn on_terminate(
        &self,
        _tx: &mut Self::Transaction,
        flow: &DataFlow,
    ) -> HandlerResult<()> {
        self.tokens.revoke_flow(&flow.id);
        Ok(())
    }

    async fn on_started(&self, _tx: &mut Self::Transaction, _flow: &DataFlow) -> HandlerResult<()> {
        // The DataAddress/token were already returned synchronously from
        // `on_start`; nothing further to do when the control plane
        // acknowledges the flow reached STARTED.
        Ok(())
    }

    async fn on_suspend(&self, _tx: &mut Self::Transaction, _flow: &DataFlow) -> HandlerResult<()> {
        // Deferred past MVP scope (see ../ARCHITECTURE.md, "Signaling
        // scope"): treated as unsupported rather than silently no-op'd,
        // so a control plane calling SUSPEND does not believe it
        // succeeded when the token is, in fact, still live.
        Err(HandlerError::NotSupported(
            "SUSPEND is not implemented by this MVP data plane".into(),
        ))
    }
}
