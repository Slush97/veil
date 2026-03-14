use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use tracing_subscriber::EnvFilter;
use veil_relay::{RelayConfig, RelayServer};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let bind_addr: SocketAddr = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| ([0, 0, 0, 0], 4433).into());

    let db_path = std::env::var("VEIL_RELAY_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./mailbox.redb"));

    let max_age_secs: u64 = std::env::var("VEIL_RELAY_MAX_AGE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(86400);

    let config = RelayConfig {
        bind_addr,
        db_path,
        max_age: Duration::from_secs(max_age_secs),
        ..Default::default()
    };

    tracing::info!("starting veil-relay on {bind_addr}");
    tracing::info!("zero-knowledge mode: relay cannot read message contents");
    tracing::info!("mailbox db: {:?}, max age: {max_age_secs}s", config.db_path);

    let server = RelayServer::new(config);

    if let Err(e) = server.run().await {
        tracing::error!("relay error: {e}");
        std::process::exit(1);
    }
}
