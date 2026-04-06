use serde::{Deserialize, Serialize};
use crate::NodeId;

/// Classification of a routing slot — §5, §10.7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotKind {
    /// Greedy: closest by XOR/prefix metric.
    Greedy,
    /// Long-jump (small-world) edge — one per table, §7.1.
    LongJump,
    /// Diversity: must have a different `referrer_id` than all G-slots.
    Diversity,
}

/// A single slot entry in the routing table.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotEntry {
    pub kind:        SlotKind,
    pub peer_id:     NodeId,
    /// Who recommended this peer — used for eclipse-resistance (§10.7).
    pub referrer_id: NodeId,
    /// EWMA loss score: 0.0 = perfect, 1.0 = all probes lost (§12.3).
    pub ewma_loss:   f32,
    /// Monotone score for slot competition: higher = better.
    pub score:       f32,
}

impl SlotEntry {
    pub fn new(kind: SlotKind, peer_id: NodeId, referrer_id: NodeId) -> Self {
        SlotEntry {
            kind,
            peer_id,
            referrer_id,
            ewma_loss: 0.0,
            score: 0.5,
        }
    }

    /// Update EWMA loss — §12.3, α = 0.2.
    pub fn record_probe(&mut self, success: bool) {
        const ALPHA: f32 = 0.2;
        let fail = if success { 0.0 } else { 1.0 };
        self.ewma_loss = ALPHA * fail + (1.0 - ALPHA) * self.ewma_loss;
    }

    /// Bump or penalise score based on checkpoint agreement (§12.3).
    pub fn adjust_score(&mut self, agreed: bool) {
        if agreed {
            self.score = (self.score + 0.05).min(1.0);
        } else {
            self.score = (self.score - 0.10).max(0.0);
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.ewma_loss > 0.6
    }

    pub fn should_evict(&self) -> bool {
        self.ewma_loss > 0.85
    }
}

/// Routing table with G/L/D slots (§10.7).
///
/// Maximum 5 permanent slots: up to `max_greedy` G-slots, 1 L-slot, 1 D-slot.
#[derive(Clone, Debug)]
pub struct PeerTable {
    pub slots:      Vec<SlotEntry>,
    pub max_greedy: usize,
    /// Self node id — for XOR comparisons.
    pub self_id:    NodeId,
    /// Number of replacement attempts still allowed this stabilisation tick.
    replacements_this_tick: usize,
}

impl PeerTable {
    pub const MAX_TOTAL: usize = 5;

    pub fn new(self_id: NodeId, max_greedy: usize) -> Self {
        PeerTable {
            slots: Vec::with_capacity(Self::MAX_TOTAL),
            max_greedy,
            self_id,
            replacements_this_tick: 0,
        }
    }

    pub fn reset_tick(&mut self) {
        self.replacements_this_tick = 0;
    }

    pub fn greedy_slots(&self) -> impl Iterator<Item = &SlotEntry> {
        self.slots.iter().filter(|s| s.kind == SlotKind::Greedy)
    }

    pub fn long_jump_slot(&self) -> Option<&SlotEntry> {
        self.slots.iter().find(|s| s.kind == SlotKind::LongJump)
    }

    pub fn diversity_slot(&self) -> Option<&SlotEntry> {
        self.slots.iter().find(|s| s.kind == SlotKind::Diversity)
    }

    /// All slots as peers for routing (all kinds).
    pub fn all_peers(&self) -> Vec<NodeId> {
        self.slots.iter().map(|s| s.peer_id).collect()
    }

    /// All referrer_ids currently present in G-slots.
    pub fn greedy_referrers(&self) -> Vec<NodeId> {
        self.slots
            .iter()
            .filter(|s| s.kind == SlotKind::Greedy)
            .map(|s| s.referrer_id)
            .collect()
    }

    /// True if the referrer is already represented in 2+ greedy slots (§10.7 quota).
    pub fn referrer_quota_exceeded(&self, referrer: &NodeId) -> bool {
        self.slots
            .iter()
            .filter(|s| s.kind == SlotKind::Greedy && &s.referrer_id == referrer)
            .count()
            >= 2
    }

    /// Try to add or replace a slot entry.
    ///
    /// Rules (§10.7):
    /// - At most 1 replacement per tick (greedy or diversity).
    /// - At most 2 greedy slots from the same referrer.
    /// - D-slot must have a different referrer than all G-slots.
    pub fn try_insert(&mut self, entry: SlotEntry) -> bool {
        // Dedup
        if self.slots.iter().any(|s| s.peer_id == entry.peer_id) {
            return false;
        }

        match entry.kind {
            SlotKind::Greedy => self.try_insert_greedy(entry),
            SlotKind::LongJump => self.try_insert_longjump(entry),
            SlotKind::Diversity => self.try_insert_diversity(entry),
        }
    }

    fn try_insert_greedy(&mut self, entry: SlotEntry) -> bool {
        if self.referrer_quota_exceeded(&entry.referrer_id) {
            return false;
        }
        let g_count = self.slots.iter().filter(|s| s.kind == SlotKind::Greedy).count();
        if g_count < self.max_greedy {
            self.slots.push(entry);
            return true;
        }
        // Replace worst greedy slot if new entry is XOR-closer, one per tick.
        if self.replacements_this_tick >= 1 {
            return false;
        }
        let worst_idx = self.slots.iter().enumerate()
            .filter(|(_, s)| s.kind == SlotKind::Greedy)
            .max_by(|(_, a), (_, b)| {
                let da = a.peer_id.xor_distance(&self.self_id);
                let db = b.peer_id.xor_distance(&self.self_id);
                da.cmp(&db)
            })
            .map(|(i, _)| i);

        if let Some(idx) = worst_idx {
            let worst_dist = self.slots[idx].peer_id.xor_distance(&self.self_id);
            if entry.peer_id.xor_distance(&self.self_id) < worst_dist {
                self.slots[idx] = entry;
                self.replacements_this_tick += 1;
                return true;
            }
        }
        false
    }

    fn try_insert_longjump(&mut self, entry: SlotEntry) -> bool {
        if let Some(idx) = self.slots.iter().position(|s| s.kind == SlotKind::LongJump) {
            self.slots[idx] = entry;
        } else if self.slots.len() < Self::MAX_TOTAL {
            self.slots.push(entry);
        }
        true
    }

    fn try_insert_diversity(&mut self, entry: SlotEntry) -> bool {
        // D-slot referrer must differ from all G-slot referrers.
        let g_referrers = self.greedy_referrers();
        if g_referrers.contains(&entry.referrer_id) {
            return false;
        }
        if self.replacements_this_tick >= 1 {
            return false;
        }
        if let Some(idx) = self.slots.iter().position(|s| s.kind == SlotKind::Diversity) {
            self.slots[idx] = entry;
        } else if self.slots.len() < Self::MAX_TOTAL {
            self.slots.push(entry);
        } else {
            return false;
        }
        self.replacements_this_tick += 1;
        true
    }

    /// Remove slots that should be evicted, preserving invariants.
    pub fn evict_dead(&mut self) {
        self.slots.retain(|s| !s.should_evict());
    }

    /// Is the table in Recovery mode (fewer than 2 live G-slots)? §5.1.
    pub fn in_recovery(&self) -> bool {
        self.slots.iter().filter(|s| s.kind == SlotKind::Greedy && !s.should_evict()).count() < 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(b: u8) -> NodeId { NodeId([b; 32]) }

    #[test]
    fn greedy_referrer_quota() {
        let mut t = PeerTable::new(id(0), 3);
        let r = id(0xAA);
        t.try_insert(SlotEntry::new(SlotKind::Greedy, id(1), r));
        t.try_insert(SlotEntry::new(SlotKind::Greedy, id(2), r));
        // Third from same referrer should be rejected.
        assert!(!t.try_insert(SlotEntry::new(SlotKind::Greedy, id(3), r)));
    }

    #[test]
    fn diversity_slot_requires_different_referrer() {
        let mut t = PeerTable::new(id(0), 3);
        let r = id(0xBB);
        t.try_insert(SlotEntry::new(SlotKind::Greedy, id(1), r));
        // D-slot with same referrer should fail.
        assert!(!t.try_insert(SlotEntry::new(SlotKind::Diversity, id(2), r)));
        // D-slot with different referrer should succeed.
        assert!(t.try_insert(SlotEntry::new(SlotKind::Diversity, id(2), id(0xCC))));
    }

    #[test]
    fn in_recovery_with_one_greedy() {
        let mut t = PeerTable::new(id(0), 3);
        t.try_insert(SlotEntry::new(SlotKind::Greedy, id(1), id(0xAA)));
        assert!(t.in_recovery());
    }
}
