//! Critical-message routing: route_fork and 2-of-3 quorum tracker — §7.3, §12.4.
//!
//! For `CriticalUser` messages the sender forks over two independent first-hops
//! (slots with different `referrer_id`).  The destination collects copies and
//! applies the side-effect only after ≥ 2 copies with the same payload arrive.
//!
//! Design:
//!  - `CriticalSender`  — picks two fork hops and produces two envelopes.
//!  - `QuorumTracker`   — on the destination, tracks arriving copies per
//!                        `message_id` and declares quorum when ≥ 2 match.

use std::collections::HashMap;
use zona_p2p_types::{NodeId, Envelope, PeerTable};

// ── sender side ──────────────────────────────────────────────────────────────

/// Choose two independent first-hops for a CriticalUser route_fork.
///
/// Returns `(hop_a, hop_b)`.  Hops are selected from slots with **different**
/// `referrer_id` so an eclipse on one chain does not compromise the other
/// (§7.3, §12.4).  Falls back to any two distinct peers if diversity is
/// insufficient.
pub fn pick_fork_hops(table: &PeerTable) -> Option<(NodeId, NodeId)> {
    // Prefer D-slot + one G-slot with different referrer.
    let d_slot = table.diversity_slot().map(|s| (s.peer_id, s.referrer_id));
    let g_slots: Vec<_> = table
        .greedy_slots()
        .filter(|s| !s.should_evict())
        .collect();
    let l_slot = table.long_jump_slot().map(|s| (s.peer_id, s.referrer_id));

    if let Some((d_id, d_ref)) = d_slot {
        // Look for a G-slot whose referrer differs from D-slot referrer.
        if let Some(g) = g_slots.iter().find(|s| s.referrer_id != d_ref) {
            return Some((g.peer_id, d_id));
        }
    }

    // Fallback: G-slot + L-slot.
    if let Some(g) = g_slots.first() {
        if let Some((l_id, _)) = l_slot {
            if l_id != g.peer_id {
                return Some((g.peer_id, l_id));
            }
        }
    }

    // Last resort: any two distinct peers.
    let all: Vec<_> = table.slots.iter().filter(|s| !s.should_evict()).collect();
    if all.len() >= 2 {
        return Some((all[0].peer_id, all[1].peer_id));
    }
    None
}

/// Build two forked envelopes from a template.
///
/// Both copies share the same `message_id` (for dedup at the receiver) but
/// are sent to different first-hops.  The `route_fork` flag is set to `true`.
pub fn fork_envelopes(
    mut template: Envelope,
    hop_a: NodeId,
    hop_b: NodeId,
) -> (Envelope, Envelope) {
    template.route_fork = true;
    let mut a = template.clone();
    let mut b = template;
    a.to = hop_a;
    b.to = hop_b;
    (a, b)
}

// ── receiver side ─────────────────────────────────────────────────────────────

/// Result of recording an incoming critical-message copy.
#[derive(Debug, PartialEq, Eq)]
pub enum QuorumResult {
    /// Not yet enough copies — keep waiting.
    Pending,
    /// Quorum reached (≥ 2 matching copies) — apply the side-effect.
    Accepted,
    /// Two or more copies arrived but with mismatching payloads — reject.
    Inconsistent,
}

/// Per-message quorum state on the destination node.
struct PendingEntry {
    /// Payload of the first copy seen.
    payload:    Vec<u8>,
    /// Number of matching copies received.
    matches:    u8,
    /// Whether a mismatch has already been detected.
    mismatch:   bool,
}

/// Tracks incoming copies of CriticalUser messages and signals quorum.
///
/// The receiver calls `record()` for every arriving copy; the method returns
/// the quorum result so the application layer can decide whether to act.
pub struct QuorumTracker {
    pending: HashMap<u64, PendingEntry>,
}

impl QuorumTracker {
    pub fn new() -> Self {
        QuorumTracker { pending: HashMap::new() }
    }

    /// Record an incoming copy of a critical message.
    ///
    /// `message_id` — deduplicated identifier.
    /// `payload` — serialised body used for consistency check.
    pub fn record(&mut self, message_id: u64, payload: Vec<u8>) -> QuorumResult {
        let entry = self.pending.entry(message_id).or_insert(PendingEntry {
            payload:  payload.clone(),
            matches:  0,
            mismatch: false,
        });

        if entry.payload != payload {
            entry.mismatch = true;
        }

        if entry.mismatch {
            return QuorumResult::Inconsistent;
        }

        entry.matches += 1;

        // §12.4: accept on ≥ 2 matching independent copies.
        if entry.matches >= 2 {
            self.pending.remove(&message_id);
            QuorumResult::Accepted
        } else {
            QuorumResult::Pending
        }
    }

    /// Evict stale entries (e.g. after a TTL). Call periodically.
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

impl Default for QuorumTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quorum_reached_on_second_matching_copy() {
        let mut qt = QuorumTracker::new();
        let payload = b"hello".to_vec();
        assert_eq!(qt.record(1, payload.clone()), QuorumResult::Pending);
        assert_eq!(qt.record(1, payload.clone()), QuorumResult::Accepted);
    }

    #[test]
    fn inconsistent_on_payload_mismatch() {
        let mut qt = QuorumTracker::new();
        qt.record(2, b"aaa".to_vec());
        assert_eq!(qt.record(2, b"bbb".to_vec()), QuorumResult::Inconsistent);
    }

    #[test]
    fn pick_fork_returns_two_distinct_peers() {
        use zona_p2p_types::{PeerTable, SlotEntry, SlotKind};
        fn id(b: u8) -> NodeId { NodeId([b; 32]) }
        let mut table = PeerTable::new(id(0), 3);
        table.try_insert(SlotEntry::new(SlotKind::Greedy,    id(1), id(0xAA)));
        table.try_insert(SlotEntry::new(SlotKind::Diversity, id(2), id(0xBB)));
        let (a, b) = pick_fork_hops(&table).unwrap();
        assert_ne!(a, b);
    }
}
