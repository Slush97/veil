use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use esox_gfx::{GpuContext, RenderResources};
use esox_ui::{InputState, Rect, UiState, Ui, TextRenderer};
use esox_platform::{AppDelegate, MouseInputEvent};
use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use veil_core::{ChannelId, GroupId, SealedMessage};
use veil_crypto::{DeviceIdentity, MasterIdentity, PeerId};
use veil_net::ConnectionId;
use veil_store::LocalStore;

use super::message::{NetworkEvent, NetCommand};
use super::network::spawn_network_worker;
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
    pub(crate) net_cmd_tx: Option<tokio::sync::mpsc::Sender<NetCommand>>,
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
    // Username registry + contacts
    pub(crate) username_input: String,
    pub(crate) username: Option<String>,
    pub(crate) registration_status: Option<String>,
    pub(crate) contacts: Vec<(String, [u8; 32])>,
    pub(crate) contact_search_input: String,
    pub(crate) contact_search_result: Option<ContactSearchResult>,
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
            // Username registry + contacts
            username_input: String::new(),
            username: None,
            registration_status: None,
            contacts: Vec::new(),
            contact_search_input: String::new(),
            contact_search_result: None,
        }
    }
}

/// Default relay address.
pub(crate) const DEFAULT_RELAY: &str = "127.0.0.1:4433";

impl App {
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
    /// Includes: self, P2P-connected peers, and directory contacts.
    pub(crate) fn known_master_ids(&self) -> Vec<PeerId> {
        let mut ids = vec![self.master_peer_id()];
        ids.extend(self.connected_peers.iter().map(|(_, pid)| pid.clone()));
        // Add contacts — their public keys are master verifying keys from the directory
        for (_, public_key) in &self.contacts {
            ids.push(PeerId {
                verifying_key: public_key.to_vec(),
            });
        }
        ids
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

    /// Seal a message, send it via the network channel, and persist to the store.
    /// Returns `Some(sealed)` on success, `None` if device/group/keyring is unavailable.
    pub(crate) fn seal_send_persist(
        &mut self,
        content: &veil_core::MessageContent,
    ) -> Option<SealedMessage> {
        let device = self.device.as_ref()?;
        let master = self.master.as_ref()?;
        let group = self.current_group.as_ref()?;
        let ring = match group.key_ring.lock() {
            Ok(r) => r,
            Err(_) => {
                tracing::error!("key ring lock poisoned");
                return None;
            }
        };
        // DM groups (name starts with @) use master identity for signing so the
        // recipient can verify against the directory public key without needing
        // a device certificate exchange. Regular groups use device identity.
        let is_dm = group.name.starts_with('@');
        let signing_identity: veil_crypto::Identity;
        let identity_ref = if is_dm {
            signing_identity = veil_crypto::Identity::from_bytes(&master.to_bytes());
            &signing_identity
        } else {
            device.identity()
        };
        let sealed = match SealedMessage::seal(content, ring.current(), &group.id.0, identity_ref) {
            Ok(s) => s,
            Err(e) => {
                drop(ring);
                self.messages
                    .push(ChatMessage::system(format!("encrypt error: {e}")));
                return None;
            }
        };
        drop(ring);
        if let Some(ref tx) = self.net_cmd_tx
            && let Err(e) = tx.try_send(super::message::NetCommand::SendMessage(sealed.clone())) {
                tracing::warn!("failed to send message: {e}");
            }
        if let Some(ref store) = self.store
            && let Err(e) = store.store_message(&sealed) {
                tracing::warn!("failed to persist message: {e}");
            }
        Some(sealed)
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

    /// Handle a network event from the worker.
    pub(crate) fn handle_network_event(&mut self, event: NetworkEvent) {
        match event {
            // Network lifecycle
            NetworkEvent::NetworkReady { local_addr, cmd_tx } => {
                self.update_network_ready(local_addr, cmd_tx);
            }
            NetworkEvent::PeerConnected { conn_id, peer_id, session_key, device_certificate } => {
                self.update_peer_connected(conn_id, peer_id, session_key, device_certificate);
            }
            NetworkEvent::PeerDisconnected { conn_id } => {
                self.connected_peers.retain(|(id, _)| *id != conn_id);
                self.connection_state = ConnectionState::Disconnected;
            }
            NetworkEvent::PeerData { sealed } => self.update_peer_data(sealed),
            NetworkEvent::ConnectionFailed(err) => {
                self.connection_state = ConnectionState::Failed(format!("Error: {err}"));
            }

            // Relay
            NetworkEvent::RelayConnected => self.update_relay_connected(),
            NetworkEvent::RelayDisconnected(_reason) => self.update_relay_disconnected(),
            NetworkEvent::RelayError { code, message } => self.update_relay_error(code, message),

            // Invites
            NetworkEvent::InviteCreated(url) => {
                self.generated_invite_url = Some(url);
            }
            NetworkEvent::InviteAccepted { group_name, group_id, group_key } => {
                self.update_invite_accepted(group_name, group_id, group_key);
            }
            NetworkEvent::InviteFailed(err) => {
                self.connection_state = ConnectionState::Failed(format!("Invite failed: {err}"));
            }

            // Files
            NetworkEvent::FileSent { filename } => self.update_file_sent(filename),
            NetworkEvent::FileFailed(err) => self.update_file_failed(err),
            NetworkEvent::BlobRequested { conn_id, blob_id } => {
                self.update_blob_requested(conn_id, blob_id);
            }
            NetworkEvent::BlobReceived { blob_id } => self.update_blob_received(blob_id),

            // Social
            NetworkEvent::TypingStarted { peer_id, .. } => self.update_typing_started(peer_id),
            NetworkEvent::TypingStopped { peer_id } => self.update_typing_stopped(peer_id),
            NetworkEvent::ReadReceipt { .. } => {}

            // Username registry + contacts
            NetworkEvent::RegisterResult { success, message } => {
                self.update_register_result(success, message);
            }
            NetworkEvent::ContactFound { username, public_key } => {
                self.update_contact_found(username, public_key);
            }
            NetworkEvent::ContactNotFound(username) => {
                self.update_contact_not_found(username);
            }

            // LAN discovery
            NetworkEvent::LanPeerDiscovered { name, addr, fingerprint } => {
                self.update_lan_peer_discovered(name, addr, fingerprint);
            }
            NetworkEvent::LanPeerLost(name) => {
                self.discovered_peers.retain(|(n, _, _)| *n != name);
            }
        }
    }
}

// ─── VeilApp: wraps App with esox_ui state + tokio runtime ───

pub struct VeilApp {
    pub(crate) app: App,
    pub(crate) ui_state: UiState,
    pub(crate) text_renderer: Option<TextRenderer>,
    pub(crate) theme: esox_ui::Theme,
    pub(crate) tokio_rt: tokio::runtime::Runtime,
    pub(crate) net_event_rx: Option<tokio::sync::mpsc::Receiver<NetworkEvent>>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) network_spawned: bool,
    // File picker result channel
    pub(crate) file_pick_rx: Option<std::sync::mpsc::Receiver<std::path::PathBuf>>,

    // Input states for each text field
    pub(crate) input_username: InputState,
    pub(crate) input_passphrase: InputState,
    pub(crate) input_message: InputState,
    pub(crate) input_connect: InputState,
    pub(crate) input_relay_addr: InputState,
    pub(crate) input_invite_passphrase: InputState,
    pub(crate) input_invite_url: InputState,
    pub(crate) input_display_name: InputState,
    pub(crate) input_search: InputState,
    pub(crate) input_contact_search: InputState,
    pub(crate) input_device_name: InputState,
}

impl VeilApp {
    pub fn new() -> Self {
        let tokio_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime");

        Self {
            app: App::default(),
            ui_state: UiState::new(),
            text_renderer: None,
            theme: esox_ui::Theme::dark(),
            tokio_rt,
            net_event_rx: None,
            width: 1200,
            height: 800,
            network_spawned: false,
            file_pick_rx: None,
            input_username: InputState::default(),
            input_passphrase: InputState::default(),
            input_message: InputState::default(),
            input_connect: InputState::default(),
            input_relay_addr: InputState::default(),
            input_invite_passphrase: InputState::default(),
            input_invite_url: InputState::default(),
            input_display_name: InputState::default(),
            input_search: InputState::default(),
            input_contact_search: InputState::default(),
            input_device_name: InputState::default(),
        }
    }

    /// Spawn the network worker if identity is ready and not already spawned.
    pub(crate) fn maybe_spawn_network(&mut self) {
        if self.network_spawned {
            return;
        }
        if !matches!(self.app.screen, Screen::Chat | Screen::Settings) {
            return;
        }
        let Some(device) = self.app.device.as_ref() else {
            return;
        };

        let peer_id = device.device_peer_id();
        let identity_bytes = device.device_key_bytes();
        let device_cert = Some(device.certificate().clone());
        let groups: Vec<GroupId> = self.app.groups.iter().map(|g| g.id.clone()).collect();
        let blob_store = self.app.store.clone();

        let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
        self.net_event_rx = Some(event_rx);
        self.network_spawned = true;

        self.tokio_rt.spawn(spawn_network_worker(
            peer_id,
            identity_bytes,
            groups,
            device_cert,
            blob_store,
            event_tx,
        ));
    }

    /// Drain all pending network events.
    fn drain_network_events(&mut self) {
        if let Some(ref mut rx) = self.net_event_rx {
            while let Ok(event) = rx.try_recv() {
                self.app.handle_network_event(event);
            }
        }
    }

    /// Drain file picker results.
    fn drain_file_picks(&mut self) {
        if let Some(ref rx) = self.file_pick_rx {
            while let Ok(path) = rx.try_recv() {
                self.app.update_send_file(path);
            }
        }
    }

    /// Sync InputState text → App string fields after widget drawing.
    pub(crate) fn sync_inputs_to_app(&mut self) {
        self.app.username_input = self.input_username.text.clone();
        self.app.passphrase_input = self.input_passphrase.text.clone();
        self.app.message_input = self.input_message.text.clone();
        self.app.connect_input = self.input_connect.text.clone();
        self.app.relay_addr_input = self.input_relay_addr.text.clone();
        self.app.invite_passphrase = self.input_invite_passphrase.text.clone();
        self.app.invite_input = self.input_invite_url.text.clone();
        self.app.display_name_input = self.input_display_name.text.clone();
        self.app.search_query = self.input_search.text.clone();
        self.app.contact_search_input = self.input_contact_search.text.clone();
        self.app.device_name_input = self.input_device_name.text.clone();
    }

    /// Sync App string fields → InputState (after updates that clear fields).
    pub(crate) fn sync_app_to_inputs(&mut self) {
        sync_field(&mut self.input_username, &self.app.username_input);
        sync_field(&mut self.input_passphrase, &self.app.passphrase_input);
        sync_field(&mut self.input_message, &self.app.message_input);
        sync_field(&mut self.input_connect, &self.app.connect_input);
        sync_field(&mut self.input_relay_addr, &self.app.relay_addr_input);
        sync_field(&mut self.input_invite_passphrase, &self.app.invite_passphrase);
        sync_field(&mut self.input_invite_url, &self.app.invite_input);
        sync_field(&mut self.input_display_name, &self.app.display_name_input);
        sync_field(&mut self.input_search, &self.app.search_query);
        sync_field(&mut self.input_contact_search, &self.app.contact_search_input);
        sync_field(&mut self.input_device_name, &self.app.device_name_input);
    }

    /// Spawn a file picker on a background thread.
    pub(crate) fn spawn_file_picker(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.file_pick_rx = Some(rx);
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                let _ = tx.send(path);
            }
        });
    }
}

/// If `input_state.text` differs from `app_string`, overwrite and reset cursor.
fn sync_field(input_state: &mut InputState, app_string: &str) {
    if input_state.text != app_string {
        input_state.text = app_string.to_string();
        input_state.cursor = app_string.len();
        input_state.selection = None;
    }
}

impl AppDelegate for VeilApp {
    fn on_init(&mut self, gpu: &GpuContext, _resources: &mut RenderResources) {
        self.text_renderer = TextRenderer::new(gpu).ok();
    }

    fn on_redraw(
        &mut self,
        gpu: &GpuContext,
        resources: &mut RenderResources,
        frame: &mut esox_gfx::Frame,
        _perf: &esox_platform::perf::PerfMonitor,
    ) {
        // Drain async events
        self.drain_network_events();
        self.drain_file_picks();
        self.maybe_spawn_network();

        // Update theme based on app choice
        self.theme = match self.app.theme_choice {
            ThemeChoice::Dark => esox_ui::Theme::dark(),
            ThemeChoice::Light => esox_ui::Theme::light(),
        };

        // Take Ui-owned state out of self so we can pass them to Ui::begin
        // while still accessing self.app and self.input_* fields.
        let Some(mut text) = self.text_renderer.take() else {
            return;
        };
        let mut ui_state = std::mem::take(&mut self.ui_state);
        let theme = self.theme.clone();

        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            w: self.width as f32,
            h: self.height as f32,
        };

        let mut ui = Ui::begin(
            frame,
            gpu,
            resources,
            &mut text,
            &mut ui_state,
            &theme,
            viewport,
        );

        // Now self is free to borrow mutably for draw methods
        match &self.app.screen {
            Screen::Setup => self.draw_setup(&mut ui),
            Screen::ShowRecoveryPhrase(phrase) => {
                let phrase = phrase.clone();
                self.draw_recovery_phrase(&mut ui, &phrase);
            }
            Screen::Chat => self.draw_chat(&mut ui),
            Screen::Settings => self.draw_settings(&mut ui),
        }

        ui.finish();

        // Put state back
        self.text_renderer = Some(text);
        self.ui_state = ui_state;

        // Sync inputs after drawing
        self.sync_inputs_to_app();
    }

    fn on_key(&mut self, event: &KeyEvent, modifiers: ModifiersState) {
        // Handle app-level shortcuts before forwarding to UI
        if event.state == winit::event::ElementState::Pressed {
            match event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.app.update_escape_pressed();
                    self.sync_app_to_inputs();
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    if !self.ui_state.focused.is_some() || self.app.message_input.is_empty() {
                        self.app.update_up_arrow_pressed();
                        self.sync_app_to_inputs();
                    }
                }
                _ => {}
            }

            // Ctrl+F for search toggle
            if modifiers.control_key() {
                if let Key::Character(ref c) = event.logical_key {
                    if c == "f" {
                        self.app.update_toggle_search();
                        return;
                    }
                }
            }
        }

        // Forward to esox_ui for text input handling
        self.ui_state.process_key(event.clone(), modifiers);
    }

    fn on_resize(&mut self, width: u32, height: u32, _gpu: &GpuContext) {
        self.width = width;
        self.height = height;
    }

    fn on_mouse(&mut self, event: MouseInputEvent) {
        match event {
            MouseInputEvent::Moved { x, y } => {
                self.ui_state.mouse.x = x as f32;
                self.ui_state.mouse.y = y as f32;
            }
            MouseInputEvent::Press { x, y, button } => {
                self.ui_state.mouse.x = x as f32;
                self.ui_state.mouse.y = y as f32;
                if button == 0 {
                    self.ui_state.mouse_pressed = true;
                }
            }
            MouseInputEvent::Release { x, y, button } => {
                self.ui_state.mouse.x = x as f32;
                self.ui_state.mouse.y = y as f32;
                if button == 0 {
                    self.ui_state.mouse_pressed = false;
                }
            }
            MouseInputEvent::Scroll { x, y, delta_y } => {
                self.ui_state.pending_scroll = Some((x as f32, y as f32, delta_y));
            }
            MouseInputEvent::Left => {}
            MouseInputEvent::RawMotion { .. } => {}
        }
    }

    fn on_paste(&mut self, text: &str) {
        self.ui_state.ime.committed = Some(text.to_string());
    }

    fn on_ime_commit(&mut self, text: &str) {
        self.ui_state.ime.committed = Some(text.to_string());
    }

    fn on_copy(&mut self) -> Option<String> {
        // Find focused input and return selected text
        None
    }

    fn on_scale_changed(&mut self, scale_factor: f64, _gpu: &GpuContext) {
        self.ui_state.scale_factor = scale_factor as f32;
    }

    fn needs_continuous_redraw(&self) -> bool {
        // Redraw continuously to poll network events
        true
    }

    fn cursor_icon(&self, _x: f64, _y: f64) -> winit::window::CursorIcon {
        winit::window::CursorIcon::Default
    }
}
