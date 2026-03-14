use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use iced::{Subscription, Theme};

use veil_core::{ChannelId, GroupId, SealedMessage};
use veil_crypto::{DeviceIdentity, MasterIdentity, PeerId};
use veil_net::ConnectionId;
use veil_store::LocalStore;

use super::message::{Message, NetCommand};
use super::network::network_worker;
use super::types::*;

pub struct App {
    pub(crate) master: Option<MasterIdentity>,
    pub(crate) device: Option<DeviceIdentity>,
    pub(crate) screen: Screen,
    // Chat state
    pub(crate) message_input: String,
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) editing_message: Option<usize>,
    // Group state
    pub(crate) current_group: Option<GroupState>,
    pub(crate) groups: Vec<GroupState>,
    pub(crate) current_channel: Option<String>,
    pub(crate) channels: Vec<String>,
    // Connection state
    pub(crate) connect_input: String,
    pub(crate) connection_state: ConnectionState,
    // Network state
    pub(crate) net_cmd_tx: Option<futures::channel::mpsc::Sender<NetCommand>>,
    pub(crate) connected_peers: Vec<(ConnectionId, PeerId)>,
    pub(crate) local_addr: Option<SocketAddr>,
    // Identity persistence
    pub(crate) passphrase_input: String,
    // Message persistence
    pub(crate) store: Option<Arc<LocalStore>>,
    // Relay state
    pub(crate) relay_addr_input: String,
    pub(crate) relay_connected: bool,
    // Invite state
    pub(crate) invite_passphrase: String,
    pub(crate) invite_input: String,
    pub(crate) generated_invite_url: Option<String>,
    // Presence state
    pub(crate) typing_peers: Vec<(PeerId, std::time::Instant)>,
    // Phase 1: Message reliability
    pub(crate) pending_messages: Vec<SealedMessage>,
    // Phase 2: Display names + notifications
    pub(crate) display_names: HashMap<String, String>,
    pub(crate) display_name_input: String,
    pub(crate) unread_counts: HashMap<[u8; 32], usize>,
    pub(crate) notifications_enabled: bool,
    // Phase 3: Replies + reactions
    pub(crate) replying_to: Option<usize>,
    pub(crate) reactions: HashMap<[u8; 32], Vec<(PeerId, String)>>,
    // Phase 4: LAN discovery + search
    pub(crate) search_query: String,
    pub(crate) search_active: bool,
    pub(crate) search_results: Vec<usize>,
    pub(crate) discovered_peers: Vec<(String, SocketAddr, String)>,
    pub(crate) messages_loaded: usize,
    // Phase 5: Settings + visual polish
    pub(crate) theme_choice: ThemeChoice,
    pub(crate) device_name_input: String,
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

impl App {
    pub fn theme(&self) -> Theme {
        match self.theme_choice {
            ThemeChoice::Dark => Theme::Dark,
            ThemeChoice::Light => Theme::Light,
        }
    }

    /// Convenience: get the master PeerId.
    /// # Panics
    /// Panics if called before identity is set (only used after setup).
    pub(crate) fn master_peer_id(&self) -> PeerId {
        self.master
            .as_ref()
            .expect("master identity not set")
            .peer_id()
    }

    /// Build the list of known master PeerIds for message verification.
    pub(crate) fn known_master_ids(&self) -> Vec<PeerId> {
        let mut ids = vec![self.master_peer_id()];
        ids.extend(self.connected_peers.iter().map(|(_, pid)| pid.clone()));
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

    /// Resolve a fingerprint to display name, or return the fingerprint.
    pub(crate) fn resolve_display_name(&self, peer_id: &PeerId) -> String {
        let fp = peer_id.fingerprint();
        self.display_names.get(&fp).cloned().unwrap_or(fp)
    }

    pub(crate) fn resolve_display_name_str(&self, fingerprint: &str) -> String {
        self.display_names
            .get(fingerprint)
            .cloned()
            .unwrap_or_else(|| fingerprint.to_string())
    }

    /// Send a desktop notification for an incoming message.
    pub(crate) fn send_notification(&self, sender: &str, content: &str) {
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

    /// Derive a deterministic ChannelId from group + channel name.
    pub(crate) fn current_channel_id(&self, group: &GroupState) -> ChannelId {
        let channel_name = self.current_channel.as_deref().unwrap_or("general");
        let derived = blake3::derive_key(
            "veil-channel-id",
            &[group.id.0.as_slice(), channel_name.as_bytes()].concat(),
        );
        let uuid_bytes: [u8; 16] = derived[..16]
            .try_into()
            .expect("blake3 output is always 32 bytes");
        ChannelId(::uuid::Uuid::from_bytes(uuid_bytes))
    }
}
