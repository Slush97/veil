use std::net::SocketAddr;

use veil_core::{ChannelId, MessageContent, MessageKind};
use veil_net::WireMessage;

use crate::ui::app::App;
use crate::ui::message::NetCommand;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_input_changed(&mut self, value: String) {
        // Send typing indicator
        let presence = self.current_group.as_ref().map(|g| {
            let kind = if value.is_empty() {
                veil_net::PresenceKind::StoppedTyping
            } else {
                veil_net::PresenceKind::Typing
            };
            WireMessage::Presence {
                kind,
                group_id: g.id.clone(),
                sender: self.master_peer_id(),
            }
        });
        if let (Some(wire_msg), Some(tx)) = (presence, &mut self.net_cmd_tx)
            && let Err(e) = tx.try_send(NetCommand::SendPresence(wire_msg))
        {
            tracing::warn!("failed to send typing indicator: {e}");
        }
        self.message_input = value;
    }

    pub(crate) fn update_send(&mut self) {
        if !self.message_input.trim().is_empty() {
            if self.device.is_none() {
                return;
            }
            let fingerprint = self.resolve_display_name(&self.master_peer_id());

            // Build message kind: plain text or reply
            let reply_idx = self.replying_to.take();
            let parent_id = reply_idx
                .and_then(|idx| self.messages.get(idx))
                .and_then(|m| m.id.clone());

            let kind = if let Some(parent_id) = parent_id.clone() {
                MessageKind::Reply {
                    parent_id,
                    content: Box::new(MessageKind::Text(self.message_input.clone())),
                }
            } else {
                MessageKind::Text(self.message_input.clone())
            };

            // Derive channel ID before borrowing self mutably
            let channel_id = self
                .current_group
                .as_ref()
                .map(|g| self.current_channel_id(g));

            let Some(channel_id) = channel_id else {
                return;
            };

            let content = MessageContent {
                kind,
                timestamp: chrono::Utc::now(),
                channel_id: channel_id.clone(),
                expires_at: None,
            };

            // The helper sends via the channel — but for update_send we need to know
            // if the channel was available to decide Sent vs Sending+queue.
            let had_channel = self.net_cmd_tx.is_some();

            if let Some(sealed) = self.seal_send_persist(&content) {
                let mut cm = ChatMessage {
                    id: Some(sealed.id.clone()),
                    sender: fingerprint,
                    sender_id: Some(self.master_peer_id()),
                    content: self.message_input.clone(),
                    timestamp: chrono::Utc::now().format("%H:%M").to_string(),
                    datetime: Some(chrono::Utc::now()),
                    edited: false,
                    deleted: false,
                    status: Some(if had_channel {
                        MessageStatus::Sent
                    } else {
                        MessageStatus::Sending
                    }),
                    reply_to_content: None,
                    reply_to_sender: None,
                    channel_id: Some(channel_id),
                    file_info: None,
                    pinned: false,
                    expires_at: None,
                };

                // If this was a reply, attach context
                if let Some(parent_id) = parent_id
                    && let Some(parent) = self
                        .messages
                        .iter()
                        .find(|m| m.id.as_ref() == Some(&parent_id))
                {
                    cm.reply_to_content = Some(parent.content.clone());
                    cm.reply_to_sender = Some(parent.sender.clone());
                }

                self.messages.push(cm);

                if !had_channel {
                    self.pending_messages.push(sealed);
                }
            }

            self.message_input.clear();
        }
    }

    pub(crate) fn update_select_group(&mut self, name: String) {
        self.current_group = self.groups.iter().find(|g| g.name == name).cloned();
        // Clear unread count for this group
        if let Some(ref group) = self.current_group {
            self.unread_counts.remove(&group.id.0);
        }
        // Load message history for the newly selected group
        self.messages.clear();
        self.load_message_history();
    }

    pub(crate) fn update_connect_to_peer(&mut self) {
        if let Ok(addr) = self.connect_input.parse::<SocketAddr>() {
            self.connection_state = ConnectionState::Connecting(format!("Connecting to {addr}..."));
            if let Some(ref mut tx) = self.net_cmd_tx
                && let Err(e) = tx.try_send(NetCommand::Connect(addr))
            {
                tracing::warn!("failed to send connect command: {e}");
            }
        } else {
            self.connection_state =
                ConnectionState::Failed("Invalid address (use host:port)".into());
        }
        self.connect_input.clear();
    }

    pub(crate) fn update_edit_message(&mut self, idx: usize) {
        if idx < self.messages.len() {
            let msg = &self.messages[idx];
            // Only allow editing own messages
            if msg.sender_id.as_ref() == Some(&self.master_peer_id()) && !msg.deleted {
                self.editing_message = Some(idx);
                self.message_input = msg.content.clone();
            }
        }
    }

    pub(crate) fn update_cancel_edit(&mut self) {
        self.editing_message = None;
        self.message_input.clear();
    }

    pub(crate) fn update_confirm_edit(&mut self) {
        if let Some(idx) = self.editing_message.take() {
            if idx < self.messages.len() && !self.message_input.trim().is_empty() {
                let msg_id = self.messages[idx].id.clone();
                if let Some(msg_id) = msg_id {
                    let content = MessageContent {
                        kind: MessageKind::Edit {
                            target_id: msg_id,
                            new_text: self.message_input.clone(),
                        },
                        timestamp: chrono::Utc::now(),
                        channel_id: ChannelId::new(),
                        expires_at: None,
                    };
                    if self.seal_send_persist(&content).is_some() {
                        self.messages[idx].content = self.message_input.clone();
                        self.messages[idx].edited = true;
                    }
                }
            }
            self.message_input.clear();
        }
    }

    pub(crate) fn update_delete_message(&mut self, idx: usize) {
        if idx < self.messages.len() {
            let is_own = self.messages[idx].sender_id.as_ref() == Some(&self.master_peer_id());
            let msg_id = self.messages[idx].id.clone();
            if is_own && let Some(msg_id) = msg_id {
                let content = MessageContent {
                    kind: MessageKind::Delete { target_id: msg_id },
                    timestamp: chrono::Utc::now(),
                    channel_id: ChannelId::new(),
                    expires_at: None,
                };
                if self.seal_send_persist(&content).is_some() {
                    self.messages[idx].deleted = true;
                    self.messages[idx].content = "[deleted]".into();
                }
            }
        }
    }
}
