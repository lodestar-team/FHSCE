//! FHSCE gateway — Horizon TAP v2 (GraphTally) payment layer in front of the
//! File Hosting Service data plane.
//!
//! All the payment machinery (receipt validation, RAV aggregation, on-chain
//! collection, persistence, the TAP-gated reverse proxy) lives in `horizon-core`.
//! This binary just loads config and hands off to it: consumers send a signed
//! `TAP-Receipt` header, the gateway verifies + meters it and proxies the
//! (range-aware) request through to the upstream `file-service`.

use horizon_core::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fhsce_gateway=info,horizon_core=info".into()),
        )
        .init();

    let config = Config::load()?;
    tracing::info!(
        upstream = %config.backend.upstream_url,
        data_service = %config.tap.data_service_address,
        "FHSCE gateway starting — file hosting on Horizon"
    );

    horizon_core::run(config).await
}
