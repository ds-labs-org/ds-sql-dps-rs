//! Implements Contreforts' [`ContrefortsConnector`] trait for a single
//! file exposed as a dataspace data offer, so this project's configuration
//! graph (path/metadata/policy) is reachable through Contreforts' own
//! connector interface — the semantic-configuration layer this project
//! was asked to use, wired to a graph the project owns itself (see
//! `../ARCHITECTURE.md`, "Contreforts coupling").
//!
//! Contreforts' own domain is business-system sync (ERP, git forges,
//! groupware), not the Dataspace Protocol — its `EntityKind`s, ontology
//! and `pull()`/`since` semantics were designed for that. This connector
//! reuses only the *interface* (the trait, the declaration mechanism), not
//! any of Contreforts' business vocabulary: it mints its own entity kind
//! ([`DATA_OFFER_KIND`]) and its own namespace in `declaration.ttl`, per
//! the trait's documented extension point (`EntityKind::new`).

use std::sync::Arc;

use chrono::NaiveDateTime;
use config_graph::ConfigGraph;
use contreforts_core::{ConnectorError, ContrefortsConnector, Document, EntityKind};

/// This connector recognises exactly one entity kind: a file exposed as a
/// dataspace data offer. Minted via `EntityKind::new`, per
/// `contreforts-core`'s open-vocabulary design — no change to that crate
/// was needed to add it.
pub const DATA_OFFER_KIND: &str = "ds-sql-dps-rs:data-offer";

/// Adapts a [`ConfigGraph`] to Contreforts' connector interface. MVP scope
/// holds exactly one offer, addressed by a fixed `remote_id`
/// ([`PRIMARY_OFFER_ID`]) — a real product would enumerate every offer
/// the graph holds instead.
pub const PRIMARY_OFFER_ID: &str = "primary";

pub struct FileOfferConnector {
    graph: Arc<ConfigGraph>,
}

impl FileOfferConnector {
    pub fn new(graph: Arc<ConfigGraph>) -> Self {
        Self { graph }
    }

    fn require_data_offer_kind(&self, kind: &EntityKind) -> Result<(), ConnectorError> {
        if kind.as_str() == DATA_OFFER_KIND {
            Ok(())
        } else {
            Err(ConnectorError::UnsupportedKind {
                connector: self.source_name().to_string(),
                kind: kind.as_str().to_string(),
            })
        }
    }
}

#[async_trait::async_trait]
impl ContrefortsConnector for FileOfferConnector {
    fn source_name(&self) -> &str {
        "ds-sql-dps-rs"
    }

    fn declaration_ttl(&self) -> &'static str {
        include_str!("declaration.ttl")
    }

    async fn pull(
        &self,
        kind: EntityKind,
        _since: Option<NaiveDateTime>,
    ) -> Result<Vec<Document>, ConnectorError> {
        self.require_data_offer_kind(&kind)?;
        Ok(vec![self.get(kind, PRIMARY_OFFER_ID).await?])
    }

    async fn get(&self, kind: EntityKind, remote_id: &str) -> Result<Document, ConnectorError> {
        self.require_data_offer_kind(&kind)?;

        let to_api_err = |e: config_graph::ConfigGraphError| ConnectorError::Api {
            message: e.to_string(),
        };
        let file_path = self.graph.file_path(remote_id).map_err(to_api_err)?;
        let media_type = self.graph.media_type(remote_id).map_err(to_api_err)?;
        let policy = self.graph.policy_jsonld(remote_id).map_err(to_api_err)?;

        Ok(Document {
            name: remote_id.to_string(),
            remote_id: remote_id.to_string(),
            kind,
            source: self.source_name().to_string(),
            modified: None,
            fields: serde_json::json!({
                "filePath": file_path.to_string_lossy(),
                "mediaType": media_type,
                "policy": policy,
            }),
        })
    }

    async fn push(&self, _doc: &Document) -> Result<Document, ConnectorError> {
        Err(ConnectorError::Unsupported {
            connector: self.source_name().to_string(),
            operation: "push".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unsupported_kind_errors_naming_kind_and_connector() {
        let graph = Arc::new(ConfigGraph::open_in_memory().expect("open store"));
        let connector = FileOfferConnector::new(graph);

        let err = connector
            .pull(EntityKind::new("customer"), None)
            .await
            .expect_err("a kind this connector does not handle must error");

        match err {
            ConnectorError::UnsupportedKind { connector, kind } => {
                assert_eq!(connector, "ds-sql-dps-rs");
                assert_eq!(kind, "customer");
            }
            other => panic!("expected UnsupportedKind, got {other:?}"),
        }
    }
}
