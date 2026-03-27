use bincode::Options;
use serde::{Deserialize, Serialize};

/// Maximum relay message size (4 MiB — smaller than peer protocol since relay
/// messages are envelopes, not raw blobs).
pub const MAX_RELAY_MESSAGE_SIZE: u64 = 4 * 1024 * 1024;

/// Messages exchanged between clients and the relay.
///
/// The relay protocol is intentionally separate from the peer wire protocol.
/// This keeps the relay binary free of crypto dependencies and ensures it
/// cannot interpret message contents even in theory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RelayMessage {
    /// Client → Relay: authenticate and subscribe to routing tags.
    /// The `peer_id_bytes` is the client's Ed25519 public key (32 bytes) —
    /// the relay stores it opaquely for mailbox addressing but cannot verify it.
    Hello {
        peer_id_bytes: [u8; 32],
        routing_tags: Vec<[u8; 32]>,
        version: u32,
    },

    /// Client → Relay: subscribe to additional routing tags.
    /// Includes an Ed25519 signature over the concatenated routing tags
    /// to prove the subscriber controls the peer_id from Hello.
    Subscribe {
        routing_tags: Vec<[u8; 32]>,
        /// Ed25519 signature over the concatenated routing tag bytes.
        signature: Vec<u8>,
    },

    /// Client → Relay: unsubscribe from routing tags.
    Unsubscribe {
        routing_tags: Vec<[u8; 32]>,
    },

    /// Bidirectional: forward an opaque payload to all subscribers of a routing tag.
    /// Client → Relay: "send this to everyone on this tag."
    /// Relay → Client: "someone sent this on a tag you subscribe to."
    Forward {
        routing_tag: [u8; 32],
        payload: Vec<u8>,
    },

    /// Client → Relay: request queued messages (mailbox drain).
    DrainMailbox,

    /// Relay → Client: mailbox contents.
    MailboxBatch {
        messages: Vec<ForwardEnvelope>,
        remaining: u64,
    },

    /// Relay → Client: status/error feedback.
    Status {
        code: StatusCode,
        message: String,
    },

    /// Client → Relay: register a username for directory lookup.
    Register {
        username: String,
        public_key: [u8; 32],
        /// Ed25519 signature over b"veil-register-v1:" || username_lowercase.
        signature: Vec<u8>,
    },

    /// Client → Relay: look up a username in the directory.
    Lookup {
        username: String,
    },

    /// Relay → Client: result of a Register request.
    RegisterResult {
        success: bool,
        message: String,
    },

    /// Relay → Client: result of a Lookup request.
    LookupResult {
        username: String,
        public_key: Option<[u8; 32]>,
    },

    Ping(u64),
    Pong(u64),

    // ── Voice / Video (v3) ─────────────────────────────────────────────

    /// Client → Relay: join a voice channel.
    /// The relay creates (or joins) a voice room and returns an SDP offer.
    VoiceJoin {
        /// blake3("veil-voice-room-id", group_id || channel_name)
        room_id: [u8; 32],
        /// The group this room belongs to.
        group_id: [u8; 32],
    },

    /// Relay → Client: SDP offer for the WebRTC session.
    VoiceOffer {
        room_id: [u8; 32],
        participant_id: u64,
        sdp: String,
        /// UDP address of the relay's voice endpoint (host:port).
        voice_endpoint: String,
        /// Current participants already in the room (peer_id_bytes each).
        participants: Vec<[u8; 32]>,
    },

    /// Client → Relay: SDP answer completing the WebRTC handshake.
    VoiceAnswer {
        room_id: [u8; 32],
        participant_id: u64,
        sdp: String,
    },

    /// Bidirectional: trickle ICE candidate exchange.
    VoiceIceCandidate {
        room_id: [u8; 32],
        participant_id: u64,
        candidate: String,
    },

    /// Client → Relay: leave a voice channel.
    VoiceLeave {
        room_id: [u8; 32],
    },

    /// Relay → Client: a participant joined the voice room.
    VoiceParticipantJoined {
        room_id: [u8; 32],
        peer_id_bytes: [u8; 32],
    },

    /// Relay → Client: a participant left the voice room.
    VoiceParticipantLeft {
        room_id: [u8; 32],
        peer_id_bytes: [u8; 32],
    },

    /// Relay → Client: speaking indicator derived from RTP audio-level
    /// header extensions (RFC 6464). Not part of encrypted payload.
    VoiceSpeaking {
        room_id: [u8; 32],
        peer_id_bytes: [u8; 32],
        /// Audio level 0–127 from RTP header extension.
        audio_level: u8,
    },
}

/// A queued message in the mailbox.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardEnvelope {
    pub routing_tag: [u8; 32],
    pub payload: Vec<u8>,
    /// Unix timestamp when the relay received this message.
    pub received_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StatusCode {
    Ok,
    TagLimitExceeded,
    MailboxFull,
    RateLimited,
    BadVersion,
}

/// Current relay protocol version.
pub const RELAY_PROTOCOL_VERSION: u32 = 3;

impl RelayMessage {
    fn bincode_options() -> impl bincode::Options {
        bincode::options()
            .with_limit(MAX_RELAY_MESSAGE_SIZE)
            .allow_trailing_bytes()
    }

    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        Self::bincode_options().serialize(self)
    }

    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        Self::bincode_options().deserialize(data)
    }
}
