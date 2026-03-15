use std::sync::Arc;

use veil_core::{BlobId, ChannelId, GroupId, MessageId};
use veil_crypto::{DeviceCertificate, GroupKey, GroupKeyRing, PeerId};

/// Send status for outbound messages.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum MessageStatus {
    Sending,
    Sent,
    Delivered,
}

/// Structured connection state for clearer UI feedback.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum ConnectionState {
    #[default]
    Disconnected,
    Connecting(String),
    Connected(String),
    Reconnecting,
    Warning(String),
    Failed(String),
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => write!(f, "Disconnected"),
            Self::Connecting(msg) => write!(f, "{msg}"),
            Self::Connected(msg) => write!(f, "{msg}"),
            Self::Reconnecting => write!(f, "Reconnecting..."),
            Self::Warning(msg) => write!(f, "{msg}"),
            Self::Failed(msg) => write!(f, "{msg}"),
        }
    }
}

/// Status of a file attachment in a chat message.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum FileStatus {
    Available,
    Downloading,
    Unavailable,
}

/// File attachment metadata.
#[derive(Clone, Debug)]
pub(crate) struct FileInfo {
    pub(crate) blob_id: BlobId,
    pub(crate) filename: String,
    pub(crate) size_str: String,
    pub(crate) status: FileStatus,
}

#[derive(Clone)]
pub(crate) struct GroupState {
    pub(crate) name: String,
    pub(crate) id: GroupId,
    pub(crate) key_ring: Arc<std::sync::Mutex<GroupKeyRing>>,
    pub(crate) device_certs: Vec<DeviceCertificate>,
    /// Known group members (master PeerIds) for signature verification.
    /// Populated from MemberAdded control messages and invite acceptance.
    pub(crate) members: Vec<PeerId>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ThemeChoice {
    Dark,
    Light,
}

pub(crate) enum Screen {
    Setup,
    ShowRecoveryPhrase(String),
    Chat,
    Settings,
}

pub(crate) struct ChatMessage {
    pub(crate) id: Option<MessageId>,
    pub(crate) sender: String,
    pub(crate) sender_id: Option<PeerId>,
    pub(crate) content: String,
    pub(crate) timestamp: String,
    pub(crate) datetime: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) edited: bool,
    pub(crate) deleted: bool,
    pub(crate) status: Option<MessageStatus>,
    pub(crate) reply_to_content: Option<String>,
    pub(crate) reply_to_sender: Option<String>,
    pub(crate) channel_id: Option<ChannelId>,
    pub(crate) file_info: Option<FileInfo>,
}

impl ChatMessage {
    pub(crate) fn user(
        id: MessageId,
        sender_id: PeerId,
        content: String,
        dt: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            id: Some(id),
            sender: sender_id.fingerprint(),
            sender_id: Some(sender_id),
            content,
            timestamp: dt.format("%H:%M").to_string(),
            datetime: Some(dt),
            edited: false,
            deleted: false,
            status: None,
            reply_to_content: None,
            reply_to_sender: None,
            channel_id: None,
            file_info: None,
        }
    }

    pub(crate) fn user_with_channel(
        id: MessageId,
        sender_id: PeerId,
        content: String,
        dt: chrono::DateTime<chrono::Utc>,
        channel_id: ChannelId,
    ) -> Self {
        let mut msg = Self::user(id, sender_id, content, dt);
        msg.channel_id = Some(channel_id);
        msg
    }

    pub(crate) fn system(content: String) -> Self {
        Self {
            id: None,
            sender: "system".into(),
            sender_id: None,
            content,
            timestamp: chrono::Utc::now().format("%H:%M").to_string(),
            datetime: Some(chrono::Utc::now()),
            edited: false,
            deleted: false,
            status: None,
            reply_to_content: None,
            reply_to_sender: None,
            channel_id: None,
            file_info: None,
        }
    }
}

/// Result of a contact search (username lookup).
#[derive(Clone, Debug)]
pub(crate) enum ContactSearchResult {
    Found { username: String, public_key: [u8; 32] },
    NotFound(String),
    Searching,
}

/// Wrapper around Arc<GroupKey> that implements Debug (GroupKey intentionally omits Debug).
#[derive(Clone)]
pub struct SharedGroupKey(pub(crate) Arc<GroupKey>);

impl std::fmt::Debug for SharedGroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedGroupKey(***)")
    }
}
