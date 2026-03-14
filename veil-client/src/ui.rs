use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures::StreamExt;
use iced::widget::{
    button, column, container, horizontal_space, row, scrollable, text, text_input, Column,
};
use iced::{Element, Length, Subscription, Theme};
use zeroize::Zeroize;

use veil_core::{
    ChannelId, ControlMessage, GroupId, MessageContent, MessageDeduplicator, MessageKind,
    SealedMessage,
    invite::{self, InvitePayload},
    routing_tag_for_group,
};
use veil_crypto::{
    DeviceCertificate, DeviceIdentity, GroupKey, GroupKeyRing, MasterIdentity, PeerId,
};
use veil_net::{
    ConnectionId, Discovery, DiscoveryEvent, PeerEvent, PeerManager, RelayClient, RelayEvent,
    WireMessage, create_endpoint,
};
use veil_store::LocalStore;

/// Send status for outbound messages.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
enum MessageStatus {
    Sending,
    Sent,
    Delivered,
}

/// Structured connection state for clearer UI feedback.
#[derive(Clone, Debug, PartialEq)]
enum ConnectionState {
    Disconnected,
    Connecting(String),
    Connected(String),
    Reconnecting,
    Warning(String),
    Failed(String),
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::Disconnected
    }
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
enum FileStatus {
    Available,
    Downloading,
    Unavailable,
}

/// File attachment metadata.
#[derive(Clone, Debug)]
struct FileInfo {
    blob_id: veil_core::BlobId,
    filename: String,
    size_str: String,
    status: FileStatus,
}

pub struct App {
    master: Option<MasterIdentity>,
    device: Option<DeviceIdentity>,
    screen: Screen,
    // Chat state
    message_input: String,
    messages: Vec<ChatMessage>,
    editing_message: Option<usize>,
    // Group state
    current_group: Option<GroupState>,
    groups: Vec<GroupState>,
    current_channel: Option<String>,
    channels: Vec<String>,
    // Connection state
    connect_input: String,
    connection_state: ConnectionState,
    // Network state
    net_cmd_tx: Option<futures::channel::mpsc::Sender<NetCommand>>,
    connected_peers: Vec<(ConnectionId, PeerId)>,
    local_addr: Option<SocketAddr>,
    // Identity persistence
    passphrase_input: String,
    // Message persistence
    store: Option<Arc<LocalStore>>,
    // Relay state
    relay_addr_input: String,
    relay_connected: bool,
    // Invite state
    invite_passphrase: String,
    invite_input: String,
    generated_invite_url: Option<String>,
    // Presence state
    typing_peers: Vec<(PeerId, std::time::Instant)>,
    // Phase 1: Message reliability
    pending_messages: Vec<SealedMessage>,
    // Phase 2: Display names + notifications
    display_names: HashMap<String, String>,
    display_name_input: String,
    unread_counts: HashMap<[u8; 32], usize>,
    notifications_enabled: bool,
    // Phase 3: Replies + reactions
    replying_to: Option<usize>,
    reactions: HashMap<[u8; 32], Vec<(PeerId, String)>>,
    // Phase 4: LAN discovery + search
    search_query: String,
    search_active: bool,
    search_results: Vec<usize>,
    discovered_peers: Vec<(String, SocketAddr, String)>,
    messages_loaded: usize,
    // Phase 5: Settings + visual polish
    theme_choice: ThemeChoice,
    device_name_input: String,
}

#[derive(Clone)]
struct GroupState {
    name: String,
    id: GroupId,
    key_ring: Arc<std::sync::Mutex<GroupKeyRing>>,
    device_certs: Vec<DeviceCertificate>,
}

#[derive(Clone, Debug, PartialEq)]
enum ThemeChoice {
    Dark,
    Light,
}

enum Screen {
    Setup,
    ShowRecoveryPhrase(String),
    Chat,
    Settings,
}

struct ChatMessage {
    id: Option<veil_core::MessageId>,
    sender: String,
    sender_id: Option<PeerId>,
    content: String,
    timestamp: String,
    datetime: Option<chrono::DateTime<chrono::Utc>>,
    edited: bool,
    deleted: bool,
    status: Option<MessageStatus>,
    reply_to_content: Option<String>,
    reply_to_sender: Option<String>,
    channel_id: Option<ChannelId>,
    file_info: Option<FileInfo>,
}

impl ChatMessage {
    fn user(
        id: veil_core::MessageId,
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

    fn user_with_channel(
        id: veil_core::MessageId,
        sender_id: PeerId,
        content: String,
        dt: chrono::DateTime<chrono::Utc>,
        channel_id: ChannelId,
    ) -> Self {
        let mut msg = Self::user(id, sender_id, content, dt);
        msg.channel_id = Some(channel_id);
        msg
    }

    fn system(content: String) -> Self {
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

/// Wrapper around Arc<GroupKey> that implements Debug (GroupKey intentionally omits Debug).
#[derive(Clone)]
pub(crate) struct SharedGroupKey(Arc<GroupKey>);

impl std::fmt::Debug for SharedGroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedGroupKey(***)")
    }
}

pub enum NetCommand {
    Connect(SocketAddr),
    SendMessage(SealedMessage),
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
        blob_id: veil_core::BlobId,
        data: Vec<u8>,
    },
    /// Request a full blob from peers.
    RequestBlob {
        blob_id: veil_core::BlobId,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
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
        sealed: SealedMessage,
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
        last_read: veil_core::MessageId,
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
        blob_id: veil_core::BlobId,
    },
    BlobReceived {
        blob_id: veil_core::BlobId,
    },
    SaveFile(veil_core::BlobId, String),
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

impl Default for App {
    fn default() -> Self {
        Self {
            master: None,
            device: None,
            screen: Screen::Setup,
            message_input: String::new(),
            messages: Vec::new(),
            current_group: None,
            groups: Vec::new(),
            current_channel: None,
            channels: Vec::new(),
            connect_input: String::new(),
            connection_state: ConnectionState::Disconnected,
            net_cmd_tx: None,
            connected_peers: Vec::new(),
            local_addr: None,
            passphrase_input: String::new(),
            store: None,
            relay_addr_input: String::new(),
            relay_connected: false,
            invite_passphrase: String::new(),
            invite_input: String::new(),
            generated_invite_url: None,
            editing_message: None,
            typing_peers: Vec::new(),
            // Phase 1
            pending_messages: Vec::new(),
            // Phase 2
            display_names: HashMap::new(),
            display_name_input: String::new(),
            unread_counts: HashMap::new(),
            notifications_enabled: true,
            // Phase 3
            replying_to: None,
            reactions: HashMap::new(),
            // Phase 4
            search_query: String::new(),
            search_active: false,
            search_results: Vec::new(),
            discovered_peers: Vec::new(),
            messages_loaded: 500,
            // Phase 5
            theme_choice: ThemeChoice::Dark,
            device_name_input: String::new(),
        }
    }
}

fn veil_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("VEIL_DATA_DIR") {
        return std::path::PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("veil")
}

fn network_worker(
    peer_id: PeerId,
    identity_bytes: [u8; 32],
    groups: Vec<GroupId>,
    device_cert: Option<DeviceCertificate>,
    blob_store: Option<Arc<LocalStore>>,
) -> impl Send + futures::Stream<Item = Message> {
    iced::stream::channel(100, move |mut output| async move {
        use futures::SinkExt;

        let bind_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let endpoint = match create_endpoint(bind_addr) {
            Ok(ep) => ep,
            Err(e) => {
                let _ = output
                    .send(Message::ConnectionFailed(format!(
                        "Failed to create endpoint: {e}"
                    )))
                    .await;
                futures::future::pending::<()>().await;
                return;
            }
        };

        let local_addr = endpoint.local_addr().unwrap();

        let mut manager =
            PeerManager::new(endpoint.clone(), peer_id.clone(), identity_bytes);
        if let Some(ref cert) = device_cert {
            manager.set_device_cert(cert.clone());
        }
        let mut event_rx = manager.take_event_receiver().unwrap();
        let connections = manager.connections_handle();
        let event_tx = manager.event_sender();

        let (cmd_tx, mut cmd_rx) = futures::channel::mpsc::channel::<NetCommand>(100);

        let _ = output
            .send(Message::NetworkReady {
                local_addr,
                cmd_tx,
            })
            .await;

        // Spawn accept loop
        tokio::spawn(PeerManager::accept_loop(
            endpoint.clone(),
            peer_id.clone(),
            identity_bytes,
            event_tx,
            connections,
            device_cert,
        ));

        // Relay client state
        let mut relay_client: Option<RelayClient> = None;
        let mut relay_event_rx: Option<tokio::sync::mpsc::Receiver<RelayEvent>> = None;

        // Dedup to prevent duplicate messages from P2P + relay
        let mut dedup = MessageDeduplicator::with_capacity(2048);

        // mDNS discovery
        let mut discovery_rx: Option<tokio::sync::mpsc::Receiver<DiscoveryEvent>> = None;
        if let Ok(discovery) = Discovery::new() {
            if let Some(ref ep_addr) = Some(local_addr) {
                let fp = peer_id.fingerprint();
                let _ = discovery.register(ep_addr.port(), &fp);
            }
            if let Ok(rx) = discovery.browse() {
                discovery_rx = Some(rx);
            }
        }

        // Main event loop
        loop {
            // Build a future for relay events
            let relay_event_fut = async {
                if let Some(ref mut rx) = relay_event_rx {
                    rx.recv().await
                } else {
                    futures::future::pending().await
                }
            };

            // Build a future for discovery events
            let discovery_fut = async {
                if let Some(ref mut rx) = discovery_rx {
                    rx.recv().await
                } else {
                    futures::future::pending().await
                }
            };

            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(PeerEvent::Connected { conn_id, peer_id, session_key, device_certificate }) => {
                            let _ = output
                                .send(Message::PeerConnected { conn_id, peer_id, session_key, device_certificate })
                                .await;
                        }
                        Some(PeerEvent::Disconnected { conn_id }) => {
                            let _ = output
                                .send(Message::PeerDisconnected { conn_id })
                                .await;
                        }
                        Some(PeerEvent::Message { conn_id, message, .. }) => {
                            match message {
                                WireMessage::MessagePush(sealed) => {
                                    if dedup.check(&sealed).is_ok() {
                                        let _ = output
                                            .send(Message::PeerData { sealed })
                                            .await;
                                    }
                                }
                                WireMessage::MessageSync { group_id, since } => {
                                    // Respond with messages from our store
                                    if let Some(ref store) = blob_store {
                                        let tag = routing_tag_for_group(&group_id.0);
                                        if let Ok(msgs) = store.list_messages_by_tag(&tag, 100, 0) {
                                            let filtered: Vec<SealedMessage> = if let Some(ref since_id) = since {
                                                msgs.into_iter().filter(|m| m.id != *since_id && m.sent_at > 0).collect()
                                            } else {
                                                msgs
                                            };
                                            let _ = manager.send_to(conn_id, &WireMessage::MessageBatch {
                                                group_id,
                                                messages: filtered,
                                                has_more: false,
                                            }).await;
                                        }
                                    }
                                }
                                WireMessage::MessageBatch { messages, .. } => {
                                    for sealed in messages {
                                        if dedup.check(&sealed).is_ok() {
                                            let _ = output
                                                .send(Message::PeerData { sealed })
                                                .await;
                                        }
                                    }
                                }
                                WireMessage::BlobFullRequest { blob_id } => {
                                    let _ = output
                                        .send(Message::BlobRequested { conn_id, blob_id })
                                        .await;
                                }
                                WireMessage::BlobFull { blob_id, data } => {
                                    if let Some(ref store) = blob_store {
                                        if let Err(e) = store.store_blob_full(&blob_id, &data) {
                                            tracing::warn!("failed to store received blob: {e}");
                                        }
                                    }
                                    let _ = output
                                        .send(Message::BlobReceived { blob_id })
                                        .await;
                                }
                                WireMessage::BlobShard(shard) => {
                                    if let Some(ref store) = blob_store {
                                        let _ = store.store_blob_shard(&shard);
                                    }
                                }
                                WireMessage::Presence { kind, group_id, sender } => {
                                    match kind {
                                        veil_net::PresenceKind::Typing => {
                                            let _ = output
                                                .send(Message::TypingStarted {
                                                    peer_id: sender,
                                                    group_id,
                                                })
                                                .await;
                                        }
                                        veil_net::PresenceKind::StoppedTyping => {
                                            let _ = output
                                                .send(Message::TypingStopped {
                                                    peer_id: sender,
                                                })
                                                .await;
                                        }
                                        veil_net::PresenceKind::ReadReceipt { last_read } => {
                                            let _ = output
                                                .send(Message::ReadReceipt {
                                                    peer_id: sender,
                                                    last_read,
                                                })
                                                .await;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        None => break,
                    }
                }
                relay_event = relay_event_fut => {
                    match relay_event {
                        Some(RelayEvent::Connected) => {
                            let _ = output.send(Message::RelayConnected).await;
                        }
                        Some(RelayEvent::Disconnected(reason)) => {
                            let _ = output.send(Message::RelayDisconnected(reason)).await;
                        }
                        Some(RelayEvent::Message { routing_tag: _, payload }) => {
                            // Try to decode as WireMessage
                            if let Ok(WireMessage::MessagePush(sealed)) = WireMessage::decode(&payload) {
                                if dedup.check(&sealed).is_ok() {
                                    let _ = output
                                        .send(Message::PeerData { sealed })
                                        .await;
                                }
                            }
                        }
                        Some(RelayEvent::Error { code, message }) => {
                            let _ = output.send(Message::RelayError { code, message }).await;
                        }
                        Some(RelayEvent::MailboxDrained { messages, .. }) => {
                            for envelope in messages {
                                if let Ok(WireMessage::MessagePush(sealed)) = WireMessage::decode(&envelope.payload) {
                                    if dedup.check(&sealed).is_ok() {
                                        let _ = output
                                            .send(Message::PeerData { sealed })
                                            .await;
                                    }
                                }
                            }
                        }
                        None => {
                            relay_client = None;
                            relay_event_rx = None;
                        }
                    }
                }
                discovery_event = discovery_fut => {
                    match discovery_event {
                        Some(DiscoveryEvent::PeerFound { instance_name, addr, fingerprint }) => {
                            let _ = output.send(Message::LanPeerDiscovered {
                                name: instance_name,
                                addr,
                                fingerprint,
                            }).await;
                        }
                        Some(DiscoveryEvent::PeerLost { instance_name }) => {
                            let _ = output.send(Message::LanPeerLost(instance_name)).await;
                        }
                        None => {
                            discovery_rx = None;
                        }
                    }
                }
                cmd = cmd_rx.next() => {
                    match cmd {
                        Some(NetCommand::Connect(addr)) => {
                            if let Err(e) = manager.connect(addr).await {
                                let _ = output
                                    .send(Message::ConnectionFailed(e.to_string()))
                                    .await;
                            }
                        }
                        Some(NetCommand::SendMessage(sealed)) => {
                            // Broadcast via P2P
                            let wire_msg = WireMessage::MessagePush(sealed.clone());
                            manager.broadcast(&wire_msg).await;

                            // Also forward via relay if connected
                            if let Some(ref rc) = relay_client {
                                if let Ok(payload) = wire_msg.encode() {
                                    let _ = rc.forward_message(sealed.routing_tag, payload).await;
                                }
                            }
                        }
                        Some(NetCommand::ConnectRelay(addr)) => {
                            // Compute routing tags for all current groups
                            let tags: Vec<[u8; 32]> = groups
                                .iter()
                                .map(|g| routing_tag_for_group(&g.0))
                                .collect();

                            let mut pid_bytes = [0u8; 32];
                            let vk = &peer_id.verifying_key;
                            pid_bytes[..vk.len().min(32)].copy_from_slice(&vk[..vk.len().min(32)]);

                            let (rc, rx) = RelayClient::spawn(
                                addr,
                                endpoint.clone(),
                                pid_bytes,
                                identity_bytes,
                                tags,
                            );
                            relay_client = Some(rc);
                            relay_event_rx = Some(rx);
                        }
                        Some(NetCommand::CreateInvite {
                            group_id,
                            group_name,
                            relay_addr,
                            passphrase,
                            group_key,
                        }) => {
                            match invite::create_open_invite(
                                group_id,
                                group_name,
                                relay_addr,
                                &group_key,
                                passphrase.as_bytes(),
                            ) {
                                Ok(payload) => match payload.to_url() {
                                    Ok(url) => {
                                        let _ = output.send(Message::InviteCreated(url)).await;
                                    }
                                    Err(e) => {
                                        let _ = output.send(Message::InviteFailed(e.to_string())).await;
                                    }
                                },
                                Err(e) => {
                                    let _ = output.send(Message::InviteFailed(e.to_string())).await;
                                }
                            }
                        }
                        Some(NetCommand::AcceptInvite { url, passphrase }) => {
                            match InvitePayload::from_url(&url) {
                                Ok(payload) => {
                                    match invite::accept_invite(&payload, passphrase.as_bytes()) {
                                        Ok(group_key) => {
                                            // Subscribe to the new group's routing tag on relay
                                            let tag = routing_tag_for_group(&payload.group_id.0);
                                            if let Some(ref rc) = relay_client {
                                                let _ = rc.subscribe(vec![tag]).await;
                                            }
                                            let _ = output
                                                .send(Message::InviteAccepted {
                                                    group_name: payload.group_name,
                                                    group_id: payload.group_id,
                                                    group_key: SharedGroupKey(Arc::new(group_key)),
                                                })
                                                .await;
                                        }
                                        Err(e) => {
                                            let _ = output.send(Message::InviteFailed(e.to_string())).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = output.send(Message::InviteFailed(e.to_string())).await;
                                }
                            }
                        }
                        Some(NetCommand::SendFile {
                            path,
                            group_id,
                            group_key,
                            store,
                            identity_bytes: id_bytes,
                        }) => {
                            let filename = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file")
                                .to_string();

                            match std::fs::read(&path) {
                                Ok(file_data) => {
                                    let size_bytes = file_data.len() as u64;

                                    if file_data.len() < veil_store::INLINE_THRESHOLD {
                                        // Small file: encrypt and send inline
                                        let ciphertext = match group_key.encrypt(&file_data) {
                                            Ok(ct) => ct,
                                            Err(e) => {
                                                let _ = output.send(Message::FileFailed(e.to_string())).await;
                                                continue;
                                            }
                                        };
                                        let blob_id = veil_core::BlobId(
                                            *blake3::hash(&ciphertext).as_bytes(),
                                        );
                                        let ciphertext_len = ciphertext.len() as u64;
                                        let content = MessageContent {
                                            kind: MessageKind::File {
                                                blob_id,
                                                filename: filename.clone(),
                                                size_bytes,
                                                ciphertext_len,
                                                inline_data: Some(ciphertext),
                                            },
                                            timestamp: chrono::Utc::now(),
                                            channel_id: ChannelId::new(),
                                        };
                                        let identity = veil_crypto::Identity::from_bytes(&id_bytes);
                                        match SealedMessage::seal(&content, &group_key, &group_id.0, &identity) {
                                            Ok(sealed) => {
                                                let wire_msg = WireMessage::MessagePush(sealed.clone());
                                                manager.broadcast(&wire_msg).await;
                                                if let Some(ref rc) = relay_client {
                                                    if let Ok(payload) = wire_msg.encode() {
                                                        let _ = rc.forward_message(sealed.routing_tag, payload).await;
                                                    }
                                                }
                                                if let Err(e) = store.store_message(&sealed) {
                                        tracing::warn!("failed to persist message: {e}");
                                    }
                                                let _ = output.send(Message::FileSent { filename }).await;
                                            }
                                            Err(e) => {
                                                let _ = output.send(Message::FileFailed(e.to_string())).await;
                                            }
                                        }
                                    } else {
                                        // Large file: encrypt, store full copy, shard, send message reference
                                        let ciphertext = match group_key.encrypt(&file_data) {
                                            Ok(ct) => ct,
                                            Err(e) => {
                                                let _ = output.send(Message::FileFailed(e.to_string())).await;
                                                continue;
                                            }
                                        };
                                        let blob_id = veil_core::BlobId(
                                            *blake3::hash(&ciphertext).as_bytes(),
                                        );
                                        let ciphertext_len = ciphertext.len() as u64;

                                        // Store full encrypted blob locally (never lose it)
                                        let _ = store.store_blob_full(&blob_id, &ciphertext);

                                        // Erasure-code into shards and store them too
                                        if let Ok((shards, _)) = veil_store::encode_blob(&file_data, &group_key) {
                                            for shard in &shards {
                                                let _ = store.store_blob_shard(shard);
                                            }
                                            // Broadcast shards to peers
                                            for shard in shards {
                                                let _ = manager.broadcast(&WireMessage::BlobShard(shard)).await;
                                            }
                                        }

                                        // Send the message referencing the blob (no inline data)
                                        let content = MessageContent {
                                            kind: MessageKind::File {
                                                blob_id,
                                                filename: filename.clone(),
                                                size_bytes,
                                                ciphertext_len,
                                                inline_data: None,
                                            },
                                            timestamp: chrono::Utc::now(),
                                            channel_id: ChannelId::new(),
                                        };
                                        let identity = veil_crypto::Identity::from_bytes(&id_bytes);
                                        match SealedMessage::seal(&content, &group_key, &group_id.0, &identity) {
                                            Ok(sealed) => {
                                                let wire_msg = WireMessage::MessagePush(sealed.clone());
                                                manager.broadcast(&wire_msg).await;
                                                if let Some(ref rc) = relay_client {
                                                    if let Ok(payload) = wire_msg.encode() {
                                                        let _ = rc.forward_message(sealed.routing_tag, payload).await;
                                                    }
                                                }
                                                if let Err(e) = store.store_message(&sealed) {
                                        tracing::warn!("failed to persist message: {e}");
                                    }
                                                let _ = output.send(Message::FileSent { filename }).await;
                                            }
                                            Err(e) => {
                                                let _ = output.send(Message::FileFailed(e.to_string())).await;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = output.send(Message::FileFailed(format!("read error: {e}"))).await;
                                }
                            }
                        }
                        Some(NetCommand::SendPresence(wire_msg)) => {
                            manager.broadcast(&wire_msg).await;
                        }
                        Some(NetCommand::RequestBlob { blob_id }) => {
                            // Broadcast BlobFullRequest to all peers
                            manager.broadcast(&WireMessage::BlobFullRequest { blob_id }).await;
                        }
                        Some(NetCommand::BlobResponse { conn_id, blob_id, data }) => {
                            let _ = manager.send_to(
                                conn_id,
                                &WireMessage::BlobFull { blob_id, data },
                            ).await;
                        }
                        None => break,
                    }
                }
            }
        }
    })
}

impl App {
    pub fn theme(&self) -> Theme {
        match self.theme_choice {
            ThemeChoice::Dark => Theme::Dark,
            ThemeChoice::Light => Theme::Light,
        }
    }

    /// Convenience: get the master PeerId.
    fn master_peer_id(&self) -> PeerId {
        self.master.as_ref().unwrap().peer_id()
    }

    /// Build the list of known master PeerIds for message verification.
    fn known_master_ids(&self) -> Vec<PeerId> {
        let mut ids = vec![self.master_peer_id()];
        ids.extend(
            self.connected_peers
                .iter()
                .map(|(_, pid)| pid.clone()),
        );
        ids
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if !matches!(self.screen, Screen::Chat | Screen::Settings) {
            return Subscription::none();
        }

        let Some(device) = self.device.as_ref() else {
            return Subscription::none();
        };
        let peer_id = device.device_peer_id();
        let identity_bytes = device.device_key_bytes();
        let device_cert = Some(device.certificate().clone());
        let groups: Vec<GroupId> = self.groups.iter().map(|g| g.id.clone()).collect();
        let blob_store = self.store.clone();
        Subscription::run_with_id(
            "veil-network",
            network_worker(peer_id, identity_bytes, groups, device_cert, blob_store),
        )
    }

    fn setup_after_identity(&mut self) {
        let Some(master) = self.master.as_ref() else {
            tracing::error!("setup_after_identity called without master identity");
            return;
        };

        // Open message store — derive from master key bytes
        let data_dir = veil_data_dir();
        std::fs::create_dir_all(&data_dir).ok();
        let storage_key = LocalStore::derive_storage_key(&master.to_bytes());
        match LocalStore::open(&data_dir.join("messages.db"), storage_key) {
            Ok(store) => {
                self.store = Some(Arc::new(store));
            }
            Err(e) => {
                tracing::warn!("Failed to open message store: {e}");
            }
        }

        let master_id = master.peer_id().verifying_key.clone();

        // Try to load v2 groups first, then fall back to v1 + migration
        if let Some(ref store) = self.store {
            let loaded = match store.list_groups_v2() {
                Ok(v2_groups) if !v2_groups.is_empty() => {
                    self.groups = v2_groups
                        .into_iter()
                        .map(|(id, name, keyring)| GroupState {
                            name,
                            id: GroupId(id),
                            key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                            device_certs: Vec::new(),
                        })
                        .collect();
                    true
                }
                _ => false,
            };

            if !loaded {
                // Try v1 groups and migrate
                match store.list_groups() {
                    Ok(v1_groups) if !v1_groups.is_empty() => {
                        self.groups = v1_groups
                            .into_iter()
                            .map(|(id, name, key)| {
                                let keyring = GroupKeyRing::new(key, master_id.clone());
                                // Re-save as v2
                                let _ = store.store_group_v2(&id, &name, &keyring);
                                GroupState {
                                    name,
                                    id: GroupId(id),
                                    key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                                    device_certs: Vec::new(),
                                }
                            })
                            .collect();
                    }
                    _ => {}
                }
            }

            self.current_group = self.groups.first().cloned();

            // Load device certs from store into group state
            if let Ok(certs) = store.list_device_certs() {
                for group in &mut self.groups {
                    group.device_certs = certs.clone();
                }
            }
        }

        // If no groups loaded, create a default one
        if self.groups.is_empty() {
            let group_key = GroupKey::generate();
            let peer_id = master.peer_id();
            let group_id_bytes = blake3::derive_key(
                "veil-group-id",
                &bincode::serialize(&("My Group", &peer_id)).unwrap_or_default(),
            );

            let keyring = GroupKeyRing::new(group_key, master_id);

            // Persist the default group
            if let Some(ref store) = self.store {
                let _ = store.store_group_v2(&group_id_bytes, "My Group", &keyring);
            }

            let group_state = GroupState {
                name: "My Group".into(),
                id: GroupId(group_id_bytes),
                key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                device_certs: Vec::new(),
            };

            self.groups = vec![group_state.clone()];
            self.current_group = Some(group_state);
        }

        self.channels = vec!["general".into(), "random".into()];
        self.current_channel = Some("general".into());

        // Load display names from store
        if let Some(ref store) = self.store {
            if let Ok(names) = store.list_display_names() {
                for (fp, name) in names {
                    self.display_names.insert(fp, name);
                }
            }
            // Load settings
            if let Ok(Some(theme)) = store.get_setting("theme") {
                self.theme_choice = if theme == "light" {
                    ThemeChoice::Light
                } else {
                    ThemeChoice::Dark
                };
            }
            if let Ok(Some(notif)) = store.get_setting("notifications") {
                self.notifications_enabled = notif == "true";
            }
            if let Ok(Some(relay)) = store.get_setting("relay_addr") {
                self.relay_addr_input = relay;
            }
        }

        // Load message history for the current group
        self.load_message_history();
    }

    /// Load message history from the store for the current group.
    fn load_message_history(&mut self) {
        self.load_message_history_with_limit(self.messages_loaded);
    }

    fn load_message_history_with_limit(&mut self, limit: usize) {
        let Some(ref store) = self.store else { return };
        let Some(ref group) = self.current_group else { return };

        let routing_tag = routing_tag_for_group(&group.id.0);
        match store.list_messages_by_tag(&routing_tag, limit, 0) {
            Ok(sealed_messages) => {
                let members = self.known_master_ids();
                let ring = match group.key_ring.lock() {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!("key ring lock poisoned: {e}");
                        return;
                    }
                };

                for sealed in &sealed_messages {
                    if let Ok((content, sender)) =
                        sealed.verify_and_open_with_keyring(&ring, &members, &group.device_certs)
                    {
                        match content.kind {
                            MessageKind::Text(ref txt) => {
                                let mut cm = ChatMessage::user_with_channel(
                                    sealed.id.clone(),
                                    sender,
                                    txt.clone(),
                                    content.timestamp,
                                    content.channel_id.clone(),
                                );
                                cm.sender = self.resolve_display_name_str(&cm.sender);
                                self.messages.push(cm);
                            }
                            MessageKind::Reply { ref parent_id, content: ref reply_content } => {
                                if let MessageKind::Text(ref txt) = **reply_content {
                                    let mut cm = ChatMessage::user_with_channel(
                                        sealed.id.clone(),
                                        sender,
                                        txt.clone(),
                                        content.timestamp,
                                        content.channel_id.clone(),
                                    );
                                    // Find parent message for reply context
                                    if let Some(parent) = self.messages.iter().find(|m| m.id.as_ref() == Some(parent_id)) {
                                        cm.reply_to_content = Some(parent.content.clone());
                                        cm.reply_to_sender = Some(parent.sender.clone());
                                    }
                                    cm.sender = self.resolve_display_name_str(&cm.sender);
                                    self.messages.push(cm);
                                }
                            }
                            MessageKind::Reaction { ref target_id, ref emoji } => {
                                let key = target_id.0;
                                self.reactions.entry(key).or_default().push((sender, emoji.clone()));
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load message history: {e}");
            }
        }
    }

    /// Resolve a fingerprint to display name, or return the fingerprint.
    fn resolve_display_name(&self, peer_id: &PeerId) -> String {
        let fp = peer_id.fingerprint();
        self.display_names.get(&fp).cloned().unwrap_or(fp)
    }

    fn resolve_display_name_str(&self, fingerprint: &str) -> String {
        self.display_names.get(fingerprint).cloned().unwrap_or_else(|| fingerprint.to_string())
    }

    /// Send a desktop notification for an incoming message.
    fn send_notification(&self, sender: &str, content: &str) {
        if !self.notifications_enabled {
            return;
        }
        let preview = if content.len() > 100 {
            format!("{}...", &content[..97])
        } else {
            content.to_string()
        };
        let _ = notify_rust::Notification::new()
            .summary(&format!("Veil - {sender}"))
            .body(&preview)
            .show();
    }

    /// Handle a control message received from a peer.
    fn handle_control_message(&mut self, ctrl: ControlMessage, sender: PeerId) {
        match ctrl {
            ControlMessage::KeyRotation { epoch, key_packages } => {
                match &epoch.reason {
                    veil_crypto::EpochReason::ScheduledRotation => {
                        // All members derive the new key independently
                        if let Some(ref group) = self.current_group {
                            let Ok(mut ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                            ring.rotate_forward(sender.verifying_key.clone());

                            if let Some(ref store) = self.store {
                                let _ = store.store_group_v2(&group.id.0, &group.name, &ring);
                            }
                        }
                        self.messages.push(ChatMessage::system(format!("Key rotated (epoch {})", epoch.epoch)));
                    }
                    veil_crypto::EpochReason::Eviction { .. } => {
                        // Find our KeyPackage and decrypt the new key
                        if let Some(ref group) = self.current_group {
                            let our_master_id = self.master_peer_id().verifying_key;
                            if let Some(pkg) = key_packages
                                .iter()
                                .find(|p| p.recipient_master_id == our_master_id)
                            {
                                let eph = veil_crypto::EphemeralKeyPair::generate();
                                let peer_pub = x25519_dalek::PublicKey::from(pkg.ephemeral_public);
                                if let Ok(new_key) = GroupKey::decrypt_from_peer(
                                    &pkg.encrypted_key,
                                    eph,
                                    &peer_pub,
                                ) {
                                    let Ok(mut ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                                    ring.apply_eviction(new_key, epoch.clone());

                                    if let Some(ref store) = self.store {
                                        let _ = store.store_group_v2(
                                            &group.id.0,
                                            &group.name,
                                            &ring,
                                        );
                                    }
                                }
                            }
                        }
                        self.messages.push(ChatMessage::system(format!("Member evicted, key rotated (epoch {})", epoch.epoch)));
                    }
                    veil_crypto::EpochReason::Genesis => {
                        // Genesis — nothing to do
                    }
                }
            }
            ControlMessage::DeviceAnnouncement { certificate } => {
                if certificate.verify() {
                    // Store the certificate
                    if let Some(ref store) = self.store {
                        let _ = store.store_device_cert(&certificate.device_id, &certificate);
                    }

                    // Add to all groups' device_certs
                    for group in &mut self.groups {
                        if !group
                            .device_certs
                            .iter()
                            .any(|c| c.device_id == certificate.device_id)
                        {
                            group.device_certs.push(certificate.clone());
                        }
                    }

                    self.messages.push(ChatMessage::system(format!(
                        "Device '{}' announced by {}",
                        certificate.device_name,
                        certificate.master_id.fingerprint()
                    )));
                }
            }
            ControlMessage::DeviceRevoked { revocation } => {
                if revocation.verify() {
                    // Remove the revoked device cert from all groups
                    for group in &mut self.groups {
                        group
                            .device_certs
                            .retain(|c| c.device_id != revocation.revoked_device_id);
                    }

                    self.messages.push(ChatMessage::system(format!(
                        "Device revoked by {}",
                        revocation.master_id.fingerprint()
                    )));
                }
            }
            ControlMessage::MemberAdded {
                member_id,
                display_name,
                invited_by,
            } => {
                // Store the display name from the control message
                let fp = member_id.fingerprint();
                if !display_name.is_empty() {
                    self.display_names.insert(fp.clone(), display_name.clone());
                    if let Some(ref store) = self.store {
                        let _ = store.store_display_name(&fp, &display_name);
                    }
                }
                let invited_by_name = self.resolve_display_name(&invited_by);
                self.messages.push(ChatMessage::system(format!(
                    "{display_name} was added by {invited_by_name}",
                )));
            }
            ControlMessage::MemberRemoved {
                member_id,
                removed_by,
            } => {
                self.messages.push(ChatMessage::system(format!(
                    "{} was removed by {}",
                    member_id.fingerprint(),
                    removed_by.fingerprint()
                )));
            }
            ControlMessage::MetadataUpdate { field, value } => {
                let desc = match field {
                    veil_core::MetadataField::GroupName => format!("Group renamed to '{value}'"),
                    veil_core::MetadataField::GroupDescription => {
                        format!("Group description updated")
                    }
                    veil_core::MetadataField::ChannelAdded { name, .. } => {
                        format!("Channel #{name} added")
                    }
                    veil_core::MetadataField::ChannelRemoved { name } => {
                        format!("Channel #{name} removed")
                    }
                };
                self.messages.push(ChatMessage::system(desc));
            }
        }
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::CreateIdentity => {
                // Generate master identity + device
                let (master, phrase) = MasterIdentity::generate();
                let device_name = hostname::get()
                    .ok()
                    .and_then(|h| h.into_string().ok())
                    .unwrap_or_else(|| "Unknown Device".into());
                let device = DeviceIdentity::new(&master, device_name);

                self.master = Some(master);
                self.device = Some(device);

                // Show recovery phrase — identity will be saved after confirmation
                self.screen = Screen::ShowRecoveryPhrase(phrase);
            }
            Message::ConfirmRecoveryPhrase => {
                // User confirmed the recovery phrase — save identity and go to chat
                let data_dir = veil_data_dir();
                std::fs::create_dir_all(&data_dir).ok();
                let keystore = data_dir.join("identity.veil");

                let (Some(master), Some(device)) = (self.master.as_ref(), self.device.as_ref()) else {
                    return;
                };

                if let Err(e) = veil_crypto::save_device_identity(
                    master.entropy(),
                    device,
                    self.passphrase_input.as_bytes(),
                    &keystore,
                ) {
                    self.connection_state = ConnectionState::Failed(format!("Failed to save identity: {e}"));
                }

                // Zeroize passphrase after use
                self.passphrase_input.zeroize();

                self.screen = Screen::Chat;
                self.setup_after_identity();
            }
            Message::LoadIdentity => {
                let data_dir = veil_data_dir();
                let keystore = data_dir.join("identity.veil");

                // Try v2 format first
                match veil_crypto::load_device_identity(
                    self.passphrase_input.as_bytes(),
                    &keystore,
                ) {
                    Ok((master, device)) => {
                        self.master = Some(master);
                        self.device = Some(device);
                        self.passphrase_input.zeroize();
                        self.screen = Screen::Chat;
                        self.setup_after_identity();
                    }
                    Err(_) => {
                        // Try v1 format and migrate
                        let device_name = hostname::get()
                            .ok()
                            .and_then(|h| h.into_string().ok())
                            .unwrap_or_else(|| "Unknown Device".into());

                        match veil_crypto::migrate_v1_to_v2(
                            self.passphrase_input.as_bytes(),
                            &keystore,
                            device_name,
                        ) {
                            Ok((master, device, phrase)) => {
                                self.master = Some(master);
                                self.device = Some(device);
                                self.passphrase_input.zeroize();
                                // Show recovery phrase for migration
                                self.screen = Screen::ShowRecoveryPhrase(phrase);
                            }
                            Err(e) => {
                                self.passphrase_input.zeroize();
                                self.connection_state = ConnectionState::Failed(format!("Failed to load: {e}"));
                            }
                        }
                    }
                }
            }
            Message::PassphraseChanged(value) => {
                self.passphrase_input = value;
            }
            Message::InputChanged(value) => {
                // Send typing indicator
                let presence = self.current_group.as_ref().map(|g| {
                    let kind = if value.is_empty() {
                        veil_net::PresenceKind::StoppedTyping
                    } else {
                        veil_net::PresenceKind::Typing
                    };
                    WireMessage::Presence {
                        kind,
                        group_id: g.id.clone(),
                        sender: self.master_peer_id(),
                    }
                });
                if let (Some(wire_msg), Some(tx)) = (presence, &mut self.net_cmd_tx) {
                    let _ = tx.try_send(NetCommand::SendPresence(wire_msg));
                }
                self.message_input = value;
            }
            Message::Send => {
                if !self.message_input.trim().is_empty() {
                    let Some(device) = self.device.as_ref() else { return };
                    let fingerprint = self.resolve_display_name(&self.master_peer_id());

                    if let Some(ref group) = self.current_group {
                        // Build message kind: plain text or reply
                        let kind = if let Some(reply_idx) = self.replying_to.take() {
                            if let Some(reply_msg) = self.messages.get(reply_idx) {
                                if let Some(ref parent_id) = reply_msg.id {
                                    MessageKind::Reply {
                                        parent_id: parent_id.clone(),
                                        content: Box::new(MessageKind::Text(self.message_input.clone())),
                                    }
                                } else {
                                    MessageKind::Text(self.message_input.clone())
                                }
                            } else {
                                MessageKind::Text(self.message_input.clone())
                            }
                        } else {
                            MessageKind::Text(self.message_input.clone())
                        };

                        // Derive deterministic channel ID from group + channel name
                        let channel_id = self.current_channel_id(group);

                        let content = MessageContent {
                            kind,
                            timestamp: chrono::Utc::now(),
                            channel_id: channel_id.clone(),
                        };

                        let Ok(ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                        match SealedMessage::seal(
                            &content,
                            ring.current(),
                            &group.id.0,
                            device.identity(),
                        ) {
                            Ok(sealed) => {
                                drop(ring);
                                let mut cm = ChatMessage {
                                    id: Some(sealed.id.clone()),
                                    sender: fingerprint,
                                    sender_id: Some(self.master_peer_id()),
                                    content: self.message_input.clone(),
                                    timestamp: chrono::Utc::now().format("%H:%M").to_string(),
                                    datetime: Some(chrono::Utc::now()),
                                    edited: false,
                                    deleted: false,
                                    status: Some(MessageStatus::Sending),
                                    reply_to_content: None,
                                    reply_to_sender: None,
                                    channel_id: Some(channel_id),
                                    file_info: None,
                                };

                                // If this was a reply, attach context
                                if let Some(reply_idx) = self.messages.len().checked_sub(1).and_then(|_| {
                                    // Check if we had a reply target from the message kind
                                    if let MessageKind::Reply { ref parent_id, .. } = content.kind {
                                        self.messages.iter().position(|m| m.id.as_ref() == Some(parent_id))
                                    } else {
                                        None
                                    }
                                }) {
                                    let parent = &self.messages[reply_idx];
                                    cm.reply_to_content = Some(parent.content.clone());
                                    cm.reply_to_sender = Some(parent.sender.clone());
                                }

                                self.messages.push(cm);

                                // Send to connected peers + relay, or queue if unavailable
                                let sent = if let Some(ref mut tx) = self.net_cmd_tx {
                                    tx.try_send(NetCommand::SendMessage(sealed.clone())).is_ok()
                                } else {
                                    false
                                };

                                if sent {
                                    // Mark as sent
                                    if let Some(last) = self.messages.last_mut() {
                                        last.status = Some(MessageStatus::Sent);
                                    }
                                } else {
                                    // Queue for later delivery
                                    self.pending_messages.push(sealed.clone());
                                }

                                // Persist
                                if let Some(ref store) = self.store {
                                    if let Err(e) = store.store_message(&sealed) {
                                        tracing::warn!("failed to persist message: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                drop(ring);
                                self.messages.push(ChatMessage::system(format!("encrypt error: {e}")));
                            }
                        }
                    }

                    self.message_input.clear();
                }
            }
            Message::SelectGroup(name) => {
                self.current_group = self.groups.iter().find(|g| g.name == name).cloned();
                // Clear unread count for this group
                if let Some(ref group) = self.current_group {
                    self.unread_counts.remove(&group.id.0);
                }
                // Load message history for the newly selected group
                self.messages.clear();
                self.load_message_history();
            }
            Message::SelectChannel(name) => {
                self.current_channel = Some(name);
            }
            Message::ConnectInputChanged(value) => {
                self.connect_input = value;
            }
            Message::ConnectToPeer => {
                if let Ok(addr) = self.connect_input.parse::<SocketAddr>() {
                    self.connection_state = ConnectionState::Connecting(format!("Connecting to {addr}..."));
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::Connect(addr));
                    }
                } else {
                    self.connection_state = ConnectionState::Failed("Invalid address (use host:port)".into());
                }
                self.connect_input.clear();
            }
            Message::NetworkReady {
                local_addr,
                cmd_tx,
            } => {
                self.local_addr = Some(local_addr);
                self.net_cmd_tx = Some(cmd_tx);
                self.connection_state = ConnectionState::Connected(format!("Listening on {local_addr}"));

                // Flush pending messages
                let pending: Vec<SealedMessage> = self.pending_messages.drain(..).collect();
                for sealed in pending {
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::SendMessage(sealed));
                    }
                }
                // Update status of pending messages to Sent
                for msg in &mut self.messages {
                    if msg.status == Some(MessageStatus::Sending) {
                        msg.status = Some(MessageStatus::Sent);
                    }
                }

                // Auto-connect to relay if we have a saved address
                if !self.relay_addr_input.is_empty() {
                    if let Ok(addr) = self.relay_addr_input.parse::<SocketAddr>() {
                        if let Some(ref mut tx) = self.net_cmd_tx {
                            let _ = tx.try_send(NetCommand::ConnectRelay(addr));
                        }
                    }
                }
            }
            Message::PeerConnected {
                conn_id,
                peer_id,
                session_key,
                device_certificate,
            } => {
                // Use the DH-derived session key
                let group_key = GroupKey::from_storage_key(session_key);

                let our_key = &self.master_peer_id().verifying_key;
                let their_key = &peer_id.verifying_key;
                let group_id_bytes = {
                    let (first, second) = if our_key <= their_key {
                        (our_key.as_slice(), their_key.as_slice())
                    } else {
                        (their_key.as_slice(), our_key.as_slice())
                    };
                    let mut combined = Vec::with_capacity(first.len() + second.len());
                    combined.extend_from_slice(first);
                    combined.extend_from_slice(second);
                    blake3::derive_key("veil-pairwise-group-id", &combined)
                };

                let master_id = self.master_peer_id().verifying_key.clone();
                let keyring = GroupKeyRing::new(group_key, master_id);

                let mut device_certs = Vec::new();
                if let Some(cert) = device_certificate {
                    if cert.verify() {
                        // Store peer's device cert
                        if let Some(ref store) = self.store {
                            let _ = store.store_device_cert(&cert.device_id, &cert);
                        }
                        device_certs.push(cert);
                    }
                }

                let group_state = GroupState {
                    name: format!("Chat with {}", peer_id.fingerprint()),
                    id: GroupId(group_id_bytes),
                    key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                    device_certs,
                };

                self.groups.push(group_state.clone());
                self.current_group = Some(group_state);

                self.connected_peers
                    .push((conn_id, peer_id.clone()));
                self.connection_state =
                    ConnectionState::Connected(format!("Connected to {}", peer_id.fingerprint()));
            }
            Message::PeerDisconnected { conn_id } => {
                self.connected_peers.retain(|(id, _)| *id != conn_id);
                self.connection_state = ConnectionState::Disconnected;
            }
            Message::PeerData { sealed, .. } => {
                // Try all groups to find the one that can decrypt
                let mut decrypted = false;
                let members = self.known_master_ids();

                for group in &self.groups {
                    let Ok(ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            continue;
                        };
                    if let Ok((content, sender)) = sealed.verify_and_open_with_keyring(
                        &ring,
                        &members,
                        &group.device_certs,
                    ) {
                        drop(ring);

                        // Track unread for non-active groups
                        let is_active = self.current_group.as_ref()
                            .is_some_and(|g| g.id == group.id);
                        let is_own = sender == self.master_peer_id();

                        if !is_active && !is_own {
                            *self.unread_counts.entry(group.id.0).or_insert(0) += 1;
                        }

                        let sender_name = self.resolve_display_name(&sender);

                        match content.kind {
                            MessageKind::Text(ref txt) => {
                                let mut cm = ChatMessage::user_with_channel(
                                    sealed.id.clone(),
                                    sender.clone(),
                                    txt.clone(),
                                    content.timestamp,
                                    content.channel_id.clone(),
                                );
                                cm.sender = sender_name.clone();
                                self.messages.push(cm);

                                // Desktop notification for messages from others
                                if !is_own {
                                    self.send_notification(&sender_name, txt);
                                }
                            }
                            MessageKind::Reply { ref parent_id, content: ref reply_content } => {
                                if let MessageKind::Text(ref txt) = **reply_content {
                                    let mut cm = ChatMessage::user_with_channel(
                                        sealed.id.clone(),
                                        sender.clone(),
                                        txt.clone(),
                                        content.timestamp,
                                        content.channel_id.clone(),
                                    );
                                    cm.sender = sender_name.clone();
                                    // Find parent message for reply context
                                    if let Some(parent) = self.messages.iter().find(|m| m.id.as_ref() == Some(parent_id)) {
                                        cm.reply_to_content = Some(parent.content.clone());
                                        cm.reply_to_sender = Some(parent.sender.clone());
                                    }
                                    self.messages.push(cm);

                                    if !is_own {
                                        self.send_notification(&sender_name, txt);
                                    }
                                }
                            }
                            MessageKind::Reaction { ref target_id, ref emoji } => {
                                let key = target_id.0;
                                self.reactions.entry(key).or_default().push((sender.clone(), emoji.clone()));
                            }
                            MessageKind::Edit { target_id, new_text } => {
                                if let Some(msg) = self.messages.iter_mut().find(|m| {
                                    m.id.as_ref() == Some(&target_id)
                                        && m.sender_id.as_ref() == Some(&sender)
                                }) {
                                    msg.content = new_text;
                                    msg.edited = true;
                                }
                            }
                            MessageKind::Delete { target_id } => {
                                // Find the target message and mark deleted (only if same sender)
                                if let Some(msg) = self.messages.iter_mut().find(|m| {
                                    m.id.as_ref() == Some(&target_id)
                                        && m.sender_id.as_ref() == Some(&sender)
                                }) {
                                    msg.deleted = true;
                                    msg.content = "[deleted]".into();
                                }
                            }
                            MessageKind::Control(ctrl) => {
                                self.handle_control_message(ctrl, sender);
                            }
                            MessageKind::File {
                                ref filename,
                                size_bytes,
                                ref inline_data,
                                ref blob_id,
                                ciphertext_len,
                                ..
                            } => {
                                let size_str = if size_bytes < 1024 {
                                    format!("{size_bytes} B")
                                } else if size_bytes < 1_048_576 {
                                    format!("{:.1} KB", size_bytes as f64 / 1024.0)
                                } else {
                                    format!("{:.1} MB", size_bytes as f64 / 1_048_576.0)
                                };

                                let file_status = if inline_data.is_some() {
                                    // Small file — data is right here
                                    if let (Some(data), Some(store)) =
                                        (inline_data, &self.store)
                                    {
                                        if let Err(e) = store.store_blob_full(blob_id, data) {
                                            tracing::warn!("failed to store inline blob: {e}");
                                        }
                                    }
                                    FileStatus::Available
                                } else {
                                    // Large file — need to fetch blob
                                    let have_blob = self
                                        .store
                                        .as_ref()
                                        .and_then(|s| s.get_blob_full(blob_id).ok())
                                        .flatten()
                                        .is_some();

                                    if have_blob {
                                        FileStatus::Available
                                    } else {
                                        // Try to reconstruct from shards
                                        let reconstructed = self.store.as_ref().and_then(|store| {
                                            let shards = store.list_blob_shards(blob_id).ok()?;
                                            if shards.len() >= veil_store::blob::DATA_SHARDS {
                                                let shard_opts: Vec<Option<veil_store::BlobShard>> =
                                                    (0..veil_store::blob::TOTAL_SHARDS)
                                                        .map(|i| {
                                                            shards
                                                                .iter()
                                                                .find(|s| s.shard_index == i as u8)
                                                                .cloned()
                                                        })
                                                        .collect();
                                                for group in &self.groups {
                                                    if let Ok(ring) = group.key_ring.lock() {
                                                        if let Ok(data) = veil_store::decode_blob(
                                                            &shard_opts,
                                                            ciphertext_len as usize,
                                                            ring.current(),
                                                        ) {
                                                            return Some(data);
                                                        }
                                                    }
                                                }
                                                None
                                            } else {
                                                None
                                            }
                                        });

                                        if reconstructed.is_some() {
                                            FileStatus::Available
                                        } else {
                                            // Request full blob from peers via BlobFullRequest
                                            if let Some(ref mut tx) = self.net_cmd_tx {
                                                let _ = tx.try_send(NetCommand::RequestBlob {
                                                    blob_id: blob_id.clone(),
                                                });
                                            }
                                            FileStatus::Downloading
                                        }
                                    }
                                };

                                let mut cm = ChatMessage::user(
                                    sealed.id.clone(),
                                    sender.clone(),
                                    format!("[file: {filename} ({size_str})]"),
                                    content.timestamp,
                                );
                                cm.file_info = Some(FileInfo {
                                    blob_id: blob_id.clone(),
                                    filename: filename.clone(),
                                    size_str: size_str.clone(),
                                    status: file_status,
                                });
                                self.messages.push(cm);
                            }
                            _ => {
                                // Image, Video, etc. — display placeholder
                                self.messages.push(ChatMessage::user(sealed.id.clone(), sender.clone(), "[unsupported message type]".into(), content.timestamp));
                            }
                        }
                        decrypted = true;
                        break;
                    }
                }

                if !decrypted {
                    self.messages.push(ChatMessage::system("Failed to decrypt message".into()));
                }

                // Persist incoming message
                if let Some(ref store) = self.store {
                    if let Err(e) = store.store_message(&sealed) {
                                        tracing::warn!("failed to persist message: {e}");
                                    }
                }
            }
            Message::ConnectionFailed(err) => {
                self.connection_state = ConnectionState::Failed(format!("Error: {err}"));
            }
            // Relay events
            Message::RelayConnected => {
                self.relay_connected = true;
                self.connection_state = ConnectionState::Connected("Relay connected".into());

                // Flush pending messages on relay connect
                let pending: Vec<SealedMessage> = self.pending_messages.drain(..).collect();
                for sealed in pending {
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::SendMessage(sealed));
                    }
                }
                for msg in &mut self.messages {
                    if msg.status == Some(MessageStatus::Sending) {
                        msg.status = Some(MessageStatus::Sent);
                    }
                }
            }
            Message::RelayDisconnected(_reason) => {
                self.relay_connected = false;
                self.connection_state = ConnectionState::Reconnecting;
            }
            Message::RelayError { code, message } => {
                self.connection_state = ConnectionState::Warning(
                    format!("Relay: {code} — {message}")
                );
                self.messages.push(ChatMessage::system(
                    format!("Relay warning: [{code}] {message}")
                ));
            }
            // Relay UI
            Message::RelayAddrChanged(value) => {
                self.relay_addr_input = value;
            }
            Message::ConnectToRelay => {
                if let Ok(addr) = self.relay_addr_input.parse::<SocketAddr>() {
                    self.connection_state = ConnectionState::Connecting(format!("Connecting to relay {addr}..."));
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::ConnectRelay(addr));
                    }
                    // Persist relay address
                    if let Some(ref store) = self.store {
                        let _ = store.store_setting("relay_addr", &self.relay_addr_input);
                    }
                } else {
                    self.connection_state = ConnectionState::Failed("Invalid relay address (use host:port)".into());
                }
            }
            // Invite UI
            Message::InvitePassphraseChanged(value) => {
                self.invite_passphrase = value;
            }
            Message::CreateInvite => {
                if let Some(ref group) = self.current_group {
                    let relay_addr = if self.relay_addr_input.is_empty() {
                        "localhost:4433".into()
                    } else {
                        self.relay_addr_input.clone()
                    };
                    let Ok(ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                    let current_key = Arc::new(ring.current().duplicate());
                    drop(ring);
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::CreateInvite {
                            group_id: group.id.clone(),
                            group_name: group.name.clone(),
                            relay_addr,
                            passphrase: self.invite_passphrase.clone(),
                            group_key: current_key,
                        });
                    }
                }
            }
            Message::InviteCreated(url) => {
                self.generated_invite_url = Some(url);
            }
            Message::InviteInputChanged(value) => {
                self.invite_input = value;
            }
            Message::AcceptInvite => {
                if !self.invite_input.is_empty() {
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::AcceptInvite {
                            url: self.invite_input.clone(),
                            passphrase: self.invite_passphrase.clone(),
                        });
                    }
                    // Zeroize invite passphrase after use
                    self.invite_passphrase.zeroize();
                }
            }
            Message::InviteAccepted {
                group_name,
                group_id,
                group_key,
            } => {
                let master_id = self.master_peer_id().verifying_key.clone();
                let (key_bytes, generation) = group_key.0.to_raw_parts();
                let keyring =
                    GroupKeyRing::new(GroupKey::from_raw_parts(key_bytes, generation), master_id);

                let group_state = GroupState {
                    name: group_name,
                    id: group_id,
                    key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                    device_certs: Vec::new(),
                };

                // Persist the new group as v2
                if let Some(ref store) = self.store {
                    let Ok(ring) = group_state.key_ring.lock() else {
                        tracing::error!("key ring lock poisoned");
                        return;
                    };
                    let _ = store.store_group_v2(
                        &group_state.id.0,
                        &group_state.name,
                        &ring,
                    );
                }

                self.groups.push(group_state.clone());
                self.current_group = Some(group_state);
                self.invite_input.clear();
                self.connection_state = ConnectionState::Connected("Invite accepted!".into());
            }
            Message::InviteFailed(err) => {
                self.connection_state = ConnectionState::Failed(format!("Invite failed: {err}"));
            }
            // File handling
            Message::PickFile => {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.update(Message::SendFile(path));
                }
            }
            Message::SendFile(path) => {
                if let Some(ref group) = self.current_group {
                    let Some(device) = self.device.as_ref() else { return };
                    let Ok(ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                    let group_key = Arc::new(ring.current().duplicate());
                    drop(ring);

                    if let Some(ref store) = self.store {
                        if let Some(ref mut tx) = self.net_cmd_tx {
                            let _ = tx.try_send(NetCommand::SendFile {
                                path,
                                group_id: group.id.clone(),
                                group_key,
                                store: store.clone(),
                                identity_bytes: device.device_key_bytes(),
                            });
                        }
                    }
                }
            }
            Message::FileSent { filename } => {
                self.messages.push(ChatMessage {
                    id: None,
                    sender: self.resolve_display_name(&self.master_peer_id()),
                    sender_id: Some(self.master_peer_id()),
                    content: format!("Sent file: {filename}"),
                    timestamp: chrono::Utc::now().format("%H:%M").to_string(),
                    datetime: Some(chrono::Utc::now()),
                    edited: false,
                    deleted: false,
                    status: Some(MessageStatus::Sent),
                    reply_to_content: None,
                    reply_to_sender: None,
                    channel_id: None,
                    file_info: None,
                });
            }
            Message::FileFailed(err) => {
                self.messages.push(ChatMessage::system(format!("File send failed: {err}")));
            }
            Message::BlobRequested { conn_id, blob_id } => {
                // Look up the full blob in our store and send it back
                if let Some(ref store) = self.store {
                    if let Ok(Some(data)) = store.get_blob_full(&blob_id) {
                        if let Some(ref mut tx) = self.net_cmd_tx {
                            let _ = tx.try_send(NetCommand::BlobResponse {
                                conn_id,
                                blob_id,
                                data,
                            });
                        }
                    }
                }
            }
            Message::BlobReceived { blob_id } => {
                // Find matching file messages and flip status to Available
                for msg in &mut self.messages {
                    if let Some(ref mut fi) = msg.file_info {
                        if fi.blob_id == blob_id && fi.status == FileStatus::Downloading {
                            fi.status = FileStatus::Available;
                            msg.content = format!("[file: {} ({})]", fi.filename, fi.size_str);
                        }
                    }
                }
            }
            Message::SaveFile(blob_id, filename) => {
                if let Some(ref store) = self.store {
                    match store.get_blob_full(&blob_id) {
                        Ok(Some(encrypted_data)) => {
                            // Decrypt with group key
                            let decrypted = self.current_group.as_ref().and_then(|group| {
                                let ring = group.key_ring.lock().ok()?;
                                ring.current().decrypt(&encrypted_data).ok()
                            });

                            match decrypted {
                                Some(data) => {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_file_name(&filename)
                                        .save_file()
                                    {
                                        if let Err(e) = std::fs::write(&path, &data) {
                                            self.messages.push(ChatMessage::system(
                                                format!("Failed to save file: {e}")
                                            ));
                                        } else {
                                            self.messages.push(ChatMessage::system(
                                                format!("File saved to {}", path.display())
                                            ));
                                        }
                                    }
                                }
                                None => {
                                    self.messages.push(ChatMessage::system(
                                        "Failed to decrypt file".into()
                                    ));
                                }
                            }
                        }
                        Ok(None) => {
                            self.messages.push(ChatMessage::system(
                                "File data not available".into()
                            ));
                        }
                        Err(e) => {
                            self.messages.push(ChatMessage::system(
                                format!("Failed to load file: {e}")
                            ));
                        }
                    }
                }
            }
            // Edit/Delete
            Message::EditMessage(idx) => {
                if idx < self.messages.len() {
                    let msg = &self.messages[idx];
                    // Only allow editing own messages
                    if msg.sender_id.as_ref() == Some(&self.master_peer_id()) && !msg.deleted {
                        self.editing_message = Some(idx);
                        self.message_input = msg.content.clone();
                    }
                }
            }
            Message::CancelEdit => {
                self.editing_message = None;
                self.message_input.clear();
            }
            Message::ConfirmEdit => {
                if let Some(idx) = self.editing_message.take() {
                    if idx < self.messages.len() && !self.message_input.trim().is_empty() {
                        let msg = &self.messages[idx];
                        if let Some(ref msg_id) = msg.id {
                            // Send edit as a new sealed message
                            if let Some(ref group) = self.current_group {
                                let Some(device) = self.device.as_ref() else { return };
                                let content = MessageContent {
                                    kind: MessageKind::Edit {
                                        target_id: msg_id.clone(),
                                        new_text: self.message_input.clone(),
                                    },
                                    timestamp: chrono::Utc::now(),
                                    channel_id: ChannelId::new(),
                                };
                                let Ok(ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                                if let Ok(sealed) = SealedMessage::seal(
                                    &content,
                                    ring.current(),
                                    &group.id.0,
                                    device.identity(),
                                ) {
                                    drop(ring);
                                    if let Some(ref mut tx) = self.net_cmd_tx {
                                        let _ = tx.try_send(NetCommand::SendMessage(sealed.clone()));
                                    }
                                    if let Some(ref store) = self.store {
                                        if let Err(e) = store.store_message(&sealed) {
                                        tracing::warn!("failed to persist message: {e}");
                                    }
                                    }
                                }
                            }
                            // Update local display
                            self.messages[idx].content = self.message_input.clone();
                            self.messages[idx].edited = true;
                        }
                    }
                    self.message_input.clear();
                }
            }
            Message::DeleteMessage(idx) => {
                if idx < self.messages.len() {
                    let msg = &self.messages[idx];
                    // Only allow deleting own messages
                    if msg.sender_id.as_ref() == Some(&self.master_peer_id()) {
                        if let Some(ref msg_id) = msg.id {
                            // Send delete as a new sealed message
                            if let Some(ref group) = self.current_group {
                                let Some(device) = self.device.as_ref() else { return };
                                let content = MessageContent {
                                    kind: MessageKind::Delete {
                                        target_id: msg_id.clone(),
                                    },
                                    timestamp: chrono::Utc::now(),
                                    channel_id: ChannelId::new(),
                                };
                                let Ok(ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                                if let Ok(sealed) = SealedMessage::seal(
                                    &content,
                                    ring.current(),
                                    &group.id.0,
                                    device.identity(),
                                ) {
                                    drop(ring);
                                    if let Some(ref mut tx) = self.net_cmd_tx {
                                        let _ = tx.try_send(NetCommand::SendMessage(sealed.clone()));
                                    }
                                    if let Some(ref store) = self.store {
                                        if let Err(e) = store.store_message(&sealed) {
                                        tracing::warn!("failed to persist message: {e}");
                                    }
                                    }
                                }
                            }
                            // Update local display
                            self.messages[idx].deleted = true;
                            self.messages[idx].content = "[deleted]".into();
                        }
                    }
                }
            }
            // Typing/Presence
            Message::TypingStarted { peer_id, .. } => {
                // Update or insert typing peer with current timestamp
                if let Some(entry) = self.typing_peers.iter_mut().find(|(p, _)| *p == peer_id) {
                    entry.1 = std::time::Instant::now();
                } else {
                    self.typing_peers.push((peer_id, std::time::Instant::now()));
                }
            }
            Message::TypingStopped { peer_id } => {
                self.typing_peers.retain(|(p, _)| *p != peer_id);
            }
            Message::ReadReceipt { .. } => {
                // Update delivered status for matching messages
            }
            // Phase 2: Display names
            Message::DisplayNameInputChanged(value) => {
                self.display_name_input = value;
            }
            Message::SetDisplayName => {
                if !self.display_name_input.trim().is_empty() {
                    let fp = self.master_peer_id().fingerprint();
                    let name = self.display_name_input.trim().to_string();
                    self.display_names.insert(fp.clone(), name.clone());
                    if let Some(ref store) = self.store {
                        let _ = store.store_display_name(&fp, &name);
                    }
                    self.display_name_input.clear();
                }
            }
            // Phase 3: Replies + reactions
            Message::ReplyTo(idx) => {
                if idx < self.messages.len() {
                    self.replying_to = Some(idx);
                }
            }
            Message::CancelReply => {
                self.replying_to = None;
            }
            Message::React(idx, emoji) => {
                if idx < self.messages.len() {
                    let msg = &self.messages[idx];
                    if let Some(ref msg_id) = msg.id {
                        // Send reaction as sealed message
                        if let Some(ref group) = self.current_group {
                            let Some(device) = self.device.as_ref() else { return };
                            let content = MessageContent {
                                kind: MessageKind::Reaction {
                                    target_id: msg_id.clone(),
                                    emoji: emoji.clone(),
                                },
                                timestamp: chrono::Utc::now(),
                                channel_id: self.current_channel_id(group),
                            };
                            let Ok(ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                            if let Ok(sealed) = SealedMessage::seal(
                                &content,
                                ring.current(),
                                &group.id.0,
                                device.identity(),
                            ) {
                                drop(ring);
                                if let Some(ref mut tx) = self.net_cmd_tx {
                                    let _ = tx.try_send(NetCommand::SendMessage(sealed.clone()));
                                }
                                if let Some(ref store) = self.store {
                                    if let Err(e) = store.store_message(&sealed) {
                                        tracing::warn!("failed to persist message: {e}");
                                    }
                                }
                            }
                        }
                        // Update local reactions
                        let key = msg_id.0;
                        let our_pid = self.master_peer_id();
                        self.reactions.entry(key).or_default().push((our_pid, emoji));
                    }
                }
            }
            // Phase 4: Search + discovery
            Message::ToggleSearch => {
                self.search_active = !self.search_active;
                if !self.search_active {
                    self.search_query.clear();
                    self.search_results.clear();
                }
            }
            Message::SearchQueryChanged(query) => {
                self.search_query = query;
                // Search in-memory messages by substring
                self.search_results = self
                    .messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| {
                        !m.deleted
                            && m.sender != "system"
                            && m.content
                                .to_lowercase()
                                .contains(&self.search_query.to_lowercase())
                    })
                    .map(|(i, _)| i)
                    .collect();
            }
            Message::ConnectDiscoveredPeer(addr) => {
                self.connection_state = ConnectionState::Connecting(format!("Connecting to LAN peer {addr}..."));
                if let Some(ref mut tx) = self.net_cmd_tx {
                    let _ = tx.try_send(NetCommand::Connect(addr));
                }
            }
            Message::LanPeerDiscovered {
                name,
                addr,
                fingerprint,
            } => {
                // Don't add ourselves
                let our_fp = self
                    .master
                    .as_ref()
                    .map(|m| m.peer_id().fingerprint())
                    .unwrap_or_default();
                if fingerprint != our_fp {
                    if !self.discovered_peers.iter().any(|(_, a, _)| *a == addr) {
                        self.discovered_peers
                            .push((name, addr, fingerprint));
                    }
                }
            }
            Message::LanPeerLost(name) => {
                self.discovered_peers.retain(|(n, _, _)| *n != name);
            }
            Message::LoadMoreMessages => {
                self.messages_loaded += 500;
                self.messages.clear();
                self.load_message_history();
            }
            // Phase 5: Settings
            Message::OpenSettings => {
                self.screen = Screen::Settings;
            }
            Message::CloseSettings => {
                self.screen = Screen::Chat;
            }
            Message::ToggleTheme => {
                self.theme_choice = match self.theme_choice {
                    ThemeChoice::Dark => ThemeChoice::Light,
                    ThemeChoice::Light => ThemeChoice::Dark,
                };
                if let Some(ref store) = self.store {
                    let theme_str = match self.theme_choice {
                        ThemeChoice::Dark => "dark",
                        ThemeChoice::Light => "light",
                    };
                    let _ = store.store_setting("theme", theme_str);
                }
            }
            Message::ToggleNotifications => {
                self.notifications_enabled = !self.notifications_enabled;
                if let Some(ref store) = self.store {
                    let _ = store.store_setting(
                        "notifications",
                        if self.notifications_enabled { "true" } else { "false" },
                    );
                }
            }
            Message::DeviceNameInputChanged(value) => {
                self.device_name_input = value;
            }
            Message::ExportIdentity => {
                // Show master fingerprint for copy
                if let Some(ref master) = self.master {
                    self.connection_state = ConnectionState::Connected(format!("Fingerprint: {}", master.peer_id().fingerprint()));
                }
            }
            // Keyboard shortcuts
            Message::EscapePressed => {
                if self.editing_message.is_some() {
                    self.editing_message = None;
                    self.message_input.clear();
                } else if self.replying_to.is_some() {
                    self.replying_to = None;
                } else if self.search_active {
                    self.search_active = false;
                    self.search_query.clear();
                    self.search_results.clear();
                }
            }
            Message::UpArrowPressed => {
                // Edit last own message if input is empty
                if self.message_input.is_empty() && self.editing_message.is_none() {
                    let our_id = self.master.as_ref().map(|m| m.peer_id());
                    if let Some(idx) = self.messages.iter().rposition(|m| {
                        m.sender_id.as_ref() == our_id.as_ref()
                            && m.id.is_some()
                            && !m.deleted
                    }) {
                        self.editing_message = Some(idx);
                        self.message_input = self.messages[idx].content.clone();
                    }
                }
            }
        }
    }

    /// Derive a deterministic ChannelId from group + channel name.
    fn current_channel_id(&self, group: &GroupState) -> ChannelId {
        let channel_name = self.current_channel.as_deref().unwrap_or("general");
        let derived = blake3::derive_key(
            "veil-channel-id",
            &[group.id.0.as_slice(), channel_name.as_bytes()].concat(),
        );
        let uuid_bytes: [u8; 16] = derived[..16].try_into().unwrap();
        ChannelId(::uuid::Uuid::from_bytes(uuid_bytes))
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::Setup => self.view_setup(),
            Screen::ShowRecoveryPhrase(phrase) => self.view_recovery_phrase(phrase),
            Screen::Chat => self.view_chat(),
            Screen::Settings => self.view_settings(),
        }
    }

    fn view_setup(&self) -> Element<'_, Message> {
        container(
            column![
                text("Veil").size(48),
                text("Encrypted. Decentralized. Yours.").size(16),
                text_input("Passphrase (optional)", &self.passphrase_input)
                    .on_input(Message::PassphraseChanged)
                    .secure(true)
                    .padding(8)
                    .width(300),
                row![
                    button("Create New")
                        .on_press(Message::CreateIdentity)
                        .padding(12),
                    button("Load Existing")
                        .on_press(Message::LoadIdentity)
                        .padding(12),
                ]
                .spacing(12),
                text(self.connection_state.to_string()).size(12),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    }

    fn view_recovery_phrase(&self, phrase: &str) -> Element<'_, Message> {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        let mut word_rows = Column::new().spacing(8);
        for (i, word) in words.iter().enumerate() {
            word_rows = word_rows.push(
                text(format!("{}. {}", i + 1, word)).size(18),
            );
        }

        container(
            column![
                text("Your Recovery Phrase").size(32),
                text("Write these 12 words down and store them safely.").size(14),
                text("You will need them to recover your identity.").size(14),
                container(word_rows.padding(16))
                    .padding(16),
                button("I have saved my recovery phrase")
                    .on_press(Message::ConfirmRecoveryPhrase)
                    .padding(12),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    }

    fn view_chat(&self) -> Element<'_, Message> {
        let sidebar = self.view_sidebar();
        let chat = self.view_messages();
        row![sidebar, chat].into()
    }

    fn view_sidebar(&self) -> Element<'_, Message> {
        let mut group_list = Column::new().spacing(4).padding(8);
        group_list = group_list.push(text("Groups").size(12));

        for group in &self.groups {
            let is_selected = self
                .current_group
                .as_ref()
                .is_some_and(|g| g.name == group.name);
            let unread = self.unread_counts.get(&group.id.0).copied().unwrap_or(0);
            let badge = if unread > 0 {
                format!(" ({})", unread)
            } else {
                String::new()
            };
            let label = if is_selected {
                text(format!("> {}{badge}", group.name)).size(14)
            } else {
                text(format!("  {}{badge}", group.name)).size(14)
            };
            group_list = group_list.push(
                button(label)
                    .on_press(Message::SelectGroup(group.name.clone()))
                    .width(Length::Fill)
                    .padding(4),
            );
        }

        let mut channel_list = Column::new().spacing(4).padding(8);
        channel_list = channel_list.push(text("Channels").size(12));

        for channel in &self.channels {
            let is_selected = self.current_channel.as_ref() == Some(channel);
            let label = if is_selected {
                text(format!("# {channel}")).size(14)
            } else {
                text(format!("  # {channel}")).size(14)
            };
            channel_list = channel_list.push(
                button(label)
                    .on_press(Message::SelectChannel(channel.clone()))
                    .width(Length::Fill)
                    .padding(2),
            );
        }

        // Members section
        let mut peers_section = Column::new().spacing(2).padding(8);
        peers_section = peers_section.push(text("Members").size(12));
        if let Some(ref master) = self.master {
            let our_name = self.resolve_display_name(&master.peer_id());
            peers_section = peers_section.push(
                text(format!("{our_name} (you)")).size(10),
            );
        }
        for (_, pid) in &self.connected_peers {
            let name = self.resolve_display_name(pid);
            let is_typing = self
                .typing_peers
                .iter()
                .any(|(p, t)| p == pid && t.elapsed() < std::time::Duration::from_secs(5));
            let status = if is_typing { " (typing)" } else { " (online)" };
            peers_section = peers_section.push(text(format!("{name}{status}")).size(10));
        }

        // Set display name
        let name_section = column![
            text("Display Name").size(12),
            row![
                text_input("Your name", &self.display_name_input)
                    .on_input(Message::DisplayNameInputChanged)
                    .on_submit(Message::SetDisplayName)
                    .padding(4)
                    .width(Length::Fill),
                button("Set").on_press(Message::SetDisplayName).padding(4),
            ].spacing(4),
        ]
        .spacing(4)
        .padding(8);

        // LAN Peers
        let mut lan_section = Column::new().spacing(2).padding(8);
        if !self.discovered_peers.is_empty() {
            lan_section = lan_section.push(text("LAN Peers").size(12));
            for (_, addr, fp) in &self.discovered_peers {
                let label = self.display_names.get(fp)
                    .cloned()
                    .unwrap_or_else(|| fp.clone());
                lan_section = lan_section.push(
                    button(text(format!("{label}")).size(10))
                        .on_press(Message::ConnectDiscoveredPeer(*addr))
                        .padding(2)
                        .width(Length::Fill),
                );
            }
        }

        // Connect-to-peer input
        let connect_section = column![
            text("Connect").size(12),
            text_input("host:port", &self.connect_input)
                .on_input(Message::ConnectInputChanged)
                .on_submit(Message::ConnectToPeer)
                .padding(4)
                .width(Length::Fill),
            text(self.connection_state.to_string()).size(10),
        ]
        .spacing(4)
        .padding(8);

        // Relay section
        let relay_status_text = if self.relay_connected {
            "Relay: connected"
        } else {
            "Relay: disconnected"
        };
        let relay_section = column![
            text("Relay").size(12),
            text_input("relay host:port", &self.relay_addr_input)
                .on_input(Message::RelayAddrChanged)
                .on_submit(Message::ConnectToRelay)
                .padding(4)
                .width(Length::Fill),
            button("Connect Relay")
                .on_press(Message::ConnectToRelay)
                .padding(4),
            text(relay_status_text).size(10),
        ]
        .spacing(4)
        .padding(8);

        // Invite section
        let mut invite_section = column![
            text("Invite").size(12),
            text_input("Passphrase", &self.invite_passphrase)
                .on_input(Message::InvitePassphraseChanged)
                .secure(true)
                .padding(4)
                .width(Length::Fill),
            button("Create Invite")
                .on_press(Message::CreateInvite)
                .padding(4),
        ]
        .spacing(4)
        .padding(8);

        if let Some(ref url) = self.generated_invite_url {
            invite_section = invite_section.push(
                text_input("Invite URL", url)
                    .padding(4)
                    .width(Length::Fill),
            );
        }

        invite_section = invite_section.push(
            text_input("Paste invite URL", &self.invite_input)
                .on_input(Message::InviteInputChanged)
                .on_submit(Message::AcceptInvite)
                .padding(4)
                .width(Length::Fill),
        );
        invite_section = invite_section.push(
            button("Join").on_press(Message::AcceptInvite).padding(4),
        );

        // Settings button at bottom
        let settings_button = container(
            button("Settings").on_press(Message::OpenSettings).padding(6).width(Length::Fill),
        )
        .padding(8);

        container(
            scrollable(
                column![
                    group_list,
                    channel_list,
                    peers_section,
                    name_section,
                    lan_section,
                    connect_section,
                    relay_section,
                    invite_section,
                    settings_button,
                ]
                .spacing(8)
                .width(220),
            )
        )
        .height(Length::Fill)
        .into()
    }

    fn view_messages(&self) -> Element<'_, Message> {
        let addr_str = self
            .local_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "starting...".into());

        let display_name = self
            .master
            .as_ref()
            .map(|m| self.resolve_display_name(&m.peer_id()))
            .unwrap_or_else(|| "???".into());

        let relay_indicator = if self.relay_connected { " | relay" } else { "" };
        let health_indicator = match &self.connection_state {
            ConnectionState::Connected(_) => "",
            ConnectionState::Warning(_) => " | [!]",
            ConnectionState::Reconnecting => " | [reconnecting]",
            ConnectionState::Failed(_) => " | [error]",
            ConnectionState::Connecting(_) => " | [connecting]",
            ConnectionState::Disconnected => "",
        };

        let header = row![
            text(
                self.current_channel
                    .as_deref()
                    .map(|c| format!("# {c}"))
                    .unwrap_or_default()
            )
            .size(20),
            horizontal_space(),
            text(format!("{display_name}  |  {addr_str}{relay_indicator}{health_indicator}"))
                .size(12),
        ]
        .padding(12);

        // Search bar (toggled by Ctrl+F or button)
        let search_bar = if self.search_active {
            Some(
                row![
                    text_input("Search messages...", &self.search_query)
                        .on_input(Message::SearchQueryChanged)
                        .padding(6)
                        .width(Length::Fill),
                    text(format!("{} matches", self.search_results.len())).size(11),
                    button(text("X").size(11))
                        .on_press(Message::ToggleSearch)
                        .padding(4),
                ]
                .spacing(8)
                .padding([0, 12]),
            )
        } else {
            None
        };

        // Derive current channel ID for filtering
        let current_channel_id = self.current_group.as_ref().map(|g| self.current_channel_id(g));

        let our_id = self.master.as_ref().map(|m| m.peer_id());
        let mut messages_col = Column::new().spacing(4).padding(12);
        let mut last_date: Option<String> = None;
        let mut last_sender: Option<String> = None;

        for (idx, msg) in self.messages.iter().enumerate() {
            // Channel filtering: skip messages from other channels
            if let (Some(msg_ch), Some(cur_ch)) = (&msg.channel_id, &current_channel_id) {
                if msg_ch != cur_ch {
                    continue;
                }
            }

            // If searching, only show matching messages
            if self.search_active && !self.search_query.is_empty() && !self.search_results.contains(&idx) {
                continue;
            }

            // Date separator
            if let Some(dt) = msg.datetime {
                let date_str = dt.format("%B %d, %Y").to_string();
                if last_date.as_ref() != Some(&date_str) {
                    messages_col = messages_col.push(
                        container(text(date_str.clone()).size(10))
                            .width(Length::Fill)
                            .center_x(Length::Fill)
                            .padding(8),
                    );
                    last_date = Some(date_str);
                    last_sender = None;
                }
            }

            // System messages - italic/muted style
            if msg.sender == "system" {
                messages_col = messages_col.push(
                    container(text(format!("-- {} --", msg.content)).size(11))
                        .width(Length::Fill)
                        .center_x(Length::Fill)
                        .padding(4),
                );
                last_sender = None;
                continue;
            }

            let show_sender = last_sender.as_ref() != Some(&msg.sender);
            last_sender = Some(msg.sender.clone());

            let mut msg_col = Column::new();

            if show_sender {
                msg_col = msg_col.push(text(&msg.sender).size(11));
            }

            // Reply context (quoted block above the message)
            if let (Some(reply_content), Some(reply_sender)) =
                (&msg.reply_to_content, &msg.reply_to_sender)
            {
                let preview = if reply_content.len() > 60 {
                    format!("{}...", &reply_content[..57])
                } else {
                    reply_content.clone()
                };
                msg_col = msg_col.push(
                    container(
                        text(format!("{reply_sender}: {preview}")).size(10),
                    )
                    .padding(4),
                );
            }

            // Message content with edit/delete/status indicators
            let content_widget: Element<'_, Message> = if let Some(ref fi) = msg.file_info {
                // Render file attachment widget
                let status_text = match fi.status {
                    FileStatus::Available => "Ready",
                    FileStatus::Downloading => "Downloading...",
                    FileStatus::Unavailable => "Unavailable",
                };
                let mut file_row = row![
                    text(format!("[{}] {} ({})", status_text, fi.filename, fi.size_str)).size(13),
                ]
                .spacing(8);
                if fi.status == FileStatus::Available {
                    file_row = file_row.push(
                        button(text("Save").size(11))
                            .on_press(Message::SaveFile(fi.blob_id.clone(), fi.filename.clone()))
                            .padding(4),
                    );
                }
                file_row.into()
            } else if msg.deleted {
                text("[deleted]").size(13).into()
            } else if msg.edited {
                text(format!("{} (edited)", msg.content)).size(13).into()
            } else {
                text(&msg.content).size(13).into()
            };

            // Send status indicator
            let status_str = match &msg.status {
                Some(MessageStatus::Sending) => " ...",
                Some(MessageStatus::Sent) => " ok",
                Some(MessageStatus::Delivered) => " ok",
                None => "",
            };

            let mut msg_row = row![
                msg_col.push(content_widget),
                horizontal_space(),
                text(format!("{}{status_str}", msg.timestamp)).size(9),
            ]
            .spacing(8);

            // Action buttons: reply, react, edit, delete
            let is_own = msg.sender_id.as_ref() == our_id.as_ref();
            if msg.id.is_some() && !msg.deleted {
                // Reply button for all messages
                msg_row = msg_row.push(
                    button(text("reply").size(9))
                        .on_press(Message::ReplyTo(idx))
                        .padding(2),
                );

                // Quick reaction buttons
                for emoji in &["\u{1F44D}", "\u{2764}", "\u{1F602}", "\u{1F525}", "\u{1F440}"] {
                    msg_row = msg_row.push(
                        button(text(*emoji).size(11))
                            .on_press(Message::React(idx, emoji.to_string()))
                            .padding(1),
                    );
                }

                if is_own {
                    msg_row = msg_row.push(
                        button(text("edit").size(9))
                            .on_press(Message::EditMessage(idx))
                            .padding(2),
                    );
                    msg_row = msg_row.push(
                        button(text("del").size(9))
                            .on_press(Message::DeleteMessage(idx))
                            .padding(2),
                    );
                }
            }

            messages_col = messages_col.push(msg_row);

            // Reaction badges below message
            if let Some(ref msg_id) = msg.id {
                if let Some(reactions) = self.reactions.get(&msg_id.0) {
                    if !reactions.is_empty() {
                        // Group reactions by emoji
                        let mut counts: HashMap<&str, usize> = HashMap::new();
                        for (_, emoji) in reactions {
                            *counts.entry(emoji.as_str()).or_insert(0) += 1;
                        }
                        let mut reaction_row = iced::widget::Row::new().spacing(4);
                        for (emoji, count) in &counts {
                            let label = if *count > 1 {
                                format!("{emoji} {count}")
                            } else {
                                emoji.to_string()
                            };
                            reaction_row = reaction_row.push(
                                container(text(label).size(11)).padding(2),
                            );
                        }
                        messages_col = messages_col.push(reaction_row);
                    }
                }
            }
        }

        // Typing indicator with display names
        for (peer, instant) in &self.typing_peers {
            if instant.elapsed() < std::time::Duration::from_secs(5) {
                let name = self.resolve_display_name(peer);
                messages_col = messages_col.push(
                    text(format!("{name} is typing...")).size(11),
                );
            }
        }

        // Reply preview bar above input
        let reply_bar = if let Some(reply_idx) = self.replying_to {
            if let Some(reply_msg) = self.messages.get(reply_idx) {
                let preview = if reply_msg.content.len() > 80 {
                    format!("{}...", &reply_msg.content[..77])
                } else {
                    reply_msg.content.clone()
                };
                Some(
                    row![
                        text(format!("Replying to {}: {preview}", reply_msg.sender)).size(11),
                        horizontal_space(),
                        button(text("X").size(11))
                            .on_press(Message::CancelReply)
                            .padding(2),
                    ]
                    .spacing(8)
                    .padding([4, 12]),
                )
            } else {
                None
            }
        } else {
            None
        };

        // Input row
        let input_row = if self.editing_message.is_some() {
            row![
                text_input("Edit message...", &self.message_input)
                    .on_input(Message::InputChanged)
                    .on_submit(Message::ConfirmEdit)
                    .padding(10)
                    .width(Length::Fill),
                button("Save").on_press(Message::ConfirmEdit).padding(10),
                button("Cancel").on_press(Message::CancelEdit).padding(10),
            ]
            .spacing(8)
            .padding(12)
        } else {
            row![
                text_input("Type a message...", &self.message_input)
                    .on_input(Message::InputChanged)
                    .on_submit(Message::Send)
                    .padding(10)
                    .width(Length::Fill),
                button("File").on_press(Message::PickFile).padding(10),
                button("Send").on_press(Message::Send).padding(10),
            ]
            .spacing(8)
            .padding(12)
        };

        let mut main_col = Column::new()
            .width(Length::Fill)
            .height(Length::Fill);

        main_col = main_col.push(header);
        if let Some(sb) = search_bar {
            main_col = main_col.push(sb);
        }
        main_col = main_col.push(
            scrollable(messages_col)
                .height(Length::Fill)
                .anchor_bottom(),
        );
        if let Some(rb) = reply_bar {
            main_col = main_col.push(rb);
        }
        main_col = main_col.push(input_row);

        main_col.into()
    }

    fn view_settings(&self) -> Element<'_, Message> {
        let master_fp = self
            .master
            .as_ref()
            .map(|m| m.peer_id().fingerprint())
            .unwrap_or_else(|| "???".into());

        let display_name = self
            .master
            .as_ref()
            .map(|m| self.resolve_display_name(&m.peer_id()))
            .unwrap_or_else(|| "Not set".into());

        let device_name = self
            .device
            .as_ref()
            .map(|d| d.certificate().device_name.clone())
            .unwrap_or_else(|| "Unknown".into());

        let theme_label = match self.theme_choice {
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
        };

        let notif_label = if self.notifications_enabled {
            "Enabled"
        } else {
            "Disabled"
        };

        let relay_display = if self.relay_addr_input.is_empty() {
            "Not configured".to_string()
        } else {
            self.relay_addr_input.clone()
        };

        container(
            column![
                row![
                    text("Settings").size(32),
                    horizontal_space(),
                    button("Back").on_press(Message::CloseSettings).padding(8),
                ]
                .padding(12),
                column![
                    text("Identity").size(18),
                    text(format!("Display Name: {display_name}")).size(14),
                    text(format!("Device: {device_name}")).size(14),
                    text(format!("Fingerprint: {master_fp}")).size(12),
                    button("Copy Fingerprint")
                        .on_press(Message::ExportIdentity)
                        .padding(6),
                ]
                .spacing(8)
                .padding(16),
                column![
                    text("Appearance").size(18),
                    row![
                        text(format!("Theme: {theme_label}")).size(14),
                        button("Toggle").on_press(Message::ToggleTheme).padding(6),
                    ]
                    .spacing(8),
                ]
                .spacing(8)
                .padding(16),
                column![
                    text("Notifications").size(18),
                    row![
                        text(format!("Desktop Notifications: {notif_label}")).size(14),
                        button("Toggle").on_press(Message::ToggleNotifications).padding(6),
                    ]
                    .spacing(8),
                ]
                .spacing(8)
                .padding(16),
                column![
                    text("Network").size(18),
                    text(format!("Relay: {relay_display}")).size(14),
                    text(format!(
                        "Connected Peers: {}",
                        self.connected_peers.len()
                    ))
                    .size(14),
                    text(format!(
                        "LAN Peers: {}",
                        self.discovered_peers.len()
                    ))
                    .size(14),
                ]
                .spacing(8)
                .padding(16),
                text(self.connection_state.to_string()).size(10).width(Length::Fill),
            ]
            .spacing(12)
            .width(Length::Fill),
        )
        .center(Length::Fill)
        .into()
    }
}
