mod chat;
mod files;
mod identity;
mod invites;
mod network_events;
mod relay;
mod settings;
mod social;

use iced::Element;

use crate::ui::app::App;
use crate::ui::message::Message;
use crate::ui::types::Screen;

impl App {
    pub fn update(&mut self, message: Message) {
        match message {
            // Identity / setup
            Message::CreateIdentity => self.update_create_identity(),
            Message::ConfirmRecoveryPhrase => self.update_confirm_recovery_phrase(),
            Message::LoadIdentity => self.update_load_identity(),
            Message::PassphraseChanged(value) => {
                self.passphrase_input = value;
            }

            // Chat
            Message::InputChanged(value) => self.update_input_changed(value),
            Message::Send => self.update_send(),
            Message::SelectGroup(name) => self.update_select_group(name),
            Message::SelectChannel(name) => {
                self.current_channel = Some(name);
            }

            // Edit/Delete
            Message::EditMessage(idx) => self.update_edit_message(idx),
            Message::CancelEdit => self.update_cancel_edit(),
            Message::ConfirmEdit => self.update_confirm_edit(),
            Message::DeleteMessage(idx) => self.update_delete_message(idx),

            // Connection
            Message::ConnectInputChanged(value) => {
                self.connect_input = value;
            }
            Message::ConnectToPeer => self.update_connect_to_peer(),

            // Network events
            Message::NetworkReady { local_addr, cmd_tx } => {
                self.update_network_ready(local_addr, cmd_tx);
            }
            Message::PeerConnected {
                conn_id,
                peer_id,
                session_key,
                device_certificate,
            } => {
                self.update_peer_connected(conn_id, peer_id, session_key, device_certificate);
            }
            Message::PeerDisconnected { conn_id } => {
                self.connected_peers.retain(|(id, _)| *id != conn_id);
                self.connection_state = super::types::ConnectionState::Disconnected;
            }
            Message::PeerData { sealed } => self.update_peer_data(sealed),
            Message::ConnectionFailed(err) => {
                self.connection_state =
                    super::types::ConnectionState::Failed(format!("Error: {err}"));
            }

            // Relay
            Message::RelayConnected => self.update_relay_connected(),
            Message::RelayDisconnected(_reason) => self.update_relay_disconnected(),
            Message::RelayError { code, message } => self.update_relay_error(code, message),
            Message::RelayAddrChanged(value) => {
                self.relay_addr_input = value;
            }
            Message::ConnectToRelay => self.update_connect_to_relay(),

            // Invites
            Message::InvitePassphraseChanged(value) => {
                self.invite_passphrase = value;
            }
            Message::CreateInvite => self.update_create_invite(),
            Message::InviteCreated(url) => {
                self.generated_invite_url = Some(url);
            }
            Message::InviteInputChanged(value) => {
                self.invite_input = value;
            }
            Message::AcceptInvite => self.update_accept_invite(),
            Message::InviteAccepted {
                group_name,
                group_id,
                group_key,
            } => {
                self.update_invite_accepted(group_name, group_id, group_key);
            }
            Message::InviteFailed(err) => {
                self.connection_state =
                    super::types::ConnectionState::Failed(format!("Invite failed: {err}"));
            }

            // Files
            Message::PickFile => self.update_pick_file(),
            Message::SendFile(path) => self.update_send_file(path),
            Message::FileSent { filename } => self.update_file_sent(filename),
            Message::FileFailed(err) => self.update_file_failed(err),
            Message::BlobRequested { conn_id, blob_id } => {
                self.update_blob_requested(conn_id, blob_id);
            }
            Message::BlobReceived { blob_id } => self.update_blob_received(blob_id),
            Message::SaveFile(blob_id, filename) => self.update_save_file(blob_id, filename),

            // Social (typing, display names, replies, reactions, search, discovery)
            Message::TypingStarted { peer_id, .. } => self.update_typing_started(peer_id),
            Message::TypingStopped { peer_id } => self.update_typing_stopped(peer_id),
            Message::ReadReceipt { .. } => {}
            Message::DisplayNameInputChanged(value) => {
                self.display_name_input = value;
            }
            Message::SetDisplayName => self.update_set_display_name(),
            Message::ReplyTo(idx) => self.update_reply_to(idx),
            Message::CancelReply => {
                self.replying_to = None;
            }
            Message::React(idx, emoji) => self.update_react(idx, emoji),
            Message::ToggleSearch => self.update_toggle_search(),
            Message::SearchQueryChanged(query) => self.update_search_query(query),
            Message::ConnectDiscoveredPeer(addr) => self.update_connect_discovered_peer(addr),
            Message::LanPeerDiscovered {
                name,
                addr,
                fingerprint,
            } => {
                self.update_lan_peer_discovered(name, addr, fingerprint);
            }
            Message::LanPeerLost(name) => {
                self.discovered_peers.retain(|(n, _, _)| *n != name);
            }
            Message::LoadMoreMessages => {
                self.messages_loaded += 500;
                self.messages.clear();
                self.load_message_history();
            }

            // Settings
            Message::OpenSettings => {
                self.screen = Screen::Settings;
            }
            Message::CloseSettings => {
                self.screen = Screen::Chat;
            }
            Message::ToggleTheme => self.update_toggle_theme(),
            Message::ToggleNotifications => self.update_toggle_notifications(),
            Message::DeviceNameInputChanged(value) => {
                self.device_name_input = value;
            }
            Message::ExportIdentity => self.update_export_identity(),

            // Keyboard shortcuts
            Message::EscapePressed => self.update_escape_pressed(),
            Message::UpArrowPressed => self.update_up_arrow_pressed(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::Setup => self.view_setup(),
            Screen::ShowRecoveryPhrase(phrase) => self.view_recovery_phrase(phrase),
            Screen::Chat => self.view_chat(),
            Screen::Settings => self.view_settings(),
        }
    }
}
