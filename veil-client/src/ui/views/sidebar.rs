use esox_ui::id::fnv1a_runtime;
use esox_ui::{RichText, Span, Status, Ui, id};

use crate::ui::app::VeilApp;
use crate::ui::types::*;

use super::initials;

impl VeilApp {
    pub(crate) fn draw_sidebar(&mut self, ui: &mut Ui) {
        let visible_h = self.height as f32;

        ui.surface(|ui| {
            // ── Main scrollable area (reserve 56px for bottom panel) ──
            ui.scrollable(id!("sidebar_scroll"), visible_h - 56.0, |ui| {
                ui.padding(12.0, |ui| {
                    // ── Brand + Connection Status ──
                    let accent = ui.theme().accent;
                    let brand_size = ui.theme().heading_font_size;
                    ui.row(|ui| {
                        ui.rich_label(&RichText::new().push(Span {
                            text: "veil",
                            color: Some(accent),
                            bold: true,
                            size: Some(brand_size),
                            letter_spacing: Some(2.0),
                            weight: None,
                            background: None,
                            decoration: esox_ui::TextDecoration::None,
                        }));
                        ui.fill_space(0.0);
                        if self.app.relay_connected {
                            ui.status_pill_success("Online");
                        } else if !self.app.connected_peers.is_empty() {
                            ui.status_pill_warning("P2P");
                        } else {
                            ui.status_pill_error("Offline");
                        }
                    });

                    ui.spacing(12.0);
                    ui.separator();
                    ui.spacing(12.0);

                    // ── Groups ──
                    ui.header_label("GROUPS");
                    ui.spacing(6.0);

                    let groups: Vec<(String, [u8; 32], usize)> = self
                        .app
                        .groups
                        .iter()
                        .map(|g| {
                            let unread =
                                self.app.unread_counts.get(&g.id.0).copied().unwrap_or(0);
                            (g.name.clone(), g.id.0, unread)
                        })
                        .collect();

                    let current_group_name =
                        self.app.current_group.as_ref().map(|g| g.name.clone());

                    if groups.is_empty() {
                        ui.muted_label("No groups yet");
                    }

                    for (name, _id_bytes, unread) in &groups {
                        let is_selected = current_group_name.as_ref() == Some(name);
                        let btn_id = fnv1a_runtime(&format!("group_{name}"));
                        let gi = initials(name);

                        if is_selected {
                            let accent_dim = ui.theme().accent_dim;
                            ui.box_container()
                                .bg(accent_dim)
                                .radius(6.0)
                                .padding(6.0)
                                .show(|ui| {
                                    ui.row_spaced(8.0, |ui| {
                                        ui.avatar(&gi, 28.0);
                                        ui.label_colored(name, accent);
                                        ui.fill_space(0.0);
                                        if *unread > 0 {
                                            ui.badge(*unread as u32);
                                        }
                                    });
                                });
                        } else {
                            ui.padding(6.0, |ui| {
                                ui.row_spaced(8.0, |ui| {
                                    ui.avatar(&gi, 28.0);
                                    if ui.text_button(btn_id, name).clicked {
                                        self.sync_inputs_to_app();
                                        self.app.update_select_group(name.clone());
                                        self.sync_app_to_inputs();
                                    }
                                    ui.fill_space(0.0);
                                    if *unread > 0 {
                                        ui.badge(*unread as u32);
                                    }
                                });
                            });
                        }
                    }

                    // ── Channels ──
                    let channels = self.app.channels.clone();
                    let current_channel = self.app.current_channel.clone();
                    if !channels.is_empty() {
                        ui.spacing(16.0);
                        ui.header_label("CHANNELS");
                        ui.spacing(6.0);

                        for channel in &channels {
                            let is_selected = current_channel.as_ref() == Some(channel);
                            let label = format!("# {channel}");
                            let ch_btn_id = fnv1a_runtime(&format!("ch_{channel}"));

                            if is_selected {
                                let accent_dim = ui.theme().accent_dim;
                                ui.box_container()
                                    .bg(accent_dim)
                                    .radius(4.0)
                                    .padding(4.0)
                                    .show(|ui| {
                                        ui.label_colored(&label, accent);
                                    });
                            } else {
                                ui.padding(4.0, |ui| {
                                    if ui.text_button(ch_btn_id, &label).clicked {
                                        self.app.current_channel = Some(channel.clone());
                                    }
                                });
                            }
                        }
                    }

                    ui.spacing(16.0);
                    ui.separator();
                    ui.spacing(12.0);

                    // ── Members ──
                    let member_count = 1 + self.app.connected_peers.len();
                    ui.row(|ui| {
                        ui.header_label("MEMBERS");
                        ui.fill_space(0.0);
                        let fg_dim = ui.theme().fg_dim;
                        ui.label_colored(&format!("{member_count}"), fg_dim);
                    });
                    ui.spacing(6.0);

                    // Self
                    if let Some(ref master) = self.app.master {
                        let our_name = self.app.resolve_display_name(&master.peer_id());
                        let our_ini = initials(&our_name);
                        let fg_muted = ui.theme().fg_muted;
                        ui.row_spaced(8.0, |ui| {
                            ui.avatar_with_status(&our_ini, 24.0, Status::Online);
                            ui.rich_label(
                                &RichText::new()
                                    .span(&our_name)
                                    .colored(" (you)", fg_muted),
                            );
                        });
                        ui.spacing(4.0);
                    }

                    // Connected peers
                    let peers: Vec<_> = self
                        .app
                        .connected_peers
                        .iter()
                        .map(|(_, pid)| {
                            let name = self.app.resolve_display_name(pid);
                            let is_typing = self.app.typing_peers.iter().any(|(p, t)| {
                                p == pid
                                    && t.elapsed() < std::time::Duration::from_secs(5)
                            });
                            (name, is_typing)
                        })
                        .collect();

                    for (name, is_typing) in &peers {
                        let pi = initials(name);
                        ui.row_spaced(8.0, |ui| {
                            ui.avatar_with_status(&pi, 24.0, Status::Online);
                            if *is_typing {
                                ui.rich_label(
                                    &RichText::new()
                                        .span(name)
                                        .colored(" typing...", accent),
                                );
                            } else {
                                ui.label(name);
                            }
                        });
                        ui.spacing(4.0);
                    }

                    if peers.is_empty() && self.app.master.is_some() {
                        ui.muted_label("No peers online");
                    }
                });
            });

            // ── Bottom user panel ──
            ui.separator();
            ui.padding(10.0, |ui| {
                let accent = ui.theme().accent;
                ui.row_spaced(8.0, |ui| {
                    if let Some(ref master) = self.app.master {
                        let name = self.app.resolve_display_name(&master.peer_id());
                        let ui_ini = initials(&name);
                        ui.avatar_colored_with_status(&ui_ini, 28.0, accent, Status::Online);
                        if let Some(ref username) = self.app.username {
                            ui.label(&format!("@{username}"));
                        } else {
                            ui.label_truncated(&name);
                        }
                    }
                    ui.fill_space(0.0);
                    if ui.ghost_button(id!("settings_btn"), "\u{2699}").clicked {
                        self.app.screen = Screen::Settings;
                    }
                });
            });
        });
    }
}
