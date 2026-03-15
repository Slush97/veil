use std::sync::Arc;

use veil_core::{MessageKind, routing_tag_for_group};
use veil_crypto::{GroupKey, GroupKeyRing};
use veil_store::LocalStore;

use super::app::App;
use super::network::veil_data_dir;
use super::types::*;

impl App {
    pub(crate) fn setup_after_identity(&mut self) {
        let Some(master) = self.master.as_ref() else {
            tracing::error!("setup_after_identity called without master identity");
            return;
        };

        // Open message store — derive from master key bytes
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

        // Try to load v2 groups first, then fall back to v1 + migration
        if let Some(ref store) = self.store {
            let loaded = match store.list_groups_v2() {
                Ok(v2_groups) if !v2_groups.is_empty() => {
                    self.groups = v2_groups
                        .into_iter()
                        .map(|(id, name, keyring)| GroupState {
                            name,
                            id: veil_core::GroupId(id),
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
                // Try v1 groups and migrate
                match store.list_groups() {
                    Ok(v1_groups) if !v1_groups.is_empty() => {
                        self.groups = v1_groups
                            .into_iter()
                            .map(|(id, name, key)| {
                                let keyring = GroupKeyRing::new(key, master_id.clone());
                                // Re-save as v2
                                if let Err(e) = store.store_group_v2(&id, &name, &keyring) {
                                    tracing::warn!("failed to persist migrated group: {e}");
                                }
                                GroupState {
                                    name,
                                    id: veil_core::GroupId(id),
                                    key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                                    device_certs: Vec::new(),
                                    members: Vec::new(),
                                }
                            })
                            .collect();
                    }
                    _ => {}
                }
            }

            self.current_group = self.groups.first().cloned();

            // Load device certs from store into group state
            if let Ok(certs) = store.list_device_certs() {
                for group in &mut self.groups {
                    group.device_certs = certs.clone();
                }
            }
        }

        // If no groups loaded, create a default one
        if self.groups.is_empty() {
            let group_key = GroupKey::generate();
            let peer_id = master.peer_id();
            let group_id_bytes = blake3::derive_key(
                "veil-group-id",
                &bincode::serialize(&("My Group", &peer_id)).unwrap_or_default(),
            );

            let keyring = GroupKeyRing::new(group_key, master_id);

            // Persist the default group
            if let Some(ref store) = self.store
                && let Err(e) = store.store_group_v2(&group_id_bytes, "My Group", &keyring) {
                    tracing::warn!("failed to persist default group: {e}");
                }

            let group_state = GroupState {
                name: "My Group".into(),
                id: veil_core::GroupId(group_id_bytes),
                key_ring: Arc::new(std::sync::Mutex::new(keyring)),
                device_certs: Vec::new(),
                members: Vec::new(),
            };

            self.groups = vec![group_state.clone()];
            self.current_group = Some(group_state);
        }

        self.channels = vec!["general".into(), "random".into()];
        self.current_channel = Some("general".into());

        // Load display names from store
        if let Some(ref store) = self.store {
            if let Ok(names) = store.list_display_names() {
                for (fp, name) in names {
                    self.display_names.insert(fp, name);
                }
            }
            // Load username
            if let Ok(Some(stored_username)) = store.get_setting("username") {
                self.username = Some(stored_username);
            }

            // Load contacts and create DM group states
            if let Ok(contacts) = store.list_contacts() {
                self.contacts = contacts;
            }

            // Load settings
            if let Ok(Some(theme)) = store.get_setting("theme") {
                self.theme_choice = if theme == "light" {
                    ThemeChoice::Light
                } else {
                    ThemeChoice::Dark
                };
            }
            if let Ok(Some(notif)) = store.get_setting("notifications") {
                self.notifications_enabled = notif == "true";
            }
            if let Ok(Some(relay)) = store.get_setting("relay_addr") {
                self.relay_addr_input = relay;
            }
        }

        // Load message history for the current group
        self.load_message_history();
    }

    /// Load message history from the store for the current group.
    pub(crate) fn load_message_history(&mut self) {
        self.load_message_history_with_limit(self.messages_loaded);
    }

    pub(crate) fn load_message_history_with_limit(&mut self, limit: usize) {
        let Some(ref store) = self.store else { return };
        let Some(ref group) = self.current_group else {
            return;
        };

        let routing_tag = routing_tag_for_group(&group.id.0);
        match store.list_messages_by_tag(&routing_tag, limit, 0) {
            Ok(sealed_messages) => {
                let mut members = self.known_master_ids();
                for m in &group.members {
                    if !members.iter().any(|existing| existing.verifying_key == m.verifying_key) {
                        members.push(m.clone());
                    }
                }
                let ring = match group.key_ring.lock() {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!("key ring lock poisoned: {e}");
                        return;
                    }
                };

                for sealed in &sealed_messages {
                    if let Ok((content, sender)) =
                        sealed.verify_and_open_with_keyring(&ring, &members, &group.device_certs)
                    {
                        match content.kind {
                            MessageKind::Text(ref txt) => {
                                let mut cm = ChatMessage::user_with_channel(
                                    sealed.id.clone(),
                                    sender,
                                    txt.clone(),
                                    content.timestamp,
                                    content.channel_id.clone(),
                                );
                                cm.sender = self.resolve_display_name_str(&cm.sender);
                                self.messages.push(cm);
                            }
                            MessageKind::Reply {
                                ref parent_id,
                                content: ref reply_content,
                            } => {
                                if let MessageKind::Text(ref txt) = **reply_content {
                                    let mut cm = ChatMessage::user_with_channel(
                                        sealed.id.clone(),
                                        sender,
                                        txt.clone(),
                                        content.timestamp,
                                        content.channel_id.clone(),
                                    );
                                    // Find parent message for reply context
                                    if let Some(parent) = self
                                        .messages
                                        .iter()
                                        .find(|m| m.id.as_ref() == Some(parent_id))
                                    {
                                        cm.reply_to_content = Some(parent.content.clone());
                                        cm.reply_to_sender = Some(parent.sender.clone());
                                    }
                                    cm.sender = self.resolve_display_name_str(&cm.sender);
                                    self.messages.push(cm);
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
                                    .push((sender, emoji.clone()));
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load message history: {e}");
            }
        }
    }
}
