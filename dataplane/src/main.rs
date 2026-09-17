mod config;
mod handler;
mod public;
mod tokens;

use std::sync::Arc;

use axum::routing::get;
use axum::{Extension, Router};
use config_graph::ConfigGraph;
use contreforts_connector::{FileOfferConnector, DATA_OFFER_KIND};
use contreforts_core::{ContrefortsConnector, EntityKind};
use dataplane_sdk::core::db::control_plane::memory::MemoryControlPlaneRepo;
use dataplane_sdk::core::db::control_plane::ControlPlaneRepo;
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env();

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

    let tokens = Arc::new(TokenStore::default());
    let handler = FileOfferHandler::<MemoryContext>::new(graph.clone(), tokens.clone(), config.public_base_url.clone());

    let ctx = MemoryContext;
    let flows = MemoryDataFlowRepo::default();
    let control_planes = MemoryControlPlaneRepo::default();

    let control_plane = ControlPlane::builder()
        .id("ds-sql-dps-rs-demo-control-plane")
        .url(format!("http://localhost:{}/callback", config.signaling_port))
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
    let public_addr = format!("0.0.0.0:{}", config.public_port);

    let signaling_listener = tokio::net::TcpListener::bind(&signaling_addr).await?;
    let public_listener = tokio::net::TcpListener::bind(&public_addr).await?;

    tracing::info!(%signaling_addr, "Data Plane Signaling API listening (POST /api/v1/dataflows/start, .../{{id}}/terminate)");
    tracing::info!(%public_addr, "public data endpoint listening (GET /public/{{dataset_id}})");
    tracing::warn!(
        "this data plane does not register itself with a control plane's DataPlaneSelector API — see ../ARCHITECTURE.md, \"What this MVP does not do\""
    );

    tokio::select! {
        result = axum::serve(signaling_listener, signaling_router) => result?,
        result = axum::serve(public_listener, public_router) => result?,
        _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
    }

    Ok(())
}
