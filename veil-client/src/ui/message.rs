use std::net::SocketAddr;
use std::sync::Arc;

use veil_core::{BlobId, GroupId, MessageId};
use veil_crypto::{DeviceCertificate, GroupKey, PeerId};
use veil_net::{ConnectionId, WireMessage};
use veil_store::LocalStore;

use super::types::SharedGroupKey;

pub enum NetCommand {
    Connect(SocketAddr),
    SendMessage(veil_core::SealedMessage),
    ConnectRelay(SocketAddr),
    CreateInvite {
        group_id: GroupId,
        group_name: String,
        relay_addr: String,
        passphrase: String,
        group_key: Arc<GroupKey>,
    },
    AcceptInvite {
        url: String,
        passphrase: String,
    },
    /// Send a file. The network worker handles inline vs sharded based on size.
    SendFile {
        path: std::path::PathBuf,
        group_id: GroupId,
        group_key: Arc<GroupKey>,
        store: Arc<LocalStore>,
        identity_bytes: [u8; 32],
    },
    /// Send a presence signal to peers.
    SendPresence(WireMessage),
    /// Respond to a blob request from a peer (full blob fallback).
    BlobResponse {
        conn_id: ConnectionId,
        blob_id: BlobId,
        data: Vec<u8>,
    },
    /// Request a full blob from peers.
    RequestBlob {
        blob_id: BlobId,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code, clippy::enum_variant_names)]
pub enum Message {
    // Setup
    CreateIdentity,
    LoadIdentity,
    PassphraseChanged(String),
    // Recovery phrase
    ConfirmRecoveryPhrase,
    // Chat
    InputChanged(String),
    Send,
    SelectGroup(String),
    SelectChannel(String),
    // Connection
    ConnectInputChanged(String),
    ConnectToPeer,
    // Network events
    NetworkReady {
        local_addr: SocketAddr,
        cmd_tx: futures::channel::mpsc::Sender<NetCommand>,
    },
    PeerConnected {
        conn_id: ConnectionId,
        peer_id: PeerId,
        session_key: [u8; 32],
        device_certificate: Option<DeviceCertificate>,
    },
    PeerDisconnected {
        conn_id: ConnectionId,
    },
    PeerData {
        sealed: veil_core::SealedMessage,
    },
    ConnectionFailed(String),
    // Relay
    RelayConnected,
    RelayDisconnected(String),
    // Relay UI
    RelayAddrChanged(String),
    ConnectToRelay,
    // Invite UI
    InvitePassphraseChanged(String),
    CreateInvite,
    InviteCreated(String),
    InviteInputChanged(String),
    AcceptInvite,
    InviteAccepted {
        group_name: String,
        group_id: GroupId,
        group_key: SharedGroupKey,
    },
    InviteFailed(String),
    // Edit/Delete
    EditMessage(usize),
    CancelEdit,
    ConfirmEdit,
    DeleteMessage(usize),
    // Typing/Presence
    TypingStarted {
        peer_id: PeerId,
        group_id: GroupId,
    },
    TypingStopped {
        peer_id: PeerId,
    },
    ReadReceipt {
        peer_id: PeerId,
        last_read: MessageId,
    },
    // File
    PickFile,
    SendFile(std::path::PathBuf),
    FileSent {
        filename: String,
    },
    FileFailed(String),
    BlobRequested {
        conn_id: ConnectionId,
        blob_id: BlobId,
    },
    BlobReceived {
        blob_id: BlobId,
    },
    SaveFile(BlobId, String),
    // Relay errors
    RelayError {
        code: String,
        message: String,
    },
    // Phase 2: Display names
    DisplayNameInputChanged(String),
    SetDisplayName,
    // Phase 3: Replies + reactions
    ReplyTo(usize),
    CancelReply,
    React(usize, String),
    // Phase 4: Search + discovery
    ToggleSearch,
    SearchQueryChanged(String),
    ConnectDiscoveredPeer(SocketAddr),
    LanPeerDiscovered {
        name: String,
        addr: SocketAddr,
        fingerprint: String,
    },
    LanPeerLost(String),
    LoadMoreMessages,
    // Phase 5: Settings
    OpenSettings,
    CloseSettings,
    ToggleTheme,
    ToggleNotifications,
    DeviceNameInputChanged(String),
    ExportIdentity,
    // Keyboard shortcuts
    EscapePressed,
    UpArrowPressed,
}
