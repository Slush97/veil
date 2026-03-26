//! Username directory backed by redb.
//!
//! Stores `username → public_key` mappings so clients can discover each other
//! by human-readable names. The relay only stores public information (usernames
//! and Ed25519 public keys) — it cannot derive encryption keys or read messages.

use std::sync::Arc;

use redb::{Database, TableDefinition};
use tracing::info;

/// Table: lowercase username → bincode({ public_key: [u8;32], registered_at: i64 })
const USERNAMES: TableDefinition<&str, &[u8]> = TableDefinition::new("usernames");

/// Reverse index: hex(public_key) → username (one username per key)
const KEY_TO_USERNAME: TableDefinition<&str, &str> = TableDefinition::new("key_to_username");

#[derive(serde::Serialize, serde::Deserialize)]
struct UsernameRecord {
    public_key: [u8; 32],
    registered_at: i64,
}

#[derive(Debug, PartialEq)]
pub enum RegisterOutcome {
    Success,
    UsernameTaken,
    KeyAlreadyRegistered(String),
    InvalidUsername,
}

pub struct DirectoryStore {
    db: Arc<Database>,
}

impl DirectoryStore {
    pub fn new(db: Arc<Database>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Ensure tables exist
        let txn = db.begin_write()?;
        {
            let _ = txn.open_table(USERNAMES)?;
            let _ = txn.open_table(KEY_TO_USERNAME)?;
        }
        txn.commit()?;
        Ok(Self { db })
    }

    /// Register a username for a public key.
    pub fn register(
        &self,
        username: &str,
        public_key: &[u8; 32],
    ) -> Result<RegisterOutcome, Box<dyn std::error::Error + Send + Sync>> {
        if !is_valid_username(username) {
            return Ok(RegisterOutcome::InvalidUsername);
        }

        let username_lower = username.to_lowercase();
        let key_hex = hex::encode(public_key);

        // Check if key already has a username
        {
            let rtxn = self.db.begin_read()?;
            let key_table = rtxn.open_table(KEY_TO_USERNAME)?;
            if let Some(existing) = key_table.get(key_hex.as_str())? {
                return Ok(RegisterOutcome::KeyAlreadyRegistered(
                    existing.value().to_string(),
                ));
            }
        }

        // Check if username is taken
        {
            let rtxn = self.db.begin_read()?;
            let user_table = rtxn.open_table(USERNAMES)?;
            if user_table.get(username_lower.as_str())?.is_some() {
                return Ok(RegisterOutcome::UsernameTaken);
            }
        }

        let record = UsernameRecord {
            public_key: *public_key,
            registered_at: chrono_timestamp(),
        };
        let record_bytes = bincode::serialize(&record)?;

        let txn = self.db.begin_write()?;
        {
            let mut user_table = txn.open_table(USERNAMES)?;
            let mut key_table = txn.open_table(KEY_TO_USERNAME)?;
            user_table.insert(username_lower.as_str(), record_bytes.as_slice())?;
            key_table.insert(key_hex.as_str(), username_lower.as_str())?;
        }
        txn.commit()?;

        info!("registered username: {username_lower}");
        Ok(RegisterOutcome::Success)
    }

    /// Look up a public key by username.
    pub fn lookup(
        &self,
        username: &str,
    ) -> Result<Option<[u8; 32]>, Box<dyn std::error::Error + Send + Sync>> {
        let username_lower = username.to_lowercase();
        let rtxn = self.db.begin_read()?;
        let user_table = rtxn.open_table(USERNAMES)?;
        match user_table.get(username_lower.as_str())? {
            Some(value) => {
                let record: UsernameRecord = bincode::deserialize(value.value())?;
                Ok(Some(record.public_key))
            }
            None => Ok(None),
        }
    }
}

/// Validate username: 3-20 chars, alphanumeric + underscore only.
fn is_valid_username(username: &str) -> bool {
    let len = username.len();
    (3..=20).contains(&len)
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn chrono_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_dir() -> (DirectoryStore, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let db = Arc::new(Database::create(tmp.path()).unwrap());
        let store = DirectoryStore::new(db).unwrap();
        (store, tmp)
    }

    #[test]
    fn register_and_lookup() {
        let (store, _tmp) = test_dir();
        let key = [1u8; 32];
        assert_eq!(
            store.register("Alice", &key).unwrap(),
            RegisterOutcome::Success
        );
        let found = store.lookup("alice").unwrap();
        assert_eq!(found, Some(key));
    }

    #[test]
    fn case_insensitive_lookup() {
        let (store, _tmp) = test_dir();
        let key = [2u8; 32];
        store.register("Bob", &key).unwrap();
        assert_eq!(store.lookup("BOB").unwrap(), Some(key));
        assert_eq!(store.lookup("bob").unwrap(), Some(key));
    }

    #[test]
    fn username_taken() {
        let (store, _tmp) = test_dir();
        store.register("carol", &[3u8; 32]).unwrap();
        assert_eq!(
            store.register("Carol", &[4u8; 32]).unwrap(),
            RegisterOutcome::UsernameTaken
        );
    }

    #[test]
    fn key_already_registered() {
        let (store, _tmp) = test_dir();
        let key = [5u8; 32];
        store.register("dave", &key).unwrap();
        match store.register("dave2", &key).unwrap() {
            RegisterOutcome::KeyAlreadyRegistered(name) => assert_eq!(name, "dave"),
            other => panic!("expected KeyAlreadyRegistered, got {other:?}"),
        }
    }

    #[test]
    fn invalid_usernames() {
        let (store, _tmp) = test_dir();
        let key = [6u8; 32];
        assert_eq!(
            store.register("ab", &key).unwrap(),
            RegisterOutcome::InvalidUsername
        ); // too short
        assert_eq!(
            store.register("a".repeat(21).as_str(), &key).unwrap(),
            RegisterOutcome::InvalidUsername
        ); // too long
        assert_eq!(
            store.register("no spaces", &key).unwrap(),
            RegisterOutcome::InvalidUsername
        );
        assert_eq!(
            store.register("no@special", &key).unwrap(),
            RegisterOutcome::InvalidUsername
        );
    }

    #[test]
    fn valid_usernames() {
        let (store, _tmp) = test_dir();
        assert_eq!(
            store.register("abc", &[7u8; 32]).unwrap(),
            RegisterOutcome::Success
        );
        assert_eq!(
            store.register("user_123", &[8u8; 32]).unwrap(),
            RegisterOutcome::Success
        );
        assert_eq!(
            store.register("a".repeat(20).as_str(), &[9u8; 32]).unwrap(),
            RegisterOutcome::Success
        );
    }

    #[test]
    fn lookup_nonexistent() {
        let (store, _tmp) = test_dir();
        assert_eq!(store.lookup("nobody").unwrap(), None);
    }
}
