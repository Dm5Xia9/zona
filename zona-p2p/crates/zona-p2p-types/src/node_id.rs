use serde::{Deserialize, Serialize};
use std::fmt;

/// 256-bit node identifier. NodeId = SHA-256(pubkey) — §6.1, §12.2.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub [u8; 32]);

impl NodeId {
    pub const ZERO: NodeId = NodeId([0u8; 32]);

    /// XOR distance to another NodeId — basis of greedy routing (§7.1).
    pub fn xor_distance(&self, other: &NodeId) -> [u8; 32] {
        let mut d = [0u8; 32];
        for i in 0..32 {
            d[i] = self.0[i] ^ other.0[i];
        }
        d
    }

    /// Returns true if `self` is closer to `target` than `other` is.
    pub fn is_closer_to(&self, target: &NodeId, other: &NodeId) -> bool {
        self.xor_distance(target) < other.xor_distance(target)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_bytes(raw: [u8; 32]) -> Self {
        NodeId(raw)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.0[..8]))
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({}…)", hex::encode(&self.0[..4]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_reflexive() {
        let a = NodeId([1u8; 32]);
        assert_eq!(a.xor_distance(&a), [0u8; 32]);
    }

    #[test]
    fn xor_closer() {
        let target = NodeId([0u8; 32]);
        let close  = NodeId([1u8; 32]);
        let far    = NodeId([0xFF; 32]);
        assert!(close.is_closer_to(&target, &far));
    }
}
