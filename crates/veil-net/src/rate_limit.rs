//! Per-peer token-bucket rate limiter for P2P connections.
//!
//! Prevents a malicious peer from flooding the client with messages.
//! Mirrors the relay server's rate limiting pattern.

use std::collections::HashMap;
use std::time::Instant;

use crate::ConnectionId;

/// Configuration for per-peer rate limiting.
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Maximum messages per second per peer.
    pub max_per_second: f64,
    /// Burst capacity (max tokens that can accumulate).
    pub burst: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_per_second: 50.0,
            burst: 50.0,
        }
    }
}

struct TokenBucket {
    tokens: f64,
    last_replenish: Instant,
}

/// Per-peer rate limiter using token buckets.
pub struct PeerRateLimiter {
    config: RateLimitConfig,
    buckets: HashMap<ConnectionId, TokenBucket>,
}

impl PeerRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: HashMap::new(),
        }
    }

    /// Register a new peer connection with a full token bucket.
    pub fn add_peer(&mut self, conn_id: ConnectionId) {
        self.buckets.insert(
            conn_id,
            TokenBucket {
                tokens: self.config.burst,
                last_replenish: Instant::now(),
            },
        );
    }

    /// Remove tracking for a disconnected peer.
    pub fn remove_peer(&mut self, conn_id: ConnectionId) {
        self.buckets.remove(&conn_id);
    }

    /// Check if a message from this peer is allowed.
    /// Returns `true` if allowed, `false` if rate-limited (message should be dropped).
    pub fn check(&mut self, conn_id: ConnectionId) -> bool {
        let Some(bucket) = self.buckets.get_mut(&conn_id) else {
            // Unknown peer — allow (they'll be added on connect)
            return true;
        };

        // Replenish tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_replenish).as_secs_f64();
        bucket.tokens =
            (bucket.tokens + elapsed * self.config.max_per_second).min(self.config.burst);
        bucket.last_replenish = now;

        // Try to consume one token
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_limit() {
        let mut rl = PeerRateLimiter::new(RateLimitConfig {
            max_per_second: 10.0,
            burst: 10.0,
        });
        let conn = 1;
        rl.add_peer(conn);

        // Should allow up to burst capacity
        for _ in 0..10 {
            assert!(rl.check(conn));
        }
    }

    #[test]
    fn blocks_above_limit() {
        let mut rl = PeerRateLimiter::new(RateLimitConfig {
            max_per_second: 5.0,
            burst: 5.0,
        });
        let conn = 1;
        rl.add_peer(conn);

        // Exhaust tokens
        for _ in 0..5 {
            assert!(rl.check(conn));
        }
        // Next should be blocked
        assert!(!rl.check(conn));
    }

    #[test]
    fn replenishes_over_time() {
        let mut rl = PeerRateLimiter::new(RateLimitConfig {
            max_per_second: 100.0,
            burst: 5.0,
        });
        let conn = 1;
        rl.add_peer(conn);

        // Exhaust tokens
        for _ in 0..5 {
            assert!(rl.check(conn));
        }
        assert!(!rl.check(conn));

        // Wait for replenishment (100/sec = 1 token per 10ms)
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Should have replenished some tokens
        assert!(rl.check(conn));
    }

    #[test]
    fn unknown_peer_allowed() {
        let mut rl = PeerRateLimiter::new(RateLimitConfig::default());
        // Unknown connection ID — should pass through
        assert!(rl.check(999));
    }

    #[test]
    fn add_remove_peer() {
        let mut rl = PeerRateLimiter::new(RateLimitConfig::default());
        let conn = 1;
        rl.add_peer(conn);
        assert!(rl.check(conn));
        rl.remove_peer(conn);
        // After removal, treated as unknown — still allowed
        assert!(rl.check(conn));
    }
}
