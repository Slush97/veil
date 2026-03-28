use std::net::SocketAddr;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

use veil_core::{GroupId, MessageDeduplicator, routing_tag_for_group};
use veil_crypto::{DeviceCertificate, PeerId};
use veil_net::{
    Discovery, DiscoveryEvent, PeerEvent, PeerManager, PresenceKind, RelayClient, RelayEvent,
    WireMessage, create_endpoint,
};
use veil_store::LocalStore;

use crate::state::{AppState, NetCommand};

/// Event names emitted to the frontend via Tauri's event system.
mod events {
    pub const NETWORK_READY: &str = "veil://network-ready";
    pub const PEER_CONNECTED: &str = "veil://peer-connected";
    pub const PEER_DISCONNECTED: &str = "veil://peer-disconnected";
    pub const MESSAGE_RECEIVED: &str = "veil://message-received";
    pub const CONNECTION_FAILED: &str = "veil://connection-failed";
    pub const RELAY_CONNECTED: &str = "veil://relay-connected";
    pub const RELAY_DISCONNECTED: &str = "veil://relay-disconnected";
    pub const TYPING_STARTED: &str = "veil://typing-started";
    pub const TYPING_STOPPED: &str = "veil://typing-stopped";
    pub const LAN_PEER_DISCOVERED: &str = "veil://lan-peer-discovered";
    pub const LAN_PEER_LOST: &str = "veil://lan-peer-lost";
    // Voice events
    pub const VOICE_OFFER: &str = "veil://voice-offer";
    pub const VOICE_ICE_CANDIDATE: &str = "veil://voice-ice-candidate";
    pub const VOICE_PARTICIPANT_JOINED: &str = "veil://voice-participant-joined";
    pub const VOICE_PARTICIPANT_LEFT: &str = "veil://voice-participant-left";
    pub const VOICE_SPEAKING: &str = "veil://voice-speaking";
}

/// Spawn the network worker. Called once after identity is loaded.
///
/// Mirrors `spawn_network_worker` from the Iced client but bridges
/// events to Tauri's event system instead of an mpsc channel.
pub async fn spawn_network_worker(
    app_handle: AppHandle,
    shared_state: Arc<RwLock<AppState>>,
    peer_id: PeerId,
    identity_bytes: [u8; 32],
    groups: Vec<GroupId>,
    device_cert: Option<DeviceCertificate>,
    _blob_store: Option<Arc<LocalStore>>,
) {
    let bind_addr: SocketAddr = "0.0.0.0:0".parse().expect("valid literal address");
    let endpoint = match create_endpoint(bind_addr) {
        Ok(ep) => ep,
        Err(e) => {
            let _ = app_handle.emit(
                events::CONNECTION_FAILED,
                format!("Failed to create endpoint: {e}"),
            );
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
    let peer_event_tx = manager.event_sender();

    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<NetCommand>(100);

    // Store cmd_tx and local_addr in shared state
    {
        let mut s = shared_state.write().await;
        s.net_cmd_tx = Some(cmd_tx);
        s.local_addr = Some(local_addr);
    }

    let _ = app_handle.emit(
        events::NETWORK_READY,
        serde_json::json!({ "localAddr": local_addr.to_string() }),
    );

    // Spawn accept loop
    tokio::spawn(PeerManager::accept_loop(
        endpoint.clone(),
        peer_id.clone(),
        identity_bytes,
        peer_event_tx,
        connections,
        device_cert,
    ));

    // Relay state
    let mut relay_client: Option<RelayClient> = None;
    let mut relay_event_rx: Option<tokio::sync::mpsc::Receiver<RelayEvent>> = None;

    // Dedup
    let mut dedup = MessageDeduplicator::with_capacity(2048);

    // Rate limiting
    let mut rate_limiter = veil_net::PeerRateLimiter::new(veil_net::RateLimitConfig::default());

    // mDNS discovery
    let mut discovery_rx: Option<tokio::sync::mpsc::Receiver<DiscoveryEvent>> = None;
    if let Ok(discovery) = Discovery::new() {
        let fp = peer_id.fingerprint();
        let _ = discovery.register(local_addr.port(), &fp);
        if let Ok(rx) = discovery.browse() {
            discovery_rx = Some(rx);
        }
    }

    // Auto-connect to relay if stored
    {
        let s = shared_state.read().await;
        if let Some(ref store) = s.store {
            if let Ok(Some(relay_addr)) = store.get_setting("relay_addr") {
                if let Ok(addr) = relay_addr.parse::<SocketAddr>() {
                    let routing_tags: Vec<[u8; 32]> =
                        groups.iter().map(|g| routing_tag_for_group(&g.0)).collect();

                    let mut pid_bytes = [0u8; 32];
                    let vk = &peer_id.verifying_key;
                    pid_bytes[..vk.len().min(32)].copy_from_slice(&vk[..vk.len().min(32)]);

                    let (client, rx) = RelayClient::spawn(
                        addr,
                        endpoint.clone(),
                        pid_bytes,
                        identity_bytes,
                        routing_tags,
                    );
                    relay_client = Some(client);
                    relay_event_rx = Some(rx);
                    let _ = app_handle.emit(events::RELAY_CONNECTED, ());
                    let mut sw = shared_state.write().await;
                    sw.relay_connected = true;
                }
            }
        }
    }

    // Main event loop
    loop {
        // Build futures for optional channels
        let relay_event_fut = async {
            if let Some(ref mut rx) = relay_event_rx {
                rx.recv().await
            } else {
                futures::future::pending().await
            }
        };

        let discovery_fut = async {
            if let Some(ref mut rx) = discovery_rx {
                rx.recv().await
            } else {
                futures::future::pending().await
            }
        };

        tokio::select! {
            // P2P peer events
            event = event_rx.recv() => {
                match event {
                    Some(PeerEvent::Connected { conn_id, peer_id: remote_id, .. }) => {
                        let fp = remote_id.fingerprint();
                        tracing::info!("peer connected: {fp}");
                        rate_limiter.add_peer(conn_id);
                        {
                            let mut s = shared_state.write().await;
                            s.connected_peers.push((conn_id, remote_id.clone()));
                        }
                        let _ = app_handle.emit(events::PEER_CONNECTED, serde_json::json!({
                            "peerId": fp,
                            "connId": conn_id,
                        }));
                    }
                    Some(PeerEvent::Disconnected { conn_id }) => {
                        rate_limiter.remove_peer(conn_id);
                        {
                            let mut s = shared_state.write().await;
                            s.connected_peers.retain(|(id, _)| *id != conn_id);
                        }
                        let _ = app_handle.emit(events::PEER_DISCONNECTED, serde_json::json!({
                            "connId": conn_id,
                        }));
                    }
                    Some(PeerEvent::Message { conn_id, message, .. }) => {
                        if !rate_limiter.check(conn_id) {
                            tracing::warn!("rate limited peer {conn_id}, dropping message");
                            continue;
                        }
                        match message {
                            WireMessage::MessagePush(sealed) => {
                                if dedup.check(&sealed).is_ok() {
                                    // Store the message
                                    {
                                        let s = shared_state.read().await;
                                        if let Some(ref store) = s.store {
                                            let _ = store.store_message(&sealed);
                                        }
                                    }
                                    emit_message_received(&app_handle, &shared_state, &sealed).await;
                                }
                            }
                            WireMessage::Presence { kind, group_id, sender } => {
                                match kind {
                                    PresenceKind::Typing => {
                                        let _ = app_handle.emit(events::TYPING_STARTED, serde_json::json!({
                                            "peerId": sender.fingerprint(),
                                            "groupId": hex::encode(group_id.0),
                                        }));
                                    }
                                    PresenceKind::StoppedTyping => {
                                        let _ = app_handle.emit(events::TYPING_STOPPED, serde_json::json!({
                                            "peerId": sender.fingerprint(),
                                        }));
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                    None => break,
                }
            }

            // Relay events
            relay_event = relay_event_fut => {
                match relay_event {
                    Some(RelayEvent::Connected) => {
                        let _ = app_handle.emit(events::RELAY_CONNECTED, ());
                        let mut s = shared_state.write().await;
                        s.relay_connected = true;
                    }
                    Some(RelayEvent::Disconnected(reason)) => {
                        tracing::info!("relay disconnected: {reason}");
                        {
                            let mut s = shared_state.write().await;
                            s.relay_connected = false;
                        }
                        let _ = app_handle.emit(events::RELAY_DISCONNECTED, &reason);
                        relay_client = None;
                        relay_event_rx = None;
                    }
                    Some(RelayEvent::Message { payload, .. }) => {
                        if let Ok(WireMessage::MessagePush(sealed)) = WireMessage::decode(&payload) {
                            if dedup.check(&sealed).is_ok() {
                                {
                                    let s = shared_state.read().await;
                                    if let Some(ref store) = s.store {
                                        let _ = store.store_message(&sealed);
                                    }
                                }
                                emit_message_received(&app_handle, &shared_state, &sealed).await;
                            }
                        }
                    }
                    Some(RelayEvent::MailboxDrained { messages, .. }) => {
                        for envelope in messages {
                            if let Ok(WireMessage::MessagePush(sealed)) = WireMessage::decode(&envelope.payload) {
                                if dedup.check(&sealed).is_ok() {
                                    {
                                        let s = shared_state.read().await;
                                        if let Some(ref store) = s.store {
                                            let _ = store.store_message(&sealed);
                                        }
                                    }
                                    emit_message_received(&app_handle, &shared_state, &sealed).await;
                                }
                            }
                        }
                    }
                    Some(RelayEvent::VoiceOffer { room_id, participant_id, sdp, voice_endpoint, participants }) => {
                        // Store the participant_id assigned by the SFU
                        {
                            let mut s = shared_state.write().await;
                            s.voice.participant_id = Some(participant_id);
                        }
                        let _ = app_handle.emit(events::VOICE_OFFER, serde_json::json!({
                            "roomId": hex::encode(room_id),
                            "participantId": participant_id,
                            "sdp": sdp,
                            "voiceEndpoint": voice_endpoint,
                            "participants": participants.iter().map(hex::encode).collect::<Vec<_>>(),
                        }));
                    }
                    Some(RelayEvent::VoiceIceCandidate { room_id, participant_id, candidate }) => {
                        let _ = app_handle.emit(events::VOICE_ICE_CANDIDATE, serde_json::json!({
                            "roomId": hex::encode(room_id),
                            "participantId": participant_id,
                            "candidate": candidate,
                        }));
                    }
                    Some(RelayEvent::VoiceParticipantJoined { room_id, peer_id_bytes }) => {
                        let peer_hex = hex::encode(peer_id_bytes);
                        // Update participant list in state
                        {
                            let mut s = shared_state.write().await;
                            let display_name = s.display_names.get(&peer_hex).cloned().unwrap_or_else(|| peer_hex.clone());
                            if let Some(ref mut room) = s.voice.current_room {
                                room.participants.push(crate::state::VoiceParticipantInfo {
                                    peer_id: peer_hex.clone(),
                                    display_name,
                                    is_muted: false,
                                    is_speaking: false,
                                });
                            }
                        }
                        let _ = app_handle.emit(events::VOICE_PARTICIPANT_JOINED, serde_json::json!({
                            "roomId": hex::encode(room_id),
                            "peerId": peer_hex,
                        }));
                    }
                    Some(RelayEvent::VoiceParticipantLeft { room_id, peer_id_bytes }) => {
                        let peer_hex = hex::encode(peer_id_bytes);
                        // Remove from participant list
                        {
                            let mut s = shared_state.write().await;
                            if let Some(ref mut room) = s.voice.current_room {
                                room.participants.retain(|p| p.peer_id != peer_hex);
                            }
                        }
                        let _ = app_handle.emit(events::VOICE_PARTICIPANT_LEFT, serde_json::json!({
                            "roomId": hex::encode(room_id),
                            "peerId": peer_hex,
                        }));
                    }
                    Some(RelayEvent::VoiceSpeaking { room_id, peer_id_bytes, audio_level }) => {
                        let peer_hex = hex::encode(peer_id_bytes);
                        let speaking = audio_level > 10;
                        // Update speaking state
                        {
                            let mut s = shared_state.write().await;
                            if let Some(ref mut room) = s.voice.current_room {
                                if let Some(p) = room.participants.iter_mut().find(|p| p.peer_id == peer_hex) {
                                    p.is_speaking = speaking;
                                }
                            }
                        }
                        let _ = app_handle.emit(events::VOICE_SPEAKING, serde_json::json!({
                            "roomId": hex::encode(room_id),
                            "peerId": peer_hex,
                            "audioLevel": audio_level,
                            "speaking": speaking,
                        }));
                    }
                    Some(RelayEvent::Error { code, message }) => {
                        tracing::warn!("relay error: {code}: {message}");
                    }
                    None => {
                        relay_client = None;
                        relay_event_rx = None;
                    }
                    _ => {}
                }
            }

            // mDNS discovery
            discovery_event = discovery_fut => {
                match discovery_event {
                    Some(DiscoveryEvent::PeerFound { instance_name, addr, fingerprint }) => {
                        let _ = app_handle.emit(events::LAN_PEER_DISCOVERED, serde_json::json!({
                            "name": instance_name,
                            "addr": addr.to_string(),
                            "fingerprint": fingerprint,
                        }));
                    }
                    Some(DiscoveryEvent::PeerLost { instance_name }) => {
                        let _ = app_handle.emit(events::LAN_PEER_LOST, &instance_name);
                    }
                    None => {
                        discovery_rx = None;
                    }
                }
            }

            // Commands from UI
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(NetCommand::SendMessage(sealed)) => {
                        let wire_msg = WireMessage::MessagePush(sealed.clone());
                        manager.broadcast(&wire_msg).await;

                        if let Some(ref rc) = relay_client {
                            if let Ok(payload) = wire_msg.encode() {
                                let _ = rc.forward_message(sealed.routing_tag, payload).await;
                            }
                        }
                    }
                    Some(NetCommand::Connect(addr)) => {
                        if let Err(e) = manager.connect(addr).await {
                            tracing::warn!("failed to connect to {addr}: {e}");
                            let _ = app_handle.emit(events::CONNECTION_FAILED, format!("{e}"));
                        }
                    }
                    Some(NetCommand::ConnectRelay(addr)) => {
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
                    Some(NetCommand::SendPresence(wire)) => {
                        manager.broadcast(&wire).await;
                    }
                    Some(NetCommand::SubscribeTag(tag)) => {
                        if let Some(ref rc) = relay_client {
                            let _ = rc.subscribe(vec![tag]).await;
                        }
                    }
                    Some(NetCommand::VoiceJoin { room_id, group_id }) => {
                        if let Some(ref rc) = relay_client {
                            if let Err(e) = rc.voice_join(room_id, group_id).await {
                                tracing::warn!("voice join failed: {e}");
                            }
                        } else {
                            tracing::warn!("voice join requested but no relay client");
                        }
                    }
                    Some(NetCommand::VoiceAnswer { room_id, participant_id, sdp }) => {
                        if let Some(ref rc) = relay_client {
                            if let Err(e) = rc.voice_answer(room_id, participant_id, sdp).await {
                                tracing::warn!("voice answer failed: {e}");
                            }
                        }
                    }
                    Some(NetCommand::VoiceIceCandidate { room_id, participant_id, candidate }) => {
                        if let Some(ref rc) = relay_client {
                            if let Err(e) = rc.voice_ice_candidate(room_id, participant_id, candidate).await {
                                tracing::warn!("voice ice candidate failed: {e}");
                            }
                        }
                    }
                    Some(NetCommand::VoiceLeave { room_id }) => {
                        if let Some(ref rc) = relay_client {
                            if let Err(e) = rc.voice_leave(room_id).await {
                                tracing::warn!("voice leave failed: {e}");
                            }
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

/// Decrypt a received sealed message and emit to frontend.
async fn emit_message_received(
    app_handle: &AppHandle,
    shared_state: &Arc<RwLock<AppState>>,
    sealed: &veil_core::SealedMessage,
) {
    let s = shared_state.read().await;

    for group in &s.groups {
        let ring = match group.key_ring.lock() {
            Ok(r) => r,
            Err(_) => continue,
        };

        let mut known = s.known_master_ids();
        for m in &group.members {
            if !known
                .iter()
                .any(|existing| existing.verifying_key == m.verifying_key)
            {
                known.push(m.clone());
            }
        }

        if let Ok((msg_content, sender)) =
            sealed.verify_and_open_with_keyring(&ring, &known, &group.device_certs)
        {
            let sender_fp = sender.fingerprint();
            let sender_name = s
                .display_names
                .get(&sender_fp)
                .cloned()
                .unwrap_or_else(|| sender_fp.clone());

            let self_fp = s
                .master
                .as_ref()
                .map(|m| m.peer_id().fingerprint())
                .unwrap_or_default();

            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD;

            let mut payload = serde_json::json!({
                "id": hex::encode(sealed.id.0),
                "senderId": sender_fp,
                "senderName": sender_name,
                "timestamp": msg_content.timestamp.timestamp(),
                "isSelf": sender_fp == self_fp,
                "groupId": hex::encode(group.id.0),
                "channelId": msg_content.channel_id.0.to_string(),
            });

            let obj = payload.as_object_mut().unwrap();

            // Unwrap Reply wrapper
            let kind_ref = match &msg_content.kind {
                veil_core::MessageKind::Reply { parent_id, content } => {
                    obj.insert("replyToId".into(), hex::encode(parent_id.0).into());
                    content.as_ref()
                }
                other => other,
            };

            match kind_ref {
                veil_core::MessageKind::Text(txt) => {
                    obj.insert("content".into(), txt.clone().into());
                    obj.insert("kindType".into(), "text".into());
                }
                veil_core::MessageKind::Image { blob_id, width, height, thumbnail, .. } => {
                    obj.insert("kindType".into(), "image".into());
                    obj.insert("blobId".into(), hex::encode(blob_id.0).into());
                    obj.insert("width".into(), (*width).into());
                    obj.insert("height".into(), (*height).into());
                    if !thumbnail.is_empty() {
                        obj.insert("thumbnailB64".into(), b64.encode(thumbnail).into());
                    }
                }
                veil_core::MessageKind::Video { blob_id, duration_secs, thumbnail, .. } => {
                    obj.insert("kindType".into(), "video".into());
                    obj.insert("blobId".into(), hex::encode(blob_id.0).into());
                    obj.insert("durationSecs".into(), (*duration_secs).into());
                    if !thumbnail.is_empty() {
                        obj.insert("thumbnailB64".into(), b64.encode(thumbnail).into());
                    }
                }
                veil_core::MessageKind::Audio { blob_id, duration_secs, waveform, .. } => {
                    obj.insert("kindType".into(), "audio".into());
                    obj.insert("blobId".into(), hex::encode(blob_id.0).into());
                    obj.insert("durationSecs".into(), (*duration_secs).into());
                    obj.insert("waveform".into(), serde_json::json!(waveform));
                }
                veil_core::MessageKind::File { blob_id, filename, size_bytes, .. } => {
                    obj.insert("kindType".into(), "file".into());
                    obj.insert("blobId".into(), hex::encode(blob_id.0).into());
                    obj.insert("filename".into(), filename.clone().into());
                    obj.insert("sizeBytes".into(), (*size_bytes).into());
                }
                _ => {
                    // Control messages, etc — skip emitting
                    break;
                }
            }

            let _ = app_handle.emit(events::MESSAGE_RECEIVED, payload);
            break; // Found the group — stop trying
        }
    }
}
