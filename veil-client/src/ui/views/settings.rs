use esox_ui::{Ui, id};

use crate::ui::app::VeilApp;
use crate::ui::types::*;

impl VeilApp {
    pub(crate) fn draw_settings(&mut self, ui: &mut Ui) {
        let master_fp = self.app.master.as_ref()
            .map(|m| m.peer_id().fingerprint())
            .unwrap_or_else(|| "???".into());

        let display_name = self.app.master.as_ref()
            .map(|m| self.app.resolve_display_name(&m.peer_id()))
            .unwrap_or_else(|| "Not set".into());

        let device_name = self.app.device.as_ref()
            .map(|d| d.certificate().device_name.clone())
            .unwrap_or_else(|| "Unknown".into());

        let theme_label = match self.app.theme_choice {
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
        };

        let notif_label = if self.app.notifications_enabled {
            "Enabled"
        } else {
            "Disabled"
        };

        let relay_display = if self.app.relay_addr_input.is_empty() {
            "Not configured".to_string()
        } else {
            self.app.relay_addr_input.clone()
        };

        let connected_peers = self.app.connected_peers.len();
        let discovered_peers = self.app.discovered_peers.len();
        let conn_state = self.app.connection_state.to_string();

        ui.center_horizontal(600.0, |ui| {
            ui.padding(12.0, |ui| {
                // Header
                ui.row(|ui| {
                    ui.heading("Settings");
                    ui.fill_space(0.0);
                    if ui.button(id!("settings_back"), "Back").clicked {
                        self.app.screen = Screen::Chat;
                    }
                });

                ui.spacing(16.0);

                // Identity section
                ui.heading("Identity");
                ui.spacing(8.0);
                ui.label(&format!("Display Name: {display_name}"));
                ui.label(&format!("Device: {device_name}"));
                ui.muted_label(&format!("Fingerprint: {master_fp}"));
                ui.spacing(4.0);
                if ui.button(id!("copy_fp"), "Copy Fingerprint").clicked {
                    self.app.update_export_identity();
                    let _ = esox_platform::Clipboard::write(&master_fp);
                }

                ui.spacing(16.0);

                // Appearance section
                ui.heading("Appearance");
                ui.spacing(8.0);
                ui.row_spaced(8.0, |ui| {
                    ui.label(&format!("Theme: {theme_label}"));
                    if ui.button(id!("toggle_theme"), "Toggle").clicked {
                        self.app.update_toggle_theme();
                    }
                });

                ui.spacing(16.0);

                // Notifications section
                ui.heading("Notifications");
                ui.spacing(8.0);
                ui.row_spaced(8.0, |ui| {
                    ui.label(&format!("Desktop Notifications: {notif_label}"));
                    if ui.button(id!("toggle_notif"), "Toggle").clicked {
                        self.app.update_toggle_notifications();
                    }
                });

                ui.spacing(16.0);

                // Network section
                ui.heading("Network");
                ui.spacing(8.0);
                ui.label(&format!("Relay: {relay_display}"));
                ui.label(&format!("Connected Peers: {connected_peers}"));
                ui.label(&format!("LAN Peers: {discovered_peers}"));

                ui.spacing(8.0);
                ui.muted_label(&conn_state);
            });
        });
    }
}
