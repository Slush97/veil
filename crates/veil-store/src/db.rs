use std::path::Path;

use redb::{Database, ReadableTable, TableDefinition};
use veil_core::{MessageId, SealedMessage};
use veil_crypto::{DeviceCertificate, GroupKey, GroupKeyRing};

use crate::StoreError;

const MESSAGES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("messages");
const BLOBS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("blob_shards");
const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");

/// Local database for storing messages and blob shards.
/// Values are encrypted at rest using a storage key derived from the user's identity.
pub struct LocalStore {
    db: Database,
    storage_key: GroupKey,
}

impl LocalStore {
    /// Open the local store. `storage_key` is derived from the user's identity
    /// and used to encrypt all values at rest.
    pub fn open(path: &Path, storage_key: GroupKey) -> Result<Self, StoreError> {
        let db = Database::create(path).map_err(|e| StoreError::Database(e.to_string()))?;

        let tx = db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let _ = tx.open_table(MESSAGES);
            let _ = tx.open_table(BLOBS);
            let _ = tx.open_table(METADATA);
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(Self { db, storage_key })
    }

    /// Derive a storage key from identity bytes using blake3 key derivation.
    pub fn derive_storage_key(identity_bytes: &[u8; 32]) -> GroupKey {
        let derived = blake3::derive_key("veil-local-storage-key", identity_bytes);
        unsafe_group_key_from_bytes(derived)
    }

    pub fn store_message(&self, msg: &SealedMessage) -> Result<(), StoreError> {
        let key = &msg.id.0;
        let plaintext = bincode::serialize(msg).map_err(|e| StoreError::Database(e.to_string()))?;
        let encrypted = self
            .storage_key
            .encrypt(&plaintext)
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(MESSAGES)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(key.as_slice(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    pub fn get_message(&self, id: &MessageId) -> Result<Option<SealedMessage>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(MESSAGES)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        match table
            .get(id.0.as_slice())
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            Some(value) => {
                let plaintext = self
                    .storage_key
                    .decrypt(value.value())
                    .map_err(|e| StoreError::Crypto(e.to_string()))?;
                let msg = bincode::deserialize(&plaintext)
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                Ok(Some(msg))
            }
            None => Ok(None),
        }
    }

    /// List all stored messages, decrypting each from the database.
    pub fn list_messages(&self) -> Result<Vec<SealedMessage>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(MESSAGES)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut messages = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let value_bytes: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(value_bytes)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;
            let msg: SealedMessage = bincode::deserialize(&plaintext)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            messages.push(msg);
        }
        Ok(messages)
    }

    /// List messages matching a specific routing tag, with pagination.
    pub fn list_messages_by_tag(
        &self,
        routing_tag: &[u8; 32],
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SealedMessage>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(MESSAGES)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut messages = Vec::new();
        let mut skipped = 0;
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let value_bytes: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(value_bytes)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;
            let msg: SealedMessage = bincode::deserialize(&plaintext)
                .map_err(|e| StoreError::Database(e.to_string()))?;

            if msg.routing_tag == *routing_tag {
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                messages.push(msg);
                if messages.len() >= limit {
                    break;
                }
            }
        }
        Ok(messages)
    }

    /// Store an encrypted group key in the metadata table.
    /// The group key is encrypted with the storage key before persisting.
    pub fn store_group(
        &self,
        group_id: &[u8; 32],
        group_name: &str,
        group_key: &GroupKey,
    ) -> Result<(), StoreError> {
        let (key_bytes, generation) = group_key.to_raw_parts();
        // Serialize: name_len(4) + name + key(32) + generation(8)
        let name_bytes = group_name.as_bytes();
        let mut plaintext = Vec::with_capacity(4 + name_bytes.len() + 32 + 8);
        plaintext.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        plaintext.extend_from_slice(name_bytes);
        plaintext.extend_from_slice(&key_bytes);
        plaintext.extend_from_slice(&generation.to_le_bytes());

        let encrypted = self
            .storage_key
            .encrypt(&plaintext)
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let meta_key = format!("group:{}", hex::encode(group_id));

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(METADATA)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(meta_key.as_str(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    /// Load all persisted groups from the metadata table.
    /// Returns (group_id, group_name, group_key) tuples.
    pub fn list_groups(&self) -> Result<Vec<([u8; 32], String, GroupKey)>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(METADATA)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut groups = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let key_str: &str = entry.0.value();
            if !key_str.starts_with("group:") {
                continue;
            }

            let group_id_hex = &key_str[6..];
            let group_id_bytes =
                hex::decode(group_id_hex).map_err(|e| StoreError::Database(e.to_string()))?;
            if group_id_bytes.len() != 32 {
                continue;
            }
            let mut group_id = [0u8; 32];
            group_id.copy_from_slice(&group_id_bytes);

            let encrypted: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(encrypted)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;

            if plaintext.len() < 4 {
                continue;
            }
            let name_len =
                u32::from_le_bytes(plaintext[..4].try_into().expect("slice is exactly 4 bytes"))
                    as usize;
            if plaintext.len() < 4 + name_len + 32 + 8 {
                continue;
            }
            let name = String::from_utf8_lossy(&plaintext[4..4 + name_len]).to_string();
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(&plaintext[4 + name_len..4 + name_len + 32]);
            let generation = u64::from_le_bytes(
                plaintext[4 + name_len + 32..4 + name_len + 40]
                    .try_into()
                    .expect("slice is exactly 8 bytes"),
            );

            groups.push((
                group_id,
                name,
                GroupKey::from_raw_parts(key_bytes, generation),
            ));
        }

        Ok(groups)
    }

    /// Store a device certificate in the metadata table.
    pub fn store_device_cert(
        &self,
        device_id: &veil_crypto::PeerId,
        cert: &DeviceCertificate,
    ) -> Result<(), StoreError> {
        let cert_bytes =
            bincode::serialize(cert).map_err(|e| StoreError::Database(e.to_string()))?;
        let encrypted = self
            .storage_key
            .encrypt(&cert_bytes)
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let meta_key = format!("devcert:{}", device_id.fingerprint());

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(METADATA)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(meta_key.as_str(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    /// List all stored device certificates.
    pub fn list_device_certs(&self) -> Result<Vec<DeviceCertificate>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(METADATA)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut certs = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let key_str: &str = entry.0.value();
            if !key_str.starts_with("devcert:") {
                continue;
            }

            let encrypted: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(encrypted)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;

            let cert: DeviceCertificate = bincode::deserialize(&plaintext)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            certs.push(cert);
        }

        Ok(certs)
    }

    /// Store a group with its full keyring (v2 format).
    pub fn store_group_v2(
        &self,
        group_id: &[u8; 32],
        group_name: &str,
        keyring: &GroupKeyRing,
    ) -> Result<(), StoreError> {
        let ring_data = keyring.to_persist_data();
        let ring_bytes =
            bincode::serialize(&ring_data).map_err(|e| StoreError::Database(e.to_string()))?;

        // Serialize: name_len(4) + name + ring_data
        let name_bytes = group_name.as_bytes();
        let mut plaintext = Vec::with_capacity(4 + name_bytes.len() + ring_bytes.len());
        plaintext.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        plaintext.extend_from_slice(name_bytes);
        plaintext.extend_from_slice(&ring_bytes);

        let encrypted = self
            .storage_key
            .encrypt(&plaintext)
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let meta_key = format!("group_v2:{}", hex::encode(group_id));

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(METADATA)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(meta_key.as_str(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    /// Load all v2 groups from the metadata table.
    /// Returns (group_id, group_name, GroupKeyRing) tuples.
    pub fn list_groups_v2(&self) -> Result<Vec<([u8; 32], String, GroupKeyRing)>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(METADATA)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut groups = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let key_str: &str = entry.0.value();
            if !key_str.starts_with("group_v2:") {
                continue;
            }

            let group_id_hex = &key_str[9..];
            let group_id_bytes =
                hex::decode(group_id_hex).map_err(|e| StoreError::Database(e.to_string()))?;
            if group_id_bytes.len() != 32 {
                continue;
            }
            let mut group_id = [0u8; 32];
            group_id.copy_from_slice(&group_id_bytes);

            let encrypted: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(encrypted)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;

            if plaintext.len() < 4 {
                continue;
            }
            let name_len =
                u32::from_le_bytes(plaintext[..4].try_into().expect("slice is exactly 4 bytes"))
                    as usize;
            if plaintext.len() < 4 + name_len {
                continue;
            }
            let name = String::from_utf8_lossy(&plaintext[4..4 + name_len]).to_string();

            let ring_data: veil_crypto::KeyRingData =
                bincode::deserialize(&plaintext[4 + name_len..])
                    .map_err(|e| StoreError::Database(e.to_string()))?;
            let keyring = GroupKeyRing::from_persist_data(ring_data);

            groups.push((group_id, name, keyring));
        }

        Ok(groups)
    }

    /// Delete a v2 group from the metadata table.
    pub fn delete_group_v2(&self, group_id: &[u8; 32]) -> Result<(), StoreError> {
        let meta_key = format!("group_v2:{}", hex::encode(group_id));

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(METADATA)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .remove(meta_key.as_str())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    /// Store a full encrypted blob locally. The sender always keeps a complete copy
    /// so recipients can fall back to direct transfer if not enough shards are available.
    pub fn store_blob_full(
        &self,
        blob_id: &veil_core::BlobId,
        encrypted_data: &[u8],
    ) -> Result<(), StoreError> {
        let encrypted = self
            .storage_key
            .encrypt(encrypted_data)
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        // Use "full:" prefix to distinguish from shards
        let mut key = b"full:".to_vec();
        key.extend_from_slice(&blob_id.0);

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(BLOBS)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(key.as_slice(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    /// Retrieve a full encrypted blob by ID.
    pub fn get_blob_full(
        &self,
        blob_id: &veil_core::BlobId,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let mut key = b"full:".to_vec();
        key.extend_from_slice(&blob_id.0);

        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(BLOBS)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        match table
            .get(key.as_slice())
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            Some(value) => {
                let plaintext = self
                    .storage_key
                    .decrypt(value.value())
                    .map_err(|e| StoreError::Crypto(e.to_string()))?;
                Ok(Some(plaintext))
            }
            None => Ok(None),
        }
    }

    /// List available shard indices for a blob.
    pub fn list_blob_shards(
        &self,
        blob_id: &veil_core::BlobId,
    ) -> Result<Vec<crate::blob::BlobShard>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(BLOBS)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut shards = Vec::new();
        for idx in 0..crate::blob::TOTAL_SHARDS as u8 {
            let mut key = blob_id.0.to_vec();
            key.push(idx);

            if let Some(value) = table
                .get(key.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?
            {
                let plaintext = self
                    .storage_key
                    .decrypt(value.value())
                    .map_err(|e| StoreError::Crypto(e.to_string()))?;
                let shard: crate::blob::BlobShard = bincode::deserialize(&plaintext)
                    .map_err(|e| StoreError::Database(e.to_string()))?;
                shards.push(shard);
            }
        }

        Ok(shards)
    }

    pub fn store_blob_shard(&self, shard: &crate::blob::BlobShard) -> Result<(), StoreError> {
        let mut key = shard.blob_id.0.to_vec();
        key.push(shard.shard_index);

        let plaintext =
            bincode::serialize(shard).map_err(|e| StoreError::Database(e.to_string()))?;
        let encrypted = self
            .storage_key
            .encrypt(&plaintext)
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(BLOBS)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(key.as_slice(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;

        Ok(())
    }

    /// Get the latest message ID for a routing tag (for sync protocol).
    pub fn latest_message_id_by_tag(
        &self,
        routing_tag: &[u8; 32],
    ) -> Result<Option<MessageId>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(MESSAGES)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut latest: Option<(i64, MessageId)> = None;
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let value_bytes: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(value_bytes)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;
            let msg: SealedMessage = bincode::deserialize(&plaintext)
                .map_err(|e| StoreError::Database(e.to_string()))?;

            if msg.routing_tag == *routing_tag {
                match &latest {
                    Some((ts, _)) if msg.sent_at > *ts => {
                        latest = Some((msg.sent_at, msg.id));
                    }
                    None => {
                        latest = Some((msg.sent_at, msg.id));
                    }
                    _ => {}
                }
            }
        }
        Ok(latest.map(|(_, id)| id))
    }

    /// Store a display name for a peer.
    pub fn store_display_name(&self, peer_fingerprint: &str, name: &str) -> Result<(), StoreError> {
        let meta_key = format!("name:{peer_fingerprint}");
        let encrypted = self
            .storage_key
            .encrypt(name.as_bytes())
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(METADATA)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(meta_key.as_str(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    /// Load all display names.
    pub fn list_display_names(&self) -> Result<Vec<(String, String)>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(METADATA)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut names = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let key_str: &str = entry.0.value();
            if !key_str.starts_with("name:") {
                continue;
            }
            let fingerprint = key_str[5..].to_string();
            let encrypted: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(encrypted)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;
            let name = String::from_utf8_lossy(&plaintext).to_string();
            names.push((fingerprint, name));
        }
        Ok(names)
    }

    /// Store a setting value.
    pub fn store_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        let meta_key = format!("setting:{key}");
        let encrypted = self
            .storage_key
            .encrypt(value.as_bytes())
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(METADATA)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(meta_key.as_str(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    /// Get a setting value.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        let meta_key = format!("setting:{key}");
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(METADATA)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        match table
            .get(meta_key.as_str())
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            Some(value) => {
                let plaintext = self
                    .storage_key
                    .decrypt(value.value())
                    .map_err(|e| StoreError::Crypto(e.to_string()))?;
                Ok(Some(String::from_utf8_lossy(&plaintext).to_string()))
            }
            None => Ok(None),
        }
    }

    /// Store a contact (username → public key).
    pub fn store_contact(&self, username: &str, public_key: &[u8; 32]) -> Result<(), StoreError> {
        let meta_key = format!("contact:{}", username.to_lowercase());
        let encrypted = self
            .storage_key
            .encrypt(public_key.as_slice())
            .map_err(|e| StoreError::Crypto(e.to_string()))?;

        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(METADATA)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .insert(meta_key.as_str(), encrypted.as_slice())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    /// List all stored contacts. Returns (username, public_key) pairs.
    pub fn list_contacts(&self) -> Result<Vec<(String, [u8; 32])>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(METADATA)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let mut contacts = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let key_str: &str = entry.0.value();
            if !key_str.starts_with("contact:") {
                continue;
            }
            let username = key_str[8..].to_string();
            let encrypted: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(encrypted)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;
            if plaintext.len() != 32 {
                continue;
            }
            let mut public_key = [0u8; 32];
            public_key.copy_from_slice(&plaintext);
            contacts.push((username, public_key));
        }
        Ok(contacts)
    }

    /// Remove a contact by username.
    pub fn remove_contact(&self, username: &str) -> Result<(), StoreError> {
        let meta_key = format!("contact:{}", username.to_lowercase());
        let tx = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        {
            let mut table = tx
                .open_table(METADATA)
                .map_err(|e| StoreError::Database(e.to_string()))?;
            table
                .remove(meta_key.as_str())
                .map_err(|e| StoreError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        Ok(())
    }

    /// Search messages by substring, returning matching sealed messages.
    pub fn search_messages(
        &self,
        routing_tag: &[u8; 32],
        group_key: &GroupKey,
        known_members: &[veil_crypto::PeerId],
        query: &str,
    ) -> Result<Vec<SealedMessage>, StoreError> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let table = tx
            .open_table(MESSAGES)
            .map_err(|e| StoreError::Database(e.to_string()))?;

        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| StoreError::Database(e.to_string()))?
        {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let value_bytes: &[u8] = entry.1.value();
            let plaintext = self
                .storage_key
                .decrypt(value_bytes)
                .map_err(|e| StoreError::Crypto(e.to_string()))?;
            let msg: SealedMessage = bincode::deserialize(&plaintext)
                .map_err(|e| StoreError::Database(e.to_string()))?;

            if msg.routing_tag != *routing_tag {
                continue;
            }

            // Try to decrypt and check content
            if let Ok((content, _)) = msg.verify_and_open(group_key, known_members)
                && let veil_core::MessageKind::Text(ref txt) = content.kind
                && txt.to_lowercase().contains(&query_lower)
            {
                results.push(msg);
            }
        }
        Ok(results)
    }
}

/// Create a GroupKey from raw bytes. This is only used internally for
/// deriving storage keys — the key material comes from blake3 KDF.
fn unsafe_group_key_from_bytes(bytes: [u8; 32]) -> GroupKey {
    GroupKey::from_storage_key(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use veil_core::{MessageId, SealedMessage};

    #[test]
    fn store_and_retrieve_message() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        let msg = SealedMessage {
            id: MessageId([1u8; 32]),
            routing_tag: [2u8; 32],
            ciphertext: vec![3, 4, 5],
            signature: vec![6, 7, 8],
            key_generation: 0,
            sent_at: 1000,
        };

        store.store_message(&msg).unwrap();
        let retrieved = store.get_message(&msg.id).unwrap().unwrap();
        assert_eq!(retrieved.ciphertext, msg.ciphertext);
    }

    #[test]
    fn store_and_list_groups() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        let group_key = GroupKey::from_storage_key([42u8; 32]);
        let group_id = [1u8; 32];
        store
            .store_group(&group_id, "Test Group", &group_key)
            .unwrap();

        let groups = store.list_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, group_id);
        assert_eq!(groups[0].1, "Test Group");
    }

    #[test]
    fn store_and_list_device_certs() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        let (master, _) = veil_crypto::MasterIdentity::generate();
        let device = veil_crypto::DeviceIdentity::new(&master, "Phone".into());
        let cert = device.certificate().clone();

        store
            .store_device_cert(&device.device_peer_id(), &cert)
            .unwrap();

        let certs = store.list_device_certs().unwrap();
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0].device_name, "Phone");
        assert!(certs[0].verify());
    }

    #[test]
    fn store_and_list_groups_v2() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        let group_key = GroupKey::from_storage_key([42u8; 32]);
        let group_id = [1u8; 32];
        let keyring = veil_crypto::GroupKeyRing::new(group_key, b"alice".to_vec());

        store
            .store_group_v2(&group_id, "Test Group v2", &keyring)
            .unwrap();

        let groups = store.list_groups_v2().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, group_id);
        assert_eq!(groups[0].1, "Test Group v2");
        assert_eq!(groups[0].2.generation(), 0);
    }

    #[test]
    fn list_messages_by_tag() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        let tag_a = [1u8; 32];
        let tag_b = [2u8; 32];

        for i in 0..5u8 {
            let msg = SealedMessage {
                id: MessageId([i; 32]),
                routing_tag: if i < 3 { tag_a } else { tag_b },
                ciphertext: vec![i],
                signature: vec![i],
                key_generation: 0,
                sent_at: i as i64,
            };
            store.store_message(&msg).unwrap();
        }

        let tag_a_msgs = store.list_messages_by_tag(&tag_a, 100, 0).unwrap();
        assert_eq!(tag_a_msgs.len(), 3);

        let tag_b_msgs = store.list_messages_by_tag(&tag_b, 100, 0).unwrap();
        assert_eq!(tag_b_msgs.len(), 2);

        // Test pagination
        let paged = store.list_messages_by_tag(&tag_a, 2, 0).unwrap();
        assert_eq!(paged.len(), 2);
    }

    #[test]
    fn store_and_list_display_names() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        store.store_display_name("abc123", "Alice").unwrap();
        store.store_display_name("def456", "Bob").unwrap();

        let names = store.list_display_names().unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.iter().any(|(fp, n)| fp == "abc123" && n == "Alice"));
        assert!(names.iter().any(|(fp, n)| fp == "def456" && n == "Bob"));
    }

    #[test]
    fn store_and_get_settings() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        store.store_setting("theme", "dark").unwrap();
        store.store_setting("relay_addr", "127.0.0.1:4433").unwrap();

        assert_eq!(store.get_setting("theme").unwrap(), Some("dark".into()));
        assert_eq!(
            store.get_setting("relay_addr").unwrap(),
            Some("127.0.0.1:4433".into())
        );
        assert_eq!(store.get_setting("nonexistent").unwrap(), None);
    }

    #[test]
    fn store_and_list_contacts() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        store.store_contact("alice", &[10u8; 32]).unwrap();
        store.store_contact("bob", &[20u8; 32]).unwrap();

        let contacts = store.list_contacts().unwrap();
        assert_eq!(contacts.len(), 2);
        assert!(
            contacts
                .iter()
                .any(|(n, k)| n == "alice" && *k == [10u8; 32])
        );
        assert!(contacts.iter().any(|(n, k)| n == "bob" && *k == [20u8; 32]));

        store.remove_contact("alice").unwrap();
        let contacts = store.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].0, "bob");
    }

    #[test]
    fn latest_message_id_by_tag() {
        let tmp = NamedTempFile::new().unwrap();
        let storage_key = LocalStore::derive_storage_key(&[1u8; 32]);
        let store = LocalStore::open(tmp.path(), storage_key).unwrap();

        let tag = [1u8; 32];
        for i in 0..3u8 {
            let msg = SealedMessage {
                id: MessageId([i; 32]),
                routing_tag: tag,
                ciphertext: vec![i],
                signature: vec![i],
                key_generation: 0,
                sent_at: (i as i64) * 100,
            };
            store.store_message(&msg).unwrap();
        }

        let latest = store.latest_message_id_by_tag(&tag).unwrap().unwrap();
        assert_eq!(latest, MessageId([2u8; 32]));

        // Non-existent tag
        let none = store.latest_message_id_by_tag(&[99u8; 32]).unwrap();
        assert!(none.is_none());
    }
}
