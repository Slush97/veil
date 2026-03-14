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
            let _ = store.store_setting("theme", theme_str);
        }
    }

    pub(crate) fn update_toggle_notifications(&mut self) {
        self.notifications_enabled = !self.notifications_enabled;
        if let Some(ref store) = self.store {
            let _ = store.store_setting(
                "notifications",
                if self.notifications_enabled {
                    "true"
                } else {
                    "false"
                },
            );
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
