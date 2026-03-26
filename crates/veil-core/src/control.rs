//! Control messages for group state coordination.
//!
//! Control messages flow through the same relay infrastructure as chat messages
//! but carry structured operations: key rotations, device announcements,
//! membership changes, and metadata updates.
//!
//! Every client independently validates authorization rules — there is no
//! server-side enforcement. Authorization is based on the signature chain:
//! device key → device certificate → master key → role in group.

use serde::{Deserialize, Serialize};
use veil_crypto::{DeviceCertificate, DeviceRevocation, EpochReason, KeyEpoch, KeyPackage, PeerId};

use crate::group::{Role, role_level};

/// A control message that modifies group state.
///
/// These are embedded in `MessageKind::Control` and sealed/verified like
/// regular messages, but processed differently by the client.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Key rotation — either scheduled or eviction.
    ///
    /// For scheduled rotations, `key_packages` is empty (all members derive
    /// the new key independently). For eviction, `key_packages` contains the
    /// new key encrypted for each remaining member.
    KeyRotation {
        epoch: KeyEpoch,
        key_packages: Vec<KeyPackage>,
    },

    /// A new device was added by a member.
    ///
    /// Other members should store this certificate and accept messages
    /// signed by the new device key.
    DeviceAnnouncement { certificate: DeviceCertificate },

    /// A device was revoked by its owner.
    ///
    /// Other members should reject future messages from the revoked device.
    DeviceRevoked { revocation: DeviceRevocation },

    /// A new member was added to the group.
    MemberAdded {
        /// Master PeerId of the new member.
        member_id: PeerId,
        /// Display name for the new member.
        display_name: String,
        /// Master PeerId of who invited them.
        invited_by: PeerId,
    },

    /// A member was removed from the group.
    ///
    /// A `KeyRotation` with `EpochReason::Eviction` should follow immediately
    /// (or be bundled in the same message batch).
    MemberRemoved {
        /// Master PeerId of the removed member.
        member_id: PeerId,
        /// Master PeerId of who removed them.
        removed_by: PeerId,
    },

    /// A member's role was changed.
    RoleChanged {
        member_id: PeerId,
        new_role: Role,
        changed_by: PeerId,
    },

    /// Group metadata was updated (name, description, etc.).
    MetadataUpdate { field: MetadataField, value: String },
}

/// Which piece of group metadata was updated.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MetadataField {
    GroupName,
    GroupDescription,
    ChannelAdded { name: String, kind: String },
    ChannelRemoved { channel_id: String },
    CategoryAdded { name: String },
    CategoryRemoved { category_id: String },
    ChannelMoved { channel_id: String, category_id: Option<String> },
}

/// Minimum role required to perform each control operation.
impl ControlMessage {
    pub fn required_role(&self) -> Role {
        match self {
            // Any member can announce their own devices
            ControlMessage::DeviceAnnouncement { .. } => Role::Member,
            // Any member can revoke their own devices
            ControlMessage::DeviceRevoked { .. } => Role::Member,
            // Any member can trigger a scheduled rotation
            ControlMessage::KeyRotation { epoch, .. } => match &epoch.reason {
                EpochReason::ScheduledRotation => Role::Member,
                EpochReason::Eviction { .. } => Role::Admin,
                EpochReason::Genesis => Role::Owner,
            },
            // Only admins+ can manage membership
            ControlMessage::MemberAdded { .. } => Role::Admin,
            ControlMessage::MemberRemoved { .. } => Role::Admin,
            // Only admins+ can change roles (further checks needed: can't promote above own level)
            ControlMessage::RoleChanged { .. } => Role::Admin,
            // Only admins+ can change metadata
            ControlMessage::MetadataUpdate { .. } => Role::Admin,
        }
    }

    /// Validate authorization: does the sender's role permit this operation?
    ///
    /// `sender_role` is the role of the master identity that signed this message.
    /// Returns true if the sender is authorized.
    pub fn is_authorized(&self, sender_role: &Role) -> bool {
        let required = self.required_role();
        role_level(sender_role) >= role_level(&required)
    }

    /// For device announcements and revocations, verify that the sender
    /// is the owner of the device (not someone else trying to announce/revoke
    /// another user's device).
    pub fn validate_self_only(&self, sender_master_id: &PeerId) -> bool {
        match self {
            ControlMessage::DeviceAnnouncement { certificate } => {
                certificate.master_id == *sender_master_id
            }
            ControlMessage::DeviceRevoked { revocation } => {
                revocation.master_id == *sender_master_id
            }
            _ => true, // Other messages don't have this constraint
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_can_do_everything() {
        let member_added = ControlMessage::MemberAdded {
            member_id: PeerId {
                verifying_key: vec![1; 32],
            },
            display_name: "Bob".into(),
            invited_by: PeerId {
                verifying_key: vec![2; 32],
            },
        };
        assert!(member_added.is_authorized(&Role::Owner));
        assert!(member_added.is_authorized(&Role::Admin));
        assert!(!member_added.is_authorized(&Role::Moderator));
        assert!(!member_added.is_authorized(&Role::Member));
    }

    #[test]
    fn member_can_announce_device() {
        use veil_crypto::MasterIdentity;

        let (master, _) = MasterIdentity::generate();
        let device = veil_crypto::DeviceIdentity::new(&master, "Phone".into());

        let announcement = ControlMessage::DeviceAnnouncement {
            certificate: device.certificate().clone(),
        };

        assert!(announcement.is_authorized(&Role::Member));
        assert!(announcement.validate_self_only(&master.peer_id()));

        // Another user can't announce this device
        let (other, _) = MasterIdentity::generate();
        assert!(!announcement.validate_self_only(&other.peer_id()));
    }

    #[test]
    fn scheduled_rotation_any_member() {
        let rotation = ControlMessage::KeyRotation {
            epoch: KeyEpoch {
                epoch: 1,
                reason: EpochReason::ScheduledRotation,
                author: vec![1; 32],
                created_at: 0,
            },
            key_packages: vec![],
        };
        assert!(rotation.is_authorized(&Role::Member));
    }

    #[test]
    fn eviction_requires_admin() {
        let rotation = ControlMessage::KeyRotation {
            epoch: KeyEpoch {
                epoch: 1,
                reason: EpochReason::Eviction {
                    removed: vec![3; 32],
                },
                author: vec![1; 32],
                created_at: 0,
            },
            key_packages: vec![],
        };
        assert!(!rotation.is_authorized(&Role::Member));
        assert!(rotation.is_authorized(&Role::Admin));
        assert!(rotation.is_authorized(&Role::Owner));
    }
}
