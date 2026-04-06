//! Peer Exchange (PEX) manager — §10.7.
//!
//! Rules:
//! - At most q=3 candidates per round (§12.2).
//! - Each candidate carries `referrer_id`.
//! - Quota: no more than 2 slots from the same referrer.
//! - At most 1 G/D slot replacement per stabilisation tick (§10.7).

use zona_p2p_types::{NodeId, Descriptor, PexCandidate, SlotEntry, SlotKind, PeerTable};

/// Maximum candidates returned in one PEX round.
pub const PEX_Q: usize = 3;

pub struct PexRequest {
    pub requester: NodeId,
}

pub struct PexResponse {
    pub candidates: Vec<PexCandidate>,
}

/// Manages PEX interactions for a node.
pub struct PexManager {
    pub self_id: NodeId,
}

impl PexManager {
    pub fn new(self_id: NodeId) -> Self {
        PexManager { self_id }
    }

    /// Build a PEX response from the current peer table — up to `PEX_Q` entries.
    pub fn build_response(&self, table: &PeerTable, descriptors: &[Descriptor]) -> PexResponse {
        let candidates: Vec<PexCandidate> = table
            .slots
            .iter()
            .filter(|s| !s.should_evict())
            .take(PEX_Q)
            .filter_map(|slot| {
                descriptors.iter().find(|d| d.node_id == slot.peer_id).map(|desc| PexCandidate {
                    descriptor:  desc.clone(),
                    referrer_id: self.self_id,
                })
            })
            .collect();

        PexResponse { candidates }
    }

    /// Process incoming PEX candidates and insert eligible ones into the table.
    ///
    /// Returns the number of entries actually inserted.
    pub fn apply_candidates(
        &self,
        candidates: Vec<PexCandidate>,
        table:      &mut PeerTable,
    ) -> usize {
        let mut inserted = 0;
        for c in candidates.into_iter().take(PEX_Q) {
            // Skip self.
            if c.descriptor.node_id == self.self_id {
                continue;
            }
            // Basic integrity: referrer_id should not be self for non-bootstrap.
            let entry = self.classify_candidate(&c, table);
            if table.try_insert(entry) {
                inserted += 1;
                tracing::debug!(
                    peer = %c.descriptor.node_id,
                    referrer = %c.referrer_id,
                    "PEX: inserted peer"
                );
            }
        }
        inserted
    }

    /// Classify the slot kind for a PEX candidate.
    fn classify_candidate(&self, c: &PexCandidate, table: &PeerTable) -> SlotEntry {
        let greedy_referrers = table.greedy_referrers();
        let kind = if !greedy_referrers.contains(&c.referrer_id) {
            // Different referrer → candidate for D-slot (diversity).
            SlotKind::Diversity
        } else {
            SlotKind::Greedy
        };
        SlotEntry::new(kind, c.descriptor.node_id, c.referrer_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zona_p2p_types::{PeerTable, SlotEntry, SlotKind, reach_spec::ReachList, Descriptor};

    fn id(b: u8) -> NodeId { NodeId([b; 32]) }

    fn make_desc(node_id: NodeId) -> Descriptor {
        Descriptor {
            node_id,
            pubkey:      [0u8; 32],
            reach:       ReachList(vec![]),
            version:     1,
            provisional: false,
            signature:   vec![0u8; 64],
        }
    }

    fn candidate(peer: u8, referrer: u8) -> PexCandidate {
        PexCandidate { descriptor: make_desc(id(peer)), referrer_id: id(referrer) }
    }

    #[test]
    fn pex_respects_q_limit() {
        let mgr = PexManager::new(id(0));
        let mut table = PeerTable::new(id(0), 3);
        table.try_insert(SlotEntry::new(SlotKind::Greedy, id(1), id(0xAA)));
        table.try_insert(SlotEntry::new(SlotKind::Greedy, id(2), id(0xBB)));
        table.try_insert(SlotEntry::new(SlotKind::Greedy, id(3), id(0xCC)));
        table.try_insert(SlotEntry::new(SlotKind::Greedy, id(4), id(0xDD)));

        let descs: Vec<Descriptor> = [1u8, 2, 3, 4].iter().map(|&b| make_desc(id(b))).collect();
        let resp = mgr.build_response(&table, &descs);
        assert!(resp.candidates.len() <= PEX_Q);
    }

    #[test]
    fn diversity_slot_for_new_referrer() {
        let mgr = PexManager::new(id(0));
        let mut table = PeerTable::new(id(0), 3);
        table.try_insert(SlotEntry::new(SlotKind::Greedy, id(1), id(0xAA)));

        // Candidate from new referrer → should go to D-slot.
        let n = mgr.apply_candidates(vec![candidate(2, 0xBB)], &mut table);
        assert_eq!(n, 1);
        assert!(table.diversity_slot().is_some());
    }
}
