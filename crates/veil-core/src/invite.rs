use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use veil_crypto::GroupKey;

use crate::message::GroupId;

/// Payload embedded in an invite URL.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvitePayload {
    pub group_id: GroupId,
    pub group_name: String,
    pub relay_addr: String,
    pub key_material: InviteKeyMaterial,
}

/// How the group key is protected in the invite.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InviteKeyMaterial {
    /// Open invite — anyone with the link + passphrase can join.
    Passphrase {
        salt: [u8; 16],
        encrypted_group_key: Vec<u8>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum InviteError {
    #[error("invalid invite URL format")]
    InvalidUrl,
    #[error("base64 decode failed")]
    Base64(#[from] base64::DecodeError),
    #[error("deserialization failed")]
    Deserialize(String),
    #[error("serialization failed")]
    Serialize(String),
    #[error("crypto error: {0}")]
    Crypto(#[from] veil_crypto::EncryptError),
}

impl InvitePayload {
    /// Encode this invite as a `veil://<relay_addr>/<base64url(bincode(self))>` URL.
    pub fn to_url(&self) -> Result<String, InviteError> {
        let bytes = bincode::serialize(self).map_err(|e| InviteError::Serialize(e.to_string()))?;
        let encoded = URL_SAFE_NO_PAD.encode(&bytes);
        Ok(format!("veil://{}/{}", self.relay_addr, encoded))
    }

    /// Parse an invite from a `veil://` URL.
    pub fn from_url(url: &str) -> Result<Self, InviteError> {
        let stripped = url.strip_prefix("veil://").ok_or(InviteError::InvalidUrl)?;

        // Find the last '/' to split relay_addr from payload
        let slash_pos = stripped.rfind('/').ok_or(InviteError::InvalidUrl)?;
        let encoded = &stripped[slash_pos + 1..];

        if encoded.is_empty() {
            return Err(InviteError::InvalidUrl);
        }

        let bytes = URL_SAFE_NO_PAD.decode(encoded)?;
        bincode::deserialize(&bytes).map_err(|e| InviteError::Deserialize(e.to_string()))
    }
}

/// Create an open invite that anyone with the passphrase can accept.
pub fn create_open_invite(
    group_id: GroupId,
    group_name: String,
    relay_addr: String,
    group_key: &GroupKey,
    passphrase: &[u8],
) -> Result<InvitePayload, InviteError> {
    let (salt, encrypted_group_key) = group_key.encrypt_with_passphrase(passphrase)?;
    Ok(InvitePayload {
        group_id,
        group_name,
        relay_addr,
        key_material: InviteKeyMaterial::Passphrase {
            salt,
            encrypted_group_key,
        },
    })
}

/// Accept an invite by decrypting the group key with the passphrase.
pub fn accept_invite(
    payload: &InvitePayload,
    passphrase: &[u8],
) -> Result<GroupKey, InviteError> {
    match &payload.key_material {
        InviteKeyMaterial::Passphrase {
            salt,
            encrypted_group_key,
        } => {
            let key =
                GroupKey::decrypt_with_passphrase(encrypted_group_key, salt, passphrase)?;
            Ok(key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_roundtrip() {
        let group_key = GroupKey::generate();
        let group_id = GroupId([42u8; 32]);
        let passphrase = b"super secret";

        let invite = create_open_invite(
            group_id.clone(),
            "Test Group".into(),
            "relay.example.com:4433".into(),
            &group_key,
            passphrase,
        )
        .unwrap();

        let url = invite.to_url().unwrap();
        assert!(url.starts_with("veil://relay.example.com:4433/"));

        let parsed = InvitePayload::from_url(&url).unwrap();
        assert_eq!(parsed.group_id, group_id);
        assert_eq!(parsed.group_name, "Test Group");
        assert_eq!(parsed.relay_addr, "relay.example.com:4433");

        let decrypted_key = accept_invite(&parsed, passphrase).unwrap();

        // Verify the key works
        let msg = group_key.encrypt(b"hello").unwrap();
        assert!(decrypted_key.decrypt(&msg).is_ok());
    }

    #[test]
    fn invite_wrong_passphrase_fails() {
        let group_key = GroupKey::generate();
        let group_id = GroupId([1u8; 32]);

        let invite = create_open_invite(
            group_id,
            "Group".into(),
            "localhost:4433".into(),
            &group_key,
            b"correct",
        )
        .unwrap();

        let url = invite.to_url().unwrap();
        let parsed = InvitePayload::from_url(&url).unwrap();
        assert!(accept_invite(&parsed, b"wrong").is_err());
    }

    #[test]
    fn invalid_url_rejected() {
        assert!(InvitePayload::from_url("http://example.com").is_err());
        assert!(InvitePayload::from_url("veil://").is_err());
        assert!(InvitePayload::from_url("veil://host/").is_err());
    }
}
