use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tauri::State;
use tokio::sync::RwLock;

use veil_core::{MessageContent, MessageKind, routing_tag_for_group};
use veil_crypto::{DeviceIdentity, MasterIdentity};

use crate::state::{AppState, veil_data_dir};

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
    Ok(())
}

// ── Channel commands ──

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

            match &msg_content.kind {
                MessageKind::Text(txt) => {
                    messages.push(MessageInfo {
                        id: hex::encode(sealed.id.0),
                        sender_id: sender_fp,
                        sender_name,
                        content: txt.clone(),
                        timestamp: ts,
                        is_self,
                        channel_id: Some(ch),
                        reply_to_sender: None,
                        reply_to_preview: None,
                    });
                }
                MessageKind::Reply {
                    parent_id,
                    content: reply_kind,
                } => {
                    if let MessageKind::Text(txt) = reply_kind.as_ref() {
                        let parent_hex = hex::encode(parent_id.0);
                        let (rts, rtp) = messages
                            .iter()
                            .find(|m: &&MessageInfo| m.id == parent_hex)
                            .map(|p| {
                                (
                                    Some(p.sender_name.clone()),
                                    Some(p.content.chars().take(60).collect::<String>()),
                                )
                            })
                            .unwrap_or((None, None));
                        messages.push(MessageInfo {
                            id: hex::encode(sealed.id.0),
                            sender_id: sender_fp,
                            sender_name,
                            content: txt.clone(),
                            timestamp: ts,
                            is_self,
                            channel_id: Some(ch),
                            reply_to_sender: rts,
                            reply_to_preview: rtp,
                        });
                    }
                }
                _ => {}
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
    })
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
