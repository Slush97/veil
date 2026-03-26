use std::collections::HashMap;

use esox_ui::id::fnv1a_runtime;
use esox_ui::{RichText, Ui, id};

use crate::ui::app::VeilApp;
use crate::ui::types::*;

use super::initials;

impl VeilApp {
    pub(crate) fn draw_messages(&mut self, ui: &mut Ui) {
        let member_count = 1 + self.app.connected_peers.len();

        // ── Header bar ──
        ui.surface(|ui| {
            ui.padding(12.0, |ui| {
                ui.row(|ui| {
                    let channel_label = self
                        .app
                        .current_channel
                        .as_deref()
                        .map(|c| format!("# {c}"))
                        .unwrap_or_else(|| "No channel".into());
                    ui.heading(&channel_label);

                    ui.fill_space(0.0);

                    let dim = ui.theme().fg_dim;
                    ui.label_colored(
                        &format!(
                            "{member_count} member{}",
                            if member_count != 1 { "s" } else { "" }
                        ),
                        dim,
                    );

                    if ui.ghost_button(id!("search_toggle"), "\u{1F50D}").clicked {
                        self.app.update_toggle_search();
                    }
                });
            });
        });

        // ── Search bar (Ctrl+F) ──
        if self.app.search_active {
            ui.surface(|ui| {
                ui.padding(8.0, |ui| {
                    ui.row_spaced(8.0, |ui| {
                        ui.text_input(
                            id!("search_input"),
                            &mut self.input_search,
                            "Search messages...",
                        );
                        let query = self.input_search.text.clone();
                        if query != self.app.search_query {
                            self.app.update_search_query(query);
                        }
                        let dim = ui.theme().fg_dim;
                        ui.label_colored(
                            &format!("{} matches", self.app.search_results.len()),
                            dim,
                        );
                        if ui.ghost_button(id!("close_search"), "\u{2715}").clicked {
                            self.app.update_toggle_search();
                            self.sync_app_to_inputs();
                        }
                    });
                });
            });
        }

        // ── Build display data ──
        let current_channel_id = self
            .app
            .current_group
            .as_ref()
            .map(|g| self.app.current_channel_id(g));
        let our_id = self.app.master.as_ref().map(|m| m.peer_id());

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
            if let (Some(msg_ch), Some(cur_ch)) = (&msg.channel_id, &current_channel_id)
                && msg_ch != cur_ch
            {
                continue;
            }

            if self.app.search_active
                && !self.app.search_query.is_empty()
                && !self.app.search_results.contains(&idx)
            {
                continue;
            }

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
                Some(MessageStatus::Sending) => "\u{2022}\u{2022}\u{2022}",
                Some(MessageStatus::Sent) => "\u{2713}",
                Some(MessageStatus::Delivered) => "\u{2713}\u{2713}",
                None => "",
            };

            let (has_reply, reply_sender, reply_preview) =
                if let (Some(rc), Some(rs)) = (&msg.reply_to_content, &msg.reply_to_sender) {
                    let preview = if rc.len() > 60 {
                        format!("{}...", &rc[..57])
                    } else {
                        rc.clone()
                    };
                    (true, rs.clone(), preview)
                } else {
                    (false, String::new(), String::new())
                };

            let reactions = if let Some(ref msg_id) = msg.id {
                if let Some(rx) = self.app.reactions.get(&msg_id.0) {
                    let mut counts: HashMap<&str, usize> = HashMap::new();
                    for (_, emoji) in rx {
                        *counts.entry(emoji.as_str()).or_insert(0) += 1;
                    }
                    counts
                        .into_iter()
                        .map(|(e, c)| (e.to_string(), c))
                        .collect()
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

        let typing_names: Vec<String> = self
            .app
            .typing_peers
            .iter()
            .filter(|(_, t)| t.elapsed() < std::time::Duration::from_secs(5))
            .map(|(p, _)| self.app.resolve_display_name(p))
            .collect();

        // ── Message list ──
        let scroll_h = self.height as f32 - 140.0;
        ui.scrollable(id!("msg_scroll"), scroll_h, |ui| {
            ui.padding(16.0, |ui| {
                if display_msgs.is_empty() && !self.app.search_active {
                    ui.spacing(scroll_h * 0.3);
                    ui.empty_state("No messages yet. Start a conversation!");
                    return;
                }

                let accent = ui.theme().accent;
                let accent_dim = ui.theme().accent_dim;

                for msg in &display_msgs {
                    // Date separator
                    if let Some(ref date) = msg.show_date {
                        ui.spacing(16.0);
                        ui.separator();
                        ui.spacing(4.0);
                        let dim = ui.theme().fg_dim;
                        ui.label_colored(date, dim);
                        ui.spacing(12.0);
                    }

                    // System message
                    if msg.is_system {
                        let dim = ui.theme().fg_dim;
                        ui.label_colored(
                            &format!("\u{2014} {} \u{2014}", msg.content),
                            dim,
                        );
                        ui.spacing(4.0);
                        continue;
                    }

                    // Sender header with avatar
                    if msg.show_sender {
                        ui.spacing(12.0);
                        let sender_initials = initials(&msg.sender);
                        let sender_color = if msg.is_own {
                            accent
                        } else {
                            ui.theme().fg
                        };
                        let dim = ui.theme().fg_dim;
                        ui.row_spaced(8.0, |ui| {
                            ui.avatar(&sender_initials, 32.0);
                            ui.rich_label(
                                &RichText::new()
                                    .colored_bold(&msg.sender, sender_color)
                                    .colored(&format!("  {}", msg.timestamp), dim),
                            );
                        });
                        ui.spacing(2.0);
                    }

                    // Reply context — accent-tinted box
                    if msg.has_reply {
                        ui.box_container()
                            .bg(accent_dim)
                            .radius(4.0)
                            .padding(6.0)
                            .show(|ui| {
                                let muted = ui.theme().fg_muted;
                                ui.rich_label(
                                    &RichText::new()
                                        .colored_bold(&msg.reply_sender, muted)
                                        .colored(
                                            &format!(": {}", msg.reply_preview),
                                            muted,
                                        ),
                                );
                            });
                        ui.spacing(2.0);
                    }

                    // Content
                    if let Some(ref fi) = msg.file_info {
                        let status_icon = match fi.status {
                            FileStatus::Available => "\u{1F4CE}",
                            FileStatus::Downloading => "\u{231B}",
                            FileStatus::Unavailable => "\u{2716}",
                        };
                        ui.card(|ui| {
                            ui.row_spaced(8.0, |ui| {
                                ui.label(
                                    &format!("{status_icon} {}", fi.filename),
                                );
                                let dim = ui.theme().fg_dim;
                                ui.label_colored(&fi.size_str, dim);
                                ui.fill_space(0.0);
                                if fi.status == FileStatus::Available {
                                    let save_id =
                                        fnv1a_runtime(&format!("save_{}", fi.filename));
                                    if ui.ghost_button(save_id, "Save").clicked {
                                        self.sync_inputs_to_app();
                                        self.app.update_save_file(
                                            fi.blob_id.clone(),
                                            fi.filename.clone(),
                                        );
                                    }
                                }
                            });
                        });
                    } else if msg.deleted {
                        let dim = ui.theme().fg_dim;
                        ui.label_colored("[message deleted]", dim);
                    } else if msg.edited {
                        let dim = ui.theme().fg_dim;
                        render_rich_content(ui, &msg.content, msg.idx);
                        ui.rich_label(
                            &RichText::new().colored(" (edited)", dim),
                        );
                    } else {
                        render_rich_content(ui, &msg.content, msg.idx);
                    }

                    // Status
                    if msg.is_own && !msg.status_str.is_empty() {
                        let dim = ui.theme().fg_dim;
                        ui.label_colored(msg.status_str, dim);
                    }

                    // Reactions
                    if !msg.reactions.is_empty() {
                        ui.spacing(2.0);
                        ui.row_spaced(4.0, |ui| {
                            for (emoji, count) in &msg.reactions {
                                let label = if *count > 1 {
                                    format!("{emoji} {count}")
                                } else {
                                    emoji.clone()
                                };
                                let chip_id =
                                    fnv1a_runtime(&format!("rx_{}_{emoji}", msg.idx));
                                ui.small_button(chip_id, &label, accent_dim);
                            }
                        });
                    }

                    // Actions
                    if msg.has_id && !msg.deleted {
                        let idx = msg.idx;
                        ui.spacing(2.0);
                        ui.row_spaced(4.0, |ui| {
                            if ui
                                .text_button(
                                    fnv1a_runtime(&format!("reply_{idx}")),
                                    "reply",
                                )
                                .clicked
                            {
                                self.app.update_reply_to(idx);
                            }

                            for emoji in &[
                                "\u{1F44D}",
                                "\u{2764}",
                                "\u{1F602}",
                                "\u{1F525}",
                                "\u{1F440}",
                            ] {
                                let react_id =
                                    fnv1a_runtime(&format!("react_{idx}_{emoji}"));
                                if ui.text_button(react_id, emoji).clicked {
                                    self.sync_inputs_to_app();
                                    self.app.update_react(idx, emoji.to_string());
                                }
                            }

                            if msg.is_own {
                                if ui
                                    .text_button(
                                        fnv1a_runtime(&format!("edit_{idx}")),
                                        "edit",
                                    )
                                    .clicked
                                {
                                    self.app.update_edit_message(idx);
                                    self.sync_app_to_inputs();
                                }
                                if ui
                                    .text_button(
                                        fnv1a_runtime(&format!("del_{idx}")),
                                        "del",
                                    )
                                    .clicked
                                {
                                    self.sync_inputs_to_app();
                                    self.app.update_delete_message(idx);
                                }
                            }
                        });
                    }
                }

                // Typing indicators
                if !typing_names.is_empty() {
                    ui.spacing(8.0);
                    let accent = ui.theme().accent;
                    for name in &typing_names {
                        let ti = initials(name);
                        ui.row_spaced(8.0, |ui| {
                            ui.avatar(&ti, 20.0);
                            ui.label_colored(&format!("{name} is typing..."), accent);
                        });
                    }
                }
            });
        });

        // ── Reply preview bar ──
        if let Some(reply_idx) = self.app.replying_to
            && let Some(reply_msg) = self.app.messages.get(reply_idx)
        {
            let preview = if reply_msg.content.len() > 80 {
                format!("{}...", &reply_msg.content[..77])
            } else {
                reply_msg.content.clone()
            };
            let sender = reply_msg.sender.clone();
            ui.surface(|ui| {
                ui.padding(8.0, |ui| {
                    ui.row_spaced(8.0, |ui| {
                        let accent = ui.theme().accent;
                        let muted = ui.theme().fg_muted;
                        ui.rich_label(
                            &RichText::new()
                                .colored("Replying to ", muted)
                                .colored_bold(&sender, accent)
                                .colored(&format!(": {preview}"), muted),
                        );
                        ui.fill_space(0.0);
                        if ui.ghost_button(id!("cancel_reply"), "\u{2715}").clicked {
                            self.app.replying_to = None;
                        }
                    });
                });
            });
        }

        // ── Input bar ──
        ui.surface(|ui| {
            ui.padding(12.0, |ui| {
                if self.app.editing_message.is_some() {
                    ui.row_spaced(8.0, |ui| {
                        ui.text_input(
                            id!("msg_input"),
                            &mut self.input_message,
                            "Edit message...",
                        );
                        if ui.button(id!("save_edit"), "Save").clicked {
                            self.sync_inputs_to_app();
                            self.app.update_confirm_edit();
                            self.sync_app_to_inputs();
                        }
                        if ui.ghost_button(id!("cancel_edit"), "Cancel").clicked {
                            self.app.update_cancel_edit();
                            self.sync_app_to_inputs();
                        }
                    });
                } else {
                    ui.row_spaced(8.0, |ui| {
                        if ui.ghost_button(id!("file_btn"), "\u{1F4CE}").clicked {
                            self.spawn_file_picker();
                        }
                        ui.text_input(
                            id!("msg_input"),
                            &mut self.input_message,
                            "Type a message...",
                        );
                        if ui.button(id!("send_btn"), "Send").clicked {
                            self.sync_inputs_to_app();
                            self.app.update_send();
                            self.sync_app_to_inputs();
                        }
                    });
                }
            });
        });
    }
}

/// Render message content with lightweight markdown-style formatting.
///
/// Recognizes:
/// - `> text` at line start → blockquote
/// - ` ```lang\n...\n``` ` → code block with optional language
/// - `||text||` → spoiler (inline, single-line)
/// - Everything else → label_wrapped (plain text)
fn render_rich_content(ui: &mut Ui<'_>, content: &str, msg_index: usize) {
    let lines: Vec<&str> = content.lines().collect();
    let len = lines.len();
    let mut i = 0;

    while i < len {
        let line = lines[i];

        // Code fence: ```lang ... ```
        if line.starts_with("```") {
            let language = line.strip_prefix("```").unwrap().trim();
            let lang = if language.is_empty() {
                None
            } else {
                Some(language)
            };

            // Collect lines until closing ```
            let mut code_lines = Vec::new();
            i += 1;
            while i < len && !lines[i].starts_with("```") {
                code_lines.push(lines[i]);
                i += 1;
            }
            if i < len {
                i += 1; // skip closing ```
            }

            let code = code_lines.join("\n");
            let block_id = fnv1a_runtime(&format!("code_{msg_index}_{i}"));
            if let Some(lang) = lang {
                ui.code_block_lang(block_id, lang, &code);
            } else {
                ui.code_block(block_id, &code);
            }
            continue;
        }

        // Blockquote: lines starting with >
        if line.starts_with("> ") || line == ">" {
            let mut quote_lines = Vec::new();
            while i < len && (lines[i].starts_with("> ") || lines[i] == ">") {
                let stripped = lines[i]
                    .strip_prefix("> ")
                    .unwrap_or(lines[i].strip_prefix('>').unwrap_or(lines[i]));
                quote_lines.push(stripped);
                i += 1;
            }
            let quote_text = quote_lines.join("\n");
            ui.blockquote(|ui| {
                ui.label_wrapped(&quote_text);
            });
            continue;
        }

        // Spoiler: ||hidden text||
        if line.contains("||") {
            render_spoiler_line(ui, line, msg_index, i);
            i += 1;
            continue;
        }

        // Plain text — accumulate consecutive plain lines into one block.
        let start = i;
        while i < len
            && !lines[i].starts_with("```")
            && !lines[i].starts_with("> ")
            && lines[i] != ">"
            && !lines[i].contains("||")
        {
            i += 1;
        }
        let plain = lines[start..i].join("\n");
        if !plain.is_empty() {
            ui.label_wrapped(&plain);
        }
    }
}

/// Handle a single line that may contain `||spoiler||` segments.
fn render_spoiler_line(ui: &mut Ui<'_>, line: &str, msg_index: usize, line_index: usize) {
    let mut rest = line;
    let mut seg = 0;

    while let Some(start) = rest.find("||") {
        // Text before the spoiler
        let before = &rest[..start];
        if !before.is_empty() {
            ui.label_wrapped(before);
        }

        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("||") {
            let hidden = &after_open[..end];
            let spoiler_id = fnv1a_runtime(&format!("spoiler_{msg_index}_{line_index}_{seg}"));
            ui.spoiler(spoiler_id, |ui| {
                ui.label_wrapped(hidden);
            });
            rest = &after_open[end + 2..];
            seg += 1;
        } else {
            // Unmatched || — just render as plain text
            ui.label_wrapped(&rest[start..]);
            return;
        }
    }

    // Remaining text after last spoiler
    if !rest.is_empty() {
        ui.label_wrapped(rest);
    }
}
