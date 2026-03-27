use std::collections::HashMap;

use esox_ui::id::fnv1a_runtime;
use esox_ui::{
    Easing, ImageHandle, Rect, RichText, SpacingScale, Span, TextSize, Ui, WidgetStyle, id,
};

use crate::ui::app::VeilApp;
use crate::ui::types::*;

use super::initials;

impl VeilApp {
    pub(crate) fn draw_messages(&mut self, ui: &mut Ui) {
        let member_count = 1 + self.app.connected_peers.len();
        let accent = ui.theme().accent;
        let accent_dim = ui.theme().accent_dim;

        // ── Header bar ──
        ui.surface(|ui| {
            ui.padding(SpacingScale::Lg, |ui| {
                ui.row(|ui| {
                    let channel_label = self.app.current_channel.as_deref().unwrap_or("general");

                    let dim = ui.theme().fg_dim;
                    ui.rich_label(&RichText::new().colored("# ", dim).bold(channel_label));

                    ui.spacer();

                    ui.label_sized(
                        &format!(
                            "{member_count} member{}",
                            if member_count != 1 { "s" } else { "" }
                        ),
                        TextSize::Sm,
                    );

                    ui.spacing(8.0);

                    // Pins toggle
                    let pin_btn_id = id!("pins_toggle");
                    let pin_label = if self.app.show_pins {
                        "\u{1F4CC}\u{2713}"
                    } else {
                        "\u{1F4CC}"
                    };
                    if ui.ghost_button(pin_btn_id, pin_label).clicked {
                        self.app.show_pins = !self.app.show_pins;
                    }
                    ui.tooltip(pin_btn_id, "Show pinned messages");

                    ui.spacing(4.0);

                    let search_id = id!("search_toggle");
                    if ui.ghost_button(search_id, "\u{1F50D}").clicked {
                        self.app.update_toggle_search();
                    }
                    ui.tooltip(search_id, "Search messages (Ctrl+F)");
                });
            });
        });

        // ── Search bar (Ctrl+F) ──
        if self.app.search_active {
            ui.surface(|ui| {
                ui.padding(SpacingScale::Md, |ui| {
                    ui.row_spaced(10.0, |ui| {
                        ui.text_input(
                            id!("search_input"),
                            &mut self.input_search,
                            "Search messages\u{2026}",
                        );
                        let query = self.input_search.text.clone();
                        if query != self.app.search_query {
                            self.app.update_search_query(query);
                        }
                        let count = self.app.search_results.len();
                        let dim = ui.theme().fg_dim;
                        ui.label_colored(
                            &format!("{count} match{}", if count != 1 { "es" } else { "" }),
                            dim,
                        );
                        let close_id = id!("close_search");
                        if ui.ghost_button(close_id, "\u{2715}").clicked {
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
            // Media
            thumbnail_handle: Option<ImageHandle>,
            image_dimensions: Option<(u32, u32)>,
            audio_info: Option<AudioInfo>,
            link_previews: Vec<LinkPreviewInfo>,
            pinned: bool,
            expires_at: Option<i64>,
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
                thumbnail_handle: msg.thumbnail_handle,
                image_dimensions: msg.image_dimensions,
                audio_info: msg.audio_info.clone(),
                link_previews: msg.link_previews.clone(),
                pinned: msg.pinned,
                expires_at: msg.expires_at,
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
        ui.scrollable_fill(id!("msg_scroll"), |ui| {
            ui.padding(SpacingScale::Lg, |ui| {
                if display_msgs.is_empty() && !self.app.search_active {
                    ui.add_space(40.0);
                    ui.empty_state("No messages yet. Start a conversation!");
                    return;
                }

                let region_x = ui.cursor_x();
                let region_w = ui.region_width();

                for msg in &display_msgs {
                    // Track the start of each message for hover detection.
                    let msg_start_y = ui.cursor_y();

                    // ── Date separator ── centered with lines
                    if let Some(ref date) = msg.show_date {
                        ui.spacing(20.0);
                        let w = ui.region_width();
                        ui.center_horizontal(w * 0.4, |ui| {
                            ui.separator();
                            ui.spacing(6.0);
                            let dim = ui.theme().fg_dim;
                            ui.label_colored(date, dim);
                            ui.spacing(6.0);
                            ui.separator();
                        });
                        ui.spacing(12.0);
                    }

                    // ── System message ── muted, centered
                    if msg.is_system {
                        let dim = ui.theme().fg_dim;
                        let w = ui.region_width();
                        ui.center_horizontal(w * 0.7, |ui| {
                            ui.label_colored(&format!("\u{2014}  {}  \u{2014}", msg.content), dim);
                        });
                        ui.spacing(6.0);
                        continue;
                    }

                    // ── Sender header ──
                    if msg.show_sender {
                        ui.spacing(14.0);
                        let sender_initials = initials(&msg.sender);
                        let sender_color = if msg.is_own { accent } else { ui.theme().fg };
                        let dim = ui.theme().fg_dim;
                        ui.row_spaced(10.0, |ui| {
                            ui.avatar(&sender_initials, 34.0);
                            ui.rich_label(
                                &RichText::new()
                                    .colored_bold(&msg.sender, sender_color)
                                    .push(Span {
                                        text: &format!("  {}", msg.timestamp),
                                        color: Some(dim),
                                        bold: false,
                                        size: Some(ui.theme().font_size * 0.85),
                                        letter_spacing: None,
                                        weight: None,
                                        background: None,
                                        decoration: esox_ui::TextDecoration::None,
                                    }),
                            );
                        });
                        ui.spacing(4.0);
                    }

                    // Left-indent message content to align with text after avatar
                    let content_style = WidgetStyle {
                        padding: Some(esox_ui::Spacing {
                            top: 0.0,
                            bottom: 0.0,
                            left: 44.0, // avatar 34 + gap 10
                            right: 0.0,
                        }),
                        ..Default::default()
                    };

                    ui.with_style(content_style, |ui| {
                        // ── Pinned badge ──
                        if msg.pinned {
                            let dim = ui.theme().fg_dim;
                            ui.label_colored("\u{1F4CC} pinned", dim);
                            ui.spacing(2.0);
                        }

                        // ── Ephemeral badge ──
                        if let Some(expires_at) = msg.expires_at {
                            let now = chrono::Utc::now().timestamp();
                            let remaining = expires_at - now;
                            let dim = ui.theme().fg_dim;
                            if remaining > 0 {
                                let label = if remaining > 3600 {
                                    format!("\u{23F3} {}h", remaining / 3600)
                                } else if remaining > 60 {
                                    format!("\u{23F3} {}m", remaining / 60)
                                } else {
                                    format!("\u{23F3} {}s", remaining)
                                };
                                ui.label_colored(&label, dim);
                            } else {
                                ui.label_colored("\u{23F3} expired", dim);
                            }
                            ui.spacing(2.0);
                        }

                        // ── Reply context ──
                        if msg.has_reply {
                            let border = ui.theme().accent;
                            ui.box_container()
                                .bg(accent_dim)
                                .radius(4.0)
                                .padding(8.0)
                                .border(border, 0.0)
                                .show(|ui| {
                                    let muted = ui.theme().fg_muted;
                                    ui.rich_label(
                                        &RichText::new()
                                            .colored_bold(
                                                &format!("  {}", msg.reply_sender),
                                                accent,
                                            )
                                            .colored(&format!("  {}", msg.reply_preview), muted),
                                    );
                                });
                            ui.spacing(4.0);
                        }

                        // ── Content ──
                        if let Some(handle) = msg.thumbnail_handle {
                            // Image/video thumbnail
                            let (w, h) = msg.image_dimensions.unwrap_or((300, 200));
                            let max_w = 300.0f32;
                            let scale = (max_w / w as f32).min(1.0);
                            let display_w = w as f32 * scale;
                            let display_h = h as f32 * scale;

                            if let Some(ref cache) = self.image_cache {
                                let img_id = fnv1a_runtime(&format!("img_{}", msg.idx));
                                ui.image(img_id, cache, handle, display_w, display_h);
                            }

                            // Show file info below thumbnail
                            if let Some(ref fi) = msg.file_info {
                                ui.spacing(4.0);
                                let dim = ui.theme().fg_dim;
                                ui.row_spaced(8.0, |ui| {
                                    ui.label_colored(
                                        &format!("{}x{}", w, h),
                                        dim,
                                    );
                                    ui.label_colored(&fi.size_str, dim);
                                    if fi.status == FileStatus::Available {
                                        let save_id =
                                            fnv1a_runtime(&format!("save_{}", msg.idx));
                                        if ui.text_button(save_id, "Save").clicked {
                                            self.sync_inputs_to_app();
                                            self.app.update_save_file(
                                                fi.blob_id.clone(),
                                                fi.filename.clone(),
                                            );
                                        }
                                    } else if fi.status == FileStatus::Downloading {
                                        ui.spinner();
                                    }
                                });
                            }
                        } else if let Some(ref ai) = msg.audio_info {
                            // Audio waveform display
                            render_waveform(ui, ai, msg.idx, accent, accent_dim);
                        } else if let Some(ref fi) = msg.file_info {
                            let status_icon = match fi.status {
                                FileStatus::Available => "\u{1F4CE}",
                                FileStatus::Downloading => "\u{231B}",
                                FileStatus::Unavailable => "\u{2716}",
                            };
                            ui.card(|ui| {
                                ui.row_spaced(10.0, |ui| {
                                    ui.label(status_icon);
                                    ui.label(&fi.filename);
                                    ui.label_sized(&fi.size_str, TextSize::Sm);
                                    ui.spacer();
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
                                        ui.tooltip(save_id, "Save file to disk");
                                    }
                                });
                            });
                        } else if msg.deleted {
                            let dim = ui.theme().fg_dim;
                            ui.label_colored("[message deleted]", dim);
                        } else if msg.edited {
                            let dim = ui.theme().fg_dim;
                            render_rich_content(ui, &msg.content, msg.idx);
                            ui.rich_label(&RichText::new().push(Span {
                                text: " (edited)",
                                color: Some(dim),
                                bold: false,
                                size: Some(ui.theme().font_size * 0.85),
                                letter_spacing: None,
                                weight: None,
                                background: None,
                                decoration: esox_ui::TextDecoration::None,
                            }));
                        } else {
                            render_rich_content(ui, &msg.content, msg.idx);
                        }

                        // ── Link preview cards ──
                        if !msg.link_previews.is_empty() {
                            ui.spacing(6.0);
                            for (pi, preview) in msg.link_previews.iter().enumerate() {
                                render_link_preview(ui, preview, msg.idx, pi, accent, accent_dim);
                            }
                        }

                        // ── Status indicator ──
                        if msg.is_own && !msg.status_str.is_empty() {
                            let status_color = ui.theme().fg_dim;
                            ui.label_colored(msg.status_str, status_color);
                        }

                        // ── Reactions ──
                        if !msg.reactions.is_empty() {
                            ui.spacing(4.0);
                            ui.row_spaced(4.0, |ui| {
                                for (emoji, count) in &msg.reactions {
                                    let label = if *count > 1 {
                                        format!("{emoji} {count}")
                                    } else {
                                        emoji.clone()
                                    };
                                    let chip_id = fnv1a_runtime(&format!("rx_{}_{emoji}", msg.idx));
                                    ui.small_button(chip_id, &label, accent_dim);
                                }
                            });
                        }

                        // ── Actions — only on hover ──
                        if msg.has_id && !msg.deleted {
                            let idx = msg.idx;
                            let action_id = fnv1a_runtime(&format!("actions_{idx}"));
                            // Compute the actual message rect from cursor tracking.
                            let msg_end_y = ui.cursor_y();
                            let msg_h = (msg_end_y - msg_start_y).max(20.0);
                            let msg_rect = Rect::new(region_x, msg_start_y, region_w, msg_h);
                            let is_hovering = ui.is_hovered(msg_rect);
                            let opacity = ui.animate_bool(
                                action_id,
                                is_hovering,
                                100.0,
                                Easing::EaseOutCubic,
                            );

                            if opacity > 0.02 {
                                let action_style = WidgetStyle {
                                    opacity: Some(opacity),
                                    ..Default::default()
                                };
                                ui.spacing(2.0);
                                ui.with_style(action_style, |ui| {
                                    ui.row_spaced(2.0, |ui| {
                                        let reply_id = fnv1a_runtime(&format!("reply_{idx}"));
                                        if ui.text_button(reply_id, "reply").clicked {
                                            self.app.update_reply_to(idx);
                                        }
                                        ui.tooltip(reply_id, "Reply to message");

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

                                        // Pin/unpin action
                                        let pin_id = fnv1a_runtime(&format!("pin_{idx}"));
                                        let pin_label = if msg.pinned { "unpin" } else { "pin" };
                                        if ui.text_button(pin_id, pin_label).clicked {
                                            self.sync_inputs_to_app();
                                            self.app.update_toggle_pin(idx);
                                        }
                                        ui.tooltip(
                                            pin_id,
                                            if msg.pinned {
                                                "Unpin message"
                                            } else {
                                                "Pin message"
                                            },
                                        );

                                        if msg.is_own {
                                            let edit_id = fnv1a_runtime(&format!("edit_{idx}"));
                                            if ui.text_button(edit_id, "edit").clicked {
                                                self.app.update_edit_message(idx);
                                                self.sync_app_to_inputs();
                                            }
                                            ui.tooltip(edit_id, "Edit message");

                                            let del_id = fnv1a_runtime(&format!("del_{idx}"));
                                            if ui.text_button(del_id, "del").clicked {
                                                self.sync_inputs_to_app();
                                                self.app.update_delete_message(idx);
                                            }
                                            ui.tooltip(del_id, "Delete message");
                                        }
                                    });
                                });
                            }
                        }
                    });
                }

                // ── Typing indicators ──
                if !typing_names.is_empty() {
                    ui.spacing(12.0);
                    for name in &typing_names {
                        let ti = initials(name);
                        ui.row_spaced(10.0, |ui| {
                            ui.avatar(&ti, 20.0);
                            ui.label_colored(&format!("{name} is typing\u{2026}"), accent);
                        });
                        ui.spacing(2.0);
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
            ui.box_container()
                .bg(accent_dim)
                .radius(0.0)
                .padding(0.0)
                .show(|ui| {
                    ui.padding(SpacingScale::Md, |ui| {
                        ui.row_spaced(10.0, |ui| {
                            let muted = ui.theme().fg_muted;
                            ui.rich_label(
                                &RichText::new()
                                    .colored("Replying to ", muted)
                                    .colored_bold(&sender, accent)
                                    .colored(&format!(": {preview}"), muted),
                            );
                            ui.spacer();
                            if ui.ghost_button(id!("cancel_reply"), "\u{2715}").clicked {
                                self.app.replying_to = None;
                            }
                        });
                    });
                });
        }

        // ── Pinned messages panel ──
        if self.app.show_pins {
            let pinned: Vec<(String, String, String)> = self
                .app
                .messages
                .iter()
                .filter(|m| m.pinned && !m.deleted)
                .map(|m| {
                    let preview = if m.content.len() > 80 {
                        format!("{}...", &m.content[..77])
                    } else {
                        m.content.clone()
                    };
                    (m.sender.clone(), preview, m.timestamp.clone())
                })
                .collect();

            ui.box_container()
                .bg(accent_dim)
                .radius(0.0)
                .padding(0.0)
                .show(|ui| {
                    ui.padding(SpacingScale::Md, |ui| {
                        let dim = ui.theme().fg_dim;
                        ui.row(|ui| {
                            ui.label_colored(
                                &format!("\u{1F4CC} Pinned Messages ({})", pinned.len()),
                                accent,
                            );
                            ui.spacer();
                            if ui.ghost_button(id!("close_pins"), "\u{2715}").clicked {
                                self.app.show_pins = false;
                            }
                        });
                        if pinned.is_empty() {
                            ui.spacing(4.0);
                            ui.muted_label("No pinned messages");
                        } else {
                            for (sender, content, ts) in &pinned {
                                ui.spacing(6.0);
                                ui.rich_label(
                                    &RichText::new()
                                        .colored_bold(sender, accent)
                                        .colored(&format!("  {ts}"), dim),
                                );
                                ui.label_wrapped(content);
                            }
                        }
                    });
                });
        }

        // ── Emoji picker ──
        if self.app.emoji_picker_open {
            let raised = ui.theme().bg_raised;
            ui.box_container()
                .bg(raised)
                .radius(0.0)
                .padding(0.0)
                .show(|ui| {
                    ui.padding(SpacingScale::Md, |ui| {
                        ui.row(|ui| {
                            ui.text_input(
                                id!("emoji_search"),
                                &mut self.input_emoji_search,
                                "Search emoji\u{2026}",
                            );
                            ui.spacer();
                            if ui.ghost_button(id!("close_emoji"), "\u{2715}").clicked {
                                self.app.emoji_picker_open = false;
                            }
                        });
                        ui.spacing(8.0);
                        let search = self.input_emoji_search.text.to_lowercase();
                        let emojis = [
                            ("\u{1F44D}", "thumbsup"),
                            ("\u{1F44E}", "thumbsdown"),
                            ("\u{2764}", "heart"),
                            ("\u{1F602}", "joy"),
                            ("\u{1F525}", "fire"),
                            ("\u{1F440}", "eyes"),
                            ("\u{1F389}", "tada"),
                            ("\u{1F914}", "thinking"),
                            ("\u{1F60D}", "heart_eyes"),
                            ("\u{1F622}", "cry"),
                            ("\u{1F44B}", "wave"),
                            ("\u{1F64F}", "pray"),
                            ("\u{1F680}", "rocket"),
                            ("\u{2705}", "check"),
                            ("\u{274C}", "x"),
                            ("\u{1F4AF}", "100"),
                            ("\u{1F60E}", "sunglasses"),
                            ("\u{1F4A1}", "bulb"),
                            ("\u{1F3C6}", "trophy"),
                            ("\u{1F916}", "robot"),
                        ];
                        ui.row_spaced(2.0, |ui| {
                            for (emoji, name) in &emojis {
                                if !search.is_empty() && !name.contains(&search) {
                                    continue;
                                }
                                let eid = fnv1a_runtime(&format!("ep_{name}"));
                                if ui.ghost_button(eid, emoji).clicked {
                                    self.input_message.text.push_str(emoji);
                                    self.input_message.cursor = self.input_message.text.len();
                                    self.app.emoji_picker_open = false;
                                }
                                ui.tooltip(eid, &format!(":{name}:"));
                            }
                        });
                        // Custom emoji section
                        if !self.app.custom_emoji.is_empty() {
                            ui.spacing(8.0);
                            ui.separator();
                            ui.spacing(4.0);
                            let dim = ui.theme().fg_dim;
                            ui.label_colored("Custom", dim);
                            ui.spacing(4.0);
                            ui.row_spaced(2.0, |ui| {
                                let customs: Vec<_> = self.app.custom_emoji.clone();
                                for (shortcode, _blob_id) in &customs {
                                    if !search.is_empty()
                                        && !shortcode.to_lowercase().contains(&search)
                                    {
                                        continue;
                                    }
                                    let eid = fnv1a_runtime(&format!("ce_{shortcode}"));
                                    let label = format!(":{shortcode}:");
                                    if ui.ghost_button(eid, &label).clicked {
                                        self.input_message.text.push_str(&label);
                                        self.input_message.cursor =
                                            self.input_message.text.len();
                                        self.app.emoji_picker_open = false;
                                    }
                                }
                            });
                        }
                    });
                });
        }

        // ── Input bar ──
        ui.surface(|ui| {
            ui.padding(SpacingScale::Lg, |ui| {
                if self.app.editing_message.is_some() {
                    // Editing mode
                    ui.row_spaced(10.0, |ui| {
                        ui.text_input(
                            id!("msg_input"),
                            &mut self.input_message,
                            "Edit message\u{2026}",
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
                    // Normal input
                    ui.row_spaced(10.0, |ui| {
                        let file_id = id!("file_btn");
                        if ui.ghost_button(file_id, "\u{1F4CE}").clicked {
                            self.spawn_file_picker();
                        }
                        ui.tooltip(file_id, "Attach file");

                        let emoji_id = id!("emoji_btn");
                        if ui.ghost_button(emoji_id, "\u{1F642}").clicked {
                            self.app.emoji_picker_open = !self.app.emoji_picker_open;
                        }
                        ui.tooltip(emoji_id, "Emoji picker");

                        ui.text_input(
                            id!("msg_input"),
                            &mut self.input_message,
                            "Type a message\u{2026}",
                        );

                        // Only show send button when there's text
                        if !self.input_message.text.trim().is_empty()
                            && ui.button(id!("send_btn"), "Send").clicked
                        {
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
/// - `> text` at line start -> blockquote
/// - ` ```lang\n...\n``` ` -> code block with optional language
/// - `||text||` -> spoiler (inline, single-line)
/// - Everything else -> label_wrapped (plain text)
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
            ui.label_wrapped(&rest[start..]);
            return;
        }
    }

    if !rest.is_empty() {
        ui.label_wrapped(rest);
    }
}

/// Render an audio waveform visualization with duration.
fn render_waveform(
    ui: &mut Ui<'_>,
    audio: &AudioInfo,
    msg_idx: usize,
    accent: esox_gfx::Color,
    _accent_dim: esox_gfx::Color,
) {
    let total = audio.duration_secs as u32;
    let m = total / 60;
    let s = total % 60;
    let dur_label = format!("{m}:{s:02}");

    ui.card(|ui| {
        ui.row_spaced(10.0, |ui| {
            // Play button placeholder
            let play_id = fnv1a_runtime(&format!("play_{msg_idx}"));
            ui.ghost_button(play_id, "\u{25B6}");
            ui.tooltip(play_id, "Play audio");

            // Duration
            ui.label(&dur_label);
        });
        ui.spacing(4.0);

        // Render waveform as a text-based visualization using block chars.
        // Maps 64 amplitude samples to unicode bar characters.
        let bar_count = audio.waveform.len().min(64);
        if bar_count > 0 {
            let bars: String = audio
                .waveform
                .iter()
                .take(bar_count)
                .map(|&sample| {
                    let level = sample as usize * 8 / 256;
                    match level {
                        0 => '\u{2581}',
                        1 => '\u{2582}',
                        2 => '\u{2583}',
                        3 => '\u{2584}',
                        4 => '\u{2585}',
                        5 => '\u{2586}',
                        6 => '\u{2587}',
                        _ => '\u{2588}',
                    }
                })
                .collect();
            ui.label_colored(&bars, accent);
        }

        // Status
        if audio.status == FileStatus::Downloading {
            ui.spacing(4.0);
            ui.row_spaced(6.0, |ui| {
                ui.spinner();
                ui.muted_label("Downloading\u{2026}");
            });
        }
    });
}

/// Render a link preview embed card.
fn render_link_preview(
    ui: &mut Ui<'_>,
    preview: &LinkPreviewInfo,
    msg_idx: usize,
    preview_idx: usize,
    accent: esox_gfx::Color,
    _accent_dim: esox_gfx::Color,
) {
    let _ = (msg_idx, preview_idx);
    let bg = ui.theme().bg_raised;
    ui.box_container()
        .bg(bg)
        .radius(6.0)
        .padding(12.0)
        .show(|ui| {
            let dim = ui.theme().fg_dim;

            // Site name
            if let Some(ref site) = preview.site_name {
                ui.label_colored(site, dim);
                ui.spacing(2.0);
            }

            // Title
            if let Some(ref title) = preview.title {
                ui.rich_label(&RichText::new().colored_bold(title, accent));
                ui.spacing(4.0);
            }

            // Description
            if let Some(ref desc) = preview.description {
                let truncated = if desc.len() > 200 {
                    format!("{}...", &desc[..197])
                } else {
                    desc.clone()
                };
                ui.label_wrapped(&truncated);
                ui.spacing(4.0);
            }

            // URL
            let muted = ui.theme().fg_muted;
            let url_display = if preview.url.len() > 60 {
                format!("{}...", &preview.url[..57])
            } else {
                preview.url.clone()
            };
            ui.label_colored(&url_display, muted);
        });
    ui.spacing(4.0);
}
