use esox_ui::id::fnv1a_runtime;
use esox_ui::{Easing, RichText, SpacingScale, Span, Status, TextSize, Ui, WidgetStyle, id};

use crate::ui::app::VeilApp;
use crate::ui::types::*;

use super::initials;

impl VeilApp {
    pub(crate) fn draw_sidebar(&mut self, ui: &mut Ui) {
        let accent = ui.theme().accent;
        let accent_dim = ui.theme().accent_dim;

        ui.surface(|ui| {
            // ── Brand header ──
            ui.padding(SpacingScale::Lg, |ui| {
                ui.row(|ui| {
                    ui.rich_label(&RichText::new().push(Span {
                        text: "veil",
                        color: Some(accent),
                        bold: true,
                        size: Some(ui.theme().heading_font_size),
                        letter_spacing: Some(2.0),
                        weight: None,
                        background: None,
                        decoration: esox_ui::TextDecoration::None,
                    }));
                    ui.spacer();
                    let pill_id = if self.app.relay_connected {
                        let id = id!("status_pill");
                        ui.status_pill_success("Online");
                        id
                    } else if !self.app.connected_peers.is_empty() {
                        let id = id!("status_p2p");
                        ui.status_pill_warning("P2P");
                        id
                    } else {
                        let id = id!("status_off");
                        ui.status_pill_error("Offline");
                        id
                    };
                    ui.tooltip(
                        pill_id,
                        if self.app.relay_connected {
                            "Connected to relay server"
                        } else if !self.app.connected_peers.is_empty() {
                            "Direct peer-to-peer only"
                        } else {
                            "No connections"
                        },
                    );
                });
            });

            ui.separator();

            ui.scrollable_fill(id!("sidebar_scroll"), |ui| {
                ui.spacing(8.0);

                // ── Groups ──
                ui.padding(SpacingScale::Lg, |ui| {
                    ui.header_label("GROUPS");
                });
                ui.spacing(2.0);

                let groups: Vec<(String, [u8; 32], usize)> = self
                    .app
                    .groups
                    .iter()
                    .map(|g| {
                        let unread = self.app.unread_counts.get(&g.id.0).copied().unwrap_or(0);
                        (g.name.clone(), g.id.0, unread)
                    })
                    .collect();

                let current_group_name = self.app.current_group.as_ref().map(|g| g.name.clone());

                if groups.is_empty() {
                    ui.padding(SpacingScale::Lg, |ui| {
                        ui.muted_label("No groups yet");
                    });
                }

                for (name, _id_bytes, unread) in &groups {
                    let is_selected = current_group_name.as_ref() == Some(name);
                    let btn_id = fnv1a_runtime(&format!("group_{name}"));
                    let gi = initials(name);

                    // Hover highlight
                    let hover_id = fnv1a_runtime(&format!("ghover_{name}"));
                    let hover_t = ui.animate_bool(hover_id, false, 120.0, Easing::EaseOutCubic);

                    if is_selected {
                        ui.box_container()
                            .bg(accent_dim)
                            .radius(0.0)
                            .padding(0.0)
                            .show(|ui| {
                                ui.padding(SpacingScale::Sm, |ui| {
                                    ui.row_spaced(10.0, |ui| {
                                        ui.avatar_colored(&gi, 32.0, accent);
                                        ui.label_colored(name, accent);
                                        ui.spacer();
                                        if *unread > 0 {
                                            ui.badge(*unread as u32);
                                        }
                                    });
                                });
                            });
                    } else {
                        // Subtle hover bg
                        let hover_bg = if hover_t > 0.01 {
                            let mut c = ui.theme().bg_raised;
                            c.a = hover_t * 0.5;
                            c
                        } else {
                            esox_gfx::Color::TRANSPARENT
                        };

                        ui.box_container()
                            .bg(hover_bg)
                            .radius(0.0)
                            .padding(0.0)
                            .show(|ui| {
                                ui.padding(SpacingScale::Sm, |ui| {
                                    ui.row_spaced(10.0, |ui| {
                                        ui.avatar(&gi, 32.0);
                                        if ui.text_button(btn_id, name).clicked {
                                            self.sync_inputs_to_app();
                                            self.app.update_select_group(name.clone());
                                            self.sync_app_to_inputs();
                                        }
                                        ui.spacer();
                                        if *unread > 0 {
                                            ui.badge(*unread as u32);
                                        }
                                    });
                                });
                            });
                    }
                }

                // ── Channels ──
                let channels = self.app.channels.clone();
                let current_channel = self.app.current_channel.clone();
                if !channels.is_empty() {
                    ui.spacing(12.0);
                    ui.padding(SpacingScale::Lg, |ui| {
                        ui.header_label("CHANNELS");
                    });
                    ui.spacing(2.0);

                    for channel in &channels {
                        let is_selected = current_channel.as_ref() == Some(channel);
                        let ch_btn_id = fnv1a_runtime(&format!("ch_{channel}"));

                        let bg = if is_selected {
                            accent_dim
                        } else {
                            esox_gfx::Color::TRANSPARENT
                        };

                        let hash_color = if is_selected {
                            accent
                        } else {
                            ui.theme().fg_dim
                        };
                        let name_color = if is_selected {
                            accent
                        } else {
                            ui.theme().fg_muted
                        };

                        ui.box_container()
                            .bg(bg)
                            .radius(0.0)
                            .padding(0.0)
                            .show(|ui| {
                                let style = WidgetStyle {
                                    padding: Some(esox_ui::Spacing {
                                        top: 5.0,
                                        bottom: 5.0,
                                        left: 22.0,
                                        right: 14.0,
                                    }),
                                    ..Default::default()
                                };
                                ui.with_style(style, |ui| {
                                    let label = format!("# {channel}");
                                    if is_selected {
                                        ui.rich_label(
                                            &RichText::new()
                                                .colored("#", hash_color)
                                                .colored(&format!(" {channel}"), name_color),
                                        );
                                    } else if ui.text_button(ch_btn_id, &label).clicked {
                                        self.sync_inputs_to_app();
                                        self.app.current_channel = Some(channel.clone());
                                        self.sync_app_to_inputs();
                                    }
                                });
                            });
                    }
                }

                ui.spacing(16.0);
                ui.separator();
                ui.spacing(8.0);

                // ── Members ──
                let member_count = 1 + self.app.connected_peers.len();
                ui.padding(SpacingScale::Lg, |ui| {
                    ui.row(|ui| {
                        ui.header_label("MEMBERS");
                        ui.spacer();
                        ui.label_sized(&format!("{member_count}"), TextSize::Xs);
                    });
                });
                ui.spacing(4.0);

                // Self
                if let Some(ref master) = self.app.master {
                    let our_name = self.app.resolve_display_name(&master.peer_id());
                    let our_ini = initials(&our_name);
                    let fg_muted = ui.theme().fg_muted;
                    ui.padding(SpacingScale::Lg, |ui| {
                        ui.row_spaced(10.0, |ui| {
                            ui.avatar_colored_with_status(
                                &our_ini,
                                26.0,
                                ui.theme().green,
                                Status::Online,
                            );
                            ui.rich_label(
                                &RichText::new().span(&our_name).colored("  you", fg_muted),
                            );
                        });
                    });
                    ui.spacing(2.0);
                }

                // Connected peers
                let peers: Vec<_> = self
                    .app
                    .connected_peers
                    .iter()
                    .map(|(_, pid)| {
                        let name = self.app.resolve_display_name(pid);
                        let is_typing = self.app.typing_peers.iter().any(|(p, t)| {
                            p == pid && t.elapsed() < std::time::Duration::from_secs(5)
                        });
                        (name, is_typing)
                    })
                    .collect();

                for (name, is_typing) in &peers {
                    let pi = initials(name);
                    ui.padding(SpacingScale::Lg, |ui| {
                        ui.row_spaced(10.0, |ui| {
                            ui.avatar_with_status(&pi, 26.0, Status::Online);
                            if *is_typing {
                                ui.rich_label(
                                    &RichText::new()
                                        .span(name)
                                        .colored("  typing\u{2026}", accent),
                                );
                            } else {
                                ui.label(name);
                            }
                        });
                    });
                    ui.spacing(2.0);
                }

                if peers.is_empty() && self.app.master.is_some() {
                    ui.padding(SpacingScale::Lg, |ui| {
                        ui.muted_label("No peers online");
                    });
                }

                ui.spacing(8.0);
            });

            // ── Bottom user panel ──
            ui.separator();
            ui.padding(SpacingScale::Md, |ui| {
                ui.row_spaced(10.0, |ui| {
                    if let Some(ref master) = self.app.master {
                        let name = self.app.resolve_display_name(&master.peer_id());
                        let ui_ini = initials(&name);
                        ui.avatar_colored_with_status(&ui_ini, 28.0, accent, Status::Online);
                        if let Some(ref username) = self.app.username {
                            ui.label_truncated(&format!("@{username}"));
                        } else {
                            ui.label_truncated(&name);
                        }
                    }
                    ui.spacer();
                    let gear_id = id!("settings_btn");
                    if ui.ghost_button(gear_id, "\u{2699}").clicked {
                        self.app.screen = Screen::Settings;
                    }
                    ui.tooltip(gear_id, "Settings");
                });
            });
        });
    }
}
