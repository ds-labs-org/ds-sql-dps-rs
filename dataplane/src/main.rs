use dataplane::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Config::from_env();

    let app = dataplane::build(config).await?;

    let signaling_addr = app.signaling_listener.local_addr()?;
    let public_addr = app.public_listener.local_addr()?;

    tracing::info!(%signaling_addr, "Data Plane Signaling API listening (POST /api/v1/dataflows/start, .../{{id}}/terminate)");
    tracing::info!(%public_addr, "public data endpoint listening (GET /public/{{dataset_id}})");
    tracing::warn!(
        "this data plane does not register itself with a control plane's DataPlaneSelector API — see ../ARCHITECTURE.md, \"What this MVP does not do\""
    );

    dataplane::serve(app).await
}
