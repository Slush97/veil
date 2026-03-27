use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use veil_crypto::{DeviceCertificate, GroupKey, GroupKeyRing, Identity, PeerId};

use crate::compression;
use crate::control::ControlMessage;

/// Maximum decompressed size for message plaintext (16 MiB).
const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Unique identifier for a message (content-addressed).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub [u8; 32]);

impl MessageId {
    pub fn from_content(content: &[u8]) -> Self {
        Self(*blake3::hash(content).as_bytes())
    }
}

/// The plaintext content of a message, before encryption.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageContent {
    pub kind: MessageKind,
    pub timestamp: DateTime<Utc>,
    pub channel_id: ChannelId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessageKind {
    Text(String),
    Image {
        blob_id: BlobId,
        width: u32,
        height: u32,
        thumbnail: Vec<u8>,
        ciphertext_len: u64,
    },
    Video {
        blob_id: BlobId,
        duration_secs: f32,
        thumbnail: Vec<u8>,
        ciphertext_len: u64,
    },
    File {
        blob_id: BlobId,
        filename: String,
        size_bytes: u64,
        ciphertext_len: u64,
        /// For small files (< 1 MiB): encrypted data inline. For large files: None (fetch via blob protocol).
        inline_data: Option<Vec<u8>>,
    },
    Reply {
        parent_id: MessageId,
        content: Box<MessageKind>,
    },
    Reaction {
        target_id: MessageId,
        emoji: String,
    },
    /// Edit a previously sent message. Only the original sender can edit.
    Edit {
        target_id: MessageId,
        new_text: String,
    },
    /// Delete a previously sent message. Only the original sender can delete.
    Delete {
        target_id: MessageId,
    },
    /// A control message for group state changes (key rotation, membership, etc.).
    Control(ControlMessage),
    /// Audio message with duration and waveform visualization data.
    Audio {
        blob_id: BlobId,
        /// Duration in seconds (0.0 if unknown).
        duration_secs: f32,
        /// Low-resolution waveform: 64 amplitude samples normalized to 0–255.
        waveform: Vec<u8>,
        ciphertext_len: u64,
    },
    /// Link preview metadata for URLs found in a previously-sent text message.
    /// Sent as a follow-up after the original text message.
    LinkPreview {
        target_id: MessageId,
        previews: Vec<EmbedPreview>,
    },
}

/// Metadata extracted from a URL for link preview display.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbedPreview {
    /// The URL that was fetched.
    pub url: String,
    /// Page title (from `<title>` or `og:title`).
    pub title: Option<String>,
    /// Page description (from meta description or `og:description`).
    pub description: Option<String>,
    /// URL of a preview image (`og:image`). Not fetched — just the URL.
    pub image_url: Option<String>,
    /// Site name (`og:site_name`).
    pub site_name: Option<String>,
}

/// Derive an opaque routing tag from a group ID.
/// Used by both message sealing and relay subscription.
pub fn routing_tag_for_group(group_id: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("veil-routing-tag", group_id)
}

/// An encrypted message as it travels over the network and is stored by pinners.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SealedMessage {
    pub id: MessageId,
    /// Opaque routing tag derived from blake3(group_key_bytes, group_id) —
    /// does not reveal the actual group_id to observers.
    pub routing_tag: [u8; 32],
    pub ciphertext: Vec<u8>,
    /// Ed25519 signature over (id || routing_tag || ciphertext || key_generation || sent_at).
    pub signature: Vec<u8>,
    /// Which key generation was used (so receivers pick the right key).
    pub key_generation: u64,
    /// Unix timestamp (seconds) when the message was sent.
    pub sent_at: i64,
}

impl SealedMessage {
    /// Seal a message: encrypt content with the group key and sign with the sender's identity.
    pub fn seal(
        content: &MessageContent,
        group_key: &GroupKey,
        group_id: &[u8; 32],
        identity: &Identity,
    ) -> Result<Self, veil_crypto::EncryptError> {
        let serialized = bincode::serialize(content)
            .map_err(|_| veil_crypto::EncryptError::SerializationFailed)?;
        let compressed = compression::compress(&serialized)
            .map_err(|_| veil_crypto::EncryptError::SerializationFailed)?;
        let ciphertext = group_key.encrypt(&compressed)?;
        let id = MessageId::from_content(&ciphertext);

        // Derive opaque routing tag
        let routing_tag = routing_tag_for_group(group_id);

        let sent_at = Utc::now().timestamp();
        let key_generation = group_key.generation();

        // Sign: id || routing_tag || ciphertext || key_generation || sent_at
        let sig_payload =
            Self::signature_payload(&id, &routing_tag, &ciphertext, key_generation, sent_at);
        let signature = identity.sign(&sig_payload);

        Ok(Self {
            id,
            routing_tag,
            ciphertext,
            signature,
            key_generation,
            sent_at,
        })
    }

    /// Verify the signature and decrypt the message.
    /// `known_members` is the list of PeerIds who are valid senders.
    /// Returns the decrypted content and the PeerId of the verified sender.
    pub fn verify_and_open(
        &self,
        group_key: &GroupKey,
        known_members: &[PeerId],
    ) -> Result<(MessageContent, PeerId), SealedMessageError> {
        // Verify key generation matches
        if self.key_generation != group_key.generation() {
            return Err(SealedMessageError::KeyGenerationMismatch);
        }

        // Verify signature against known members and identify the sender
        let sig_payload = Self::signature_payload(
            &self.id,
            &self.routing_tag,
            &self.ciphertext,
            self.key_generation,
            self.sent_at,
        );

        let sender = known_members
            .iter()
            .find(|peer| peer.verify(&sig_payload, &self.signature))
            .cloned();

        let sender = sender.ok_or(SealedMessageError::InvalidSignature)?;

        // Decrypt and decompress
        let decrypted = group_key
            .decrypt(&self.ciphertext)
            .map_err(|_| SealedMessageError::DecryptionFailed)?;
        let plaintext = compression::decompress(&decrypted, MAX_MESSAGE_SIZE)
            .map_err(SealedMessageError::DecompressionFailed)?;

        let content: MessageContent = bincode::deserialize(&plaintext)
            .map_err(|_| SealedMessageError::DeserializationFailed)?;

        Ok((content, sender))
    }

    /// Verify and decrypt using a keyring (tries multiple key generations).
    ///
    /// This is the preferred method — it handles key transitions gracefully.
    /// `known_members` can be either master PeerIds or legacy direct PeerIds.
    /// `device_certs` maps device PeerIds to their certificates for chain verification.
    pub fn verify_and_open_with_keyring(
        &self,
        keyring: &GroupKeyRing,
        known_members: &[PeerId],
        device_certs: &[DeviceCertificate],
    ) -> Result<(MessageContent, PeerId), SealedMessageError> {
        // Find a key matching this message's generation
        let group_key = keyring
            .key_for_generation(self.key_generation)
            .ok_or(SealedMessageError::KeyGenerationMismatch)?;

        // Build signature payload
        let sig_payload = Self::signature_payload(
            &self.id,
            &self.routing_tag,
            &self.ciphertext,
            self.key_generation,
            self.sent_at,
        );

        // Try direct match against known members (legacy / master key signing)
        let mut sender: Option<PeerId> = known_members
            .iter()
            .find(|peer| peer.verify(&sig_payload, &self.signature))
            .cloned();

        // If no direct match, try device certificate chain verification
        if sender.is_none() {
            for cert in device_certs {
                if let Some(master_id) = cert.verify_message(&sig_payload, &self.signature) {
                    // Verify the master is a known member
                    if known_members.contains(&master_id) {
                        sender = Some(master_id);
                        break;
                    }
                }
            }
        }

        let sender = sender.ok_or(SealedMessageError::InvalidSignature)?;

        // Decrypt and decompress
        let decrypted = group_key
            .decrypt(&self.ciphertext)
            .map_err(|_| SealedMessageError::DecryptionFailed)?;
        let plaintext = compression::decompress(&decrypted, MAX_MESSAGE_SIZE)
            .map_err(SealedMessageError::DecompressionFailed)?;

        let content: MessageContent = bincode::deserialize(&plaintext)
            .map_err(|_| SealedMessageError::DeserializationFailed)?;

        Ok((content, sender))
    }

    fn signature_payload(
        id: &MessageId,
        routing_tag: &[u8; 32],
        ciphertext: &[u8],
        key_generation: u64,
        sent_at: i64,
    ) -> Vec<u8> {
        let mut payload = Vec::with_capacity(32 + 32 + ciphertext.len() + 8 + 8);
        payload.extend_from_slice(&id.0);
        payload.extend_from_slice(routing_tag);
        payload.extend_from_slice(ciphertext);
        payload.extend_from_slice(&key_generation.to_le_bytes());
        payload.extend_from_slice(&sent_at.to_le_bytes());
        payload
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SealedMessageError {
    #[error("invalid signature: no known member signed this message")]
    InvalidSignature,
    #[error("decryption failed")]
    DecryptionFailed,
    #[error("decompression failed: {0}")]
    DecompressionFailed(#[from] crate::compression::CompressionError),
    #[error("deserialization failed")]
    DeserializationFailed,
    #[error("key generation mismatch")]
    KeyGenerationMismatch,
}

/// Content-addressed blob identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobId(pub [u8; 32]);

/// Opaque group identifier (hash of group creation parameters).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub [u8; 32]);

/// Channel within a group — each has its own encryption subkey.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub Uuid);

impl ChannelId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ChannelId {
    fn default() -> Self {
        Self::new()
    }
}

/// Category within a group — organizes channels under collapsible headings.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CategoryId(pub Uuid);

impl CategoryId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CategoryId {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_and_open_roundtrip() {
        let identity = Identity::generate();
        let group_key = GroupKey::generate();
        let group_id = [42u8; 32];

        let content = MessageContent {
            kind: MessageKind::Text("hello".into()),
            timestamp: Utc::now(),
            channel_id: ChannelId::new(),
        };

        let sealed = SealedMessage::seal(&content, &group_key, &group_id, &identity).unwrap();
        let (opened, sender) = sealed
            .verify_and_open(&group_key, &[identity.peer_id()])
            .unwrap();

        match opened.kind {
            MessageKind::Text(ref t) => assert_eq!(t, "hello"),
            _ => panic!("wrong kind"),
        }
        assert_eq!(sender.verifying_key, identity.peer_id().verifying_key);
    }

    #[test]
    fn reject_tampered_signature() {
        let identity = Identity::generate();
        let group_key = GroupKey::generate();
        let group_id = [42u8; 32];

        let content = MessageContent {
            kind: MessageKind::Text("hello".into()),
            timestamp: Utc::now(),
            channel_id: ChannelId::new(),
        };

        let mut sealed = SealedMessage::seal(&content, &group_key, &group_id, &identity).unwrap();
        // Tamper with signature
        if let Some(byte) = sealed.signature.first_mut() {
            *byte ^= 0xff;
        }

        let result = sealed.verify_and_open(&group_key, &[identity.peer_id()]);
        assert!(result.is_err());
    }

    #[test]
    fn reject_unknown_sender() {
        let alice = Identity::generate();
        let bob = Identity::generate();
        let group_key = GroupKey::generate();
        let group_id = [42u8; 32];

        let content = MessageContent {
            kind: MessageKind::Text("hello".into()),
            timestamp: Utc::now(),
            channel_id: ChannelId::new(),
        };

        // Alice seals, but Bob is the only known member
        let sealed = SealedMessage::seal(&content, &group_key, &group_id, &alice).unwrap();
        let result = sealed.verify_and_open(&group_key, &[bob.peer_id()]);
        assert!(result.is_err());
    }
}
