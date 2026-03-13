use std::net::SocketAddr;

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

    let config = RelayConfig {
        bind_addr,
        ..Default::default()
    };

    let server = RelayServer::new(config);

    tracing::info!("starting veil-relay on {bind_addr}");
    tracing::info!("zero-knowledge mode: relay cannot read message contents");

    if let Err(e) = server.run().await {
        tracing::error!("relay error: {e}");
        std::process::exit(1);
    }
}
