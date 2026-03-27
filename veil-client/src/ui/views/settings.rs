use esox_ui::id::fnv1a_runtime;
use esox_ui::{RichText, Span, Ui, id};

use crate::ui::app::VeilApp;
use crate::ui::types::*;

use super::initials;

impl VeilApp {
    pub(crate) fn draw_settings(&mut self, ui: &mut Ui) {
        let master_fp = self
            .app
            .master
            .as_ref()
            .map(|m| m.peer_id().fingerprint())
            .unwrap_or_else(|| "???".into());

        let display_name = self
            .app
            .master
            .as_ref()
            .map(|m| self.app.resolve_display_name(&m.peer_id()))
            .unwrap_or_else(|| "Not set".into());

        let device_name = self
            .app
            .device
            .as_ref()
            .map(|d| d.certificate().device_name.clone())
            .unwrap_or_else(|| "Unknown".into());

        let connected_peers = self.app.connected_peers.len();
        let discovered_peers = self.app.discovered_peers.len();

        ui.page(id!("settings_page"), self.height as f32, 600.0, |ui| {
            // ── Header ──
            ui.row(|ui| {
                let accent = ui.theme().accent;
                ui.rich_label(&RichText::new().push(Span {
                    text: "Settings",
                    color: Some(accent),
                    bold: true,
                    size: Some(ui.theme().heading_font_size * 1.2),
                    letter_spacing: None,
                    weight: None,
                    background: None,
                    decoration: esox_ui::TextDecoration::None,
                }));
                ui.spacer();
                if ui
                    .ghost_button(id!("settings_back"), "\u{2190} Back to Chat")
                    .clicked
                {
                    self.app.screen = Screen::Chat;
                }
            });

            ui.spacing(20.0);

            let mut tab = std::mem::take(&mut self.settings_tab);
            ui.tabs(
                id!("settings_tabs"),
                &mut tab,
                &["Profile", "Network", "Social", "Preferences"],
                |ui, i| {
                    ui.spacing(20.0);
                    match i {
                        0 => {
                            self.draw_settings_profile(ui, &display_name, &device_name, &master_fp)
                        }
                        1 => self.draw_settings_network(ui, connected_peers, discovered_peers),
                        2 => self.draw_settings_social(ui),
                        3 => self.draw_settings_preferences(ui),
                        _ => {}
                    }
                },
            );
            self.settings_tab = tab;
        });
    }

    fn draw_settings_profile(
        &mut self,
        ui: &mut Ui,
        display_name: &str,
        device_name: &str,
        master_fp: &str,
    ) {
        let accent = ui.theme().accent;

        // Identity card
        ui.card(|ui| {
            let user_ini = initials(display_name);
            ui.row_spaced(14.0, |ui| {
                ui.avatar_colored(&user_ini, 52.0, accent);
                ui.heading(display_name);
            });

            ui.spacing(12.0);

            ui.labeled("Device", |ui| {
                ui.label(device_name);
            });

            ui.spacing(4.0);

            let fp_color = ui.theme().fg_muted;
            ui.label_colored(&format!("Fingerprint: {master_fp}"), fp_color);

            ui.spacing(8.0);

            let copy_id = id!("copy_fp");
            if ui.ghost_button(copy_id, "Copy Fingerprint").clicked {
                self.app.update_export_identity();
                let _ = esox_platform::Clipboard::write(master_fp);
            }
            ui.tooltip(copy_id, "Copy your identity fingerprint");
        });

        ui.spacing(16.0);

        // Display name change
        ui.card(|ui| {
            ui.section("DISPLAY NAME", |ui| {
                ui.row_spaced(10.0, |ui| {
                    ui.text_input(
                        id!("display_name"),
                        &mut self.input_display_name,
                        "Your name",
                    );
                    if ui.ghost_button(id!("set_display_name"), "Update").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_set_display_name();
                        self.sync_app_to_inputs();
                    }
                });
            });
        });

        // Username
        if let Some(ref username) = self.app.username {
            ui.spacing(16.0);
            ui.card(|ui| {
                ui.section("USERNAME", |ui| {
                    ui.label_colored(&format!("@{username}"), accent);
                });
            });
        }

        ui.spacing(16.0);

        // Bio
        ui.card(|ui| {
            ui.section("BIO", |ui| {
                ui.row_spaced(10.0, |ui| {
                    ui.text_input(id!("bio_input"), &mut self.input_bio, "Tell us about yourself");
                    if ui.ghost_button(id!("set_bio"), "Update").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_set_bio();
                    }
                });
            });
        });

        ui.spacing(16.0);

        // Status
        ui.card(|ui| {
            ui.section("STATUS", |ui| {
                ui.row_spaced(10.0, |ui| {
                    ui.text_input(
                        id!("status_input"),
                        &mut self.input_status,
                        "What are you up to?",
                    );
                    if ui.ghost_button(id!("set_status"), "Update").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_set_status();
                        self.sync_app_to_inputs();
                    }
                });
            });
        });
    }

    fn draw_settings_network(
        &mut self,
        ui: &mut Ui,
        connected_peers: usize,
        discovered_peers: usize,
    ) {
        // Status
        ui.card(|ui| {
            ui.section("STATUS", |ui| {
                if self.app.relay_connected {
                    ui.status_pill_success("Relay connected");
                } else {
                    ui.status_pill_error("Relay disconnected");
                }
                ui.spacing(8.0);
                ui.label(&format!("Connected peers: {connected_peers}"));
                ui.spacing(2.0);
                ui.label(&format!("LAN peers: {discovered_peers}"));
            });
        });

        ui.spacing(16.0);

        // Connect to peer
        ui.card(|ui| {
            ui.section("CONNECT TO PEER", |ui| {
                ui.row_spaced(10.0, |ui| {
                    ui.text_input(id!("connect_addr"), &mut self.input_connect, "host:port");
                    if ui.ghost_button(id!("connect_btn"), "Connect").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_connect_to_peer();
                        self.sync_app_to_inputs();
                    }
                });
                let conn_str = self.app.connection_state.to_string();
                if conn_str != "Disconnected" {
                    ui.spacing(6.0);
                    ui.muted_label(&conn_str);
                }
            });
        });

        ui.spacing(16.0);

        // Relay server
        ui.card(|ui| {
            ui.section("RELAY SERVER", |ui| {
                ui.row_spaced(10.0, |ui| {
                    ui.text_input(
                        id!("relay_addr"),
                        &mut self.input_relay_addr,
                        "relay host:port",
                    );
                    if ui.ghost_button(id!("relay_connect"), "Connect").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_connect_to_relay();
                    }
                });
            });
        });

        // LAN peers
        let discovered: Vec<_> = self.app.discovered_peers.clone();
        if !discovered.is_empty() {
            ui.spacing(16.0);
            ui.card(|ui| {
                ui.section("LAN PEERS", |ui| {
                    for (idx, (_, addr, fp)) in discovered.iter().enumerate() {
                        if idx > 0 {
                            ui.spacing(6.0);
                        }
                        let label = self
                            .app
                            .display_names
                            .get(fp)
                            .cloned()
                            .unwrap_or_else(|| fp[..8].to_string());
                        let pi = initials(&label);
                        let btn_id = fnv1a_runtime(&format!("lan_{addr}"));
                        ui.row_spaced(10.0, |ui| {
                            ui.avatar(&pi, 26.0);
                            if ui.text_button(btn_id, &label).clicked {
                                self.sync_inputs_to_app();
                                self.app.update_connect_discovered_peer(*addr);
                            }
                            ui.spacer();
                            ui.badge_dot();
                        });
                    }
                });
            });
        }
    }

    fn draw_settings_social(&mut self, ui: &mut Ui) {
        // Find contacts
        ui.card(|ui| {
            ui.section("FIND CONTACTS", |ui| {
                ui.row_spaced(10.0, |ui| {
                    ui.text_input(
                        id!("contact_search"),
                        &mut self.input_contact_search,
                        "Search @username",
                    );
                    if ui.ghost_button(id!("contact_search_btn"), "Search").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_lookup_contact();
                    }
                });

                if let Some(ref result) = self.app.contact_search_result {
                    ui.spacing(10.0);
                    match result {
                        ContactSearchResult::Found {
                            username,
                            public_key,
                        } => {
                            let un = username.clone();
                            let pk = *public_key;
                            let green = ui.theme().green;
                            let ci = initials(&un);
                            ui.row_spaced(10.0, |ui| {
                                ui.avatar(&ci, 28.0);
                                ui.label_colored(&format!("@{un}"), green);
                                ui.spacer();
                                if ui.button(id!("add_contact"), "Add").clicked {
                                    self.sync_inputs_to_app();
                                    self.app.update_add_contact(un, pk);
                                    self.sync_app_to_inputs();
                                }
                            });
                        }
                        ContactSearchResult::NotFound(username) => {
                            ui.muted_label(&format!("@{username} not found"));
                        }
                        ContactSearchResult::Searching => {
                            ui.row_spaced(8.0, |ui| {
                                ui.spinner();
                                ui.muted_label("Searching\u{2026}");
                            });
                        }
                    }
                }
            });
        });

        ui.spacing(16.0);

        // Create invite
        ui.card(|ui| {
            ui.section("CREATE INVITE", |ui| {
                ui.row_spaced(10.0, |ui| {
                    ui.text_input(
                        id!("invite_pass"),
                        &mut self.input_invite_passphrase,
                        "Passphrase",
                    );
                    if ui.ghost_button(id!("create_invite"), "Create").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_create_invite();
                    }
                });

                if let Some(ref url) = self.app.generated_invite_url {
                    ui.spacing(6.0);
                    ui.muted_label(url);
                }
            });
        });

        ui.spacing(16.0);

        // Join group
        ui.card(|ui| {
            ui.section("JOIN GROUP", |ui| {
                ui.row_spaced(10.0, |ui| {
                    ui.text_input(
                        id!("invite_url"),
                        &mut self.input_invite_url,
                        "Paste invite URL",
                    );
                    if ui.ghost_button(id!("join_invite"), "Join").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_accept_invite();
                        self.sync_app_to_inputs();
                    }
                });
            });
        });
    }

    fn draw_settings_preferences(&mut self, ui: &mut Ui) {
        // Appearance
        ui.card(|ui| {
            ui.section("APPEARANCE", |ui| {
                let theme_label = match self.app.theme_choice {
                    ThemeChoice::Dark => "Dark",
                    ThemeChoice::Light => "Light",
                };
                ui.row(|ui| {
                    ui.label(&format!("Theme: {theme_label}"));
                    ui.spacer();
                    if ui.ghost_button(id!("toggle_theme"), "Toggle").clicked {
                        self.app.update_toggle_theme();
                    }
                });
            });
        });

        ui.spacing(16.0);

        // Notifications
        ui.card(|ui| {
            ui.section("NOTIFICATIONS", |ui| {
                let (notif_label, color) = if self.app.notifications_enabled {
                    ("Enabled", ui.theme().green)
                } else {
                    ("Disabled", ui.theme().fg_muted)
                };
                ui.row(|ui| {
                    ui.label("Desktop notifications");
                    ui.spacer();
                    ui.label_colored(notif_label, color);
                    ui.spacing(8.0);
                    if ui.ghost_button(id!("toggle_notif"), "Toggle").clicked {
                        self.app.update_toggle_notifications();
                    }
                });
            });
        });
    }
}
