use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::mpsc;

use veil_core::{GroupId, SealedMessage};
use veil_crypto::{DeviceIdentity, GroupKey, GroupKeyRing, MasterIdentity, PeerId};
use veil_net::ConnectionId;
use veil_store::LocalStore;

/// Mirrors `GroupState` from the Iced client.
#[derive(Clone)]
pub struct GroupState {
    pub name: String,
    pub id: GroupId,
    pub key_ring: Arc<std::sync::Mutex<GroupKeyRing>>,
    pub device_certs: Vec<veil_crypto::DeviceCertificate>,
    pub members: Vec<PeerId>,
}

/// A participant in a voice room.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceParticipantInfo {
    pub peer_id: String,
    pub display_name: String,
    pub is_muted: bool,
    pub is_speaking: bool,
}

/// Info about the voice room we're currently in.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRoomInfo {
    pub room_id: String,
    pub channel_name: String,
    pub participants: Vec<VoiceParticipantInfo>,
}

/// Local voice state (mute/deafen + current room).
#[derive(Clone, Debug, Default)]
pub struct VoiceState {
    pub current_room: Option<VoiceRoomInfo>,
    pub is_muted: bool,
    pub is_deafened: bool,
    /// Participant ID assigned by the SFU for our connection.
    pub participant_id: Option<u64>,
    /// Raw room_id bytes for relay commands.
    pub room_id_bytes: Option<[u8; 32]>,
}

/// Commands sent from Tauri commands → network worker.
pub enum NetCommand {
    Connect(SocketAddr),
    SendMessage(SealedMessage),
    ConnectRelay(SocketAddr),
    SendPresence(veil_net::WireMessage),
    SubscribeTag([u8; 32]),
    // Voice commands
    VoiceJoin {
        room_id: [u8; 32],
        group_id: [u8; 32],
    },
    VoiceAnswer {
        room_id: [u8; 32],
        participant_id: u64,
        sdp: String,
    },
    VoiceIceCandidate {
        room_id: [u8; 32],
        participant_id: u64,
        candidate: String,
    },
    VoiceLeave {
        room_id: [u8; 32],
    },
}

/// Application state managed by Tauri.
///
/// Wrapped in `RwLock` for concurrent access from Tauri command handlers.
/// Mirrors the essential fields from the Iced `App` struct.
pub struct AppState {
    // Identity
    pub master: Option<MasterIdentity>,
    pub device: Option<DeviceIdentity>,

    // Groups & channels
    pub groups: Vec<GroupState>,
    pub current_group_idx: Option<usize>,
    /// Per-group channel lists: group_id hex → channel names
    pub group_channels: HashMap<String, Vec<String>>,
    pub channels: Vec<String>,
    pub current_channel: Option<String>,

    // Persistence
    pub store: Option<Arc<LocalStore>>,

    // Network
    pub net_cmd_tx: Option<mpsc::Sender<NetCommand>>,
    pub connected_peers: Vec<(ConnectionId, PeerId)>,
    pub local_addr: Option<SocketAddr>,
    pub relay_connected: bool,

    // Display state
    pub display_names: HashMap<String, String>,
    pub username: Option<String>,
    pub unread_counts: HashMap<[u8; 32], usize>,

    // Voice
    pub voice: VoiceState,

    // Embedded relay hosting
    pub relay_task: Option<tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>>,
    pub relay_host_addr: Option<SocketAddr>,

    // Network worker flag
    pub network_spawned: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            master: None,
            device: None,
            groups: Vec::new(),
            current_group_idx: None,
            group_channels: HashMap::new(),
            channels: Vec::new(),
            current_channel: None,
            store: None,
            net_cmd_tx: None,
            connected_peers: Vec::new(),
            local_addr: None,
            relay_connected: false,
            display_names: HashMap::new(),
            username: None,
            unread_counts: HashMap::new(),
            voice: VoiceState::default(),
            relay_task: None,
            relay_host_addr: None,
            network_spawned: false,
        }
    }
}

impl AppState {
    /// Get the current group state, if any.
    pub fn current_group(&self) -> Option<&GroupState> {
        self.current_group_idx.and_then(|i| self.groups.get(i))
    }

    /// Build list of known master PeerIds for message verification.
    pub fn known_master_ids(&self) -> Vec<PeerId> {
        let mut ids = Vec::new();
        if let Some(ref master) = self.master {
            ids.push(master.peer_id());
        }
        ids.extend(self.connected_peers.iter().map(|(_, pid)| pid.clone()));
        ids
    }

    /// Resolve a fingerprint to display name.
    pub fn resolve_display_name(&self, peer_id: &PeerId) -> String {
        let fp = peer_id.fingerprint();
        self.display_names.get(&fp).cloned().unwrap_or(fp)
    }

    /// Derive a channel ID from group + channel name.
    pub fn current_channel_id(&self, group: &GroupState) -> veil_core::ChannelId {
        let channel_name = self.current_channel.as_deref().unwrap_or("general");
        let derived = blake3::derive_key(
            "veil-channel-id",
            &[group.id.0.as_slice(), channel_name.as_bytes()].concat(),
        );
        let uuid_bytes: [u8; 16] = derived[..16]
            .try_into()
            .expect("blake3 output is always 32 bytes");
        veil_core::ChannelId(uuid::Uuid::from_bytes(uuid_bytes))
    }

    /// Seal, send over network, and persist a message.
    pub fn seal_send_persist(
        &self,
        content: &veil_core::MessageContent,
    ) -> Option<SealedMessage> {
        let device = self.device.as_ref()?;
        let master = self.master.as_ref()?;
        let group = self.current_group()?;
        let ring = group.key_ring.lock().ok()?;

        let is_dm = group.name.starts_with('@');
        let signing_identity: veil_crypto::Identity;
        let identity_ref = if is_dm {
            signing_identity = veil_crypto::Identity::from_bytes(&master.to_bytes());
            &signing_identity
        } else {
            device.identity()
        };

        let sealed =
            SealedMessage::seal(content, ring.current(), &group.id.0, identity_ref).ok()?;
        drop(ring);

        // Send to network worker
        if let Some(ref tx) = self.net_cmd_tx {
            let _ = tx.try_send(NetCommand::SendMessage(sealed.clone()));
        }

        // Persist to local store
        if let Some(ref store) = self.store {
            if let Err(e) = store.store_message(&sealed) {
                tracing::warn!("failed to persist message: {e}");
            }
        }

        Some(sealed)
    }

    /// Set up storage and load groups/settings after identity is available.
    pub fn setup_after_identity(&mut self) {
        let Some(master) = self.master.as_ref() else {
            tracing::error!("setup_after_identity called without master identity");
            return;
        };

        let data_dir = veil_data_dir();
        std::fs::create_dir_all(&data_dir).ok();
        let storage_key = LocalStore::derive_storage_key(&master.to_bytes());
        match LocalStore::open(&data_dir.join("messages.db"), storage_key) {
            Ok(store) => {
                self.store = Some(Arc::new(store));
            }
            Err(e) => {
                tracing::warn!("Failed to open message store: {e}");
            }
        }

        let master_id = master.peer_id().verifying_key.clone();

        // Load v2 groups, fall back to v1 + migrate
        if let Some(ref store) = self.store {
            let loaded = match store.list_groups_v2() {
                Ok(v2_groups) if !v2_groups.is_empty() => {
                    self.groups = v2_groups
                        .into_iter()
                        .map(|(id, name, keyring)| GroupState {
                            name,
                            id: GroupId(id),
                            key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                            device_certs: Vec::new(),
                            members: Vec::new(),
                        })
                        .collect();
                    true
                }
                _ => false,
            };

            if !loaded {
                if let Ok(v1_groups) = store.list_groups() {
                    if !v1_groups.is_empty() {
                        self.groups = v1_groups
                            .into_iter()
                            .map(|(id, name, key)| {
                                let keyring = GroupKeyRing::new(key, master_id.clone());
                                let _ = store.store_group_v2(&id, &name, &keyring);
                                GroupState {
                                    name,
                                    id: GroupId(id),
                                    key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                                    device_certs: Vec::new(),
                                    members: Vec::new(),
                                }
                            })
                            .collect();
                    }
                }
            }

            self.current_group_idx = if self.groups.is_empty() {
                None
            } else {
                Some(0)
            };

            if let Ok(certs) = store.list_device_certs() {
                for group in &mut self.groups {
                    group.device_certs = certs.clone();
                }
            }
        }

        // Create default group if none exist
        if self.groups.is_empty() {
            let group_key = GroupKey::generate();
            let peer_id = master.peer_id();
            let group_id_bytes = blake3::derive_key(
                "veil-group-id",
                &bincode::serialize(&("My Group", &peer_id)).unwrap_or_default(),
            );
            let keyring = GroupKeyRing::new(group_key, master_id);

            if let Some(ref store) = self.store {
                let _ = store.store_group_v2(&group_id_bytes, "My Group", &keyring);
            }

            self.groups.push(GroupState {
                name: "My Group".into(),
                id: GroupId(group_id_bytes),
                key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                device_certs: Vec::new(),
                members: Vec::new(),
            });
            self.current_group_idx = Some(0);
        }

        // Load per-group channels from settings, or create defaults
        if let Some(ref store) = self.store {
            for group in &self.groups {
                let group_hex = hex::encode(group.id.0);
                let key = format!("channels:{group_hex}");
                let chans = match store.get_setting(&key) {
                    Ok(Some(json)) => serde_json::from_str::<Vec<String>>(&json)
                        .unwrap_or_else(|_| vec!["general".into(), "random".into()]),
                    _ => vec!["general".into(), "random".into()],
                };
                self.group_channels.insert(group_hex, chans);
            }
        }

        // Set channels for the current group
        if let Some(group) = self.current_group() {
            let group_hex = hex::encode(group.id.0);
            self.channels = self
                .group_channels
                .get(&group_hex)
                .cloned()
                .unwrap_or_else(|| vec!["general".into(), "random".into()]);
        } else {
            self.channels = vec!["general".into(), "random".into()];
        }
        self.current_channel = Some("general".into());

        // Load settings from store
        if let Some(ref store) = self.store {
            if let Ok(names) = store.list_display_names() {
                for (fp, name) in names {
                    self.display_names.insert(fp, name);
                }
            }
            if let Ok(Some(stored_username)) = store.get_setting("username") {
                self.username = Some(stored_username);
            }
        }
    }
}

/// Get the Veil data directory.
pub fn veil_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VEIL_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("veil")
}
