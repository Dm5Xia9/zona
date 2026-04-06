//! Periodic repair targets and long-jump L-slot target — §4, §5.1, §10.7.
//!
//! Each node periodically computes pseudo-random target NodeIds from
//! `epoch_seed || self_id` and initiates FIND_NODE toward them to
//! prevent local-minimum lock-in after churn.
//!
//! The long-jump target is computed from a *different* prefix so it is
//! structurally independent from the repair targets; it drives the L-slot
//! of the peer table (§10.7).

use sha2::{Sha256, Digest};
use zona_p2p_types::NodeId;

/// Number of repair targets per epoch tick.
pub const REPAIR_TARGETS_PER_TICK: usize = 2;

/// Compute repair targets from an epoch seed and self NodeId — §5.1, §4.
pub fn repair_targets(epoch_seed: u64, self_id: &NodeId, count: usize) -> Vec<NodeId> {
    (0..count as u64)
        .map(|i| hash_target(b"repair", epoch_seed, self_id, i))
        .collect()
}

/// Compute the long-jump (L-slot) target for this epoch — §10.7.
///
/// Uses prefix `"longjump"` to keep it structurally independent from repair
/// targets.  The resulting NodeId is typically far from `self_id` in XOR
/// space, driving the small-world edge (§7.1).
pub fn long_jump_target(epoch_seed: u64, self_id: &NodeId) -> NodeId {
    hash_target(b"longjump", epoch_seed, self_id, 0)
}

fn hash_target(prefix: &[u8], epoch_seed: u64, self_id: &NodeId, index: u64) -> NodeId {
    let mut h = Sha256::new();
    h.update(prefix);
    h.update(epoch_seed.to_le_bytes());
    h.update(self_id.as_bytes());
    h.update(index.to_le_bytes());
    let digest = h.finalize();
    let mut id = [0u8; 32];
    id.copy_from_slice(&digest);
    NodeId(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_targets_deterministic() {
        let self_id = NodeId([42u8; 32]);
        let seed = 12345u64;
        let a = repair_targets(seed, &self_id, 2);
        let b = repair_targets(seed, &self_id, 2);
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_produce_different_targets() {
        let self_id = NodeId([1u8; 32]);
        let a = repair_targets(1, &self_id, 2);
        let b = repair_targets(2, &self_id, 2);
        assert_ne!(a[0], b[0]);
    }
}
