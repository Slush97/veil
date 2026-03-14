use std::net::SocketAddr;

use veil_core::{ChannelId, MessageContent, MessageKind, SealedMessage};
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
        if let (Some(wire_msg), Some(tx)) = (presence, &mut self.net_cmd_tx) {
            let _ = tx.try_send(NetCommand::SendPresence(wire_msg));
        }
        self.message_input = value;
    }

    pub(crate) fn update_send(&mut self) {
        if !self.message_input.trim().is_empty() {
            let Some(device) = self.device.as_ref() else {
                return;
            };
            let fingerprint = self.resolve_display_name(&self.master_peer_id());

            if let Some(ref group) = self.current_group {
                // Build message kind: plain text or reply
                let kind = if let Some(reply_idx) = self.replying_to.take() {
                    if let Some(reply_msg) = self.messages.get(reply_idx) {
                        if let Some(ref parent_id) = reply_msg.id {
                            MessageKind::Reply {
                                parent_id: parent_id.clone(),
                                content: Box::new(MessageKind::Text(self.message_input.clone())),
                            }
                        } else {
                            MessageKind::Text(self.message_input.clone())
                        }
                    } else {
                        MessageKind::Text(self.message_input.clone())
                    }
                } else {
                    MessageKind::Text(self.message_input.clone())
                };

                // Derive deterministic channel ID from group + channel name
                let channel_id = self.current_channel_id(group);

                let content = MessageContent {
                    kind,
                    timestamp: chrono::Utc::now(),
                    channel_id: channel_id.clone(),
                };

                let Ok(ring) = group.key_ring.lock() else {
                    tracing::error!("key ring lock poisoned");
                    return;
                };
                match SealedMessage::seal(&content, ring.current(), &group.id.0, device.identity())
                {
                    Ok(sealed) => {
                        drop(ring);
                        let mut cm = ChatMessage {
                            id: Some(sealed.id.clone()),
                            sender: fingerprint,
                            sender_id: Some(self.master_peer_id()),
                            content: self.message_input.clone(),
                            timestamp: chrono::Utc::now().format("%H:%M").to_string(),
                            datetime: Some(chrono::Utc::now()),
                            edited: false,
                            deleted: false,
                            status: Some(MessageStatus::Sending),
                            reply_to_content: None,
                            reply_to_sender: None,
                            channel_id: Some(channel_id),
                            file_info: None,
                        };

                        // If this was a reply, attach context
                        if let Some(reply_idx) = self.messages.len().checked_sub(1).and_then(|_| {
                            if let MessageKind::Reply { ref parent_id, .. } = content.kind {
                                self.messages
                                    .iter()
                                    .position(|m| m.id.as_ref() == Some(parent_id))
                            } else {
                                None
                            }
                        }) {
                            let parent = &self.messages[reply_idx];
                            cm.reply_to_content = Some(parent.content.clone());
                            cm.reply_to_sender = Some(parent.sender.clone());
                        }

                        self.messages.push(cm);

                        // Send to connected peers + relay, or queue if unavailable
                        let sent = if let Some(ref mut tx) = self.net_cmd_tx {
                            tx.try_send(NetCommand::SendMessage(sealed.clone())).is_ok()
                        } else {
                            false
                        };

                        if sent {
                            // Mark as sent
                            if let Some(last) = self.messages.last_mut() {
                                last.status = Some(MessageStatus::Sent);
                            }
                        } else {
                            // Queue for later delivery
                            self.pending_messages.push(sealed.clone());
                        }

                        // Persist
                        if let Some(ref store) = self.store
                            && let Err(e) = store.store_message(&sealed)
                        {
                            tracing::warn!("failed to persist message: {e}");
                        }
                    }
                    Err(e) => {
                        drop(ring);
                        self.messages
                            .push(ChatMessage::system(format!("encrypt error: {e}")));
                    }
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
            if let Some(ref mut tx) = self.net_cmd_tx {
                let _ = tx.try_send(NetCommand::Connect(addr));
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
                let msg = &self.messages[idx];
                if let Some(ref msg_id) = msg.id {
                    // Send edit as a new sealed message
                    if let Some(ref group) = self.current_group {
                        let Some(device) = self.device.as_ref() else {
                            return;
                        };
                        let content = MessageContent {
                            kind: MessageKind::Edit {
                                target_id: msg_id.clone(),
                                new_text: self.message_input.clone(),
                            },
                            timestamp: chrono::Utc::now(),
                            channel_id: ChannelId::new(),
                        };
                        let Ok(ring) = group.key_ring.lock() else {
                            tracing::error!("key ring lock poisoned");
                            return;
                        };
                        if let Ok(sealed) = SealedMessage::seal(
                            &content,
                            ring.current(),
                            &group.id.0,
                            device.identity(),
                        ) {
                            drop(ring);
                            if let Some(ref mut tx) = self.net_cmd_tx {
                                let _ = tx.try_send(NetCommand::SendMessage(sealed.clone()));
                            }
                            if let Some(ref store) = self.store
                                && let Err(e) = store.store_message(&sealed)
                            {
                                tracing::warn!("failed to persist message: {e}");
                            }
                        }
                    }
                    // Update local display
                    self.messages[idx].content = self.message_input.clone();
                    self.messages[idx].edited = true;
                }
            }
            self.message_input.clear();
        }
    }

    pub(crate) fn update_delete_message(&mut self, idx: usize) {
        if idx < self.messages.len() {
            let msg = &self.messages[idx];
            // Only allow deleting own messages
            if msg.sender_id.as_ref() == Some(&self.master_peer_id())
                && let Some(ref msg_id) = msg.id
            {
                // Send delete as a new sealed message
                if let Some(ref group) = self.current_group {
                    let Some(device) = self.device.as_ref() else {
                        return;
                    };
                    let content = MessageContent {
                        kind: MessageKind::Delete {
                            target_id: msg_id.clone(),
                        },
                        timestamp: chrono::Utc::now(),
                        channel_id: ChannelId::new(),
                    };
                    let Ok(ring) = group.key_ring.lock() else {
                        tracing::error!("key ring lock poisoned");
                        return;
                    };
                    if let Ok(sealed) = SealedMessage::seal(
                        &content,
                        ring.current(),
                        &group.id.0,
                        device.identity(),
                    ) {
                        drop(ring);
                        if let Some(ref mut tx) = self.net_cmd_tx {
                            let _ = tx.try_send(NetCommand::SendMessage(sealed.clone()));
                        }
                        if let Some(ref store) = self.store
                            && let Err(e) = store.store_message(&sealed)
                        {
                            tracing::warn!("failed to persist message: {e}");
                        }
                    }
                }
                // Update local display
                self.messages[idx].deleted = true;
                self.messages[idx].content = "[deleted]".into();
            }
        }
    }
}
