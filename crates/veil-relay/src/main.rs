use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use tracing_subscriber::EnvFilter;
use veil_relay::voice::VoiceConfig;
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

    let voice_enabled = std::env::var("VEIL_RELAY_VOICE_ENABLED")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);

    let voice_port: u16 = std::env::var("VEIL_RELAY_VOICE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4434);

    let voice_config = if voice_enabled {
        Some(VoiceConfig {
            udp_bind_addr: SocketAddr::from(([0, 0, 0, 0], voice_port)),
            ..Default::default()
        })
    } else {
        None
    };

    let config = RelayConfig {
        bind_addr,
        db_path,
        max_age: Duration::from_secs(max_age_secs),
        voice_config,
        ..Default::default()
    };

    tracing::info!("starting veil-relay on {bind_addr}");
    tracing::info!("zero-knowledge mode: relay cannot read message contents");
    tracing::info!("mailbox db: {:?}, max age: {max_age_secs}s", config.db_path);
    if voice_enabled {
        tracing::info!("voice module enabled on UDP port {voice_port}");
    }

    let server = RelayServer::new(config);

    if let Err(e) = server.run().await {
        tracing::error!("relay error: {e}");
        std::process::exit(1);
    }
}
