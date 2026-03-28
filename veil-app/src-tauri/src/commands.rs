use std::sync::Arc;

use base64::Engine;
use chrono::Utc;
use serde::Serialize;
use tauri::State;
use tokio::sync::RwLock;

use veil_core::{GroupId, MessageContent, MessageKind, routing_tag_for_group};
use veil_crypto::{DeviceIdentity, GroupKey, GroupKeyRing, MasterIdentity};

use crate::state::{AppState, VoiceRoomInfo, veil_data_dir};

// ── Serializable types for the frontend ──

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInfo {
    pub master_peer_id: String,
    pub device_name: String,
    pub username: Option<String>,
    pub display_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub member_count: usize,
    pub unread_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub name: String,
    pub is_active: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageInfo {
    pub id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub content: String,
    pub timestamp: i64,
    pub is_self: bool,
    pub channel_id: Option<String>,
    pub reply_to_sender: Option<String>,
    pub reply_to_preview: Option<String>,
    // Media fields (populated for image/video/file/audio messages)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waveform: Option<Vec<u8>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub state: String,
    pub relay_connected: bool,
    pub peer_count: usize,
    pub local_addr: Option<String>,
}

// ── Version ──

#[tauri::command]
pub fn get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ── Identity commands ──

#[tauri::command]
pub async fn has_identity() -> bool {
    veil_data_dir().join("identity.veil").exists()
}

#[tauri::command]
pub async fn create_identity(
    username: String,
    passphrase: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<String, String> {
    let state = state.inner().clone();

    if username.len() < 3 || username.len() > 20 {
        return Err("Username must be 3-20 characters".into());
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err("Username may only contain alphanumeric characters and underscores".into());
    }

    let (master, phrase) = MasterIdentity::generate();
    let device_name = gethostname::gethostname().to_string_lossy().to_string();
    let device = DeviceIdentity::new(&master, device_name);

    let keystore = veil_data_dir().join("identity.veil");
    std::fs::create_dir_all(veil_data_dir()).map_err(|e| e.to_string())?;
    veil_crypto::save_device_identity(master.entropy(), &device, passphrase.as_bytes(), &keystore)
        .map_err(|e| format!("Failed to save identity: {e}"))?;

    let mut s = state.write().await;
    s.master = Some(master);
    s.device = Some(device);
    s.username = Some(username.clone());
    s.setup_after_identity();

    if let Some(ref store) = s.store {
        let _ = store.store_setting("username", &username);
    }

    Ok(phrase)
}

#[tauri::command]
pub async fn load_identity(
    passphrase: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<IdentityInfo, String> {
    let state = state.inner().clone();

    let keystore = veil_data_dir().join("identity.veil");
    if !keystore.exists() {
        return Err("No identity file found".into());
    }

    let (master, device) =
        veil_crypto::load_device_identity(passphrase.as_bytes(), &keystore)
            .map_err(|e| format!("Failed to load identity: {e}"))?;

    let peer_id = master.peer_id();
    let fingerprint = peer_id.fingerprint();
    let device_name = device.certificate().device_name.clone();

    let mut s = state.write().await;
    s.master = Some(master);
    s.device = Some(device);
    s.setup_after_identity();

    let username = s.username.clone();
    let display_name = s
        .display_names
        .get(&fingerprint)
        .cloned()
        .unwrap_or_else(|| fingerprint.clone());

    Ok(IdentityInfo {
        master_peer_id: fingerprint,
        device_name,
        username,
        display_name,
    })
}

#[tauri::command]
pub async fn dev_login(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<IdentityInfo, String> {
    let state = state.inner().clone();

    let (master, _phrase) = MasterIdentity::generate();
    let device = DeviceIdentity::new(&master, "dev".into());
    let fingerprint = master.peer_id().fingerprint();

    let mut s = state.write().await;
    s.master = Some(master);
    s.device = Some(device);
    s.setup_after_identity();

    Ok(IdentityInfo {
        master_peer_id: fingerprint,
        device_name: "dev".into(),
        username: None,
        display_name: "dev".into(),
    })
}

#[tauri::command]
pub async fn get_identity(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Option<IdentityInfo>, String> {
    let state = state.inner().clone();
    let s = state.read().await;
    let Some(master) = s.master.as_ref() else {
        return Ok(None);
    };
    let Some(device) = s.device.as_ref() else {
        return Ok(None);
    };
    let fingerprint = master.peer_id().fingerprint();

    Ok(Some(IdentityInfo {
        master_peer_id: fingerprint.clone(),
        device_name: device.certificate().device_name.clone(),
        username: s.username.clone(),
        display_name: s
            .display_names
            .get(&fingerprint)
            .cloned()
            .unwrap_or(fingerprint),
    }))
}

// ── Group commands ──

#[tauri::command]
pub async fn get_groups(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<GroupInfo>, String> {
    let state = state.inner().clone();
    let s = state.read().await;
    Ok(s.groups
        .iter()
        .map(|g| {
            let unread = s.unread_counts.get(&g.id.0).copied().unwrap_or(0);
            GroupInfo {
                id: hex::encode(g.id.0),
                name: g.name.clone(),
                member_count: g.members.len(),
                unread_count: unread,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn set_active_group(
    index: usize,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let mut s = state.write().await;
    if index >= s.groups.len() {
        return Err("Invalid group index".into());
    }
    s.current_group_idx = Some(index);
    let group_id = s.groups[index].id.0;
    s.unread_counts.remove(&group_id);

    // Switch to this group's channels
    let group_hex = hex::encode(group_id);
    s.channels = s
        .group_channels
        .get(&group_hex)
        .cloned()
        .unwrap_or_else(|| vec!["general".into(), "random".into()]);
    s.current_channel = Some("general".into());

    Ok(())
}

/// Create a new group (server).
#[tauri::command]
pub async fn create_group(
    name: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<GroupInfo, String> {
    if name.is_empty() || name.len() > 100 {
        return Err("Group name must be 1-100 characters".into());
    }

    let state = state.inner().clone();
    let mut s = state.write().await;
    let master = s.master.as_ref().ok_or("Not logged in")?;
    let master_id = master.peer_id().verifying_key.clone();
    let peer_id = master.peer_id();

    let group_key = GroupKey::generate();
    let group_id_bytes = blake3::derive_key(
        "veil-group-id",
        &bincode::serialize(&(&name, &peer_id, chrono::Utc::now().timestamp_nanos_opt()))
            .unwrap_or_default(),
    );
    let keyring = GroupKeyRing::new(group_key, master_id);

    if let Some(ref store) = s.store {
        store
            .store_group_v2(&group_id_bytes, &name, &keyring)
            .map_err(|e| format!("Failed to save group: {e}"))?;
    }

    let group_state = crate::state::GroupState {
        name: name.clone(),
        id: GroupId(group_id_bytes),
        key_ring: std::sync::Arc::new(std::sync::Mutex::new(keyring)),
        device_certs: Vec::new(),
        members: Vec::new(),
    };

    s.groups.push(group_state);
    let idx = s.groups.len() - 1;
    s.current_group_idx = Some(idx);

    // Set up default channels for this group
    let group_hex = hex::encode(group_id_bytes);
    let default_channels = vec!["general".into(), "random".into()];
    s.group_channels
        .insert(group_hex.clone(), default_channels.clone());
    s.channels = default_channels;
    s.current_channel = Some("general".into());

    if let Some(ref store) = s.store {
        let json = serde_json::to_string(&s.channels).unwrap_or_default();
        let _ = store.store_setting(&format!("channels:{group_hex}"), &json);
    }

    // Subscribe to routing tag on relay
    if let Some(ref tx) = s.net_cmd_tx {
        let tag = routing_tag_for_group(&group_id_bytes);
        let _ = tx
            .try_send(crate::state::NetCommand::SubscribeTag(tag));
    }

    Ok(GroupInfo {
        id: hex::encode(group_id_bytes),
        name,
        member_count: 0,
        unread_count: 0,
    })
}

/// Delete a group (server). Only removes it locally.
#[tauri::command]
pub async fn delete_group(
    group_id: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let mut s = state.write().await;

    let id_bytes: [u8; 32] = hex::decode(&group_id)
        .map_err(|_| "Invalid group ID")?
        .try_into()
        .map_err(|_| "Invalid group ID length")?;

    let idx = s
        .groups
        .iter()
        .position(|g| g.id.0 == id_bytes)
        .ok_or("Group not found")?;

    s.groups.remove(idx);
    s.group_channels.remove(&group_id);

    // Adjust current group index
    if s.groups.is_empty() {
        s.current_group_idx = None;
        s.channels.clear();
        s.current_channel = None;
    } else {
        let new_idx = idx.min(s.groups.len() - 1);
        s.current_group_idx = Some(new_idx);
        let group_hex = hex::encode(s.groups[new_idx].id.0);
        s.channels = s
            .group_channels
            .get(&group_hex)
            .cloned()
            .unwrap_or_else(|| vec!["general".into(), "random".into()]);
        s.current_channel = Some("general".into());
    }

    if let Some(ref store) = s.store {
        let _ = store.delete_group_v2(&id_bytes);
        let _ = store.store_setting(&format!("channels:{group_id}"), "");
    }

    Ok(())
}

/// Rename a group.
#[tauri::command]
pub async fn rename_group(
    group_id: String,
    new_name: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    if new_name.is_empty() || new_name.len() > 100 {
        return Err("Group name must be 1-100 characters".into());
    }

    let state = state.inner().clone();
    let mut s = state.write().await;

    let id_bytes: [u8; 32] = hex::decode(&group_id)
        .map_err(|_| "Invalid group ID")?
        .try_into()
        .map_err(|_| "Invalid group ID length")?;

    let group = s
        .groups
        .iter_mut()
        .find(|g| g.id.0 == id_bytes)
        .ok_or("Group not found")?;

    group.name = new_name.clone();
    let key_ring = group.key_ring.clone();

    // Re-persist with updated name
    if let Some(ref store) = s.store {
        let ring = key_ring.lock().map_err(|_| "Key ring lock poisoned")?;
        let _ = store.store_group_v2(&id_bytes, &new_name, &ring);
    }

    Ok(())
}

// ── Channel commands ──

/// Create a new channel in the current group.
#[tauri::command]
pub async fn create_channel(
    name: String,
    _kind: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let name = name.to_lowercase().replace(' ', "-");
    if name.is_empty() || name.len() > 50 {
        return Err("Channel name must be 1-50 characters".into());
    }

    let state = state.inner().clone();
    let mut s = state.write().await;
    let group = s.current_group().ok_or("No active group")?;
    let group_hex = hex::encode(group.id.0);

    if s.channels.contains(&name) {
        return Err("Channel already exists".into());
    }

    s.channels.push(name.clone());
    let channels_copy = s.channels.clone();
    s.group_channels.insert(group_hex.clone(), channels_copy);

    if let Some(ref store) = s.store {
        let json = serde_json::to_string(&s.channels).unwrap_or_default();
        let _ = store.store_setting(&format!("channels:{group_hex}"), &json);
    }

    Ok(())
}

/// Delete a channel from the current group.
#[tauri::command]
pub async fn delete_channel(
    name: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let mut s = state.write().await;
    let group = s.current_group().ok_or("No active group")?;
    let group_hex = hex::encode(group.id.0);

    if name == "general" {
        return Err("Cannot delete the general channel".into());
    }

    s.channels.retain(|c| c != &name);
    let channels_copy = s.channels.clone();
    s.group_channels.insert(group_hex.clone(), channels_copy);

    // If we deleted the active channel, switch to general
    if s.current_channel.as_deref() == Some(&name) {
        s.current_channel = Some("general".into());
    }

    if let Some(ref store) = s.store {
        let json = serde_json::to_string(&s.channels).unwrap_or_default();
        let _ = store.store_setting(&format!("channels:{group_hex}"), &json);
    }

    Ok(())
}

#[tauri::command]
pub async fn get_channels(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<ChannelInfo>, String> {
    let state = state.inner().clone();
    let s = state.read().await;
    Ok(s.channels
        .iter()
        .map(|name| ChannelInfo {
            name: name.clone(),
            is_active: s.current_channel.as_deref() == Some(name.as_str()),
        })
        .collect())
}

#[tauri::command]
pub async fn set_active_channel(
    name: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let mut s = state.write().await;
    if !s.channels.contains(&name) {
        return Err("Channel not found".into());
    }
    s.current_channel = Some(name);
    Ok(())
}

// ── Message commands ──

#[tauri::command]
pub async fn get_messages(
    limit: Option<usize>,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<MessageInfo>, String> {
    let state = state.inner().clone();
    let s = state.read().await;
    let store = s.store.as_ref().ok_or("Store not initialized")?;
    let group = s.current_group().ok_or("No active group")?;

    let routing_tag = routing_tag_for_group(&group.id.0);
    let sealed_messages = store
        .list_messages_by_tag(&routing_tag, limit.unwrap_or(500), 0)
        .map_err(|e| format!("Failed to load messages: {e}"))?;

    let mut known = s.known_master_ids();
    for m in &group.members {
        if !known
            .iter()
            .any(|existing| existing.verifying_key == m.verifying_key)
        {
            known.push(m.clone());
        }
    }

    let ring = group.key_ring.lock().map_err(|_| "Key ring lock poisoned")?;
    let self_fp = s
        .master
        .as_ref()
        .map(|m| m.peer_id().fingerprint())
        .unwrap_or_default();

    let mut messages: Vec<MessageInfo> = Vec::new();
    for sealed in &sealed_messages {
        if let Ok((msg_content, sender)) =
            sealed.verify_and_open_with_keyring(&ring, &known, &group.device_certs)
        {
            let sender_fp = sender.fingerprint();
            let sender_name = s
                .display_names
                .get(&sender_fp)
                .cloned()
                .unwrap_or_else(|| sender_fp.clone());
            let ts = msg_content.timestamp.timestamp();
            let ch = msg_content.channel_id.0.to_string();
            let is_self = sender_fp == self_fp;

            // Handle reply wrapper
            let (kind_ref, reply_to_sender, reply_to_preview) = match &msg_content.kind {
                MessageKind::Reply { parent_id, content: reply_kind } => {
                    let parent_hex = hex::encode(parent_id.0);
                    let (rts, rtp) = messages
                        .iter()
                        .find(|m: &&MessageInfo| m.id == parent_hex)
                        .map(|p| (Some(p.sender_name.clone()), Some(p.content.chars().take(60).collect::<String>())))
                        .unwrap_or((None, None));
                    (reply_kind.as_ref(), rts, rtp)
                }
                other => (other, None, None),
            };

            let mut info = message_info_from_kind(
                sealed,
                &sender_fp,
                &sender_name,
                ts,
                is_self,
                Some(ch),
                kind_ref,
            );
            info.reply_to_sender = reply_to_sender;
            info.reply_to_preview = reply_to_preview;

            // Skip control messages and other non-displayable types
            if info.kind_type.is_some() {
                messages.push(info);
            }
        }
    }

    Ok(messages)
}

#[tauri::command]
pub async fn send_message(
    text: String,
    reply_to_id: Option<String>,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<MessageInfo, String> {
    let state = state.inner().clone();
    let s = state.read().await;
    let master = s.master.as_ref().ok_or("Not logged in")?;
    let group = s.current_group().ok_or("No active group")?;
    let channel_id = s.current_channel_id(group);
    let channel_id_str = channel_id.0.to_string();

    let kind = if let Some(ref parent_hex) = reply_to_id {
        let parent_bytes: [u8; 32] = hex::decode(parent_hex)
            .map_err(|_| "Invalid reply ID")?
            .try_into()
            .map_err(|_| "Invalid reply ID length")?;
        MessageKind::Reply {
            parent_id: veil_core::MessageId(parent_bytes),
            content: Box::new(MessageKind::Text(text.clone())),
        }
    } else {
        MessageKind::Text(text.clone())
    };

    let now = Utc::now();
    let content = MessageContent {
        kind,
        timestamp: now,
        channel_id,
        expires_at: None,
    };

    let sealed = s
        .seal_send_persist(&content)
        .ok_or("Failed to seal message")?;

    let fp = master.peer_id().fingerprint();
    let display_name = s
        .display_names
        .get(&fp)
        .cloned()
        .unwrap_or_else(|| s.username.clone().unwrap_or_else(|| fp.clone()));

    Ok(MessageInfo {
        id: hex::encode(sealed.id.0),
        sender_id: fp,
        sender_name: display_name,
        content: text,
        timestamp: now.timestamp(),
        is_self: true,
        channel_id: Some(channel_id_str),
        reply_to_sender: None,
        reply_to_preview: None,
        kind_type: Some("text".into()),
        blob_id: None,
        width: None,
        height: None,
        thumbnail_b64: None,
        filename: None,
        size_bytes: None,
        duration_secs: None,
        waveform: None,
    })
}

// ── File / media commands ──

/// Send a file from disk. Detects media type, generates thumbnails, encrypts, stores, and sends.
#[tauri::command]
pub async fn send_file(
    file_path: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<MessageInfo, String> {
    use veil_core::media;

    let data = std::fs::read(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;
    let filename = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let media_type = media::detect(&data, Some(&filename));
    let file_size = data.len() as u64;

    let state = state.inner().clone();
    let s = state.read().await;
    let master = s.master.as_ref().ok_or("Not logged in")?;
    let group = s.current_group().ok_or("No active group")?;
    let channel_id = s.current_channel_id(group);
    let channel_id_str = channel_id.0.to_string();

    // Encrypt the blob with the group key
    let ring = group.key_ring.lock().map_err(|_| "Key ring lock poisoned")?;
    let group_key = ring.current();
    let blob_id = veil_core::BlobId(*blake3::hash(&data).as_bytes());
    let encrypted = group_key.encrypt(&data).map_err(|e| format!("Encrypt failed: {e}"))?;
    drop(ring);

    // Store the full blob locally
    if let Some(ref store) = s.store {
        let _ = store.store_blob_full(&blob_id, &encrypted);
    }

    // Build the appropriate MessageKind
    let kind = match media_type {
        media::MediaType::Image => {
            let meta = media::extract_image_meta(&data);
            let (width, height, thumbnail) = meta
                .map(|m| (m.width, m.height, m.thumbnail))
                .unwrap_or((0, 0, Vec::new()));
            MessageKind::Image {
                blob_id: blob_id.clone(),
                width,
                height,
                thumbnail,
                ciphertext_len: encrypted.len() as u64,
            }
        }
        media::MediaType::Video => {
            // For video, try to use image thumbnail extraction on the raw bytes
            // (won't work for most videos, but we send it anyway)
            MessageKind::Video {
                blob_id: blob_id.clone(),
                duration_secs: 0.0,
                thumbnail: Vec::new(),
                ciphertext_len: encrypted.len() as u64,
            }
        }
        media::MediaType::Audio => {
            let meta = media::extract_audio_meta(&data);
            let (duration, waveform) = meta
                .map(|m| (m.duration_secs, m.waveform))
                .unwrap_or((0.0, vec![0u8; 64]));
            MessageKind::Audio {
                blob_id: blob_id.clone(),
                duration_secs: duration,
                waveform,
                ciphertext_len: encrypted.len() as u64,
            }
        }
        media::MediaType::File => {
            let inline = if data.len() < veil_store::blob::INLINE_THRESHOLD {
                Some(encrypted.clone())
            } else {
                None
            };
            MessageKind::File {
                blob_id: blob_id.clone(),
                filename: filename.clone(),
                size_bytes: file_size,
                ciphertext_len: encrypted.len() as u64,
                inline_data: inline,
            }
        }
    };

    let now = Utc::now();
    let content = MessageContent {
        kind: kind.clone(),
        timestamp: now,
        channel_id,
        expires_at: None,
    };

    let sealed = s.seal_send_persist(&content).ok_or("Failed to seal message")?;

    let fp = master.peer_id().fingerprint();
    let display_name = s
        .display_names
        .get(&fp)
        .cloned()
        .unwrap_or_else(|| s.username.clone().unwrap_or_else(|| fp.clone()));

    Ok(message_info_from_kind(
        &sealed,
        &fp,
        &display_name,
        now.timestamp(),
        true,
        Some(channel_id_str),
        &kind,
    ))
}

/// Send raw bytes (from clipboard paste) as a media message.
#[tauri::command]
pub async fn send_file_bytes(
    data: Vec<u8>,
    filename: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<MessageInfo, String> {
    use veil_core::media;

    let media_type = media::detect(&data, Some(&filename));
    let file_size = data.len() as u64;

    let state = state.inner().clone();
    let s = state.read().await;
    let master = s.master.as_ref().ok_or("Not logged in")?;
    let group = s.current_group().ok_or("No active group")?;
    let channel_id = s.current_channel_id(group);
    let channel_id_str = channel_id.0.to_string();

    let ring = group.key_ring.lock().map_err(|_| "Key ring lock poisoned")?;
    let group_key = ring.current();
    let blob_id = veil_core::BlobId(*blake3::hash(&data).as_bytes());
    let encrypted = group_key.encrypt(&data).map_err(|e| format!("Encrypt failed: {e}"))?;
    drop(ring);

    if let Some(ref store) = s.store {
        let _ = store.store_blob_full(&blob_id, &encrypted);
    }

    let kind = match media_type {
        media::MediaType::Image => {
            let meta = media::extract_image_meta(&data);
            let (width, height, thumbnail) = meta
                .map(|m| (m.width, m.height, m.thumbnail))
                .unwrap_or((0, 0, Vec::new()));
            MessageKind::Image {
                blob_id: blob_id.clone(),
                width,
                height,
                thumbnail,
                ciphertext_len: encrypted.len() as u64,
            }
        }
        media::MediaType::Audio => {
            let meta = media::extract_audio_meta(&data);
            let (duration, waveform) = meta
                .map(|m| (m.duration_secs, m.waveform))
                .unwrap_or((0.0, vec![0u8; 64]));
            MessageKind::Audio {
                blob_id: blob_id.clone(),
                duration_secs: duration,
                waveform,
                ciphertext_len: encrypted.len() as u64,
            }
        }
        _ => {
            let inline = if data.len() < veil_store::blob::INLINE_THRESHOLD {
                Some(encrypted.clone())
            } else {
                None
            };
            MessageKind::File {
                blob_id: blob_id.clone(),
                filename: filename.clone(),
                size_bytes: file_size,
                ciphertext_len: encrypted.len() as u64,
                inline_data: inline,
            }
        }
    };

    let now = Utc::now();
    let content = MessageContent {
        kind: kind.clone(),
        timestamp: now,
        channel_id,
        expires_at: None,
    };

    let sealed = s.seal_send_persist(&content).ok_or("Failed to seal message")?;

    let fp = master.peer_id().fingerprint();
    let display_name = s
        .display_names
        .get(&fp)
        .cloned()
        .unwrap_or_else(|| s.username.clone().unwrap_or_else(|| fp.clone()));

    Ok(message_info_from_kind(
        &sealed,
        &fp,
        &display_name,
        now.timestamp(),
        true,
        Some(channel_id_str),
        &kind,
    ))
}

/// Retrieve a blob by ID, decrypt it, and return as base64.
#[tauri::command]
pub async fn get_blob(
    blob_id: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<String, String> {
    let state = state.inner().clone();
    let s = state.read().await;
    let store = s.store.as_ref().ok_or("Store not initialized")?;
    let group = s.current_group().ok_or("No active group")?;

    let id_bytes: [u8; 32] = hex::decode(&blob_id)
        .map_err(|_| "Invalid blob ID")?
        .try_into()
        .map_err(|_| "Invalid blob ID length")?;
    let bid = veil_core::BlobId(id_bytes);

    let encrypted = store
        .get_blob_full(&bid)
        .map_err(|e| format!("Blob lookup failed: {e}"))?
        .ok_or("Blob not found")?;

    let ring = group.key_ring.lock().map_err(|_| "Key ring lock poisoned")?;
    let decrypted = ring
        .current()
        .decrypt(&encrypted)
        .map_err(|e| format!("Decrypt failed: {e}"))?;

    Ok(base64::engine::general_purpose::STANDARD.encode(&decrypted))
}

/// Helper to build MessageInfo from any MessageKind.
fn message_info_from_kind(
    sealed: &veil_core::SealedMessage,
    sender_fp: &str,
    sender_name: &str,
    timestamp: i64,
    is_self: bool,
    channel_id: Option<String>,
    kind: &MessageKind,
) -> MessageInfo {
    let b64 = base64::engine::general_purpose::STANDARD;

    match kind {
        MessageKind::Text(txt) => MessageInfo {
            id: hex::encode(sealed.id.0),
            sender_id: sender_fp.to_string(),
            sender_name: sender_name.to_string(),
            content: txt.clone(),
            timestamp,
            is_self,
            channel_id,
            reply_to_sender: None,
            reply_to_preview: None,
            kind_type: Some("text".into()),
            blob_id: None,
            width: None,
            height: None,
            thumbnail_b64: None,
            filename: None,
            size_bytes: None,
            duration_secs: None,
            waveform: None,
        },
        MessageKind::Image { blob_id, width, height, thumbnail, .. } => MessageInfo {
            id: hex::encode(sealed.id.0),
            sender_id: sender_fp.to_string(),
            sender_name: sender_name.to_string(),
            content: String::new(),
            timestamp,
            is_self,
            channel_id,
            reply_to_sender: None,
            reply_to_preview: None,
            kind_type: Some("image".into()),
            blob_id: Some(hex::encode(blob_id.0)),
            width: Some(*width),
            height: Some(*height),
            thumbnail_b64: if thumbnail.is_empty() { None } else { Some(b64.encode(thumbnail)) },
            filename: None,
            size_bytes: None,
            duration_secs: None,
            waveform: None,
        },
        MessageKind::Video { blob_id, duration_secs, thumbnail, .. } => MessageInfo {
            id: hex::encode(sealed.id.0),
            sender_id: sender_fp.to_string(),
            sender_name: sender_name.to_string(),
            content: String::new(),
            timestamp,
            is_self,
            channel_id,
            reply_to_sender: None,
            reply_to_preview: None,
            kind_type: Some("video".into()),
            blob_id: Some(hex::encode(blob_id.0)),
            width: None,
            height: None,
            thumbnail_b64: if thumbnail.is_empty() { None } else { Some(b64.encode(thumbnail)) },
            filename: None,
            size_bytes: None,
            duration_secs: Some(*duration_secs),
            waveform: None,
        },
        MessageKind::Audio { blob_id, duration_secs, waveform, .. } => MessageInfo {
            id: hex::encode(sealed.id.0),
            sender_id: sender_fp.to_string(),
            sender_name: sender_name.to_string(),
            content: String::new(),
            timestamp,
            is_self,
            channel_id,
            reply_to_sender: None,
            reply_to_preview: None,
            kind_type: Some("audio".into()),
            blob_id: Some(hex::encode(blob_id.0)),
            width: None,
            height: None,
            thumbnail_b64: None,
            filename: None,
            size_bytes: None,
            duration_secs: Some(*duration_secs),
            waveform: Some(waveform.clone()),
        },
        MessageKind::File { blob_id, filename, size_bytes, .. } => MessageInfo {
            id: hex::encode(sealed.id.0),
            sender_id: sender_fp.to_string(),
            sender_name: sender_name.to_string(),
            content: String::new(),
            timestamp,
            is_self,
            channel_id,
            reply_to_sender: None,
            reply_to_preview: None,
            kind_type: Some("file".into()),
            blob_id: Some(hex::encode(blob_id.0)),
            width: None,
            height: None,
            thumbnail_b64: None,
            filename: Some(filename.clone()),
            size_bytes: Some(*size_bytes),
            duration_secs: None,
            waveform: None,
        },
        _ => MessageInfo {
            id: hex::encode(sealed.id.0),
            sender_id: sender_fp.to_string(),
            sender_name: sender_name.to_string(),
            content: String::new(),
            timestamp,
            is_self,
            channel_id,
            reply_to_sender: None,
            reply_to_preview: None,
            kind_type: None,
            blob_id: None,
            width: None,
            height: None,
            thumbnail_b64: None,
            filename: None,
            size_bytes: None,
            duration_secs: None,
            waveform: None,
        },
    }
}

// ── Connection commands ──

#[tauri::command]
pub async fn get_connection_info(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<ConnectionInfo, String> {
    let state = state.inner().clone();
    let s = state.read().await;
    let conn_state = if s.net_cmd_tx.is_some() {
        "connected"
    } else if s.network_spawned {
        "connecting"
    } else {
        "disconnected"
    };

    Ok(ConnectionInfo {
        state: conn_state.into(),
        relay_connected: s.relay_connected,
        peer_count: s.connected_peers.len(),
        local_addr: s.local_addr.map(|a| a.to_string()),
    })
}

#[tauri::command]
pub async fn connect_relay(
    addr: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let s = state.read().await;
    let socket_addr: std::net::SocketAddr =
        addr.parse().map_err(|e: std::net::AddrParseError| e.to_string())?;

    if let Some(tx) = s.net_cmd_tx.as_ref() {
        tx.send(crate::state::NetCommand::ConnectRelay(socket_addr))
            .await
            .map_err(|e| format!("Failed to send relay connect: {e}"))?;
    } else {
        return Err("Network not ready".into());
    }

    if let Some(store) = s.store.as_ref() {
        let _ = store.store_setting("relay_addr", &addr);
    }

    Ok(())
}

/// Start the network worker. Called once after identity is loaded.
#[tauri::command]
pub async fn start_network(
    app_handle: tauri::AppHandle,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let state_arc = state.inner().clone();

    let s = state_arc.read().await;
    if s.network_spawned {
        return Ok(()); // Already running
    }
    let device = s.device.as_ref().ok_or("No identity loaded")?;
    let peer_id = device.device_peer_id();
    let identity_bytes = device.device_key_bytes();
    let device_cert = Some(device.certificate().clone());
    let groups: Vec<veil_core::GroupId> = s.groups.iter().map(|g| g.id.clone()).collect();
    let blob_store = s.store.clone();
    drop(s);

    {
        let mut s = state_arc.write().await;
        s.network_spawned = true;
    }

    let state_for_worker = state_arc.clone();
    tauri::async_runtime::spawn(crate::network::spawn_network_worker(
        app_handle,
        state_for_worker,
        peer_id,
        identity_bytes,
        groups,
        device_cert,
        blob_store,
    ));

    Ok(())
}

// ── Voice commands ──

/// Serializable voice state for the frontend.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStateInfo {
    pub in_room: bool,
    pub room_id: Option<String>,
    pub channel_name: Option<String>,
    pub is_muted: bool,
    pub is_deafened: bool,
    pub participants: Vec<crate::state::VoiceParticipantInfo>,
}

/// Join a voice channel. Derives a room_id from (group_id, channel_name).
#[tauri::command]
pub async fn voice_join(
    channel_name: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let s = state.inner().clone();
    let mut st = s.write().await;

    if st.voice.current_room.is_some() {
        return Err("Already in a voice channel".into());
    }

    let group = st.current_group().ok_or("No active group")?.clone();
    let tx = st.net_cmd_tx.as_ref().ok_or("Network not ready")?.clone();

    // Derive room_id from group_id + channel_name
    let room_id = blake3::derive_key(
        "veil-voice-room-id",
        &[group.id.0.as_slice(), channel_name.as_bytes()].concat(),
    );

    st.voice.room_id_bytes = Some(room_id);
    st.voice.current_room = Some(VoiceRoomInfo {
        room_id: hex::encode(room_id),
        channel_name: channel_name.clone(),
        participants: Vec::new(),
    });
    st.voice.is_muted = false;
    st.voice.is_deafened = false;

    tx.send(crate::state::NetCommand::VoiceJoin {
        room_id,
        group_id: group.id.0,
    })
    .await
    .map_err(|e| format!("Failed to send voice join: {e}"))?;

    Ok(())
}

/// Leave the current voice channel.
#[tauri::command]
pub async fn voice_leave(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let s = state.inner().clone();
    let mut st = s.write().await;

    let room_id = st.voice.room_id_bytes.ok_or("Not in a voice channel")?;
    let tx = st.net_cmd_tx.as_ref().ok_or("Network not ready")?.clone();

    tx.send(crate::state::NetCommand::VoiceLeave { room_id })
        .await
        .map_err(|e| format!("Failed to send voice leave: {e}"))?;

    st.voice = crate::state::VoiceState::default();
    Ok(())
}

/// Toggle local mute state.
#[tauri::command]
pub async fn voice_set_mute(
    muted: bool,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let s = state.inner().clone();
    let mut st = s.write().await;
    st.voice.is_muted = muted;
    Ok(())
}

/// Toggle deafen state (deafen implies mute).
#[tauri::command]
pub async fn voice_set_deafen(
    deafened: bool,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let s = state.inner().clone();
    let mut st = s.write().await;
    st.voice.is_deafened = deafened;
    if deafened {
        st.voice.is_muted = true;
    }
    Ok(())
}

/// Forward an SDP answer to the relay's SFU.
#[tauri::command]
pub async fn voice_sdp_answer(
    sdp: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let s = state.inner().clone();
    let st = s.read().await;

    let room_id = st.voice.room_id_bytes.ok_or("Not in a voice channel")?;
    let participant_id = st.voice.participant_id.ok_or("No participant ID assigned")?;
    let tx = st.net_cmd_tx.as_ref().ok_or("Network not ready")?.clone();

    tx.send(crate::state::NetCommand::VoiceAnswer {
        room_id,
        participant_id,
        sdp,
    })
    .await
    .map_err(|e| format!("Failed to send SDP answer: {e}"))?;

    Ok(())
}

/// Forward an ICE candidate to the relay's SFU.
#[tauri::command]
pub async fn voice_ice_candidate(
    candidate: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let s = state.inner().clone();
    let st = s.read().await;

    let room_id = st.voice.room_id_bytes.ok_or("Not in a voice channel")?;
    let participant_id = st.voice.participant_id.ok_or("No participant ID assigned")?;
    let tx = st.net_cmd_tx.as_ref().ok_or("Network not ready")?.clone();

    tx.send(crate::state::NetCommand::VoiceIceCandidate {
        room_id,
        participant_id,
        candidate,
    })
    .await
    .map_err(|e| format!("Failed to send ICE candidate: {e}"))?;

    Ok(())
}

/// Export the voice encryption key derived from the current group key.
/// Returns (hex_key, generation) for use in the browser's Web Crypto API.
#[tauri::command]
pub async fn get_voice_encryption_key(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(String, u64), String> {
    let s = state.inner().clone();
    let st = s.read().await;

    let group = st.current_group().ok_or("No active group")?;
    let ring = group.key_ring.lock().map_err(|_| "Key ring lock poisoned")?;
    let voice_key = ring.current().derive_channel_key(b"voice");
    let (raw_key, generation) = voice_key.to_raw_parts();

    Ok((hex::encode(raw_key), generation))
}

/// Get the current voice state.
#[tauri::command]
pub async fn get_voice_state(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<VoiceStateInfo, String> {
    let s = state.inner().clone();
    let st = s.read().await;

    Ok(VoiceStateInfo {
        in_room: st.voice.current_room.is_some(),
        room_id: st.voice.current_room.as_ref().map(|r| r.room_id.clone()),
        channel_name: st.voice.current_room.as_ref().map(|r| r.channel_name.clone()),
        is_muted: st.voice.is_muted,
        is_deafened: st.voice.is_deafened,
        participants: st
            .voice
            .current_room
            .as_ref()
            .map(|r| r.participants.clone())
            .unwrap_or_default(),
    })
}

// ── Hosted relay commands ──

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HostedRelayStatus {
    pub hosting: bool,
    pub addr: Option<String>,
    pub voice_enabled: bool,
}

/// Start an embedded relay server on the given port.
#[tauri::command]
pub async fn start_hosted_relay(
    port: u16,
    voice_enabled: bool,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<HostedRelayStatus, String> {
    let state_arc = state.inner().clone();
    let mut s = state_arc.write().await;

    if let Some(ref handle) = s.relay_task {
        if !handle.is_finished() {
            return Err("Already hosting a relay".into());
        }
    }

    let bind_addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();
    let data_dir = veil_data_dir();
    std::fs::create_dir_all(&data_dir).ok();
    let db_path = data_dir.join("relay-mailbox.redb");

    let voice_config = if voice_enabled {
        Some(veil_relay::voice::VoiceConfig {
            udp_bind_addr: ([0, 0, 0, 0], port + 1).into(),
            ..Default::default()
        })
    } else {
        None
    };

    let config = veil_relay::RelayConfig {
        bind_addr,
        db_path,
        voice_config,
        ..Default::default()
    };

    let server = veil_relay::RelayServer::new(config);

    let handle = tokio::spawn(async move {
        server
            .run()
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })
    });

    s.relay_task = Some(handle);
    s.relay_host_addr = Some(bind_addr);

    // Persist settings
    if let Some(ref store) = s.store {
        let _ = store.store_setting("relay_host_port", &port.to_string());
        let _ = store.store_setting("relay_host_voice", if voice_enabled { "1" } else { "0" });
    }

    let display_addr = format!("127.0.0.1:{port}");

    Ok(HostedRelayStatus {
        hosting: true,
        addr: Some(display_addr),
        voice_enabled,
    })
}

/// Stop the embedded relay server.
#[tauri::command]
pub async fn stop_hosted_relay(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let state_arc = state.inner().clone();
    let mut s = state_arc.write().await;

    if let Some(handle) = s.relay_task.take() {
        handle.abort();
    }
    s.relay_host_addr = None;
    Ok(())
}

/// Get the status of the embedded relay server.
#[tauri::command]
pub async fn get_hosted_relay_status(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<HostedRelayStatus, String> {
    let state_arc = state.inner().clone();
    let s = state_arc.read().await;

    let hosting = s
        .relay_task
        .as_ref()
        .map_or(false, |h| !h.is_finished());

    Ok(HostedRelayStatus {
        hosting,
        addr: if hosting {
            s.relay_host_addr.map(|a| format!("127.0.0.1:{}", a.port()))
        } else {
            None
        },
        voice_enabled: hosting, // If hosting, voice was enabled
    })
}

// ── Invite code & IP detection ──

/// Invite code format: base64url( relay_addr_bytes | group_id[32] | group_key[32] )
/// relay_addr_bytes: 1-byte len + utf8 string

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InviteCodeInfo {
    pub code: String,
    pub relay_addr: String,
    pub group_name: String,
}

/// Generate an invite code for the current group.
#[tauri::command]
pub async fn create_invite_code(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<InviteCodeInfo, String> {
    let state = state.inner().clone();
    let s = state.read().await;

    let group = s.current_group().ok_or("No active group")?;
    let relay_addr = s
        .relay_host_addr
        .map(|a| a.to_string())
        .or_else(|| {
            s.store
                .as_ref()
                .and_then(|store| store.get_setting("relay_addr").ok().flatten())
        })
        .ok_or("No relay address — start hosting or connect to a relay first")?;

    let ring = group
        .key_ring
        .lock()
        .map_err(|_| "Key ring lock poisoned")?;
    let (key_bytes, _gen) = ring.current().to_raw_parts();

    // Encode: addr_len(1) + addr + group_id(32) + key(32)
    let addr_bytes = relay_addr.as_bytes();
    if addr_bytes.len() > 255 {
        return Err("Relay address too long".into());
    }

    let mut payload = Vec::with_capacity(1 + addr_bytes.len() + 32 + 32);
    payload.push(addr_bytes.len() as u8);
    payload.extend_from_slice(addr_bytes);
    payload.extend_from_slice(&group.id.0);
    payload.extend_from_slice(&key_bytes);

    let code = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload);

    Ok(InviteCodeInfo {
        code,
        relay_addr,
        group_name: group.name.clone(),
    })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JoinResult {
    pub group_id: String,
    pub group_name: String,
    pub relay_addr: String,
}

/// Join a server via invite code. Decodes the code, creates the group locally,
/// connects to the relay, and subscribes to the group's routing tag.
#[tauri::command]
pub async fn join_via_invite(
    code: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<JoinResult, String> {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(code.trim())
        .map_err(|_| "Invalid invite code")?;

    if payload.is_empty() {
        return Err("Invalid invite code".into());
    }

    let addr_len = payload[0] as usize;
    if payload.len() < 1 + addr_len + 32 + 32 {
        return Err("Invalid invite code (too short)".into());
    }

    let relay_addr =
        String::from_utf8(payload[1..1 + addr_len].to_vec()).map_err(|_| "Invalid relay address")?;

    let mut group_id = [0u8; 32];
    group_id.copy_from_slice(&payload[1 + addr_len..1 + addr_len + 32]);

    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&payload[1 + addr_len + 32..1 + addr_len + 64]);

    let state = state.inner().clone();
    let mut s = state.write().await;

    // Check if we already have this group
    if s.groups.iter().any(|g| g.id.0 == group_id) {
        return Err("You're already in this server".into());
    }

    let master = s.master.as_ref().ok_or("Not logged in")?;
    let master_id = master.peer_id().verifying_key.clone();

    // Reconstruct the group key from raw bytes
    let group_key = GroupKey::from_raw_parts(key_bytes, 0);
    let keyring = GroupKeyRing::new(group_key, master_id);

    // Use a derived name until we learn the real one from the group
    let group_name = format!("Server {}", &hex::encode(&group_id[..4]));

    if let Some(ref store) = s.store {
        store
            .store_group_v2(&group_id, &group_name, &keyring)
            .map_err(|e| format!("Failed to save group: {e}"))?;
        let _ = store.store_setting("relay_addr", &relay_addr);
    }

    let group_state = crate::state::GroupState {
        name: group_name.clone(),
        id: GroupId(group_id),
        key_ring: std::sync::Arc::new(std::sync::Mutex::new(keyring)),
        device_certs: Vec::new(),
        members: Vec::new(),
    };

    s.groups.push(group_state);
    let idx = s.groups.len() - 1;
    s.current_group_idx = Some(idx);

    // Set default channels
    let group_hex = hex::encode(group_id);
    let default_channels = vec!["general".into(), "random".into()];
    s.group_channels
        .insert(group_hex.clone(), default_channels.clone());
    s.channels = default_channels;
    s.current_channel = Some("general".into());

    // Subscribe to routing tag and connect to relay
    if let Some(ref tx) = s.net_cmd_tx {
        let tag = routing_tag_for_group(&group_id);
        let _ = tx.try_send(crate::state::NetCommand::SubscribeTag(tag));

        if let Ok(addr) = relay_addr.parse::<std::net::SocketAddr>() {
            let _ = tx
                .send(crate::state::NetCommand::ConnectRelay(addr))
                .await;
        }
    }

    Ok(JoinResult {
        group_id: hex::encode(group_id),
        group_name,
        relay_addr,
    })
}

/// Detect available network addresses for sharing with friends.
/// Returns Tailscale IP (100.x), LAN IPs, and localhost.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAddresses {
    pub tailscale: Option<String>,
    pub lan: Vec<String>,
    pub relay_port: u16,
    /// Best address to share: tailscale > lan > localhost
    pub best: String,
}

#[tauri::command]
pub async fn detect_addresses(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<NetworkAddresses, String> {
    let state = state.inner().clone();
    let s = state.read().await;

    let relay_port = s
        .relay_host_addr
        .map(|a| a.port())
        .unwrap_or(4433);

    let mut tailscale: Option<String> = None;
    let mut lan: Vec<String> = Vec::new();

    if let Ok(ifaces) = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|_| Ok(()))
    {
        // Use a different approach — iterate network interfaces
        let _ = ifaces;
    }

    // Parse IPs from `ip addr` or network interfaces
    for iface in get_local_ips() {
        if iface.starts_with("100.") {
            // Tailscale uses 100.x.y.z CGNAT range
            tailscale = Some(format!("{iface}:{relay_port}"));
        } else if !iface.starts_with("127.") && !iface.starts_with("::") {
            lan.push(format!("{iface}:{relay_port}"));
        }
    }

    let best = tailscale
        .clone()
        .or_else(|| lan.first().cloned())
        .unwrap_or_else(|| format!("127.0.0.1:{relay_port}"));

    Ok(NetworkAddresses {
        tailscale,
        lan,
        relay_port,
        best,
    })
}

/// Get local IP addresses by creating a UDP socket (cross-platform).
fn get_local_ips() -> Vec<String> {
    let mut ips = Vec::new();

    // Method 1: connect to public DNS to discover default route IP
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local) = socket.local_addr() {
                let ip = local.ip().to_string();
                if !ips.contains(&ip) {
                    ips.push(ip);
                }
            }
        }
    }

    // Method 2: parse `ip -4 addr` output on Linux for all interfaces
    if let Ok(output) = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            // Format: "2: eth0    inet 192.168.1.5/24 ..."
            if let Some(inet_pos) = line.find("inet ") {
                let after_inet = &line[inet_pos + 5..];
                if let Some(slash_pos) = after_inet.find('/') {
                    let ip = &after_inet[..slash_pos];
                    if ip != "127.0.0.1" && !ips.contains(&ip.to_string()) {
                        ips.push(ip.to_string());
                    }
                }
            }
        }
    }

    ips
}

/// Create a server: auto-starts relay, creates group, returns invite code.
/// This is the "one click" server creation for onboarding.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateServerResult {
    pub group_id: String,
    pub group_name: String,
    pub invite_code: String,
    pub relay_addr: String,
    pub addresses: NetworkAddresses,
}

#[tauri::command]
pub async fn create_server(
    name: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<CreateServerResult, String> {
    if name.is_empty() || name.len() > 100 {
        return Err("Server name must be 1-100 characters".into());
    }

    let state_arc = state.inner().clone();

    // 1. Start relay if not already running
    let port: u16 = {
        let s = state_arc.read().await;
        if let Some(ref handle) = s.relay_task {
            if !handle.is_finished() {
                s.relay_host_addr.map(|a| a.port()).unwrap_or(4433)
            } else {
                4433
            }
        } else {
            4433
        }
    };

    let need_relay = {
        let s = state_arc.read().await;
        s.relay_task
            .as_ref()
            .map_or(true, |h| h.is_finished())
    };

    if need_relay {
        let bind_addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();
        let data_dir = veil_data_dir();
        std::fs::create_dir_all(&data_dir).ok();
        let db_path = data_dir.join("relay-mailbox.redb");

        let config = veil_relay::RelayConfig {
            bind_addr,
            db_path,
            voice_config: Some(veil_relay::voice::VoiceConfig {
                udp_bind_addr: ([0, 0, 0, 0], port + 1).into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let server = veil_relay::RelayServer::new(config);
        let handle = tokio::spawn(async move {
            server
                .run()
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })
        });

        let mut s = state_arc.write().await;
        s.relay_task = Some(handle);
        s.relay_host_addr = Some(bind_addr);

        if let Some(ref store) = s.store {
            let _ = store.store_setting("relay_host_port", &port.to_string());
        }
    }

    // 2. Create the group
    let (group_id_bytes, group_name, key_bytes) = {
        let mut s = state_arc.write().await;
        let master = s.master.as_ref().ok_or("Not logged in")?;
        let master_id = master.peer_id().verifying_key.clone();
        let peer_id = master.peer_id();

        let group_key = GroupKey::generate();
        let (key_raw, _gen) = group_key.to_raw_parts();
        let group_id_bytes = blake3::derive_key(
            "veil-group-id",
            &bincode::serialize(&(&name, &peer_id, chrono::Utc::now().timestamp_nanos_opt()))
                .unwrap_or_default(),
        );
        let keyring = GroupKeyRing::new(group_key, master_id);

        if let Some(ref store) = s.store {
            store
                .store_group_v2(&group_id_bytes, &name, &keyring)
                .map_err(|e| format!("Failed to save group: {e}"))?;
        }

        let group_state = crate::state::GroupState {
            name: name.clone(),
            id: GroupId(group_id_bytes),
            key_ring: std::sync::Arc::new(std::sync::Mutex::new(keyring)),
            device_certs: Vec::new(),
            members: Vec::new(),
        };

        s.groups.push(group_state);
        let idx = s.groups.len() - 1;
        s.current_group_idx = Some(idx);

        let group_hex = hex::encode(group_id_bytes);
        let default_channels = vec!["general".into(), "random".into()];
        s.group_channels.insert(group_hex, default_channels.clone());
        s.channels = default_channels;
        s.current_channel = Some("general".into());

        (group_id_bytes, name.clone(), key_raw)
    };

    // 3. Get addresses
    let addresses = {
        let s = state_arc.read().await;
        let relay_port = s.relay_host_addr.map(|a| a.port()).unwrap_or(port);
        let mut tailscale: Option<String> = None;
        let mut lan: Vec<String> = Vec::new();

        for ip in get_local_ips() {
            if ip.starts_with("100.") {
                tailscale = Some(format!("{ip}:{relay_port}"));
            } else if !ip.starts_with("127.") {
                lan.push(format!("{ip}:{relay_port}"));
            }
        }

        let best = tailscale
            .clone()
            .or_else(|| lan.first().cloned())
            .unwrap_or_else(|| format!("127.0.0.1:{relay_port}"));

        NetworkAddresses {
            tailscale,
            lan,
            relay_port,
            best,
        }
    };

    // 4. Auto-connect to own relay
    {
        let s = state_arc.read().await;
        if let Some(ref tx) = s.net_cmd_tx {
            let tag = routing_tag_for_group(&group_id_bytes);
            let _ = tx.try_send(crate::state::NetCommand::SubscribeTag(tag));

            let local_addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
            let _ = tx
                .send(crate::state::NetCommand::ConnectRelay(local_addr))
                .await;
        }
        if let Some(ref store) = s.store {
            let _ = store.store_setting("relay_addr", &format!("127.0.0.1:{port}"));
        }
    }

    // 5. Generate invite code using the best address
    let relay_addr_for_invite = addresses.best.clone();
    let addr_bytes = relay_addr_for_invite.as_bytes();
    let mut payload = Vec::with_capacity(1 + addr_bytes.len() + 32 + 32);
    payload.push(addr_bytes.len() as u8);
    payload.extend_from_slice(addr_bytes);
    payload.extend_from_slice(&group_id_bytes);
    payload.extend_from_slice(&key_bytes);

    let invite_code = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload);

    Ok(CreateServerResult {
        group_id: hex::encode(group_id_bytes),
        group_name,
        invite_code,
        relay_addr: relay_addr_for_invite,
        addresses,
    })
}
