use veil_core::{ChannelId, ControlMessage, MessageContent, MessageKind, ProfileField};

use crate::ui::app::App;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_toggle_theme(&mut self) {
        self.theme_choice = match self.theme_choice {
            ThemeChoice::Dark => ThemeChoice::Light,
            ThemeChoice::Light => ThemeChoice::Dark,
        };
        if let Some(ref store) = self.store {
            let theme_str = match self.theme_choice {
                ThemeChoice::Dark => "dark",
                ThemeChoice::Light => "light",
            };
            if let Err(e) = store.store_setting("theme", theme_str) {
                tracing::warn!("failed to persist theme: {e}");
            }
        }
    }

    pub(crate) fn update_toggle_notifications(&mut self) {
        self.notifications_enabled = !self.notifications_enabled;
        if let Some(ref store) = self.store
            && let Err(e) = store.store_setting(
                "notifications",
                if self.notifications_enabled {
                    "true"
                } else {
                    "false"
                },
            )
        {
            tracing::warn!("failed to persist notifications: {e}");
        }
    }

    pub(crate) fn update_export_identity(&mut self) {
        // Show master fingerprint for copy
        if let Some(ref master) = self.master {
            self.connection_state = ConnectionState::Connected(format!(
                "Fingerprint: {}",
                master.peer_id().fingerprint()
            ));
        }
    }

    pub(crate) fn update_escape_pressed(&mut self) {
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

    /// Toggle pin/unpin on a message and broadcast the control message.
    pub(crate) fn update_toggle_pin(&mut self, idx: usize) {
        if idx >= self.messages.len() {
            return;
        }
        let msg_id = match self.messages[idx].id.clone() {
            Some(id) => id,
            None => return,
        };
        let pinned = self.messages[idx].pinned;

        let channel_id = self
            .current_group
            .as_ref()
            .map(|g| self.current_channel_id(g))
            .unwrap_or_else(ChannelId::new);

        let ctrl = if pinned {
            ControlMessage::UnpinMessage {
                channel_id: channel_id.clone(),
                message_id: msg_id,
            }
        } else {
            ControlMessage::PinMessage {
                channel_id: channel_id.clone(),
                message_id: msg_id,
            }
        };

        let content = MessageContent {
            kind: MessageKind::Control(ctrl),
            timestamp: chrono::Utc::now(),
            channel_id,
            expires_at: None,
        };

        if self.seal_send_persist(&content).is_some() {
            self.messages[idx].pinned = !pinned;
        }
    }

    /// Update bio and broadcast a ProfileUpdate control message.
    pub(crate) fn update_set_bio(&mut self) {
        if self.bio_input.trim().is_empty() {
            return;
        }
        let channel_id = self
            .current_group
            .as_ref()
            .map(|g| self.current_channel_id(g))
            .unwrap_or_else(ChannelId::new);

        let content = MessageContent {
            kind: MessageKind::Control(ControlMessage::ProfileUpdate {
                fields: vec![ProfileField::Bio(self.bio_input.clone())],
            }),
            timestamp: chrono::Utc::now(),
            channel_id,
            expires_at: None,
        };
        self.seal_send_persist(&content);
    }

    /// Update status and broadcast a ProfileUpdate control message.
    pub(crate) fn update_set_status(&mut self) {
        let channel_id = self
            .current_group
            .as_ref()
            .map(|g| self.current_channel_id(g))
            .unwrap_or_else(ChannelId::new);

        let content = MessageContent {
            kind: MessageKind::Control(ControlMessage::ProfileUpdate {
                fields: vec![ProfileField::Status(self.status_input.clone())],
            }),
            timestamp: chrono::Utc::now(),
            channel_id,
            expires_at: None,
        };
        self.seal_send_persist(&content);
        self.status_input.clear();
    }

    pub(crate) fn update_up_arrow_pressed(&mut self) {
        // Edit last own message if input is empty
        if self.message_input.is_empty() && self.editing_message.is_none() {
            let our_id = self.master.as_ref().map(|m| m.peer_id());
            if let Some(idx) = self.messages.iter().rposition(|m| {
                m.sender_id.as_ref() == our_id.as_ref() && m.id.is_some() && !m.deleted
            }) {
                self.editing_message = Some(idx);
                self.message_input = self.messages[idx].content.clone();
            }
        }
    }
}
