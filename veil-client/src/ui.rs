use std::net::SocketAddr;
use std::sync::Arc;

use futures::StreamExt;
use iced::widget::{
    button, column, container, horizontal_space, row, scrollable, text, text_input, Column,
};
use iced::{Element, Length, Subscription, Theme};
use zeroize::Zeroize;

use veil_core::{
    ChannelId, GroupId, MessageContent, MessageDeduplicator, MessageKind, SealedMessage,
    invite::{self, InvitePayload},
    routing_tag_for_group,
};
use veil_crypto::{GroupKey, Identity, PeerId};
use veil_net::{ConnectionId, PeerEvent, PeerManager, RelayClient, RelayEvent, WireMessage, create_endpoint};
use veil_store::LocalStore;

pub struct App {
    identity: Identity,
    screen: Screen,
    // Chat state
    message_input: String,
    messages: Vec<ChatMessage>,
    // Group state
    current_group: Option<GroupState>,
    groups: Vec<GroupState>,
    current_channel: Option<String>,
    channels: Vec<String>,
    // Connection state
    connect_input: String,
    connection_status: String,
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
}

#[derive(Clone)]
struct GroupState {
    name: String,
    id: GroupId,
    group_key: std::sync::Arc<GroupKey>,
}

enum Screen {
    Setup,
    Chat,
}

struct ChatMessage {
    sender: String,
    content: String,
    timestamp: String,
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
}

#[derive(Debug, Clone)]
pub enum Message {
    // Setup
    CreateIdentity,
    LoadIdentity,
    PassphraseChanged(String),
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
}

impl Default for App {
    fn default() -> Self {
        Self {
            identity: Identity::generate(),
            screen: Screen::Setup,
            message_input: String::new(),
            messages: Vec::new(),
            current_group: None,
            groups: Vec::new(),
            current_channel: None,
            channels: Vec::new(),
            connect_input: String::new(),
            connection_status: String::new(),
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
        }
    }
}

fn veil_data_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".local/share/veil")
}

fn network_worker(
    peer_id: PeerId,
    identity_bytes: [u8; 32],
    groups: Vec<GroupId>,
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
        ));

        // Relay client state
        let mut relay_client: Option<RelayClient> = None;
        let mut relay_event_rx: Option<tokio::sync::mpsc::Receiver<RelayEvent>> = None;

        // Dedup to prevent duplicate messages from P2P + relay
        let mut dedup = MessageDeduplicator::with_capacity(2048);

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

            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(PeerEvent::Connected { conn_id, peer_id, session_key }) => {
                            let _ = output
                                .send(Message::PeerConnected { conn_id, peer_id, session_key })
                                .await;
                        }
                        Some(PeerEvent::Disconnected { conn_id }) => {
                            let _ = output
                                .send(Message::PeerDisconnected { conn_id })
                                .await;
                        }
                        Some(PeerEvent::Message { message, .. }) => {
                            if let WireMessage::MessagePush(sealed) = message {
                                if dedup.check(&sealed).is_ok() {
                                    let _ = output
                                        .send(Message::PeerData { sealed })
                                        .await;
                                }
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
                        None => break,
                    }
                }
            }
        }
    })
}

impl App {
    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if !matches!(self.screen, Screen::Chat) {
            return Subscription::none();
        }

        let peer_id = self.identity.peer_id();
        let identity_bytes = self.identity.to_bytes();
        let groups: Vec<GroupId> = self.groups.iter().map(|g| g.id.clone()).collect();
        Subscription::run_with_id(
            "veil-network",
            network_worker(peer_id, identity_bytes, groups),
        )
    }

    fn setup_after_identity(&mut self) {
        // Open message store
        let data_dir = veil_data_dir();
        std::fs::create_dir_all(&data_dir).ok();
        let storage_key = LocalStore::derive_storage_key(&self.identity.to_bytes());
        match LocalStore::open(&data_dir.join("messages.db"), storage_key) {
            Ok(store) => {
                self.store = Some(Arc::new(store));
            }
            Err(e) => {
                tracing::warn!("Failed to open message store: {e}");
            }
        }

        // Try to load persisted groups
        if let Some(ref store) = self.store {
            match store.list_groups() {
                Ok(persisted_groups) if !persisted_groups.is_empty() => {
                    self.groups = persisted_groups
                        .into_iter()
                        .map(|(id, name, key)| GroupState {
                            name,
                            id: GroupId(id),
                            group_key: Arc::new(key),
                        })
                        .collect();
                    self.current_group = self.groups.first().cloned();
                }
                _ => {
                    // No persisted groups — create a default one
                    let group_key = GroupKey::generate();
                    let peer_id = self.identity.peer_id();
                    let group_id_bytes = blake3::derive_key(
                        "veil-group-id",
                        &bincode::serialize(&("My Group", &peer_id)).unwrap_or_default(),
                    );

                    let group_state = GroupState {
                        name: "My Group".into(),
                        id: GroupId(group_id_bytes),
                        group_key: Arc::new(group_key),
                    };

                    // Persist the default group
                    if let Some(ref store) = self.store {
                        let _ = store.store_group(
                            &group_state.id.0,
                            &group_state.name,
                            &group_state.group_key,
                        );
                    }

                    self.groups = vec![group_state.clone()];
                    self.current_group = Some(group_state);
                }
            }
        } else {
            // No store available — create default group without persistence
            let group_key = GroupKey::generate();
            let peer_id = self.identity.peer_id();
            let group_id_bytes = blake3::derive_key(
                "veil-group-id",
                &bincode::serialize(&("My Group", &peer_id)).unwrap_or_default(),
            );

            let group_state = GroupState {
                name: "My Group".into(),
                id: GroupId(group_id_bytes),
                group_key: Arc::new(group_key),
            };

            self.groups = vec![group_state.clone()];
            self.current_group = Some(group_state);
        }

        self.channels = vec!["general".into(), "random".into()];
        self.current_channel = Some("general".into());

        // Load message history for the current group
        self.load_message_history();
    }

    /// Load message history from the store for the current group.
    fn load_message_history(&mut self) {
        let Some(ref store) = self.store else { return };
        let Some(ref group) = self.current_group else { return };

        let routing_tag = routing_tag_for_group(&group.id.0);
        match store.list_messages_by_tag(&routing_tag, 100, 0) {
            Ok(sealed_messages) => {
                let members: Vec<PeerId> = {
                    let mut m = vec![self.identity.peer_id()];
                    m.extend(
                        self.connected_peers
                            .iter()
                            .map(|(_, pid)| pid.clone()),
                    );
                    m
                };

                for sealed in &sealed_messages {
                    if let Ok((content, sender)) =
                        sealed.verify_and_open(&group.group_key, &members)
                    {
                        if let MessageKind::Text(ref txt) = content.kind {
                            self.messages.push(ChatMessage {
                                sender: sender.fingerprint(),
                                content: txt.clone(),
                                timestamp: content
                                    .timestamp
                                    .format("%H:%M")
                                    .to_string(),
                            });
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load message history: {e}");
            }
        }
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::CreateIdentity => {
                self.identity = Identity::generate();

                // Save identity to disk
                let data_dir = veil_data_dir();
                std::fs::create_dir_all(&data_dir).ok();
                let keystore = data_dir.join("identity.veil");
                if let Err(e) = veil_crypto::save_identity(
                    &self.identity,
                    self.passphrase_input.as_bytes(),
                    &keystore,
                ) {
                    self.connection_status = format!("Failed to save identity: {e}");
                }

                // Zeroize passphrase after use
                self.passphrase_input.zeroize();

                self.screen = Screen::Chat;
                self.setup_after_identity();
            }
            Message::LoadIdentity => {
                let data_dir = veil_data_dir();
                let keystore = data_dir.join("identity.veil");
                match veil_crypto::load_identity(
                    self.passphrase_input.as_bytes(),
                    &keystore,
                ) {
                    Ok(identity) => {
                        self.identity = identity;
                        // Zeroize passphrase after use
                        self.passphrase_input.zeroize();
                        self.screen = Screen::Chat;
                        self.setup_after_identity();
                    }
                    Err(e) => {
                        // Zeroize passphrase even on failure
                        self.passphrase_input.zeroize();
                        self.connection_status = format!("Failed to load: {e}");
                    }
                }
            }
            Message::PassphraseChanged(value) => {
                self.passphrase_input = value;
            }
            Message::InputChanged(value) => {
                self.message_input = value;
            }
            Message::Send => {
                if !self.message_input.trim().is_empty() {
                    let fingerprint = self.identity.peer_id().fingerprint();

                    if let Some(ref group) = self.current_group {
                        let content = MessageContent {
                            kind: MessageKind::Text(self.message_input.clone()),
                            timestamp: chrono::Utc::now(),
                            channel_id: ChannelId::new(),
                        };

                        match SealedMessage::seal(
                            &content,
                            &group.group_key,
                            &group.id.0,
                            &self.identity,
                        ) {
                            Ok(sealed) => {
                                self.messages.push(ChatMessage {
                                    sender: fingerprint,
                                    content: self.message_input.clone(),
                                    timestamp: chrono::Utc::now()
                                        .format("%H:%M")
                                        .to_string(),
                                });

                                // Send to connected peers + relay
                                if let Some(ref mut tx) = self.net_cmd_tx {
                                    let _ = tx.try_send(
                                        NetCommand::SendMessage(sealed.clone()),
                                    );
                                }

                                // Persist
                                if let Some(ref store) = self.store {
                                    let _ = store.store_message(&sealed);
                                }
                            }
                            Err(e) => {
                                self.messages.push(ChatMessage {
                                    sender: "system".into(),
                                    content: format!("encrypt error: {e}"),
                                    timestamp: chrono::Utc::now()
                                        .format("%H:%M")
                                        .to_string(),
                                });
                            }
                        }
                    }

                    self.message_input.clear();
                }
            }
            Message::SelectGroup(name) => {
                self.current_group = self.groups.iter().find(|g| g.name == name).cloned();
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
                    self.connection_status = format!("Connecting to {addr}...");
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::Connect(addr));
                    }
                } else {
                    self.connection_status = "Invalid address (use host:port)".into();
                }
                self.connect_input.clear();
            }
            Message::NetworkReady {
                local_addr,
                cmd_tx,
            } => {
                self.local_addr = Some(local_addr);
                self.net_cmd_tx = Some(cmd_tx);
                self.connection_status = format!("Listening on {local_addr}");
            }
            Message::PeerConnected {
                conn_id,
                peer_id,
                session_key,
            } => {
                // Use the DH-derived session key instead of broken public-key derivation
                let group_key = GroupKey::from_storage_key(session_key);

                let group_id_bytes = {
                    let our_key = &self.identity.peer_id().verifying_key;
                    let their_key = &peer_id.verifying_key;
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

                let group_state = GroupState {
                    name: format!("Chat with {}", peer_id.fingerprint()),
                    id: GroupId(group_id_bytes),
                    group_key: Arc::new(group_key),
                };

                self.groups.push(group_state.clone());
                self.current_group = Some(group_state);

                self.connected_peers
                    .push((conn_id, peer_id.clone()));
                self.connection_status =
                    format!("Connected to {}", peer_id.fingerprint());
            }
            Message::PeerDisconnected { conn_id } => {
                self.connected_peers.retain(|(id, _)| *id != conn_id);
                self.connection_status = "Peer disconnected".into();
            }
            Message::PeerData { sealed, .. } => {
                // Try all groups to find the one that can decrypt
                let mut decrypted = false;
                let members: Vec<PeerId> = {
                    let mut m = vec![self.identity.peer_id()];
                    m.extend(
                        self.connected_peers
                            .iter()
                            .map(|(_, pid)| pid.clone()),
                    );
                    m
                };

                for group in &self.groups {
                    if let Ok((content, sender)) =
                        sealed.verify_and_open(&group.group_key, &members)
                    {
                        if let MessageKind::Text(ref txt) = content.kind {
                            self.messages.push(ChatMessage {
                                sender: sender.fingerprint(),
                                content: txt.clone(),
                                timestamp: content
                                    .timestamp
                                    .format("%H:%M")
                                    .to_string(),
                            });
                        }
                        decrypted = true;
                        break;
                    }
                }

                if !decrypted {
                    self.messages.push(ChatMessage {
                        sender: "system".into(),
                        content: "Failed to decrypt message".into(),
                        timestamp: chrono::Utc::now()
                            .format("%H:%M")
                            .to_string(),
                    });
                }

                // Persist incoming message
                if let Some(ref store) = self.store {
                    let _ = store.store_message(&sealed);
                }
            }
            Message::ConnectionFailed(err) => {
                self.connection_status = format!("Error: {err}");
            }
            // Relay events
            Message::RelayConnected => {
                self.relay_connected = true;
                self.connection_status = "Relay connected".into();
            }
            Message::RelayDisconnected(reason) => {
                self.relay_connected = false;
                self.connection_status = format!("Relay disconnected: {reason}");
            }
            // Relay UI
            Message::RelayAddrChanged(value) => {
                self.relay_addr_input = value;
            }
            Message::ConnectToRelay => {
                if let Ok(addr) = self.relay_addr_input.parse::<SocketAddr>() {
                    self.connection_status = format!("Connecting to relay {addr}...");
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::ConnectRelay(addr));
                    }
                } else {
                    self.connection_status = "Invalid relay address (use host:port)".into();
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
                    if let Some(ref mut tx) = self.net_cmd_tx {
                        let _ = tx.try_send(NetCommand::CreateInvite {
                            group_id: group.id.clone(),
                            group_name: group.name.clone(),
                            relay_addr,
                            passphrase: self.invite_passphrase.clone(),
                            group_key: group.group_key.clone(),
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
                let group_state = GroupState {
                    name: group_name,
                    id: group_id,
                    group_key: group_key.0,
                };

                // Persist the new group
                if let Some(ref store) = self.store {
                    let _ = store.store_group(
                        &group_state.id.0,
                        &group_state.name,
                        &group_state.group_key,
                    );
                }

                self.groups.push(group_state.clone());
                self.current_group = Some(group_state);
                self.invite_input.clear();
                self.connection_status = "Invite accepted!".into();
            }
            Message::InviteFailed(err) => {
                self.connection_status = format!("Invite failed: {err}");
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self.screen {
            Screen::Setup => self.view_setup(),
            Screen::Chat => self.view_chat(),
        }
    }

    fn view_setup(&self) -> Element<'_, Message> {
        let fingerprint = self.identity.peer_id().fingerprint();

        container(
            column![
                text("Veil").size(48),
                text("Encrypted. Decentralized. Yours.").size(16),
                text(format!("Your identity: {fingerprint}")).size(14),
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
                text(&self.connection_status).size(12),
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

        for group in &self.groups {
            let is_selected = self
                .current_group
                .as_ref()
                .is_some_and(|g| g.name == group.name);
            let label = if is_selected {
                text(format!("> {}", group.name)).size(14)
            } else {
                text(group.name.as_str()).size(14)
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

        // Peers section
        let mut peers_section = Column::new().spacing(2).padding(8);
        peers_section = peers_section.push(text("Peers").size(12));
        for (_, pid) in &self.connected_peers {
            peers_section =
                peers_section.push(text(pid.fingerprint()).size(10));
        }

        // Connect-to-peer input
        let connect_section = column![
            text("Connect").size(12),
            text_input("host:port", &self.connect_input)
                .on_input(Message::ConnectInputChanged)
                .on_submit(Message::ConnectToPeer)
                .padding(4)
                .width(Length::Fill),
            text(&self.connection_status).size(10),
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

        container(
            column![
                group_list,
                channel_list,
                peers_section,
                connect_section,
                relay_section,
                invite_section,
            ]
            .spacing(16)
            .width(220),
        )
        .height(Length::Fill)
        .into()
    }

    fn view_messages(&self) -> Element<'_, Message> {
        let addr_str = self
            .local_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "starting...".into());

        let header = row![
            text(
                self.current_channel
                    .as_deref()
                    .map(|c| format!("# {c}"))
                    .unwrap_or_default()
            )
            .size(20),
            horizontal_space(),
            text(format!(
                "{}  |  {}",
                self.identity.peer_id().fingerprint(),
                addr_str,
            ))
            .size(12),
        ]
        .padding(12);

        let mut messages = Column::new().spacing(8).padding(12);
        for msg in &self.messages {
            messages = messages.push(
                row![
                    text(&msg.sender).size(12).width(140),
                    text(&msg.content).size(14),
                    horizontal_space(),
                    text(&msg.timestamp).size(10),
                ]
                .spacing(8),
            );
        }

        let input_row = row![
            text_input("Type a message...", &self.message_input)
                .on_input(Message::InputChanged)
                .on_submit(Message::Send)
                .padding(10)
                .width(Length::Fill),
            button("Send").on_press(Message::Send).padding(10),
        ]
        .spacing(8)
        .padding(12);

        column![header, scrollable(messages).height(Length::Fill), input_row]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
