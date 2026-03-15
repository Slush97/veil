use esox_ui::{Ui, id};
use esox_ui::id::fnv1a_runtime;

use crate::ui::app::VeilApp;
use crate::ui::types::*;

impl VeilApp {
    pub(crate) fn draw_sidebar(&mut self, ui: &mut Ui) {
        let visible_h = self.height as f32;
        ui.scrollable(id!("sidebar_scroll"), visible_h, |ui| {
            ui.padding(8.0, |ui| {
                // Show username if registered
                if let Some(ref username) = self.app.username {
                    ui.label(&format!("@{username}"));
                    ui.spacing(8.0);
                }

                // Groups section
                ui.header_label("Groups");
                ui.spacing(4.0);

                let groups: Vec<(String, [u8; 32], usize)> = self.app.groups.iter().map(|g| {
                    let unread = self.app.unread_counts.get(&g.id.0).copied().unwrap_or(0);
                    (g.name.clone(), g.id.0, unread)
                }).collect();

                let current_group_name = self.app.current_group.as_ref().map(|g| g.name.clone());

                for (name, _id_bytes, unread) in &groups {
                    let is_selected = current_group_name.as_ref() == Some(name);
                    let label = if is_selected {
                        format!("> {name}")
                    } else {
                        format!("  {name}")
                    };

                    let btn_id = fnv1a_runtime(&format!("group_{name}"));
                    ui.row_spaced(4.0, |ui| {
                        if ui.button(btn_id, &label).clicked {
                            self.sync_inputs_to_app();
                            self.app.update_select_group(name.clone());
                            self.sync_app_to_inputs();
                        }
                        if *unread > 0 {
                            ui.badge(*unread as u32);
                        }
                    });
                }

                ui.spacing(8.0);

                // Channels section
                ui.header_label("Channels");
                ui.spacing(4.0);

                let channels = self.app.channels.clone();
                let current_channel = self.app.current_channel.clone();
                for channel in &channels {
                    let is_selected = current_channel.as_ref() == Some(channel);
                    let label = if is_selected {
                        format!("# {channel}")
                    } else {
                        format!("  # {channel}")
                    };
                    let btn_id = fnv1a_runtime(&format!("ch_{channel}"));
                    if ui.ghost_button(btn_id, &label).clicked {
                        self.app.current_channel = Some(channel.clone());
                    }
                }

                ui.spacing(8.0);

                // Contact search section
                ui.header_label("Add Contact");
                ui.spacing(4.0);
                ui.text_input(id!("contact_search"), &mut self.input_contact_search, "Search @username");
                ui.spacing(4.0);
                if ui.button(id!("contact_search_btn"), "Search").clicked {
                    self.sync_inputs_to_app();
                    self.app.update_lookup_contact();
                }

                if let Some(ref result) = self.app.contact_search_result {
                    ui.spacing(4.0);
                    match result {
                        ContactSearchResult::Found { username, public_key } => {
                            let un = username.clone();
                            let pk = *public_key;
                            ui.row_spaced(4.0, |ui| {
                                ui.muted_label(&format!("@{un} found"));
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
                            ui.muted_label("Searching...");
                        }
                    }
                }

                ui.spacing(8.0);

                // Members section
                ui.header_label("Members");
                ui.spacing(2.0);
                if let Some(ref master) = self.app.master {
                    let our_name = self.app.resolve_display_name(&master.peer_id());
                    ui.muted_label(&format!("{our_name} (you)"));
                }
                let peers: Vec<_> = self.app.connected_peers.iter().map(|(_, pid)| {
                    let name = self.app.resolve_display_name(pid);
                    let is_typing = self.app.typing_peers.iter().any(|(p, t)| {
                        p == pid && t.elapsed() < std::time::Duration::from_secs(5)
                    });
                    (name, is_typing)
                }).collect();
                for (name, is_typing) in &peers {
                    let status = if *is_typing { " (typing)" } else { " (online)" };
                    ui.muted_label(&format!("{name}{status}"));
                }

                ui.spacing(8.0);

                // Display name input
                ui.header_label("Display Name");
                ui.spacing(4.0);
                ui.text_input(id!("display_name"), &mut self.input_display_name, "Your name");
                ui.spacing(4.0);
                if ui.button(id!("set_display_name"), "Set").clicked {
                    self.sync_inputs_to_app();
                    self.app.update_set_display_name();
                    self.sync_app_to_inputs();
                }

                ui.spacing(8.0);

                // LAN Peers
                let discovered: Vec<_> = self.app.discovered_peers.clone();
                if !discovered.is_empty() {
                    ui.header_label("LAN Peers");
                    ui.spacing(2.0);
                    for (_, addr, fp) in &discovered {
                        let label = self.app.display_names.get(fp)
                            .cloned()
                            .unwrap_or_else(|| fp.clone());
                        let btn_id = fnv1a_runtime(&format!("lan_{addr}"));
                        if ui.ghost_button(btn_id, &label).clicked {
                            self.sync_inputs_to_app();
                            self.app.update_connect_discovered_peer(*addr);
                        }
                    }
                    ui.spacing(8.0);
                }

                // Connect to peer
                ui.header_label("Connect");
                ui.spacing(4.0);
                ui.text_input(id!("connect_addr"), &mut self.input_connect, "host:port");
                ui.spacing(4.0);
                if ui.button(id!("connect_btn"), "Connect").clicked {
                    self.sync_inputs_to_app();
                    self.app.update_connect_to_peer();
                    self.sync_app_to_inputs();
                }
                ui.muted_label(&self.app.connection_state.to_string());

                ui.spacing(8.0);

                // Relay section
                ui.header_label("Relay");
                ui.spacing(4.0);
                ui.text_input(id!("relay_addr"), &mut self.input_relay_addr, "relay host:port");
                ui.spacing(4.0);
                if ui.button(id!("relay_connect"), "Connect Relay").clicked {
                    self.sync_inputs_to_app();
                    self.app.update_connect_to_relay();
                }
                let relay_status = if self.app.relay_connected {
                    "Relay: connected"
                } else {
                    "Relay: disconnected"
                };
                ui.muted_label(relay_status);

                ui.spacing(8.0);

                // Invite section
                ui.header_label("Invite");
                ui.spacing(4.0);
                ui.text_input(id!("invite_pass"), &mut self.input_invite_passphrase, "Passphrase");
                ui.spacing(4.0);
                if ui.button(id!("create_invite"), "Create Invite").clicked {
                    self.sync_inputs_to_app();
                    self.app.update_create_invite();
                }

                if let Some(ref url) = self.app.generated_invite_url {
                    ui.spacing(4.0);
                    ui.muted_label(url);
                }

                ui.spacing(4.0);
                ui.text_input(id!("invite_url"), &mut self.input_invite_url, "Paste invite URL");
                ui.spacing(4.0);
                if ui.button(id!("join_invite"), "Join").clicked {
                    self.sync_inputs_to_app();
                    self.app.update_accept_invite();
                    self.sync_app_to_inputs();
                }

                ui.spacing(8.0);

                // Settings button
                if ui.button(id!("settings_btn"), "Settings").clicked {
                    self.app.screen = Screen::Settings;
                }
            });
        });
    }
}
