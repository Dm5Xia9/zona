//! Rate limiting for overlay operations — §12.2.
//!
//! Limits (referencing §12.2 defaults):
//! - FIND_NODE: 60 / min per key (after auth handshake)
//! - ping:      120 / min per key
//! - invite:    8 per key per rolling 24 h

use std::collections::HashMap;
use std::time::{Duration, Instant};
use zona_p2p_types::{NodeId, ProtocolError};

#[derive(Clone, Copy, Debug)]
pub struct LimitConfig {
    pub max_per_window: u32,
    pub window:         Duration,
}

impl LimitConfig {
    pub const FIND_NODE: LimitConfig = LimitConfig {
        max_per_window: 60,
        window: Duration::from_secs(60),
    };
    pub const PING: LimitConfig = LimitConfig {
        max_per_window: 120,
        window: Duration::from_secs(60),
    };
    pub const INVITE: LimitConfig = LimitConfig {
        max_per_window: 8,
        window: Duration::from_secs(86_400),
    };
    /// Unauthenticated / no-deposit profile (§12.2).
    pub const FIND_UNAUTH: LimitConfig = LimitConfig {
        max_per_window: 10,
        window: Duration::from_secs(60),
    };
}

struct Bucket {
    count:        u32,
    window_start: Instant,
    config:       LimitConfig,
}

impl Bucket {
    fn new(config: LimitConfig) -> Self {
        Bucket { count: 0, window_start: Instant::now(), config }
    }

    /// Returns true if the operation is allowed (and records it).
    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= self.config.window {
            self.count = 0;
            self.window_start = now;
        }
        if self.count < self.config.max_per_window {
            self.count += 1;
            true
        } else {
            false
        }
    }
}

/// Rate limiter tracking per-peer, per-operation buckets.
pub struct RateLimiter {
    find:   HashMap<NodeId, Bucket>,
    ping:   HashMap<NodeId, Bucket>,
    invite: HashMap<NodeId, Bucket>,
}

impl RateLimiter {
    pub fn new() -> Self {
        RateLimiter {
            find:   HashMap::new(),
            ping:   HashMap::new(),
            invite: HashMap::new(),
        }
    }

    pub fn check_find(&mut self, peer: &NodeId, authenticated: bool) -> Result<(), ProtocolError> {
        let cfg = if authenticated { LimitConfig::FIND_NODE } else { LimitConfig::FIND_UNAUTH };
        let bucket = self.find.entry(*peer).or_insert_with(|| Bucket::new(cfg));
        if bucket.try_consume() {
            Ok(())
        } else {
            Err(ProtocolError::RateLimitExceeded { op: "find_node" })
        }
    }

    pub fn check_ping(&mut self, peer: &NodeId) -> Result<(), ProtocolError> {
        let bucket = self.ping.entry(*peer).or_insert_with(|| Bucket::new(LimitConfig::PING));
        if bucket.try_consume() {
            Ok(())
        } else {
            Err(ProtocolError::RateLimitExceeded { op: "ping" })
        }
    }

    pub fn check_invite(&mut self, issuer: &NodeId) -> Result<(), ProtocolError> {
        let bucket = self.invite.entry(*issuer).or_insert_with(|| Bucket::new(LimitConfig::INVITE));
        if bucket.try_consume() {
            Ok(())
        } else {
            Err(ProtocolError::InviteQuotaExceeded)
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(b: u8) -> NodeId { NodeId([b; 32]) }

    #[test]
    fn find_rate_limit_enforced() {
        let mut rl = RateLimiter::new();
        let peer = node(1);
        for _ in 0..10 {
            assert!(rl.check_find(&peer, false).is_ok());
        }
        // 11th unauthenticated call should fail.
        assert!(rl.check_find(&peer, false).is_err());
    }

    #[test]
    fn invite_quota() {
        let mut rl = RateLimiter::new();
        let issuer = node(2);
        for _ in 0..8 {
            assert!(rl.check_invite(&issuer).is_ok());
        }
        assert!(rl.check_invite(&issuer).is_err());
    }
}
