use serde::{Deserialize, Serialize};
use veil_crypto::PeerId;

use crate::message::{CategoryId, ChannelId, GroupId};

/// A group is a community of peers (analogous to a Discord "server").
/// The group metadata itself is encrypted — pinners don't see any of this.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub description: Option<String>,
    pub categories: Vec<Category>,
    pub channels: Vec<Channel>,
    pub members: Vec<Member>,
}

/// A category organizes channels under a collapsible heading (like Discord's channel categories).
/// Channels with `category_id: None` appear above all categories as uncategorized.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
    /// Display order within the group (lower = higher in sidebar).
    pub position: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub kind: ChannelKind,
    /// Which category this channel belongs to. None = uncategorized (top-level).
    pub category_id: Option<CategoryId>,
    /// Display order within its category (lower = higher).
    pub position: u32,
    /// Per-channel permission overrides. Empty = inherit group defaults.
    pub permission_overrides: Vec<PermissionOverride>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChannelKind {
    Text,
    Voice,
    Media,
}

/// Per-channel permission override for a specific role.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionOverride {
    pub role: Role,
    pub allow: Permissions,
    pub deny: Permissions,
}

/// Granular permission flags.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Permissions {
    pub view: Option<bool>,
    pub send_messages: Option<bool>,
    pub manage_messages: Option<bool>,
    pub manage_channel: Option<bool>,
    pub connect_voice: Option<bool>,
    pub speak: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Member {
    pub peer_id: PeerId,
    pub display_name: String,
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Role {
    Owner,
    Admin,
    Moderator,
    Member,
}

#[derive(Debug, thiserror::Error)]
pub enum GroupError {
    #[error("failed to serialize group creation parameters")]
    Serialization,
}

impl Group {
    pub fn new(name: String, creator: PeerId, creator_name: String) -> Result<Self, GroupError> {
        let id_bytes = blake3::derive_key(
            "veil-group-id",
            &bincode::serialize(&(&name, &creator)).map_err(|_| GroupError::Serialization)?,
        );

        let text_cat = Category {
            id: CategoryId::new(),
            name: "Text Channels".into(),
            position: 0,
        };

        let general = Channel {
            id: ChannelId::new(),
            name: "general".into(),
            kind: ChannelKind::Text,
            category_id: Some(text_cat.id.clone()),
            position: 0,
            permission_overrides: vec![],
        };

        Ok(Self {
            id: GroupId(id_bytes),
            name,
            description: None,
            categories: vec![text_cat],
            channels: vec![general],
            members: vec![Member {
                peer_id: creator,
                display_name: creator_name,
                role: Role::Owner,
            }],
        })
    }

    /// Add a new category. Returns the CategoryId.
    pub fn add_category(&mut self, name: String) -> CategoryId {
        let position = self.categories.len() as u32;
        let cat = Category {
            id: CategoryId::new(),
            name,
            position,
        };
        let id = cat.id.clone();
        self.categories.push(cat);
        id
    }

    /// Add a channel, optionally under a category.
    pub fn add_channel(
        &mut self,
        name: String,
        kind: ChannelKind,
        category_id: Option<CategoryId>,
    ) -> ChannelId {
        let position = self
            .channels
            .iter()
            .filter(|c| c.category_id == category_id)
            .count() as u32;
        let channel = Channel {
            id: ChannelId::new(),
            name,
            kind,
            category_id,
            position,
            permission_overrides: vec![],
        };
        let id = channel.id.clone();
        self.channels.push(channel);
        id
    }

    /// Get channels in a category, sorted by position.
    pub fn channels_in_category(&self, category_id: Option<&CategoryId>) -> Vec<&Channel> {
        let mut channels: Vec<&Channel> = self
            .channels
            .iter()
            .filter(|c| c.category_id.as_ref() == category_id)
            .collect();
        channels.sort_by_key(|c| c.position);
        channels
    }

    /// Get categories sorted by position.
    pub fn sorted_categories(&self) -> Vec<&Category> {
        let mut cats: Vec<&Category> = self.categories.iter().collect();
        cats.sort_by_key(|c| c.position);
        cats
    }

    pub fn remove_category(&mut self, category_id: &CategoryId) {
        self.categories.retain(|c| c.id != *category_id);
        // Uncategorize any channels that were in this category
        for channel in &mut self.channels {
            if channel.category_id.as_ref() == Some(category_id) {
                channel.category_id = None;
            }
        }
    }

    pub fn remove_channel(&mut self, channel_id: &ChannelId) {
        self.channels.retain(|c| c.id != *channel_id);
    }

    pub fn add_member(&mut self, peer_id: PeerId, display_name: String) {
        self.members.push(Member {
            peer_id,
            display_name,
            role: Role::Member,
        });
    }

    pub fn remove_member(&mut self, peer_id: &PeerId) {
        self.members
            .retain(|m| m.peer_id.verifying_key != peer_id.verifying_key);
    }

    pub fn is_member(&self, peer_id: &PeerId) -> bool {
        self.members
            .iter()
            .any(|m| m.peer_id.verifying_key == peer_id.verifying_key)
    }

    pub fn get_member(&self, peer_id: &PeerId) -> Option<&Member> {
        self.members
            .iter()
            .find(|m| m.peer_id.verifying_key == peer_id.verifying_key)
    }

    pub fn set_member_role(&mut self, peer_id: &PeerId, role: Role) {
        if let Some(member) = self
            .members
            .iter_mut()
            .find(|m| m.peer_id.verifying_key == peer_id.verifying_key)
        {
            member.role = role;
        }
    }

    /// Check if a member has at least the given role level.
    pub fn has_permission(&self, peer_id: &PeerId, required: &Role) -> bool {
        self.get_member(peer_id)
            .map(|m| role_level(&m.role) >= role_level(required))
            .unwrap_or(false)
    }
}

/// Map roles to numeric levels for comparison.
pub fn role_level(role: &Role) -> u8 {
    match role {
        Role::Owner => 3,
        Role::Admin => 2,
        Role::Moderator => 1,
        Role::Member => 0,
    }
}
