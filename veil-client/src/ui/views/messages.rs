use std::collections::HashMap;

use esox_ui::{Ui, id};
use esox_ui::id::fnv1a_runtime;

use crate::ui::app::VeilApp;
use crate::ui::types::*;

impl VeilApp {
    pub(crate) fn draw_messages(&mut self, ui: &mut Ui) {
        let addr_str = self.app.local_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "starting...".into());

        let display_name = self.app.master.as_ref()
            .map(|m| self.app.resolve_display_name(&m.peer_id()))
            .unwrap_or_else(|| "???".into());

        let relay_indicator = if self.app.relay_connected { " | relay" } else { "" };
        let health_indicator = match &self.app.connection_state {
            ConnectionState::Connected(_) => "",
            ConnectionState::Warning(_) => " | [!]",
            ConnectionState::Reconnecting => " | [reconnecting]",
            ConnectionState::Failed(_) => " | [error]",
            ConnectionState::Connecting(_) => " | [connecting]",
            ConnectionState::Disconnected => "",
        };

        // Header row
        ui.padding(12.0, |ui| {
            ui.row(|ui| {
                let channel_label = self.app.current_channel.as_deref()
                    .map(|c| format!("# {c}"))
                    .unwrap_or_default();
                ui.heading(&channel_label);
                ui.fill_space(0.0);
                ui.muted_label(
                    &format!("{display_name}  |  {addr_str}{relay_indicator}{health_indicator}"),
                );
            });
        });

        // Search bar (toggled by Ctrl+F)
        if self.app.search_active {
            ui.padding(4.0, |ui| {
                ui.row_spaced(8.0, |ui| {
                    ui.text_input(id!("search_input"), &mut self.input_search, "Search messages...");
                    // Sync search query on every change
                    let query = self.input_search.text.clone();
                    if query != self.app.search_query {
                        self.app.update_search_query(query);
                    }
                    ui.muted_label(&format!("{} matches", self.app.search_results.len()));
                    if ui.ghost_button(id!("close_search"), "X").clicked {
                        self.app.update_toggle_search();
                        self.sync_app_to_inputs();
                    }
                });
            });
        }

        // Derive current channel ID for filtering
        let current_channel_id = self.app.current_group.as_ref()
            .map(|g| self.app.current_channel_id(g));

        let our_id = self.app.master.as_ref().map(|m| m.peer_id());

        // Build message display data before entering scrollable (avoids borrow issues)
        struct MsgDisplay {
            idx: usize,
            show_date: Option<String>,
            is_system: bool,
            show_sender: bool,
            sender: String,
            content: String,
            timestamp: String,
            has_reply: bool,
            reply_sender: String,
            reply_preview: String,
            is_own: bool,
            has_id: bool,
            deleted: bool,
            edited: bool,
            status_str: &'static str,
            file_info: Option<FileInfo>,
            reactions: Vec<(String, usize)>,
        }

        let mut display_msgs: Vec<MsgDisplay> = Vec::new();
        let mut last_date: Option<String> = None;
        let mut last_sender: Option<String> = None;

        for (idx, msg) in self.app.messages.iter().enumerate() {
            // Channel filtering
            if let (Some(msg_ch), Some(cur_ch)) = (&msg.channel_id, &current_channel_id) {
                if msg_ch != cur_ch {
                    continue;
                }
            }

            // Search filtering
            if self.app.search_active
                && !self.app.search_query.is_empty()
                && !self.app.search_results.contains(&idx)
            {
                continue;
            }

            // Date separator
            let show_date = if let Some(dt) = msg.datetime {
                let date_str = dt.format("%B %d, %Y").to_string();
                if last_date.as_ref() != Some(&date_str) {
                    last_date = Some(date_str.clone());
                    last_sender = None;
                    Some(date_str)
                } else {
                    None
                }
            } else {
                None
            };

            let is_system = msg.sender == "system";
            let show_sender = !is_system && last_sender.as_ref() != Some(&msg.sender);
            if !is_system {
                last_sender = Some(msg.sender.clone());
            } else {
                last_sender = None;
            }

            let is_own = msg.sender_id.as_ref() == our_id.as_ref();

            let status_str = match &msg.status {
                Some(MessageStatus::Sending) => " ...",
                Some(MessageStatus::Sent) => " ok",
                Some(MessageStatus::Delivered) => " ok",
                None => "",
            };

            // Reply context
            let (has_reply, reply_sender, reply_preview) = if let (Some(rc), Some(rs)) =
                (&msg.reply_to_content, &msg.reply_to_sender)
            {
                let preview = if rc.len() > 60 {
                    format!("{}...", &rc[..57])
                } else {
                    rc.clone()
                };
                (true, rs.clone(), preview)
            } else {
                (false, String::new(), String::new())
            };

            // Reactions for this message
            let reactions = if let Some(ref msg_id) = msg.id {
                if let Some(rx) = self.app.reactions.get(&msg_id.0) {
                    let mut counts: HashMap<&str, usize> = HashMap::new();
                    for (_, emoji) in rx {
                        *counts.entry(emoji.as_str()).or_insert(0) += 1;
                    }
                    counts.into_iter().map(|(e, c)| (e.to_string(), c)).collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            display_msgs.push(MsgDisplay {
                idx,
                show_date,
                is_system,
                show_sender,
                sender: msg.sender.clone(),
                content: msg.content.clone(),
                timestamp: msg.timestamp.clone(),
                has_reply,
                reply_sender,
                reply_preview,
                is_own,
                has_id: msg.id.is_some(),
                deleted: msg.deleted,
                edited: msg.edited,
                status_str,
                file_info: msg.file_info.clone(),
                reactions,
            });
        }

        // Typing peers
        let typing_names: Vec<String> = self.app.typing_peers.iter()
            .filter(|(_, t)| t.elapsed() < std::time::Duration::from_secs(5))
            .map(|(p, _)| self.app.resolve_display_name(p))
            .collect();

        // Message list (scrollable)
        let scroll_h = self.height as f32 - 120.0; // Reserve for header + input
        ui.scrollable(id!("msg_scroll"), scroll_h, |ui| {
            ui.padding(12.0, |ui| {
                for msg in &display_msgs {
                    // Date separator
                    if let Some(ref date) = msg.show_date {
                        ui.spacing(8.0);
                        ui.muted_label(date);
                        ui.spacing(8.0);
                    }

                    // System messages
                    if msg.is_system {
                        ui.muted_label(&format!("-- {} --", msg.content));
                        ui.spacing(4.0);
                        continue;
                    }

                    // Sender name
                    if msg.show_sender {
                        ui.spacing(4.0);
                        ui.header_label(&msg.sender);
                    }

                    // Reply context
                    if msg.has_reply {
                        ui.muted_label(&format!("{}: {}", msg.reply_sender, msg.reply_preview));
                    }

                    // Message content row
                    ui.row_spaced(8.0, |ui| {
                        // Content
                        if let Some(ref fi) = msg.file_info {
                            let status_text = match fi.status {
                                FileStatus::Available => "Ready",
                                FileStatus::Downloading => "Downloading...",
                                FileStatus::Unavailable => "Unavailable",
                            };
                            ui.label(
                                &format!("[{}] {} ({})", status_text, fi.filename, fi.size_str),
                            );
                            if fi.status == FileStatus::Available {
                                let save_id = fnv1a_runtime(&format!("save_{}", fi.filename));
                                if ui.ghost_button(save_id, "Save").clicked {
                                    self.sync_inputs_to_app();
                                    self.app.update_save_file(fi.blob_id.clone(), fi.filename.clone());
                                }
                            }
                        } else if msg.deleted {
                            ui.muted_label("[deleted]");
                        } else if msg.edited {
                            ui.label(&format!("{} (edited)", msg.content));
                        } else {
                            ui.label(&msg.content);
                        }

                        ui.fill_space(0.0);

                        // Timestamp + status
                        ui.muted_label(&format!("{}{}", msg.timestamp, msg.status_str));

                        // Action buttons
                        if msg.has_id && !msg.deleted {
                            let idx = msg.idx;
                            let reply_id = fnv1a_runtime(&format!("reply_{idx}"));
                            if ui.ghost_button(reply_id, "reply").clicked {
                                self.app.update_reply_to(idx);
                            }

                            // Quick reactions
                            for emoji in &["\u{1F44D}", "\u{2764}", "\u{1F602}", "\u{1F525}", "\u{1F440}"] {
                                let react_id = fnv1a_runtime(&format!("react_{idx}_{emoji}"));
                                if ui.ghost_button(react_id, emoji).clicked {
                                    self.sync_inputs_to_app();
                                    self.app.update_react(idx, emoji.to_string());
                                }
                            }

                            if msg.is_own {
                                let edit_id = fnv1a_runtime(&format!("edit_{idx}"));
                                if ui.ghost_button(edit_id, "edit").clicked {
                                    self.app.update_edit_message(idx);
                                    self.sync_app_to_inputs();
                                }
                                let del_id = fnv1a_runtime(&format!("del_{idx}"));
                                if ui.ghost_button(del_id, "del").clicked {
                                    self.sync_inputs_to_app();
                                    self.app.update_delete_message(idx);
                                }
                            }
                        }
                    });

                    // Reaction badges
                    if !msg.reactions.is_empty() {
                        ui.row_spaced(4.0, |ui| {
                            for (emoji, count) in &msg.reactions {
                                let label = if *count > 1 {
                                    format!("{emoji} {count}")
                                } else {
                                    emoji.clone()
                                };
                                ui.muted_label(&label);
                            }
                        });
                    }
                }

                // Typing indicators
                for name in &typing_names {
                    ui.muted_label(&format!("{name} is typing..."));
                }
            });
        });

        // Reply preview bar
        if let Some(reply_idx) = self.app.replying_to {
            if let Some(reply_msg) = self.app.messages.get(reply_idx) {
                let preview = if reply_msg.content.len() > 80 {
                    format!("{}...", &reply_msg.content[..77])
                } else {
                    reply_msg.content.clone()
                };
                let sender = reply_msg.sender.clone();
                ui.padding(4.0, |ui| {
                    ui.row_spaced(8.0, |ui| {
                        ui.muted_label(&format!("Replying to {sender}: {preview}"));
                        ui.fill_space(0.0);
                        if ui.ghost_button(id!("cancel_reply"), "X").clicked {
                            self.app.replying_to = None;
                        }
                    });
                });
            }
        }

        // Input row
        ui.padding(12.0, |ui| {
            if self.app.editing_message.is_some() {
                ui.row_spaced(8.0, |ui| {
                    ui.text_input(id!("msg_input"), &mut self.input_message, "Edit message...");
                    if ui.button(id!("save_edit"), "Save").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_confirm_edit();
                        self.sync_app_to_inputs();
                    }
                    if ui.button(id!("cancel_edit"), "Cancel").clicked {
                        self.app.update_cancel_edit();
                        self.sync_app_to_inputs();
                    }
                });
            } else {
                ui.row_spaced(8.0, |ui| {
                    ui.text_input(id!("msg_input"), &mut self.input_message, "Type a message...");
                    if ui.button(id!("file_btn"), "File").clicked {
                        self.spawn_file_picker();
                    }
                    if ui.button(id!("send_btn"), "Send").clicked {
                        self.sync_inputs_to_app();
                        self.app.update_send();
                        self.sync_app_to_inputs();
                    }
                });
            }
        });
    }
}
