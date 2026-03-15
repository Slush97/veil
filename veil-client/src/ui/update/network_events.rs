use std::net::SocketAddr;
use std::sync::Arc;

use veil_core::{GroupId, MessageKind, SealedMessage};
use veil_crypto::{DeviceCertificate, GroupKey, GroupKeyRing, PeerId};
use veil_net::ConnectionId;

use crate::ui::app::App;
use crate::ui::message::NetCommand;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_network_ready(
        &mut self,
        local_addr: SocketAddr,
        cmd_tx: tokio::sync::mpsc::Sender<NetCommand>,
    ) {
        self.local_addr = Some(local_addr);
        self.net_cmd_tx = Some(cmd_tx);
        self.connection_state = ConnectionState::Connected(format!("Listening on {local_addr}"));

        // Flush pending messages
        let pending: Vec<SealedMessage> = self.pending_messages.drain(..).collect();
        for sealed in pending {
            if let Some(ref mut tx) = self.net_cmd_tx
                && let Err(e) = tx.try_send(NetCommand::SendMessage(sealed)) {
                    tracing::warn!("failed to flush pending message: {e}");
                }
        }
        // Update status of pending messages to Sent
        for msg in &mut self.messages {
            if msg.status == Some(MessageStatus::Sending) {
                msg.status = Some(MessageStatus::Sent);
            }
        }

        // Auto-connect to relay if we have a saved address
        if !self.relay_addr_input.is_empty()
            && let Ok(addr) = self.relay_addr_input.parse::<SocketAddr>()
            && let Some(ref mut tx) = self.net_cmd_tx
            && let Err(e) = tx.try_send(NetCommand::ConnectRelay(addr)) {
                tracing::warn!("failed to auto-reconnect relay: {e}");
            }
    }

    pub(crate) fn update_peer_connected(
        &mut self,
        conn_id: ConnectionId,
        peer_id: PeerId,
        session_key: [u8; 32],
        device_certificate: Option<DeviceCertificate>,
    ) {
        // Use the DH-derived session key
        let group_key = GroupKey::from_storage_key(session_key);

        let our_key = &self.master_peer_id().verifying_key;
        let their_key = &peer_id.verifying_key;
        let group_id_bytes = {
            let (first, second) = if our_key <= their_key {
                (our_key.as_slice(), their_key.as_slice())
            } else {
                (their_key.as_slice(), our_key.as_slice())
            };
            let mut combined = Vec::with_capacity(first.len() + second.len());
            combined.extend_from_slice(first);
            combined.extend_from_slice(second);
            blake3::derive_key("veil-pairwise-group-id", &combined)
        };

        let master_id = self.master_peer_id().verifying_key.clone();
        let keyring = GroupKeyRing::new(group_key, master_id);

        let mut device_certs = Vec::new();
        if let Some(cert) = device_certificate
            && cert.verify()
        {
            // Store peer's device cert
            if let Some(ref store) = self.store
                && let Err(e) = store.store_device_cert(&cert.device_id, &cert) {
                    tracing::warn!("failed to persist device cert: {e}");
                }
            device_certs.push(cert);
        }

        let group_state = GroupState {
            name: format!("Chat with {}", peer_id.fingerprint()),
            id: GroupId(group_id_bytes),
            key_ring: Arc::new(std::sync::Mutex::new(keyring)),
            device_certs,
            members: vec![self.master_peer_id(), peer_id.clone()],
        };

        self.groups.push(group_state.clone());
        self.current_group = Some(group_state);

        self.connected_peers.push((conn_id, peer_id.clone()));
        self.connection_state =
            ConnectionState::Connected(format!("Connected to {}", peer_id.fingerprint()));
    }

    pub(crate) fn update_peer_data(&mut self, sealed: SealedMessage) {
        // Try all groups to find the one that can decrypt
        let mut decrypted = false;
        let global_members = self.known_master_ids();

        for group in &self.groups {
            let Ok(ring) = group.key_ring.lock() else {
                tracing::error!("key ring lock poisoned");
                continue;
            };
            // Merge global known members with this group's member list
            let mut members = global_members.clone();
            for m in &group.members {
                if !members.iter().any(|existing| existing.verifying_key == m.verifying_key) {
                    members.push(m.clone());
                }
            }
            if let Ok((content, sender)) =
                sealed.verify_and_open_with_keyring(&ring, &members, &group.device_certs)
            {
                drop(ring);

                // Track unread for non-active groups
                let is_active = self
                    .current_group
                    .as_ref()
                    .is_some_and(|g| g.id == group.id);
                let is_own = sender == self.master_peer_id();

                if !is_active && !is_own {
                    *self.unread_counts.entry(group.id.0).or_insert(0) += 1;
                }

                let sender_name = self.resolve_display_name(&sender);

                match content.kind {
                    MessageKind::Text(ref txt) => {
                        let mut cm = ChatMessage::user_with_channel(
                            sealed.id.clone(),
                            sender.clone(),
                            txt.clone(),
                            content.timestamp,
                            content.channel_id.clone(),
                        );
                        cm.sender = sender_name.clone();
                        self.messages.push(cm);

                        // Desktop notification for messages from others
                        if !is_own {
                            self.send_notification(&sender_name, txt);
                        }
                    }
                    MessageKind::Reply {
                        ref parent_id,
                        content: ref reply_content,
                    } => {
                        if let MessageKind::Text(ref txt) = **reply_content {
                            let mut cm = ChatMessage::user_with_channel(
                                sealed.id.clone(),
                                sender.clone(),
                                txt.clone(),
                                content.timestamp,
                                content.channel_id.clone(),
                            );
                            cm.sender = sender_name.clone();
                            // Find parent message for reply context
                            if let Some(parent) = self
                                .messages
                                .iter()
                                .find(|m| m.id.as_ref() == Some(parent_id))
                            {
                                cm.reply_to_content = Some(parent.content.clone());
                                cm.reply_to_sender = Some(parent.sender.clone());
                            }
                            self.messages.push(cm);

                            if !is_own {
                                self.send_notification(&sender_name, txt);
                            }
                        }
                    }
                    MessageKind::Reaction {
                        ref target_id,
                        ref emoji,
                    } => {
                        let key = target_id.0;
                        self.reactions
                            .entry(key)
                            .or_default()
                            .push((sender.clone(), emoji.clone()));
                    }
                    MessageKind::Edit {
                        target_id,
                        new_text,
                    } => {
                        if let Some(msg) = self.messages.iter_mut().find(|m| {
                            m.id.as_ref() == Some(&target_id)
                                && m.sender_id.as_ref() == Some(&sender)
                        }) {
                            msg.content = new_text;
                            msg.edited = true;
                        }
                    }
                    MessageKind::Delete { target_id } => {
                        // Find the target message and mark deleted (only if same sender)
                        if let Some(msg) = self.messages.iter_mut().find(|m| {
                            m.id.as_ref() == Some(&target_id)
                                && m.sender_id.as_ref() == Some(&sender)
                        }) {
                            msg.deleted = true;
                            msg.content = "[deleted]".into();
                        }
                    }
                    MessageKind::Control(ctrl) => {
                        self.handle_control_message(ctrl, sender);
                    }
                    MessageKind::File {
                        ref filename,
                        size_bytes,
                        ref inline_data,
                        ref blob_id,
                        ciphertext_len,
                        ..
                    } => {
                        let size_str = if size_bytes < 1024 {
                            format!("{size_bytes} B")
                        } else if size_bytes < 1_048_576 {
                            format!("{:.1} KB", size_bytes as f64 / 1024.0)
                        } else {
                            format!("{:.1} MB", size_bytes as f64 / 1_048_576.0)
                        };

                        let file_status = if inline_data.is_some() {
                            // Small file — data is right here
                            if let (Some(data), Some(store)) = (inline_data, &self.store)
                                && let Err(e) = store.store_blob_full(blob_id, data)
                            {
                                tracing::warn!("failed to store inline blob: {e}");
                            }
                            FileStatus::Available
                        } else {
                            // Large file — need to fetch blob
                            let have_blob = self
                                .store
                                .as_ref()
                                .and_then(|s| s.get_blob_full(blob_id).ok())
                                .flatten()
                                .is_some();

                            if have_blob {
                                FileStatus::Available
                            } else {
                                // Try to reconstruct from shards
                                let reconstructed = self.store.as_ref().and_then(|store| {
                                    let shards = store.list_blob_shards(blob_id).ok()?;
                                    if shards.len() >= veil_store::blob::DATA_SHARDS {
                                        let shard_opts: Vec<Option<veil_store::BlobShard>> = (0
                                            ..veil_store::blob::TOTAL_SHARDS)
                                            .map(|i| {
                                                shards
                                                    .iter()
                                                    .find(|s| s.shard_index == i as u8)
                                                    .cloned()
                                            })
                                            .collect();
                                        for group in &self.groups {
                                            if let Ok(ring) = group.key_ring.lock()
                                                && let Ok(data) = veil_store::decode_blob(
                                                    &shard_opts,
                                                    ciphertext_len as usize,
                                                    ring.current(),
                                                )
                                            {
                                                return Some(data);
                                            }
                                        }
                                        None
                                    } else {
                                        None
                                    }
                                });

                                if reconstructed.is_some() {
                                    FileStatus::Available
                                } else {
                                    // Request full blob from peers via BlobFullRequest
                                    if let Some(ref mut tx) = self.net_cmd_tx
                                        && let Err(e) = tx.try_send(NetCommand::RequestBlob {
                                            blob_id: blob_id.clone(),
                                        }) {
                                            tracing::warn!("failed to request blob: {e}");
                                        }
                                    FileStatus::Downloading
                                }
                            }
                        };

                        let mut cm = ChatMessage::user(
                            sealed.id.clone(),
                            sender.clone(),
                            format!("[file: {filename} ({size_str})]"),
                            content.timestamp,
                        );
                        cm.file_info = Some(FileInfo {
                            blob_id: blob_id.clone(),
                            filename: filename.clone(),
                            size_str: size_str.clone(),
                            status: file_status,
                        });
                        self.messages.push(cm);
                    }
                    _ => {
                        // Image, Video, etc. — display placeholder
                        self.messages.push(ChatMessage::user(
                            sealed.id.clone(),
                            sender.clone(),
                            "[unsupported message type]".into(),
                            content.timestamp,
                        ));
                    }
                }
                decrypted = true;
                break;
            }
        }

        if !decrypted {
            self.messages
                .push(ChatMessage::system("Failed to decrypt message".into()));
        }

        // Persist incoming message
        if let Some(ref store) = self.store
            && let Err(e) = store.store_message(&sealed)
        {
            tracing::warn!("failed to persist message: {e}");
        }
    }
}
