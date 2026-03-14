use std::num::NonZeroUsize;

use chrono::Utc;
use lru::LruCache;

use crate::message::{MessageId, SealedMessage};

/// Maximum age of a message before it's considered stale (24 hours).
const MAX_AGE_SECS: i64 = 24 * 60 * 60;
/// Maximum amount a message can be in the future (5 minutes).
const MAX_FUTURE_SECS: i64 = 5 * 60;
/// Default capacity of the dedup cache.
const DEFAULT_CAPACITY: usize = 100_000;

#[derive(Debug, thiserror::Error)]
pub enum DeduplicateError {
    #[error("duplicate message")]
    Duplicate,
    #[error("message too old: sent_at {sent_at}, now {now}")]
    TooOld { sent_at: i64, now: i64 },
    #[error("message from the future: sent_at {sent_at}, now {now}")]
    FromFuture { sent_at: i64, now: i64 },
}

/// Bounded LRU-based message deduplicator with timestamp validation.
pub struct MessageDeduplicator {
    seen: LruCache<MessageId, i64>,
}

impl MessageDeduplicator {
    pub fn new() -> Self {
        Self {
            seen: LruCache::new(
                NonZeroUsize::new(DEFAULT_CAPACITY).expect("DEFAULT_CAPACITY is non-zero"),
            ),
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            seen: LruCache::new(
                NonZeroUsize::new(cap.max(1)).expect("cap.max(1) is always non-zero"),
            ),
        }
    }

    /// Check whether a message should be accepted.
    /// Returns Ok(()) if accepted (and records it), or an error describing why it was rejected.
    pub fn check(&mut self, msg: &SealedMessage) -> Result<(), DeduplicateError> {
        let now = Utc::now().timestamp();

        // Reject messages older than 24 hours
        if now - msg.sent_at > MAX_AGE_SECS {
            return Err(DeduplicateError::TooOld {
                sent_at: msg.sent_at,
                now,
            });
        }

        // Reject messages more than 5 minutes in the future
        if msg.sent_at - now > MAX_FUTURE_SECS {
            return Err(DeduplicateError::FromFuture {
                sent_at: msg.sent_at,
                now,
            });
        }

        // Reject duplicates
        if self.seen.contains(&msg.id) {
            return Err(DeduplicateError::Duplicate);
        }

        self.seen.put(msg.id.clone(), msg.sent_at);
        Ok(())
    }
}

impl Default for MessageDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{MessageId, SealedMessage};

    fn make_msg(id_byte: u8, sent_at: i64) -> SealedMessage {
        SealedMessage {
            id: MessageId([id_byte; 32]),
            routing_tag: [0u8; 32],
            ciphertext: vec![],
            signature: vec![],
            key_generation: 0,
            sent_at,
        }
    }

    #[test]
    fn accepts_fresh_message() {
        let mut dedup = MessageDeduplicator::new();
        let msg = make_msg(1, Utc::now().timestamp());
        assert!(dedup.check(&msg).is_ok());
    }

    #[test]
    fn rejects_duplicate() {
        let mut dedup = MessageDeduplicator::new();
        let msg = make_msg(1, Utc::now().timestamp());
        assert!(dedup.check(&msg).is_ok());
        assert!(matches!(
            dedup.check(&msg),
            Err(DeduplicateError::Duplicate)
        ));
    }

    #[test]
    fn rejects_stale_message() {
        let mut dedup = MessageDeduplicator::new();
        let old = Utc::now().timestamp() - MAX_AGE_SECS - 1;
        let msg = make_msg(1, old);
        assert!(matches!(
            dedup.check(&msg),
            Err(DeduplicateError::TooOld { .. })
        ));
    }

    #[test]
    fn rejects_future_message() {
        let mut dedup = MessageDeduplicator::new();
        let future = Utc::now().timestamp() + MAX_FUTURE_SECS + 1;
        let msg = make_msg(1, future);
        assert!(matches!(
            dedup.check(&msg),
            Err(DeduplicateError::FromFuture { .. })
        ));
    }

    #[test]
    fn bounded_capacity() {
        let mut dedup = MessageDeduplicator::with_capacity(2);
        let now = Utc::now().timestamp();
        let m1 = make_msg(1, now);
        let m2 = make_msg(2, now);
        let m3 = make_msg(3, now);

        assert!(dedup.check(&m1).is_ok());
        assert!(dedup.check(&m2).is_ok());
        assert!(dedup.check(&m3).is_ok());
        // m1 was evicted, so it should be accepted again
        assert!(dedup.check(&m1).is_ok());
    }
}
