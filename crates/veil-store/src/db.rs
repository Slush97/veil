use std::path::Path;

use redb::{Database, ReadableTable, TableDefinition};
use veil_core::{MessageId, SealedMessage};
use veil_crypto::GroupKey;

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
        let plaintext =
            bincode::serialize(msg).map_err(|e| StoreError::Database(e.to_string()))?;
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
        for entry in table.iter().map_err(|e| StoreError::Database(e.to_string()))? {
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
        for entry in table.iter().map_err(|e| StoreError::Database(e.to_string()))? {
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
        for entry in table.iter().map_err(|e| StoreError::Database(e.to_string()))? {
            let entry = entry.map_err(|e| StoreError::Database(e.to_string()))?;
            let key_str: &str = entry.0.value();
            if !key_str.starts_with("group:") {
                continue;
            }

            let group_id_hex = &key_str[6..];
            let group_id_bytes = hex::decode(group_id_hex)
                .map_err(|e| StoreError::Database(e.to_string()))?;
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
            let name_len = u32::from_le_bytes(
                plaintext[..4].try_into().unwrap(),
            ) as usize;
            if plaintext.len() < 4 + name_len + 32 + 8 {
                continue;
            }
            let name = String::from_utf8_lossy(&plaintext[4..4 + name_len]).to_string();
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(&plaintext[4 + name_len..4 + name_len + 32]);
            let generation = u64::from_le_bytes(
                plaintext[4 + name_len + 32..4 + name_len + 40]
                    .try_into()
                    .unwrap(),
            );

            groups.push((group_id, name, GroupKey::from_raw_parts(key_bytes, generation)));
        }

        Ok(groups)
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
        store.store_group(&group_id, "Test Group", &group_key).unwrap();

        let groups = store.list_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, group_id);
        assert_eq!(groups[0].1, "Test Group");
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
}
