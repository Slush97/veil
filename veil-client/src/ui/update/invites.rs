use std::sync::Arc;

use veil_core::GroupId;
use veil_crypto::{GroupKey, GroupKeyRing};
use zeroize::Zeroize;

use crate::ui::app::App;
use crate::ui::message::NetCommand;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_create_invite(&mut self) {
        if let Some(ref group) = self.current_group {
            let relay_addr = if self.relay_addr_input.is_empty() {
                "localhost:4433".into()
            } else {
                self.relay_addr_input.clone()
            };
            let Ok(ring) = group.key_ring.lock() else {
                tracing::error!("key ring lock poisoned");
                return;
            };
            let current_key = Arc::new(ring.current().duplicate());
            drop(ring);
            if let Some(ref mut tx) = self.net_cmd_tx
                && let Err(e) = tx.try_send(NetCommand::CreateInvite {
                    group_id: group.id.clone(),
                    group_name: group.name.clone(),
                    relay_addr,
                    passphrase: self.invite_passphrase.clone(),
                    group_key: current_key,
                }) {
                    tracing::warn!("failed to send create invite: {e}");
                }
        }
    }

    pub(crate) fn update_accept_invite(&mut self) {
        if !self.invite_input.is_empty() {
            if let Some(ref mut tx) = self.net_cmd_tx
                && let Err(e) = tx.try_send(NetCommand::AcceptInvite {
                    url: self.invite_input.clone(),
                    passphrase: self.invite_passphrase.clone(),
                }) {
                    tracing::warn!("failed to send accept invite: {e}");
                }
            // Zeroize invite passphrase after use
            self.invite_passphrase.zeroize();
        }
    }

    pub(crate) fn update_invite_accepted(
        &mut self,
        group_name: String,
        group_id: GroupId,
        group_key: SharedGroupKey,
    ) {
        let master_id = self.master_peer_id().verifying_key.clone();
        let (key_bytes, generation) = group_key.0.to_raw_parts();
        let keyring = GroupKeyRing::new(GroupKey::from_raw_parts(key_bytes, generation), master_id);

        let group_state = GroupState {
            name: group_name,
            id: group_id,
            key_ring: Arc::new(std::sync::Mutex::new(keyring)),
            device_certs: Vec::new(),
            members: Vec::new(),
        };

        // Persist the new group as v2
        if let Some(ref store) = self.store {
            let Ok(ring) = group_state.key_ring.lock() else {
                tracing::error!("key ring lock poisoned");
                return;
            };
            if let Err(e) = store.store_group_v2(&group_state.id.0, &group_state.name, &ring) {
                tracing::warn!("failed to persist group: {e}");
            }
        }

        self.groups.push(group_state.clone());
        self.current_group = Some(group_state);
        self.invite_input.clear();
        self.connection_state = ConnectionState::Connected("Invite accepted!".into());
    }
}
