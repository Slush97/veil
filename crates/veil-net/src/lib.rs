pub mod discovery;
pub mod framing;
pub mod manager;
pub mod peer;
pub mod protocol;
pub mod rate_limit;
pub mod relay_client;

pub use discovery::{Discovery, DiscoveryEvent};
pub use manager::{ConnectionId, PeerEvent, PeerManager};
pub use peer::{PeerConnection, create_endpoint};
pub use protocol::{PresenceKind, WireMessage};
pub use rate_limit::{PeerRateLimiter, RateLimitConfig};
pub use relay_client::{RelayClient, RelayCommand, RelayEvent};

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("connection error: {0}")]
    Connection(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("protocol error: {0}")]
    Protocol(String),
}
