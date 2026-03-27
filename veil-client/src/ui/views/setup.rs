use esox_ui::{FieldStatus, RichText, Span, Ui, id};

use crate::ui::app::VeilApp;
use crate::ui::types::*;

impl VeilApp {
    pub(crate) fn draw_setup(&mut self, ui: &mut Ui) {
        ui.scrollable_fill(id!("setup_scroll"), |ui| {
            let top_pad = (self.height as f32 * 0.15).max(60.0);
            ui.spacing(top_pad);

            ui.max_width(380.0, |ui| {
                // ── Brand ──
                let accent = ui.theme().accent;
                let brand_size = ui.theme().heading_font_size * 2.0;
                ui.rich_label(&RichText::new().push(Span {
                    text: "veil",
                    color: Some(accent),
                    bold: true,
                    size: Some(brand_size),
                    letter_spacing: Some(4.0),
                    weight: None,
                    background: None,
                    decoration: esox_ui::TextDecoration::None,
                }));
                ui.spacing(6.0);
                ui.muted_label("Encrypted. Decentralized. Yours.");

                ui.spacing(32.0);

                // ── Login card ──
                ui.card(|ui| {
                    ui.form_field("Username", FieldStatus::None, "", |ui| {
                        ui.text_input(
                            id!("setup_username"),
                            &mut self.input_username,
                            "Choose a username",
                        )
                    });

                    ui.spacing(12.0);

                    ui.form_field(
                        "Password",
                        FieldStatus::None,
                        "Optional \u{2014} encrypts your local identity",
                        |ui| {
                            ui.text_input(
                                id!("setup_passphrase"),
                                &mut self.input_passphrase,
                                "Password",
                            )
                        },
                    );

                    ui.spacing(20.0);

                    if ui.button(id!("setup_create"), "Create Account").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_create_identity();
                        self.sync_app_to_inputs();
                    }

                    ui.spacing(6.0);

                    if ui
                        .ghost_button(id!("setup_signin"), "Sign in to existing account")
                        .clicked
                    {
                        self.sync_inputs_to_app();
                        self.app.update_load_identity();
                        self.sync_app_to_inputs();
                    }
                });

                // ── Status feedback ──
                if let Some(ref status) = self.app.registration_status {
                    ui.spacing(10.0);
                    ui.alert_info(status);
                }

                match &self.app.connection_state {
                    ConnectionState::Connected(msg) => {
                        ui.spacing(6.0);
                        ui.status_pill_success(msg);
                    }
                    ConnectionState::Connecting(msg) => {
                        ui.spacing(6.0);
                        ui.status_pill_warning(msg);
                    }
                    ConnectionState::Failed(msg) => {
                        ui.spacing(6.0);
                        ui.status_pill_error(msg);
                    }
                    ConnectionState::Warning(msg) => {
                        ui.spacing(6.0);
                        ui.status_pill_warning(msg);
                    }
                    ConnectionState::Reconnecting => {
                        ui.spacing(6.0);
                        ui.status_pill_warning("Reconnecting\u{2026}");
                    }
                    ConnectionState::Disconnected => {}
                }

                // ── Relay config ──
                ui.spacing(24.0);
                ui.card(|ui| {
                    ui.form_field("Relay Server", FieldStatus::None, "", |ui| {
                        ui.text_input(
                            id!("setup_relay"),
                            &mut self.input_relay_addr,
                            "relay host:port",
                        )
                    });
                });

                // ── Dev login ──
                ui.spacing(20.0);
                ui.separator();
                ui.spacing(10.0);
                if ui
                    .ghost_button(id!("dev_login"), "Dev Login (skip auth)")
                    .clicked
                {
                    self.app.update_dev_login();
                    self.sync_app_to_inputs();
                    self.maybe_spawn_network();
                }
            });
        });
    }

    pub(crate) fn draw_recovery_phrase(&mut self, ui: &mut Ui, phrase: &str) {
        ui.scrollable_fill(id!("recovery_scroll"), |ui| {
            let top_pad = (self.height as f32 * 0.15).max(60.0);
            ui.spacing(top_pad);

            ui.max_width(460.0, |ui| {
                let accent = ui.theme().accent;
                let title_size = ui.theme().heading_font_size * 1.3;
                ui.rich_label(&RichText::new().push(Span {
                    text: "Recovery Phrase",
                    color: Some(accent),
                    bold: true,
                    size: Some(title_size),
                    letter_spacing: None,
                    weight: None,
                    background: None,
                    decoration: esox_ui::TextDecoration::None,
                }));
                ui.spacing(10.0);
                ui.label("Write these 12 words down and store them safely.");
                ui.spacing(4.0);
                ui.muted_label("You will need them to recover your identity.");

                ui.spacing(24.0);

                ui.card(|ui| {
                    let words: Vec<&str> = phrase.split_whitespace().collect();
                    let muted = ui.theme().fg_muted;
                    ui.columns_spaced(20.0, &[1.0, 1.0], |ui, col| {
                        let start = col * 6;
                        let end = (start + 6).min(words.len());
                        for (i, word) in words.iter().enumerate().take(end).skip(start) {
                            ui.rich_label(
                                &RichText::new()
                                    .colored(&format!("{:>2}. ", i + 1), muted)
                                    .bold(word),
                            );
                            ui.spacing(6.0);
                        }
                    });
                });

                ui.spacing(24.0);
                if ui
                    .button(id!("confirm_phrase"), "I have saved my recovery phrase")
                    .clicked
                {
                    self.sync_inputs_to_app();
                    self.app.update_confirm_recovery_phrase();
                    self.sync_app_to_inputs();
                    self.maybe_spawn_network();
                }
            });
        });
    }
}
