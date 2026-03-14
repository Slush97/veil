use bincode::Options;
use serde::{Deserialize, Serialize};
use veil_core::{BlobId, GroupId, MessageId, SealedMessage};
use veil_crypto::{DeviceCertificate, PeerId};

/// Maximum size of a single wire message (16 MiB).
pub const MAX_WIRE_MESSAGE_SIZE: u64 = 16 * 1024 * 1024;

/// Wire protocol messages exchanged between peers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WireMessage {
    /// Announce presence and begin challenge-response authentication.
    /// Initiator sends with an empty signature; responder includes a signature
    /// over `(initiator_challenge || responder_peer_id)`.
    Hello {
        peer_id: PeerId,
        version: u32,
        /// Random 32-byte nonce for challenge-response authentication.
        challenge: [u8; 32],
        /// Ed25519 signature proving ownership of peer_id.
        /// Empty on the initiator's first message.
        signature: Vec<u8>,
        /// Optional device certificate proving this device belongs to a master identity.
        /// Backward-compatible: old decoders ignore the extra field via `allow_trailing_bytes()`.
        device_certificate: Option<DeviceCertificate>,
    },

    /// Initiator's response to the responder's challenge.
    /// Contains a signature over `(responder_challenge || initiator_peer_id)`.
    ChallengeResponse {
        signature: Vec<u8>,
    },

    /// Ephemeral X25519 public key for pairwise session key derivation.
    KeyExchange {
        ephemeral_public: [u8; 32],
    },

    /// Push a new encrypted message to peers.
    MessagePush(SealedMessage),

    /// Request messages newer than a given ID for a group.
    MessageSync {
        group_id: GroupId,
        since: Option<MessageId>,
    },

    /// Response to a sync request.
    MessageBatch {
        group_id: GroupId,
        messages: Vec<SealedMessage>,
        has_more: bool,
    },

    /// Request blob shards.
    BlobRequest {
        blob_id: BlobId,
        /// Which shard indices we need (or empty for any available).
        needed_shards: Vec<u8>,
    },

    /// Provide a blob shard.
    BlobShard(veil_store::BlobShard),

    /// Invite a peer to a group (contains encrypted group key).
    GroupInvite {
        group_id: GroupId,
        encrypted_group_key: Vec<u8>,
        inviter_eph_public: Vec<u8>,
    },

    /// Request the full encrypted blob (fallback when not enough shards are available).
    BlobFullRequest {
        blob_id: BlobId,
    },

    /// Provide the full encrypted blob.
    BlobFull {
        blob_id: BlobId,
        data: Vec<u8>,
    },

    /// Ephemeral presence signals — not persisted, not group-encrypted.
    /// Typing indicators and read receipts flow through here.
    Presence {
        kind: PresenceKind,
        /// Which group this applies to.
        group_id: GroupId,
        /// Sender's PeerId for display.
        sender: PeerId,
    },

    /// Ping/pong for keepalive.
    Ping(u64),
    Pong(u64),
}

/// Lightweight presence signals that don't need group-key encryption.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PresenceKind {
    /// User is currently typing.
    Typing,
    /// User stopped typing.
    StoppedTyping,
    /// User has read messages up to this ID.
    ReadReceipt { last_read: MessageId },
}

impl WireMessage {
    fn bincode_options() -> impl bincode::Options {
        bincode::options()
            .with_limit(MAX_WIRE_MESSAGE_SIZE)
            .allow_trailing_bytes()
    }

    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        Self::bincode_options().serialize(self)
    }

    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        Self::bincode_options().deserialize(data)
    }
}

/// Build the payload to sign for challenge-response authentication.
/// Format: challenge_nonce (32 bytes) || signer's peer_id verifying_key bytes.
pub fn challenge_sign_payload(challenge: &[u8; 32], peer_id: &PeerId) -> Vec<u8> {
    let mut payload = Vec::with_capacity(32 + peer_id.verifying_key.len());
    payload.extend_from_slice(challenge);
    payload.extend_from_slice(&peer_id.verifying_key);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_oversized_bincode_payload() {
        // Craft a payload that claims a Vec with length exceeding the limit.
        let mut malicious = Vec::new();
        // Variant index for MessagePush (now index 3 after Hello, ChallengeResponse, KeyExchange)
        malicious.push(3u8);
        // MessageId: 32 bytes
        malicious.extend_from_slice(&[0u8; 32]);
        // routing_tag: 32 bytes
        malicious.extend_from_slice(&[0u8; 32]);
        // ciphertext Vec length: claim u64::MAX bytes
        malicious.push(253); // 8 bytes follow for u64
        malicious.extend_from_slice(&u64::MAX.to_le_bytes());

        let result = WireMessage::decode(&malicious);
        assert!(result.is_err());
    }

    #[test]
    fn encode_decode_roundtrip() {
        let msg = WireMessage::Ping(42);
        let encoded = msg.encode().unwrap();
        let decoded = WireMessage::decode(&encoded).unwrap();
        match decoded {
            WireMessage::Ping(v) => assert_eq!(v, 42),
            _ => panic!("wrong variant"),
        }
    }
}
