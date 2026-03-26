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
                let title_size = ui.theme().heading_font_size * 1.2;
                ui.rich_label(&RichText::new().push(Span {
                    text: "Settings",
                    color: Some(accent),
                    bold: true,
                    size: Some(title_size),
                    letter_spacing: None,
                    weight: None,
                    background: None,
                    decoration: esox_ui::TextDecoration::None,
                }));
                ui.fill_space(0.0);
                if ui
                    .ghost_button(id!("settings_back"), "\u{2190} Back to Chat")
                    .clicked
                {
                    self.app.screen = Screen::Chat;
                }
            });

            ui.spacing(16.0);

            // Take tab state out of self to avoid borrow conflict with closure
            let mut tab = std::mem::take(&mut self.settings_tab);
            ui.tabs(
                id!("settings_tabs"),
                &mut tab,
                &["Profile", "Network", "Social", "Preferences"],
                |ui, i| {
                    ui.spacing(16.0);
                    match i {
                        0 => {
                            // ── Profile tab ──
                            ui.card(|ui| {
                                let accent = ui.theme().accent;
                                let user_ini = initials(&display_name);

                                ui.row_spaced(12.0, |ui| {
                                    ui.avatar_colored(&user_ini, 48.0, accent);
                                    ui.heading(&display_name);
                                });

                                ui.spacing(8.0);

                                ui.labeled("Device", |ui| {
                                    ui.label(&device_name);
                                });

                                let muted = ui.theme().fg_muted;
                                ui.label_colored(
                                    &format!("Fingerprint: {master_fp}"),
                                    muted,
                                );
                                ui.spacing(6.0);

                                if ui
                                    .ghost_button(id!("copy_fp"), "Copy Fingerprint")
                                    .clicked
                                {
                                    self.app.update_export_identity();
                                    let _ =
                                        esox_platform::Clipboard::write(&master_fp);
                                }
                            });

                            ui.spacing(12.0);

                            // Change display name
                            ui.card(|ui| {
                                ui.section("DISPLAY NAME", |ui| {
                                    ui.row_spaced(8.0, |ui| {
                                        ui.text_input(
                                            id!("display_name"),
                                            &mut self.input_display_name,
                                            "Your name",
                                        );
                                        if ui
                                            .ghost_button(
                                                id!("set_display_name"),
                                                "Update",
                                            )
                                            .clicked
                                        {
                                            self.sync_inputs_to_app();
                                            self.app.update_set_display_name();
                                            self.sync_app_to_inputs();
                                        }
                                    });
                                });
                            });

                            // Username
                            if let Some(ref username) = self.app.username {
                                ui.spacing(12.0);
                                let accent = ui.theme().accent;
                                ui.card(|ui| {
                                    ui.section("USERNAME", |ui| {
                                        ui.label_colored(
                                            &format!("@{username}"),
                                            accent,
                                        );
                                    });
                                });
                            }
                        }
                        1 => {
                            // ── Network tab ──
                            ui.card(|ui| {
                                ui.section("STATUS", |ui| {
                                    if self.app.relay_connected {
                                        ui.status_pill_success("Relay connected");
                                    } else {
                                        ui.status_pill_error("Relay disconnected");
                                    }
                                    ui.spacing(4.0);
                                    ui.label(&format!(
                                        "Connected peers: {connected_peers}"
                                    ));
                                    ui.label(&format!(
                                        "LAN peers: {discovered_peers}"
                                    ));
                                });
                            });

                            ui.spacing(12.0);

                            // Connect to peer
                            ui.card(|ui| {
                                ui.section("CONNECT TO PEER", |ui| {
                                    ui.row_spaced(8.0, |ui| {
                                        ui.text_input(
                                            id!("connect_addr"),
                                            &mut self.input_connect,
                                            "host:port",
                                        );
                                        if ui
                                            .ghost_button(
                                                id!("connect_btn"),
                                                "Connect",
                                            )
                                            .clicked
                                        {
                                            self.sync_inputs_to_app();
                                            self.app.update_connect_to_peer();
                                            self.sync_app_to_inputs();
                                        }
                                    });
                                    let conn_str =
                                        self.app.connection_state.to_string();
                                    if conn_str != "Disconnected" {
                                        ui.spacing(4.0);
                                        ui.muted_label(&conn_str);
                                    }
                                });
                            });

                            ui.spacing(12.0);

                            // Relay
                            ui.card(|ui| {
                                ui.section("RELAY SERVER", |ui| {
                                    ui.row_spaced(8.0, |ui| {
                                        ui.text_input(
                                            id!("relay_addr"),
                                            &mut self.input_relay_addr,
                                            "relay host:port",
                                        );
                                        if ui
                                            .ghost_button(
                                                id!("relay_connect"),
                                                "Connect",
                                            )
                                            .clicked
                                        {
                                            self.sync_inputs_to_app();
                                            self.app.update_connect_to_relay();
                                        }
                                    });
                                });
                            });

                            // LAN peers
                            let discovered: Vec<_> =
                                self.app.discovered_peers.clone();
                            if !discovered.is_empty() {
                                ui.spacing(12.0);
                                ui.card(|ui| {
                                    ui.section("LAN PEERS", |ui| {
                                        for (_, addr, fp) in &discovered {
                                            let label = self
                                                .app
                                                .display_names
                                                .get(fp)
                                                .cloned()
                                                .unwrap_or_else(|| {
                                                    fp[..8].to_string()
                                                });
                                            let pi = initials(&label);
                                            let btn_id = fnv1a_runtime(
                                                &format!("lan_{addr}"),
                                            );
                                            ui.row_spaced(8.0, |ui| {
                                                ui.avatar(&pi, 24.0);
                                                if ui
                                                    .text_button(btn_id, &label)
                                                    .clicked
                                                {
                                                    self.sync_inputs_to_app();
                                                    self.app
                                                        .update_connect_discovered_peer(
                                                            *addr,
                                                        );
                                                }
                                                ui.fill_space(0.0);
                                                ui.badge_dot();
                                            });
                                            ui.spacing(4.0);
                                        }
                                    });
                                });
                            }
                        }
                        2 => {
                            // ── Social tab ──
                            ui.card(|ui| {
                                ui.section("FIND CONTACTS", |ui| {
                                    ui.row_spaced(8.0, |ui| {
                                        ui.text_input(
                                            id!("contact_search"),
                                            &mut self.input_contact_search,
                                            "Search @username",
                                        );
                                        if ui
                                            .ghost_button(
                                                id!("contact_search_btn"),
                                                "Search",
                                            )
                                            .clicked
                                        {
                                            self.sync_inputs_to_app();
                                            self.app.update_lookup_contact();
                                        }
                                    });

                                    if let Some(ref result) =
                                        self.app.contact_search_result
                                    {
                                        ui.spacing(8.0);
                                        match result {
                                            ContactSearchResult::Found {
                                                username,
                                                public_key,
                                            } => {
                                                let un = username.clone();
                                                let pk = *public_key;
                                                let green = ui.theme().green;
                                                let ci = initials(&un);
                                                ui.row_spaced(8.0, |ui| {
                                                    ui.avatar(&ci, 28.0);
                                                    ui.label_colored(
                                                        &format!("@{un}"),
                                                        green,
                                                    );
                                                    ui.fill_space(0.0);
                                                    if ui
                                                        .button(
                                                            id!("add_contact"),
                                                            "Add",
                                                        )
                                                        .clicked
                                                    {
                                                        self.sync_inputs_to_app();
                                                        self.app
                                                            .update_add_contact(un, pk);
                                                        self.sync_app_to_inputs();
                                                    }
                                                });
                                            }
                                            ContactSearchResult::NotFound(username) => {
                                                ui.muted_label(&format!(
                                                    "@{username} not found"
                                                ));
                                            }
                                            ContactSearchResult::Searching => {
                                                ui.spinner();
                                                ui.muted_label("Searching...");
                                            }
                                        }
                                    }
                                });
                            });

                            ui.spacing(12.0);

                            // Create invite
                            ui.card(|ui| {
                                ui.section("CREATE INVITE", |ui| {
                                    ui.row_spaced(8.0, |ui| {
                                        ui.text_input(
                                            id!("invite_pass"),
                                            &mut self.input_invite_passphrase,
                                            "Passphrase",
                                        );
                                        if ui
                                            .ghost_button(
                                                id!("create_invite"),
                                                "Create",
                                            )
                                            .clicked
                                        {
                                            self.sync_inputs_to_app();
                                            self.app.update_create_invite();
                                        }
                                    });

                                    if let Some(ref url) =
                                        self.app.generated_invite_url
                                    {
                                        ui.spacing(4.0);
                                        ui.muted_label(url);
                                    }
                                });
                            });

                            ui.spacing(12.0);

                            // Join group
                            ui.card(|ui| {
                                ui.section("JOIN GROUP", |ui| {
                                    ui.row_spaced(8.0, |ui| {
                                        ui.text_input(
                                            id!("invite_url"),
                                            &mut self.input_invite_url,
                                            "Paste invite URL",
                                        );
                                        if ui
                                            .ghost_button(
                                                id!("join_invite"),
                                                "Join",
                                            )
                                            .clicked
                                        {
                                            self.sync_inputs_to_app();
                                            self.app.update_accept_invite();
                                            self.sync_app_to_inputs();
                                        }
                                    });
                                });
                            });
                        }
                        3 => {
                            // ── Preferences tab ──
                            ui.card(|ui| {
                                ui.section("APPEARANCE", |ui| {
                                    let theme_label = match self.app.theme_choice {
                                        ThemeChoice::Dark => "Dark",
                                        ThemeChoice::Light => "Light",
                                    };
                                    ui.labeled(
                                        &format!("Theme: {theme_label}"),
                                        |ui| {
                                            if ui
                                                .ghost_button(
                                                    id!("toggle_theme"),
                                                    "Toggle",
                                                )
                                                .clicked
                                            {
                                                self.app.update_toggle_theme();
                                            }
                                        },
                                    );
                                });
                            });

                            ui.spacing(12.0);

                            ui.card(|ui| {
                                ui.section("NOTIFICATIONS", |ui| {
                                    let (notif_label, color) =
                                        if self.app.notifications_enabled {
                                            ("Enabled", ui.theme().green)
                                        } else {
                                            ("Disabled", ui.theme().fg_muted)
                                        };
                                    ui.row(|ui| {
                                        ui.label("Desktop: ");
                                        ui.label_colored(notif_label, color);
                                        ui.fill_space(0.0);
                                        if ui
                                            .ghost_button(
                                                id!("toggle_notif"),
                                                "Toggle",
                                            )
                                            .clicked
                                        {
                                            self.app
                                                .update_toggle_notifications();
                                        }
                                    });
                                });
                            });
                        }
                        _ => {}
                    }
                },
            );
            self.settings_tab = tab;
        });
    }
}
