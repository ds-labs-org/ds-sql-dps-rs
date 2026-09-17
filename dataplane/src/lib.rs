//! Library half of the `dataplane` crate: everything `main.rs` needs to
//! assemble and run the MVP data plane, minus the process bootstrap
//! itself (tracing init, env-var parsing, logging the "listening on"
//! lines). Split out so tests can build the whole application — config
//! graph, token store, dataplane-sdk wiring, both axum routers — against
//! ephemeral ports without going through a real `main`.

pub mod config;
pub mod handler;
pub mod public;
pub mod tokens;

use std::sync::Arc;

use axum::routing::get;
use axum::{Extension, Router};
use config_graph::ConfigGraph;
use contreforts_connector::{DATA_OFFER_KIND, FileOfferConnector};
use contreforts_core::{ContrefortsConnector, EntityKind};
use dataplane_sdk::core::db::control_plane::ControlPlaneRepo;
use dataplane_sdk::core::db::control_plane::memory::MemoryControlPlaneRepo;
use dataplane_sdk::core::db::data_flow::memory::MemoryDataFlowRepo;
use dataplane_sdk::core::db::memory::MemoryContext;
use dataplane_sdk::core::db::tx::{Transaction, TransactionalContext};
use dataplane_sdk::core::model::control_plane::ControlPlane;
use dataplane_sdk::core::model::participant::ParticipantContext;
use dataplane_sdk::sdk::DataPlaneSdk;

use crate::config::Config;
use crate::handler::FileOfferHandler;
use crate::public::PublicState;
use crate::tokens::TokenStore;

/// The fully assembled application: both listeners already bound, and
/// both axum routers fully built (state already applied via
/// `.with_state(...)` — plain `axum::Router`, no generic state parameter
/// left), ready to hand to [`serve`].
///
/// Listeners are exposed rather than just addresses so a caller (a test,
/// in particular) can read back the OS-assigned port via
/// `TcpListener::local_addr()` when it was bound with port `0`.
pub struct App {
    pub signaling_listener: tokio::net::TcpListener,
    pub public_listener: tokio::net::TcpListener,
    pub signaling_router: Router,
    pub public_router: Router,
}

/// Builds the whole application from `config`: seeds the configuration
/// graph, proves the Contreforts round trip, wires the dataplane-sdk
/// signaling router and the public file-serving router together, and
/// binds both listeners.
///
/// The public listener is bound *before* the [`FileOfferHandler`] is
/// constructed, specifically so that when `config.public_port == 0` the
/// handler (and therefore every token's `DataAddress.endpoint`) is built
/// from the port the OS actually assigned rather than the literal `0` —
/// this is what lets [`Config::for_test`] bind an ephemeral public port
/// and still hand out working endpoint URLs.
pub async fn build(config: Config) -> anyhow::Result<App> {
    let graph = Arc::new(ConfigGraph::open_in_memory()?);
    graph.seed_offer(&config.file_offer())?;
    tracing::info!(dataset_id = %config.dataset_id, file_path = %config.file_path.display(), "seeded configuration graph");

    // Proves the Contreforts round trip at startup: the same graph this
    // data plane serves over the signaling API is reachable through
    // Contreforts' own connector interface, keyed by this project's own
    // EntityKind (see contreforts-connector's crate docs).
    let connector = FileOfferConnector::new(graph.clone());
    match connector
        .get(EntityKind::new(DATA_OFFER_KIND), &config.dataset_id)
        .await
    {
        Ok(doc) => tracing::info!(document = %doc.fields, "ContrefortsConnector::get round trip"),
        Err(e) => tracing::warn!(error = %e, "ContrefortsConnector::get round trip failed"),
    }

    // Bound before the handler is built (see doc comment above) so a
    // configured port of 0 can still yield a real, dereferenceable
    // public_base_url.
    let public_addr = format!("0.0.0.0:{}", config.public_port);
    let public_listener = tokio::net::TcpListener::bind(&public_addr).await?;

    let public_base_url = if config.public_port == 0 {
        format!("http://localhost:{}", public_listener.local_addr()?.port())
    } else {
        config.public_base_url.clone()
    };

    let tokens = Arc::new(TokenStore::default());
    let handler =
        FileOfferHandler::<MemoryContext>::new(graph.clone(), tokens.clone(), public_base_url);

    let ctx = MemoryContext;
    let flows = MemoryDataFlowRepo::default();
    let control_planes = MemoryControlPlaneRepo::default();

    let control_plane = ControlPlane::builder()
        .id("ds-sql-dps-rs-demo-control-plane")
        .url(format!(
            "http://localhost:{}/callback",
            config.signaling_port
        ))
        .build();
    {
        let mut tx = ctx.begin().await?;
        control_planes.create(&mut tx, &control_plane).await?;
        tx.commit().await?;
    }

    let sdk = DataPlaneSdk::builder(ctx)
        .with_repo(flows)
        .with_control_plane_repo(control_planes)
        .with_handler(handler)
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build DataPlaneSdk: {e}"))?;

    let participant = ParticipantContext::builder().id("ds-sql-dps-rs").build();

    let signaling_router = dataplane_sdk_axum::router::router()
        .layer(Extension(participant))
        .layer(Extension(control_plane))
        .with_state(sdk);

    let public_router = Router::new()
        .route("/public/{dataset_id}", get(public::get_file))
        .with_state(PublicState { graph, tokens });

    let signaling_addr = format!("0.0.0.0:{}", config.signaling_port);
    let signaling_listener = tokio::net::TcpListener::bind(&signaling_addr).await?;

    Ok(App {
        signaling_listener,
        public_listener,
        signaling_router,
        public_router,
    })
}

/// Runs both routers against their already-bound listeners until either
/// exits (an error) or the process receives ctrl-c — mirrors the
/// shutdown behavior that used to live inline in `main`.
pub async fn serve(app: App) -> anyhow::Result<()> {
    tokio::select! {
        result = axum::serve(app.signaling_listener, app.signaling_router) => result?,
        result = axum::serve(app.public_listener, app.public_router) => result?,
        _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
    }

    Ok(())
}
