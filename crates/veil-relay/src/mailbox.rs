//! Persistent mailbox backed by redb.
//!
//! Messages survive relay restarts. Each message is stored with a composite key
//! `routing_tag(32) || sequence(8 BE)` so that per-tag ordering and draining
//! are efficient.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use tracing::info;

use crate::protocol::ForwardEnvelope;

/// Table: composite key `routing_tag(32) || sequence(8 BE)` → bincode(ForwardEnvelope)
const MESSAGES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("messages");

/// Table: `routing_tag(32)` → next sequence number (u64 BE)
const COUNTERS: TableDefinition<&[u8], u64> = TableDefinition::new("counters");

pub struct MailboxStore {
    db: Arc<Database>,
    max_per_tag: usize,
    max_total: usize,
    max_age: Duration,
}

impl MailboxStore {
    /// Open the mailbox store with its own database file.
    pub fn open(
        path: &Path,
        max_per_tag: usize,
        max_total: usize,
        max_age: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Arc::new(Database::create(path)?);
        Self::with_db(db, max_per_tag, max_total, max_age)
    }

    /// Create a mailbox store sharing an existing database.
    pub fn with_db(
        db: Arc<Database>,
        max_per_tag: usize,
        max_total: usize,
        max_age: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Ensure tables exist
        let txn = db.begin_write()?;
        {
            let _ = txn.open_table(MESSAGES)?;
            let _ = txn.open_table(COUNTERS)?;
        }
        txn.commit()?;

        Ok(Self {
            db,
            max_per_tag,
            max_total,
            max_age,
        })
    }

    /// Get a reference to the underlying database for sharing with other stores.
    pub fn database(&self) -> Arc<Database> {
        self.db.clone()
    }

    /// Push a message into the mailbox for a routing tag.
    /// Returns false if mailbox is full.
    pub fn push(&self, envelope: &ForwardEnvelope) -> Result<bool, Box<dyn std::error::Error>> {
        let total = self.total_count()?;
        if total >= self.max_total {
            return Ok(false);
        }

        let tag_count = self.count_by_tag(&envelope.routing_tag)?;
        if tag_count >= self.max_per_tag {
            return Ok(false);
        }

        let txn = self.db.begin_write()?;
        {
            let mut counters = txn.open_table(COUNTERS)?;
            let mut messages = txn.open_table(MESSAGES)?;

            // Get and increment sequence number
            let seq = counters
                .get(envelope.routing_tag.as_slice())?
                .map(|v| v.value())
                .unwrap_or(0);

            // Build composite key
            let key = composite_key(&envelope.routing_tag, seq);
            let value = bincode::serialize(envelope)?;
            messages.insert(key.as_slice(), value.as_slice())?;

            counters.insert(envelope.routing_tag.as_slice(), seq + 1)?;
        }
        txn.commit()?;

        Ok(true)
    }

    /// Drain up to `limit` messages for the given routing tags.
    /// Returns the drained messages and remaining count.
    pub fn drain(
        &self,
        tags: &[[u8; 32]],
        limit: usize,
    ) -> Result<(Vec<ForwardEnvelope>, u64), Box<dyn std::error::Error>> {
        let mut batch = Vec::new();
        let mut keys_to_remove = Vec::new();

        // Read phase
        {
            let rtxn = self.db.begin_read()?;
            let messages = rtxn.open_table(MESSAGES)?;

            for tag in tags {
                if batch.len() >= limit {
                    break;
                }

                let start = composite_key(tag, 0);
                let end = composite_key_end(tag);

                let range = messages.range::<&[u8]>(start.as_slice()..end.as_slice())?;
                for entry in range {
                    if batch.len() >= limit {
                        break;
                    }
                    let entry = entry?;
                    let key = entry.0.value().to_vec();
                    let value = entry.1.value();
                    if let Ok(envelope) = bincode::deserialize::<ForwardEnvelope>(value) {
                        batch.push(envelope);
                        keys_to_remove.push(key);
                    }
                }
            }
        }

        // Delete drained messages
        if !keys_to_remove.is_empty() {
            let txn = self.db.begin_write()?;
            {
                let mut messages = txn.open_table(MESSAGES)?;
                for key in &keys_to_remove {
                    messages.remove(key.as_slice())?;
                }
            }
            txn.commit()?;
        }

        // Count remaining
        let mut remaining = 0u64;
        {
            let rtxn = self.db.begin_read()?;
            let messages = rtxn.open_table(MESSAGES)?;
            for tag in tags {
                let start = composite_key(tag, 0);
                let end = composite_key_end(tag);
                remaining += messages
                    .range::<&[u8]>(start.as_slice()..end.as_slice())?
                    .count() as u64;
            }
        }

        Ok((batch, remaining))
    }

    /// Count messages for a specific routing tag.
    pub fn count_by_tag(&self, tag: &[u8; 32]) -> Result<usize, Box<dyn std::error::Error>> {
        let rtxn = self.db.begin_read()?;
        let messages = rtxn.open_table(MESSAGES)?;
        let start = composite_key(tag, 0);
        let end = composite_key_end(tag);
        let count = messages
            .range::<&[u8]>(start.as_slice()..end.as_slice())?
            .count();
        Ok(count)
    }

    /// Total messages across all routing tags.
    pub fn total_count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        let rtxn = self.db.begin_read()?;
        let messages = rtxn.open_table(MESSAGES)?;
        Ok(messages.len()? as usize)
    }

    /// Purge messages older than max_age.
    pub fn purge_expired(&self) -> Result<usize, Box<dyn std::error::Error>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let cutoff = now - self.max_age.as_secs() as i64;

        let mut keys_to_remove = Vec::new();

        {
            let rtxn = self.db.begin_read()?;
            let messages = rtxn.open_table(MESSAGES)?;
            let iter = messages.iter()?;
            for entry in iter {
                let entry = entry?;
                let value = entry.1.value();
                if let Ok(envelope) = bincode::deserialize::<ForwardEnvelope>(value)
                    && envelope.received_at < cutoff
                {
                    keys_to_remove.push(entry.0.value().to_vec());
                }
            }
        }

        let removed = keys_to_remove.len();
        if !keys_to_remove.is_empty() {
            let txn = self.db.begin_write()?;
            {
                let mut messages = txn.open_table(MESSAGES)?;
                for key in &keys_to_remove {
                    messages.remove(key.as_slice())?;
                }
            }
            txn.commit()?;
            info!("purged {removed} expired mailbox messages");
        }

        Ok(removed)
    }
}

/// Build composite key: routing_tag(32) || sequence(8 BE)
fn composite_key(tag: &[u8; 32], seq: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(40);
    key.extend_from_slice(tag);
    key.extend_from_slice(&seq.to_be_bytes());
    key
}

/// Build an exclusive end key for range scans: tag with all sequence bytes = 0xFF
fn composite_key_end(tag: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(40);
    key.extend_from_slice(tag);
    key.extend_from_slice(&[0xFF; 8]);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_store() -> (MailboxStore, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let store = MailboxStore::open(tmp.path(), 100, 1000, Duration::from_secs(86400)).unwrap();
        (store, tmp)
    }

    fn make_envelope(tag: [u8; 32], payload: &[u8]) -> ForwardEnvelope {
        ForwardEnvelope {
            routing_tag: tag,
            payload: payload.to_vec(),
            received_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        }
    }

    #[test]
    fn push_drain_roundtrip() {
        let (store, _tmp) = test_store();
        let tag = [1u8; 32];

        let env1 = make_envelope(tag, b"hello");
        let env2 = make_envelope(tag, b"world");

        assert!(store.push(&env1).unwrap());
        assert!(store.push(&env2).unwrap());
        assert_eq!(store.count_by_tag(&tag).unwrap(), 2);
        assert_eq!(store.total_count().unwrap(), 2);

        let (batch, remaining) = store.drain(&[tag], 10).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(remaining, 0);
        assert_eq!(batch[0].payload, b"hello");
        assert_eq!(batch[1].payload, b"world");

        assert_eq!(store.total_count().unwrap(), 0);
    }

    #[test]
    fn persistence_across_reopen() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let tag = [2u8; 32];

        {
            let store = MailboxStore::open(&path, 100, 1000, Duration::from_secs(86400)).unwrap();
            store.push(&make_envelope(tag, b"persist me")).unwrap();
            assert_eq!(store.total_count().unwrap(), 1);
        }

        // Reopen
        {
            let store = MailboxStore::open(&path, 100, 1000, Duration::from_secs(86400)).unwrap();
            assert_eq!(store.total_count().unwrap(), 1);
            let (batch, _) = store.drain(&[tag], 10).unwrap();
            assert_eq!(batch.len(), 1);
            assert_eq!(batch[0].payload, b"persist me");
        }
    }

    #[test]
    fn ttl_purge() {
        let (store, _tmp) = test_store();
        let tag = [3u8; 32];

        // Insert an envelope with a very old timestamp
        let mut old = make_envelope(tag, b"expired");
        old.received_at = 0; // epoch = way past any TTL

        store.push(&old).unwrap();
        store.push(&make_envelope(tag, b"fresh")).unwrap();
        assert_eq!(store.total_count().unwrap(), 2);

        let purged = store.purge_expired().unwrap();
        assert_eq!(purged, 1);
        assert_eq!(store.total_count().unwrap(), 1);

        let (batch, _) = store.drain(&[tag], 10).unwrap();
        assert_eq!(batch[0].payload, b"fresh");
    }

    #[test]
    fn respects_max_per_tag() {
        let tmp = NamedTempFile::new().unwrap();
        let store = MailboxStore::open(
            tmp.path(),
            2, // max 2 per tag
            1000,
            Duration::from_secs(86400),
        )
        .unwrap();

        let tag = [4u8; 32];
        assert!(store.push(&make_envelope(tag, b"1")).unwrap());
        assert!(store.push(&make_envelope(tag, b"2")).unwrap());
        assert!(!store.push(&make_envelope(tag, b"3")).unwrap()); // rejected
        assert_eq!(store.count_by_tag(&tag).unwrap(), 2);
    }

    #[test]
    fn respects_max_total() {
        let tmp = NamedTempFile::new().unwrap();
        let store = MailboxStore::open(
            tmp.path(),
            100,
            2, // max 2 total
            Duration::from_secs(86400),
        )
        .unwrap();

        let tag1 = [5u8; 32];
        let tag2 = [6u8; 32];
        assert!(store.push(&make_envelope(tag1, b"1")).unwrap());
        assert!(store.push(&make_envelope(tag2, b"2")).unwrap());
        assert!(!store.push(&make_envelope(tag1, b"3")).unwrap()); // rejected
    }
}
