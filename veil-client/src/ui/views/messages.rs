use std::collections::HashMap;

use iced::widget::{
    Column, button, container, horizontal_space, row, scrollable, text, text_input,
};
use iced::{Element, Length};

use crate::ui::app::App;
use crate::ui::message::Message;
use crate::ui::types::*;

impl App {
    pub(crate) fn view_messages(&self) -> Element<'_, Message> {
        let addr_str = self
            .local_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "starting...".into());

        let display_name = self
            .master
            .as_ref()
            .map(|m| self.resolve_display_name(&m.peer_id()))
            .unwrap_or_else(|| "???".into());

        let relay_indicator = if self.relay_connected { " | relay" } else { "" };
        let health_indicator = match &self.connection_state {
            ConnectionState::Connected(_) => "",
            ConnectionState::Warning(_) => " | [!]",
            ConnectionState::Reconnecting => " | [reconnecting]",
            ConnectionState::Failed(_) => " | [error]",
            ConnectionState::Connecting(_) => " | [connecting]",
            ConnectionState::Disconnected => "",
        };

        let header = row![
            text(
                self.current_channel
                    .as_deref()
                    .map(|c| format!("# {c}"))
                    .unwrap_or_default()
            )
            .size(20),
            horizontal_space(),
            text(format!(
                "{display_name}  |  {addr_str}{relay_indicator}{health_indicator}"
            ))
            .size(12),
        ]
        .padding(12);

        // Search bar (toggled by Ctrl+F or button)
        let search_bar = if self.search_active {
            Some(
                row![
                    text_input("Search messages...", &self.search_query)
                        .on_input(Message::SearchQueryChanged)
                        .padding(6)
                        .width(Length::Fill),
                    text(format!("{} matches", self.search_results.len())).size(11),
                    button(text("X").size(11))
                        .on_press(Message::ToggleSearch)
                        .padding(4),
                ]
                .spacing(8)
                .padding([0, 12]),
            )
        } else {
            None
        };

        // Derive current channel ID for filtering
        let current_channel_id = self
            .current_group
            .as_ref()
            .map(|g| self.current_channel_id(g));

        let our_id = self.master.as_ref().map(|m| m.peer_id());
        let mut messages_col = Column::new().spacing(4).padding(12);
        let mut last_date: Option<String> = None;
        let mut last_sender: Option<String> = None;

        for (idx, msg) in self.messages.iter().enumerate() {
            // Channel filtering: skip messages from other channels
            if let (Some(msg_ch), Some(cur_ch)) = (&msg.channel_id, &current_channel_id)
                && msg_ch != cur_ch
            {
                continue;
            }

            // If searching, only show matching messages
            if self.search_active
                && !self.search_query.is_empty()
                && !self.search_results.contains(&idx)
            {
                continue;
            }

            // Date separator
            if let Some(dt) = msg.datetime {
                let date_str = dt.format("%B %d, %Y").to_string();
                if last_date.as_ref() != Some(&date_str) {
                    messages_col = messages_col.push(
                        container(text(date_str.clone()).size(10))
                            .width(Length::Fill)
                            .center_x(Length::Fill)
                            .padding(8),
                    );
                    last_date = Some(date_str);
                    last_sender = None;
                }
            }

            // System messages - italic/muted style
            if msg.sender == "system" {
                messages_col = messages_col.push(
                    container(text(format!("-- {} --", msg.content)).size(11))
                        .width(Length::Fill)
                        .center_x(Length::Fill)
                        .padding(4),
                );
                last_sender = None;
                continue;
            }

            let show_sender = last_sender.as_ref() != Some(&msg.sender);
            last_sender = Some(msg.sender.clone());

            let mut msg_col = Column::new();

            if show_sender {
                msg_col = msg_col.push(text(&msg.sender).size(11));
            }

            // Reply context (quoted block above the message)
            if let (Some(reply_content), Some(reply_sender)) =
                (&msg.reply_to_content, &msg.reply_to_sender)
            {
                let preview = if reply_content.len() > 60 {
                    format!("{}...", &reply_content[..57])
                } else {
                    reply_content.clone()
                };
                msg_col = msg_col.push(
                    container(text(format!("{reply_sender}: {preview}")).size(10)).padding(4),
                );
            }

            // Message content with edit/delete/status indicators
            let content_widget: Element<'_, Message> = if let Some(ref fi) = msg.file_info {
                // Render file attachment widget
                let status_text = match fi.status {
                    FileStatus::Available => "Ready",
                    FileStatus::Downloading => "Downloading...",
                    FileStatus::Unavailable => "Unavailable",
                };
                let mut file_row = row![
                    text(format!(
                        "[{}] {} ({})",
                        status_text, fi.filename, fi.size_str
                    ))
                    .size(13),
                ]
                .spacing(8);
                if fi.status == FileStatus::Available {
                    file_row = file_row.push(
                        button(text("Save").size(11))
                            .on_press(Message::SaveFile(fi.blob_id.clone(), fi.filename.clone()))
                            .padding(4),
                    );
                }
                file_row.into()
            } else if msg.deleted {
                text("[deleted]").size(13).into()
            } else if msg.edited {
                text(format!("{} (edited)", msg.content)).size(13).into()
            } else {
                text(&msg.content).size(13).into()
            };

            // Send status indicator
            let status_str = match &msg.status {
                Some(MessageStatus::Sending) => " ...",
                Some(MessageStatus::Sent) => " ok",
                Some(MessageStatus::Delivered) => " ok",
                None => "",
            };

            let mut msg_row = row![
                msg_col.push(content_widget),
                horizontal_space(),
                text(format!("{}{status_str}", msg.timestamp)).size(9),
            ]
            .spacing(8);

            // Action buttons: reply, react, edit, delete
            let is_own = msg.sender_id.as_ref() == our_id.as_ref();
            if msg.id.is_some() && !msg.deleted {
                // Reply button for all messages
                msg_row = msg_row.push(
                    button(text("reply").size(9))
                        .on_press(Message::ReplyTo(idx))
                        .padding(2),
                );

                // Quick reaction buttons
                for emoji in &[
                    "\u{1F44D}",
                    "\u{2764}",
                    "\u{1F602}",
                    "\u{1F525}",
                    "\u{1F440}",
                ] {
                    msg_row = msg_row.push(
                        button(text(*emoji).size(11))
                            .on_press(Message::React(idx, emoji.to_string()))
                            .padding(1),
                    );
                }

                if is_own {
                    msg_row = msg_row.push(
                        button(text("edit").size(9))
                            .on_press(Message::EditMessage(idx))
                            .padding(2),
                    );
                    msg_row = msg_row.push(
                        button(text("del").size(9))
                            .on_press(Message::DeleteMessage(idx))
                            .padding(2),
                    );
                }
            }

            messages_col = messages_col.push(msg_row);

            // Reaction badges below message
            if let Some(ref msg_id) = msg.id
                && let Some(reactions) = self.reactions.get(&msg_id.0)
                && !reactions.is_empty()
            {
                // Group reactions by emoji
                let mut counts: HashMap<&str, usize> = HashMap::new();
                for (_, emoji) in reactions {
                    *counts.entry(emoji.as_str()).or_insert(0) += 1;
                }
                let mut reaction_row = iced::widget::Row::new().spacing(4);
                for (emoji, count) in &counts {
                    let label = if *count > 1 {
                        format!("{emoji} {count}")
                    } else {
                        emoji.to_string()
                    };
                    reaction_row = reaction_row.push(container(text(label).size(11)).padding(2));
                }
                messages_col = messages_col.push(reaction_row);
            }
        }

        // Typing indicator with display names
        for (peer, instant) in &self.typing_peers {
            if instant.elapsed() < std::time::Duration::from_secs(5) {
                let name = self.resolve_display_name(peer);
                messages_col = messages_col.push(text(format!("{name} is typing...")).size(11));
            }
        }

        // Reply preview bar above input
        let reply_bar = if let Some(reply_idx) = self.replying_to {
            if let Some(reply_msg) = self.messages.get(reply_idx) {
                let preview = if reply_msg.content.len() > 80 {
                    format!("{}...", &reply_msg.content[..77])
                } else {
                    reply_msg.content.clone()
                };
                Some(
                    row![
                        text(format!("Replying to {}: {preview}", reply_msg.sender)).size(11),
                        horizontal_space(),
                        button(text("X").size(11))
                            .on_press(Message::CancelReply)
                            .padding(2),
                    ]
                    .spacing(8)
                    .padding([4, 12]),
                )
            } else {
                None
            }
        } else {
            None
        };

        // Input row
        let input_row = if self.editing_message.is_some() {
            row![
                text_input("Edit message...", &self.message_input)
                    .on_input(Message::InputChanged)
                    .on_submit(Message::ConfirmEdit)
                    .padding(10)
                    .width(Length::Fill),
                button("Save").on_press(Message::ConfirmEdit).padding(10),
                button("Cancel").on_press(Message::CancelEdit).padding(10),
            ]
            .spacing(8)
            .padding(12)
        } else {
            row![
                text_input("Type a message...", &self.message_input)
                    .on_input(Message::InputChanged)
                    .on_submit(Message::Send)
                    .padding(10)
                    .width(Length::Fill),
                button("File").on_press(Message::PickFile).padding(10),
                button("Send").on_press(Message::Send).padding(10),
            ]
            .spacing(8)
            .padding(12)
        };

        let mut main_col = Column::new().width(Length::Fill).height(Length::Fill);

        main_col = main_col.push(header);
        if let Some(sb) = search_bar {
            main_col = main_col.push(sb);
        }
        main_col = main_col.push(
            scrollable(messages_col)
                .height(Length::Fill)
                .anchor_bottom(),
        );
        if let Some(rb) = reply_bar {
            main_col = main_col.push(rb);
        }
        main_col = main_col.push(input_row);

        main_col.into()
    }
}
