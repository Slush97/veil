use veil_core::ControlMessage;
use veil_crypto::PeerId;

use super::app::App;
use super::types::ChatMessage;

impl App {
    /// Handle a control message received from a peer.
    pub(crate) fn handle_control_message(&mut self, ctrl: ControlMessage, sender: PeerId) {
        match ctrl {
            ControlMessage::KeyRotation {
                epoch,
                key_packages,
            } => {
                match &epoch.reason {
                    veil_crypto::EpochReason::ScheduledRotation => {
                        // All members derive the new key independently
                        if let Some(ref group) = self.current_group {
                            let Ok(mut ring) = group.key_ring.lock() else {
                                tracing::error!("key ring lock poisoned");
                                return;
                            };
                            ring.rotate_forward(sender.verifying_key.clone());

                            if let Some(ref store) = self.store
                                && let Err(e) =
                                    store.store_group_v2(&group.id.0, &group.name, &ring)
                            {
                                tracing::warn!("failed to persist group after key rotation: {e}");
                            }
                        }
                        self.messages.push(ChatMessage::system(format!(
                            "Key rotated (epoch {})",
                            epoch.epoch
                        )));
                    }
                    veil_crypto::EpochReason::Eviction { .. } => {
                        // Find our KeyPackage and decrypt the new key
                        if let Some(ref group) = self.current_group {
                            let our_master_id = self.master_peer_id().verifying_key;
                            if let Some(pkg) = key_packages
                                .iter()
                                .find(|p| p.recipient_master_id == our_master_id)
                            {
                                let eph = veil_crypto::EphemeralKeyPair::generate();
                                let peer_pub = x25519_dalek::PublicKey::from(pkg.ephemeral_public);
                                if let Ok(new_key) = veil_crypto::GroupKey::decrypt_from_peer(
                                    &pkg.encrypted_key,
                                    eph,
                                    &peer_pub,
                                ) {
                                    let Ok(mut ring) = group.key_ring.lock() else {
                                        tracing::error!("key ring lock poisoned");
                                        return;
                                    };
                                    ring.apply_eviction(new_key, epoch.clone());

                                    if let Some(ref store) = self.store
                                        && let Err(e) =
                                            store.store_group_v2(&group.id.0, &group.name, &ring)
                                    {
                                        tracing::warn!(
                                            "failed to persist group after key rotation: {e}"
                                        );
                                    }
                                }
                            }
                        }
                        self.messages.push(ChatMessage::system(format!(
                            "Member evicted, key rotated (epoch {})",
                            epoch.epoch
                        )));
                    }
                    veil_crypto::EpochReason::Genesis => {
                        // Genesis — nothing to do
                    }
                }
            }
            ControlMessage::DeviceAnnouncement { certificate } => {
                if certificate.verify() {
                    // Store the certificate
                    if let Some(ref store) = self.store
                        && let Err(e) =
                            store.store_device_cert(&certificate.device_id, &certificate)
                    {
                        tracing::warn!("failed to persist device cert: {e}");
                    }

                    // Add to all groups' device_certs
                    for group in &mut self.groups {
                        if !group
                            .device_certs
                            .iter()
                            .any(|c| c.device_id == certificate.device_id)
                        {
                            group.device_certs.push(certificate.clone());
                        }
                    }

                    self.messages.push(ChatMessage::system(format!(
                        "Device '{}' announced by {}",
                        certificate.device_name,
                        certificate.master_id.fingerprint()
                    )));
                }
            }
            ControlMessage::DeviceRevoked { revocation } => {
                if revocation.verify() {
                    // Remove the revoked device cert from all groups
                    for group in &mut self.groups {
                        group
                            .device_certs
                            .retain(|c| c.device_id != revocation.revoked_device_id);
                    }

                    self.messages.push(ChatMessage::system(format!(
                        "Device revoked by {}",
                        revocation.master_id.fingerprint()
                    )));
                }
            }
            ControlMessage::MemberAdded {
                member_id,
                display_name,
                invited_by,
            } => {
                // Store the member's public key in the current group for signature verification
                if let Some(ref mut group) = self.current_group {
                    if !group
                        .members
                        .iter()
                        .any(|m| m.verifying_key == member_id.verifying_key)
                    {
                        group.members.push(member_id.clone());
                    }
                }
                // Also update the group in the groups list
                for group in &mut self.groups {
                    if !group
                        .members
                        .iter()
                        .any(|m| m.verifying_key == member_id.verifying_key)
                    {
                        group.members.push(member_id.clone());
                    }
                }

                // Store the display name from the control message
                let fp = member_id.fingerprint();
                if !display_name.is_empty() {
                    self.display_names.insert(fp.clone(), display_name.clone());
                    if let Some(ref store) = self.store
                        && let Err(e) = store.store_display_name(&fp, &display_name)
                    {
                        tracing::warn!("failed to persist display name: {e}");
                    }
                }
                let invited_by_name = self.resolve_display_name(&invited_by);
                self.messages.push(ChatMessage::system(format!(
                    "{display_name} was added by {invited_by_name}",
                )));
            }
            ControlMessage::MemberRemoved {
                member_id,
                removed_by,
            } => {
                self.messages.push(ChatMessage::system(format!(
                    "{} was removed by {}",
                    member_id.fingerprint(),
                    removed_by.fingerprint()
                )));
            }
            ControlMessage::RoleChanged {
                member_id,
                new_role,
                ..
            } => {
                let name = self.resolve_display_name(&member_id);
                self.messages.push(ChatMessage::system(format!(
                    "{name}'s role changed to {new_role:?}",
                )));
            }
            ControlMessage::MetadataUpdate { field, value } => {
                let desc = match field {
                    veil_core::MetadataField::GroupName => format!("Group renamed to '{value}'"),
                    veil_core::MetadataField::GroupDescription => {
                        "Group description updated".to_string()
                    }
                    veil_core::MetadataField::ChannelAdded { name, .. } => {
                        format!("Channel #{name} added")
                    }
                    veil_core::MetadataField::ChannelRemoved { channel_id } => {
                        format!("Channel #{channel_id} removed")
                    }
                    veil_core::MetadataField::CategoryAdded { name, .. } => {
                        format!("Category '{name}' added")
                    }
                    veil_core::MetadataField::CategoryRemoved { .. } => {
                        "Category removed".to_string()
                    }
                    veil_core::MetadataField::ChannelMoved { .. } => "Channel moved".to_string(),
                };
                self.messages.push(ChatMessage::system(desc));
            }
            ControlMessage::ProfileUpdate { fields } => {
                for field in &fields {
                    match field {
                        veil_core::ProfileField::DisplayName(name) => {
                            let fp = sender.fingerprint();
                            self.display_names.insert(fp.clone(), name.clone());
                            if let Some(ref store) = self.store {
                                let _ = store.store_display_name(&fp, name);
                            }
                        }
                        veil_core::ProfileField::Avatar(blob_id) => {
                            // Request the avatar blob if we don't have it
                            if let Some(bid) = blob_id {
                                let have_it = self
                                    .store
                                    .as_ref()
                                    .and_then(|s| s.get_blob_full(bid).ok())
                                    .flatten()
                                    .is_some();
                                if !have_it {
                                    if let Some(ref mut tx) = self.net_cmd_tx {
                                        let _ = tx.try_send(
                                            crate::ui::message::NetCommand::RequestBlob {
                                                blob_id: bid.clone(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        // Bio and Status are stored but don't need special handling
                        _ => {}
                    }
                }
                let name = self.resolve_display_name(&sender);
                self.messages
                    .push(ChatMessage::system(format!("{name} updated their profile")));
            }
            ControlMessage::PinMessage {
                message_id,
                ..
            } => {
                // Mark the message as pinned in our local state
                for msg in &mut self.messages {
                    if msg.id.as_ref() == Some(&message_id) {
                        msg.pinned = true;
                    }
                }
                let name = self.resolve_display_name(&sender);
                self.messages
                    .push(ChatMessage::system(format!("{name} pinned a message")));
            }
            ControlMessage::UnpinMessage {
                message_id,
                ..
            } => {
                for msg in &mut self.messages {
                    if msg.id.as_ref() == Some(&message_id) {
                        msg.pinned = false;
                    }
                }
                let name = self.resolve_display_name(&sender);
                self.messages
                    .push(ChatMessage::system(format!("{name} unpinned a message")));
            }
            ControlMessage::SetEphemeral { ttl, .. } => {
                let name = self.resolve_display_name(&sender);
                let desc = match ttl {
                    Some(t) => format!("{name} set messages to expire after {t}s"),
                    None => format!("{name} disabled ephemeral messages"),
                };
                self.messages.push(ChatMessage::system(desc));
            }
        }
    }
}
