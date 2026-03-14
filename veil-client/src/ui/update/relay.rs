use std::net::SocketAddr;

use veil_core::SealedMessage;

use crate::ui::app::App;
use crate::ui::message::NetCommand;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_relay_connected(&mut self) {
        self.relay_connected = true;
        self.connection_state = ConnectionState::Connected("Relay connected".into());

        // Flush pending messages on relay connect
        let pending: Vec<SealedMessage> = self.pending_messages.drain(..).collect();
        for sealed in pending {
            if let Some(ref mut tx) = self.net_cmd_tx
                && let Err(e) = tx.try_send(NetCommand::SendMessage(sealed)) {
                    tracing::warn!("failed to flush pending message: {e}");
                }
        }
        for msg in &mut self.messages {
            if msg.status == Some(MessageStatus::Sending) {
                msg.status = Some(MessageStatus::Sent);
            }
        }
    }

    pub(crate) fn update_relay_disconnected(&mut self) {
        self.relay_connected = false;
        self.connection_state = ConnectionState::Reconnecting;
    }

    pub(crate) fn update_relay_error(&mut self, code: String, message: String) {
        self.connection_state = ConnectionState::Warning(format!("Relay: {code} — {message}"));
        self.messages.push(ChatMessage::system(format!(
            "Relay warning: [{code}] {message}"
        )));
    }

    pub(crate) fn update_connect_to_relay(&mut self) {
        if let Ok(addr) = self.relay_addr_input.parse::<SocketAddr>() {
            self.connection_state =
                ConnectionState::Connecting(format!("Connecting to relay {addr}..."));
            if let Some(ref mut tx) = self.net_cmd_tx
                && let Err(e) = tx.try_send(NetCommand::ConnectRelay(addr)) {
                    tracing::warn!("failed to send relay connect: {e}");
                }
            // Persist relay address
            if let Some(ref store) = self.store
                && let Err(e) = store.store_setting("relay_addr", &self.relay_addr_input) {
                    tracing::warn!("failed to persist relay address: {e}");
                }
        } else {
            self.connection_state =
                ConnectionState::Failed("Invalid relay address (use host:port)".into());
        }
    }
}
