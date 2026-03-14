use serde::{Deserialize, Serialize};
use veil_crypto::PeerId;

use crate::message::{ChannelId, GroupId};

/// A group is a community of peers (analogous to a Discord "server").
/// The group metadata itself is encrypted — pinners don't see any of this.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub channels: Vec<Channel>,
    pub members: Vec<Member>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub kind: ChannelKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChannelKind {
    Text,
    Voice,
    Media,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Member {
    pub peer_id: PeerId,
    pub display_name: String,
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Owner,
    Admin,
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

        let general = Channel {
            id: ChannelId::new(),
            name: "general".into(),
            kind: ChannelKind::Text,
        };

        Ok(Self {
            id: GroupId(id_bytes),
            name,
            channels: vec![general],
            members: vec![Member {
                peer_id: creator,
                display_name: creator_name,
                role: Role::Owner,
            }],
        })
    }

    pub fn add_channel(&mut self, name: String, kind: ChannelKind) -> ChannelId {
        let channel = Channel {
            id: ChannelId::new(),
            name,
            kind,
        };
        let id = channel.id.clone();
        self.channels.push(channel);
        id
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
}
