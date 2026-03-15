use std::sync::Arc;

use veil_core::routing_tag_for_group;
use veil_crypto::{GroupKey, GroupKeyRing, PeerId, dm};

use crate::ui::app::App;
use crate::ui::message::NetCommand;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_register_username(&mut self) {
        let username = self.username_input.trim().to_string();
        if username.is_empty() {
            self.registration_status = Some("Please enter a username".into());
            return;
        }

        // Validate locally
        let valid = username.len() >= 3
            && username.len() <= 20
            && username
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            self.registration_status =
                Some("Username must be 3-20 characters (letters, numbers, underscore)".into());
            return;
        }

        let Some(ref master) = self.master else {
            self.registration_status = Some("No identity loaded".into());
            return;
        };

        let public_key = {
            let vk = &master.peer_id().verifying_key;
            let mut pk = [0u8; 32];
            pk[..vk.len().min(32)].copy_from_slice(&vk[..vk.len().min(32)]);
            pk
        };

        // Sign b"veil-register-v1:" || username_lowercase
        let sig_message = format!("veil-register-v1:{}", username.to_lowercase());
        let signature = master.sign(sig_message.as_bytes());

        if let Some(ref mut tx) = self.net_cmd_tx {
            let _ = tx.try_send(NetCommand::RegisterUsername {
                username: username.clone(),
                public_key,
                signature,
            });
            self.registration_status = Some("Registering...".into());
        } else {
            self.registration_status = Some("Not connected to relay".into());
        }
    }

    pub(crate) fn update_register_result(&mut self, success: bool, message: String) {
        if success {
            let username = self.username_input.trim().to_lowercase();
            self.username = Some(username.clone());
            self.registration_status = Some(format!("Registered as @{username}"));
            // Persist username
            if let Some(ref store) = self.store {
                let _ = store.store_setting("username", &username);
            }
        } else {
            self.registration_status = Some(format!("Registration failed: {message}"));
        }
    }

    pub(crate) fn update_lookup_contact(&mut self) {
        let username = self.contact_search_input.trim().to_string();
        if username.is_empty() {
            return;
        }

        self.contact_search_result = Some(ContactSearchResult::Searching);

        if let Some(ref mut tx) = self.net_cmd_tx {
            let _ = tx.try_send(NetCommand::LookupUser(username));
        }
    }

    pub(crate) fn update_contact_found(&mut self, username: String, public_key: [u8; 32]) {
        self.contact_search_result = Some(ContactSearchResult::Found { username, public_key });
    }

    pub(crate) fn update_contact_not_found(&mut self, username: String) {
        self.contact_search_result = Some(ContactSearchResult::NotFound(username));
    }

    pub(crate) fn update_add_contact(&mut self, username: String, public_key: [u8; 32]) {
        let Some(ref master) = self.master else { return };

        let my_key = {
            let vk = &master.peer_id().verifying_key;
            let mut pk = [0u8; 32];
            pk[..vk.len().min(32)].copy_from_slice(&vk[..vk.len().min(32)]);
            pk
        };

        // Derive DM group ID and shared key
        let dm_group_id = dm::dm_group_id(&my_key, &public_key);

        // Derive shared symmetric key via static ECDH using MASTER signing key.
        // Both parties must use master keys (not device keys) because the directory
        // stores master public keys — using device keys would break symmetry.
        let master_signing_key = ed25519_dalek::SigningKey::from_bytes(&master.to_bytes());
        let dm_key_bytes = dm::dm_shared_key(&master_signing_key, &public_key);

        let group_key = GroupKey::from_storage_key(dm_key_bytes);
        let master_id = master.peer_id().verifying_key.clone();
        let keyring = GroupKeyRing::new(group_key, master_id);
        let group_name = format!("@{username}");

        // Persist group and contact
        if let Some(ref store) = self.store {
            let _ = store.store_group_v2(&dm_group_id, &group_name, &keyring);
            let _ = store.store_contact(&username, &public_key);
        }

        // Create GroupState for the DM
        let group_state = GroupState {
            name: group_name,
            id: veil_core::GroupId(dm_group_id),
            key_ring: Arc::new(std::sync::Mutex::new(keyring)),
            device_certs: Vec::new(),
            members: vec![
                self.master_peer_id(),
                PeerId { verifying_key: public_key.to_vec() },
            ],
        };

        self.groups.push(group_state.clone());
        self.contacts.push((username.clone(), public_key));

        // Subscribe to the DM routing tag on relay
        let routing_tag = routing_tag_for_group(&dm_group_id);
        if let Some(ref mut tx) = self.net_cmd_tx {
            let _ = tx.try_send(NetCommand::SubscribeRelay {
                tags: vec![routing_tag],
            });
        }

        // Clear search state
        self.contact_search_input.clear();
        self.contact_search_result = None;

        self.messages.push(ChatMessage::system(format!(
            "Added @{username} as a contact"
        )));
    }
}
