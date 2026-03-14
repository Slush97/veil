use std::net::SocketAddr;
use std::sync::Arc;

use futures::StreamExt;

use veil_core::GroupId;
use veil_core::{
    MessageDeduplicator, SealedMessage,
    invite::{self, InvitePayload},
    routing_tag_for_group,
};
use veil_crypto::{DeviceCertificate, PeerId};
use veil_net::{
    Discovery, DiscoveryEvent, PeerEvent, PeerManager, RelayClient, RelayEvent, WireMessage,
    create_endpoint,
};
use veil_store::LocalStore;

use super::message::{Message, NetCommand};
use super::types::SharedGroupKey;

pub(crate) fn veil_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("VEIL_DATA_DIR") {
        return std::path::PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("veil")
}

pub(crate) fn network_worker(
    peer_id: PeerId,
    identity_bytes: [u8; 32],
    groups: Vec<GroupId>,
    device_cert: Option<DeviceCertificate>,
    blob_store: Option<Arc<LocalStore>>,
) -> impl Send + futures::Stream<Item = Message> {
    iced::stream::channel(100, move |mut output| async move {
        use futures::SinkExt;

        let bind_addr: SocketAddr = "0.0.0.0:0".parse().expect("valid literal address");
        let endpoint = match create_endpoint(bind_addr) {
            Ok(ep) => ep,
            Err(e) => {
                let _ = output
                    .send(Message::ConnectionFailed(format!(
                        "Failed to create endpoint: {e}"
                    )))
                    .await;
                futures::future::pending::<()>().await;
                return;
            }
        };

        let local_addr = endpoint
            .local_addr()
            .expect("endpoint must have a local address");

        let mut manager = PeerManager::new(endpoint.clone(), peer_id.clone(), identity_bytes);
        if let Some(ref cert) = device_cert {
            manager.set_device_cert(cert.clone());
        }
        let mut event_rx = manager
            .take_event_receiver()
            .expect("event receiver already taken");
        let connections = manager.connections_handle();
        let event_tx = manager.event_sender();

        let (cmd_tx, mut cmd_rx) = futures::channel::mpsc::channel::<NetCommand>(100);

        let _ = output
            .send(Message::NetworkReady { local_addr, cmd_tx })
            .await;

        // Spawn accept loop
        tokio::spawn(PeerManager::accept_loop(
            endpoint.clone(),
            peer_id.clone(),
            identity_bytes,
            event_tx,
            connections,
            device_cert,
        ));

        // Relay client state
        let mut relay_client: Option<RelayClient> = None;
        let mut relay_event_rx: Option<tokio::sync::mpsc::Receiver<RelayEvent>> = None;

        // Dedup to prevent duplicate messages from P2P + relay
        let mut dedup = MessageDeduplicator::with_capacity(2048);

        // mDNS discovery
        let mut discovery_rx: Option<tokio::sync::mpsc::Receiver<DiscoveryEvent>> = None;
        if let Ok(discovery) = Discovery::new() {
            if let Some(ref ep_addr) = Some(local_addr) {
                let fp = peer_id.fingerprint();
                let _ = discovery.register(ep_addr.port(), &fp);
            }
            if let Ok(rx) = discovery.browse() {
                discovery_rx = Some(rx);
            }
        }

        // Main event loop
        loop {
            // Build a future for relay events
            let relay_event_fut = async {
                if let Some(ref mut rx) = relay_event_rx {
                    rx.recv().await
                } else {
                    futures::future::pending().await
                }
            };

            // Build a future for discovery events
            let discovery_fut = async {
                if let Some(ref mut rx) = discovery_rx {
                    rx.recv().await
                } else {
                    futures::future::pending().await
                }
            };

            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(PeerEvent::Connected { conn_id, peer_id, session_key, device_certificate }) => {
                            let _ = output
                                .send(Message::PeerConnected { conn_id, peer_id, session_key, device_certificate })
                                .await;
                        }
                        Some(PeerEvent::Disconnected { conn_id }) => {
                            let _ = output
                                .send(Message::PeerDisconnected { conn_id })
                                .await;
                        }
                        Some(PeerEvent::Message { conn_id, message, .. }) => {
                            match message {
                                WireMessage::MessagePush(sealed) => {
                                    if dedup.check(&sealed).is_ok() {
                                        let _ = output
                                            .send(Message::PeerData { sealed })
                                            .await;
                                    }
                                }
                                WireMessage::MessageSync { group_id, since } => {
                                    // Respond with messages from our store
                                    if let Some(ref store) = blob_store {
                                        let tag = routing_tag_for_group(&group_id.0);
                                        if let Ok(msgs) = store.list_messages_by_tag(&tag, 100, 0) {
                                            let filtered: Vec<SealedMessage> = if let Some(ref since_id) = since {
                                                msgs.into_iter().filter(|m| m.id != *since_id && m.sent_at > 0).collect()
                                            } else {
                                                msgs
                                            };
                                            let _ = manager.send_to(conn_id, &WireMessage::MessageBatch {
                                                group_id,
                                                messages: filtered,
                                                has_more: false,
                                            }).await;
                                        }
                                    }
                                }
                                WireMessage::MessageBatch { messages, .. } => {
                                    for sealed in messages {
                                        if dedup.check(&sealed).is_ok() {
                                            let _ = output
                                                .send(Message::PeerData { sealed })
                                                .await;
                                        }
                                    }
                                }
                                WireMessage::BlobFullRequest { blob_id } => {
                                    let _ = output
                                        .send(Message::BlobRequested { conn_id, blob_id })
                                        .await;
                                }
                                WireMessage::BlobFull { blob_id, data } => {
                                    if let Some(ref store) = blob_store
                                        && let Err(e) = store.store_blob_full(&blob_id, &data) {
                                            tracing::warn!("failed to store received blob: {e}");
                                        }
                                    let _ = output
                                        .send(Message::BlobReceived { blob_id })
                                        .await;
                                }
                                WireMessage::BlobShard(shard) => {
                                    if let Some(ref store) = blob_store {
                                        let _ = store.store_blob_shard(&shard);
                                    }
                                }
                                WireMessage::Presence { kind, group_id, sender } => {
                                    match kind {
                                        veil_net::PresenceKind::Typing => {
                                            let _ = output
                                                .send(Message::TypingStarted {
                                                    peer_id: sender,
                                                    group_id,
                                                })
                                                .await;
                                        }
                                        veil_net::PresenceKind::StoppedTyping => {
                                            let _ = output
                                                .send(Message::TypingStopped {
                                                    peer_id: sender,
                                                })
                                                .await;
                                        }
                                        veil_net::PresenceKind::ReadReceipt { last_read } => {
                                            let _ = output
                                                .send(Message::ReadReceipt {
                                                    peer_id: sender,
                                                    last_read,
                                                })
                                                .await;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        None => break,
                    }
                }
                relay_event = relay_event_fut => {
                    match relay_event {
                        Some(RelayEvent::Connected) => {
                            let _ = output.send(Message::RelayConnected).await;
                        }
                        Some(RelayEvent::Disconnected(reason)) => {
                            let _ = output.send(Message::RelayDisconnected(reason)).await;
                        }
                        Some(RelayEvent::Message { routing_tag: _, payload }) => {
                            // Try to decode as WireMessage
                            if let Ok(WireMessage::MessagePush(sealed)) = WireMessage::decode(&payload)
                                && dedup.check(&sealed).is_ok() {
                                    let _ = output
                                        .send(Message::PeerData { sealed })
                                        .await;
                                }
                        }
                        Some(RelayEvent::Error { code, message }) => {
                            let _ = output.send(Message::RelayError { code, message }).await;
                        }
                        Some(RelayEvent::MailboxDrained { messages, .. }) => {
                            for envelope in messages {
                                if let Ok(WireMessage::MessagePush(sealed)) = WireMessage::decode(&envelope.payload)
                                    && dedup.check(&sealed).is_ok() {
                                        let _ = output
                                            .send(Message::PeerData { sealed })
                                            .await;
                                    }
                            }
                        }
                        None => {
                            relay_client = None;
                            relay_event_rx = None;
                        }
                    }
                }
                discovery_event = discovery_fut => {
                    match discovery_event {
                        Some(DiscoveryEvent::PeerFound { instance_name, addr, fingerprint }) => {
                            let _ = output.send(Message::LanPeerDiscovered {
                                name: instance_name,
                                addr,
                                fingerprint,
                            }).await;
                        }
                        Some(DiscoveryEvent::PeerLost { instance_name }) => {
                            let _ = output.send(Message::LanPeerLost(instance_name)).await;
                        }
                        None => {
                            discovery_rx = None;
                        }
                    }
                }
                cmd = cmd_rx.next() => {
                    match cmd {
                        Some(NetCommand::Connect(addr)) => {
                            if let Err(e) = manager.connect(addr).await {
                                let _ = output
                                    .send(Message::ConnectionFailed(e.to_string()))
                                    .await;
                            }
                        }
                        Some(NetCommand::SendMessage(sealed)) => {
                            // Broadcast via P2P
                            let wire_msg = WireMessage::MessagePush(sealed.clone());
                            manager.broadcast(&wire_msg).await;

                            // Also forward via relay if connected
                            if let Some(ref rc) = relay_client
                                && let Ok(payload) = wire_msg.encode() {
                                    let _ = rc.forward_message(sealed.routing_tag, payload).await;
                                }
                        }
                        Some(NetCommand::ConnectRelay(addr)) => {
                            // Compute routing tags for all current groups
                            let tags: Vec<[u8; 32]> = groups
                                .iter()
                                .map(|g| routing_tag_for_group(&g.0))
                                .collect();

                            let mut pid_bytes = [0u8; 32];
                            let vk = &peer_id.verifying_key;
                            pid_bytes[..vk.len().min(32)].copy_from_slice(&vk[..vk.len().min(32)]);

                            let (rc, rx) = RelayClient::spawn(
                                addr,
                                endpoint.clone(),
                                pid_bytes,
                                identity_bytes,
                                tags,
                            );
                            relay_client = Some(rc);
                            relay_event_rx = Some(rx);
                        }
                        Some(NetCommand::CreateInvite {
                            group_id,
                            group_name,
                            relay_addr,
                            passphrase,
                            group_key,
                        }) => {
                            match invite::create_open_invite(
                                group_id,
                                group_name,
                                relay_addr,
                                &group_key,
                                passphrase.as_bytes(),
                            ) {
                                Ok(payload) => match payload.to_url() {
                                    Ok(url) => {
                                        let _ = output.send(Message::InviteCreated(url)).await;
                                    }
                                    Err(e) => {
                                        let _ = output.send(Message::InviteFailed(e.to_string())).await;
                                    }
                                },
                                Err(e) => {
                                    let _ = output.send(Message::InviteFailed(e.to_string())).await;
                                }
                            }
                        }
                        Some(NetCommand::AcceptInvite { url, passphrase }) => {
                            match InvitePayload::from_url(&url) {
                                Ok(payload) => {
                                    match invite::accept_invite(&payload, passphrase.as_bytes()) {
                                        Ok(group_key) => {
                                            // Subscribe to the new group's routing tag on relay
                                            let tag = routing_tag_for_group(&payload.group_id.0);
                                            if let Some(ref rc) = relay_client {
                                                let _ = rc.subscribe(vec![tag]).await;
                                            }
                                            let _ = output
                                                .send(Message::InviteAccepted {
                                                    group_name: payload.group_name,
                                                    group_id: payload.group_id,
                                                    group_key: SharedGroupKey(Arc::new(group_key)),
                                                })
                                                .await;
                                        }
                                        Err(e) => {
                                            let _ = output.send(Message::InviteFailed(e.to_string())).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = output.send(Message::InviteFailed(e.to_string())).await;
                                }
                            }
                        }
                        Some(NetCommand::SendFile {
                            path,
                            group_id,
                            group_key,
                            store,
                            identity_bytes: id_bytes,
                        }) => {
                            let filename = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file")
                                .to_string();

                            match std::fs::read(&path) {
                                Ok(file_data) => {
                                    let size_bytes = file_data.len() as u64;

                                    if file_data.len() < veil_store::INLINE_THRESHOLD {
                                        // Small file: encrypt and send inline
                                        let ciphertext = match group_key.encrypt(&file_data) {
                                            Ok(ct) => ct,
                                            Err(e) => {
                                                let _ = output.send(Message::FileFailed(e.to_string())).await;
                                                continue;
                                            }
                                        };
                                        let blob_id = veil_core::BlobId(
                                            *blake3::hash(&ciphertext).as_bytes(),
                                        );
                                        let ciphertext_len = ciphertext.len() as u64;
                                        let content = veil_core::MessageContent {
                                            kind: veil_core::MessageKind::File {
                                                blob_id,
                                                filename: filename.clone(),
                                                size_bytes,
                                                ciphertext_len,
                                                inline_data: Some(ciphertext),
                                            },
                                            timestamp: chrono::Utc::now(),
                                            channel_id: veil_core::ChannelId::new(),
                                        };
                                        let identity = veil_crypto::Identity::from_bytes(&id_bytes);
                                        match veil_core::SealedMessage::seal(&content, &group_key, &group_id.0, &identity) {
                                            Ok(sealed) => {
                                                let wire_msg = WireMessage::MessagePush(sealed.clone());
                                                manager.broadcast(&wire_msg).await;
                                                if let Some(ref rc) = relay_client
                                                    && let Ok(payload) = wire_msg.encode() {
                                                        let _ = rc.forward_message(sealed.routing_tag, payload).await;
                                                    }
                                                if let Err(e) = store.store_message(&sealed) {
                                                    tracing::warn!("failed to persist message: {e}");
                                                }
                                                let _ = output.send(Message::FileSent { filename }).await;
                                            }
                                            Err(e) => {
                                                let _ = output.send(Message::FileFailed(e.to_string())).await;
                                            }
                                        }
                                    } else {
                                        // Large file: encrypt, store full copy, shard, send message reference
                                        let ciphertext = match group_key.encrypt(&file_data) {
                                            Ok(ct) => ct,
                                            Err(e) => {
                                                let _ = output.send(Message::FileFailed(e.to_string())).await;
                                                continue;
                                            }
                                        };
                                        let blob_id = veil_core::BlobId(
                                            *blake3::hash(&ciphertext).as_bytes(),
                                        );
                                        let ciphertext_len = ciphertext.len() as u64;

                                        // Store full encrypted blob locally (never lose it)
                                        let _ = store.store_blob_full(&blob_id, &ciphertext);

                                        // Erasure-code into shards and store them too
                                        if let Ok((shards, _)) = veil_store::encode_blob(&file_data, &group_key) {
                                            for shard in &shards {
                                                let _ = store.store_blob_shard(shard);
                                            }
                                            // Broadcast shards to peers
                                            for shard in shards {
                                                let _ = manager.broadcast(&WireMessage::BlobShard(shard)).await;
                                            }
                                        }

                                        // Send the message referencing the blob (no inline data)
                                        let content = veil_core::MessageContent {
                                            kind: veil_core::MessageKind::File {
                                                blob_id,
                                                filename: filename.clone(),
                                                size_bytes,
                                                ciphertext_len,
                                                inline_data: None,
                                            },
                                            timestamp: chrono::Utc::now(),
                                            channel_id: veil_core::ChannelId::new(),
                                        };
                                        let identity = veil_crypto::Identity::from_bytes(&id_bytes);
                                        match veil_core::SealedMessage::seal(&content, &group_key, &group_id.0, &identity) {
                                            Ok(sealed) => {
                                                let wire_msg = WireMessage::MessagePush(sealed.clone());
                                                manager.broadcast(&wire_msg).await;
                                                if let Some(ref rc) = relay_client
                                                    && let Ok(payload) = wire_msg.encode() {
                                                        let _ = rc.forward_message(sealed.routing_tag, payload).await;
                                                    }
                                                if let Err(e) = store.store_message(&sealed) {
                                                    tracing::warn!("failed to persist message: {e}");
                                                }
                                                let _ = output.send(Message::FileSent { filename }).await;
                                            }
                                            Err(e) => {
                                                let _ = output.send(Message::FileFailed(e.to_string())).await;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = output.send(Message::FileFailed(format!("read error: {e}"))).await;
                                }
                            }
                        }
                        Some(NetCommand::SendPresence(wire_msg)) => {
                            manager.broadcast(&wire_msg).await;
                        }
                        Some(NetCommand::RequestBlob { blob_id }) => {
                            // Broadcast BlobFullRequest to all peers
                            manager.broadcast(&WireMessage::BlobFullRequest { blob_id }).await;
                        }
                        Some(NetCommand::BlobResponse { conn_id, blob_id, data }) => {
                            let _ = manager.send_to(
                                conn_id,
                                &WireMessage::BlobFull { blob_id, data },
                            ).await;
                        }
                        None => break,
                    }
                }
            }
        }
    })
}
