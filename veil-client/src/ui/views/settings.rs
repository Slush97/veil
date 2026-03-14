use iced::widget::{button, column, container, horizontal_space, row, text};
use iced::{Element, Length};

use crate::ui::app::App;
use crate::ui::message::Message;

impl App {
    pub(crate) fn view_settings(&self) -> Element<'_, Message> {
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
            crate::ui::types::ThemeChoice::Dark => "Dark",
            crate::ui::types::ThemeChoice::Light => "Light",
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
                        button("Toggle")
                            .on_press(Message::ToggleNotifications)
                            .padding(6),
                    ]
                    .spacing(8),
                ]
                .spacing(8)
                .padding(16),
                column![
                    text("Network").size(18),
                    text(format!("Relay: {relay_display}")).size(14),
                    text(format!("Connected Peers: {}", self.connected_peers.len())).size(14),
                    text(format!("LAN Peers: {}", self.discovered_peers.len())).size(14),
                ]
                .spacing(8)
                .padding(16),
                text(self.connection_state.to_string())
                    .size(10)
                    .width(Length::Fill),
            ]
            .spacing(12)
            .width(Length::Fill),
        )
        .center(Length::Fill)
        .into()
    }
}
