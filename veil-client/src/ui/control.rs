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
                                && let Err(e) = store.store_group_v2(&group.id.0, &group.name, &ring) {
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
                                        && let Err(e) = store.store_group_v2(&group.id.0, &group.name, &ring) {
                                            tracing::warn!("failed to persist group after key rotation: {e}");
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
                        && let Err(e) = store.store_device_cert(&certificate.device_id, &certificate) {
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
                // Store the display name from the control message
                let fp = member_id.fingerprint();
                if !display_name.is_empty() {
                    self.display_names.insert(fp.clone(), display_name.clone());
                    if let Some(ref store) = self.store
                        && let Err(e) = store.store_display_name(&fp, &display_name) {
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
            ControlMessage::MetadataUpdate { field, value } => {
                let desc = match field {
                    veil_core::MetadataField::GroupName => format!("Group renamed to '{value}'"),
                    veil_core::MetadataField::GroupDescription => {
                        "Group description updated".to_string()
                    }
                    veil_core::MetadataField::ChannelAdded { name, .. } => {
                        format!("Channel #{name} added")
                    }
                    veil_core::MetadataField::ChannelRemoved { name } => {
                        format!("Channel #{name} removed")
                    }
                };
                self.messages.push(ChatMessage::system(desc));
            }
        }
    }
}
