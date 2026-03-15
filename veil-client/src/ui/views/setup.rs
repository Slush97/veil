use esox_ui::{Ui, id};

use crate::ui::app::VeilApp;

impl VeilApp {
    pub(crate) fn draw_setup(&mut self, ui: &mut Ui) {
        let max_w = 400.0;
        ui.center_horizontal(max_w, |ui| {
            let top_pad = (self.height as f32 * 0.25).max(40.0);
            ui.spacing(top_pad);

            ui.max_width(max_w, |ui| {
                ui.heading("Veil");
                ui.spacing(4.0);
                ui.muted_label("Encrypted. Decentralized. Yours.");
                ui.spacing(20.0);

                // Username input
                ui.text_input(id!("setup_username"), &mut self.input_username, "Choose a username");
                ui.spacing(8.0);

                // Password input (plaintext for now — TODO: add secure mode to esox_ui)
                ui.text_input(id!("setup_passphrase"), &mut self.input_passphrase, "Password (optional)");
                ui.spacing(12.0);

                // Create / Sign In buttons
                ui.row_spaced(12.0, |ui| {
                    if ui.button(id!("setup_create"), "Create Account").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_create_identity();
                        self.sync_app_to_inputs();
                    }
                    if ui.button(id!("setup_signin"), "Sign In").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_load_identity();
                        self.sync_app_to_inputs();
                    }
                });
                ui.spacing(8.0);

                // Status/error feedback
                if let Some(ref status) = self.app.registration_status {
                    ui.muted_label(status);
                    ui.spacing(4.0);
                }

                // Connection state
                let state_str = self.app.connection_state.to_string();
                if state_str != "Disconnected" {
                    ui.muted_label(&state_str);
                    ui.spacing(4.0);
                }

                // Relay address
                ui.spacing(12.0);
                ui.header_label("Relay server");
                ui.spacing(4.0);
                ui.text_input(id!("setup_relay"), &mut self.input_relay_addr, "relay host:port");
            });
        });
    }

    pub(crate) fn draw_recovery_phrase(&mut self, ui: &mut Ui, phrase: &str) {
        let max_w = 500.0;
        ui.center_horizontal(max_w, |ui| {
            let top_pad = (self.height as f32 * 0.15).max(40.0);
            ui.spacing(top_pad);

            ui.max_width(max_w, |ui| {
                ui.heading("Your Recovery Phrase");
                ui.spacing(8.0);
                ui.label("Write these 12 words down and store them safely.");
                ui.label("You will need them to recover your identity.");
                ui.spacing(16.0);

                let words: Vec<&str> = phrase.split_whitespace().collect();
                for (i, word) in words.iter().enumerate() {
                    ui.label(&format!("{}. {}", i + 1, word));
                    ui.spacing(4.0);
                }

                ui.spacing(16.0);
                if ui.button(id!("confirm_phrase"), "I have saved my recovery phrase").clicked {
                    self.sync_inputs_to_app();
                    self.app.update_confirm_recovery_phrase();
                    self.sync_app_to_inputs();
                    self.maybe_spawn_network();
                }
            });
        });
    }
}
