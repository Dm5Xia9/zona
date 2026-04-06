//! Greedy XOR routing — §7.1.
//!
//! On each hop, pick the neighbour with minimum XOR-distance to the target.
//! If no neighbour is strictly closer than self, we have a local minimum.

use zona_p2p_types::{NodeId, PeerTable};

#[derive(Debug, PartialEq, Eq)]
pub enum NextHopResult {
    /// Forward to this peer.
    Forward(NodeId),
    /// We *are* the destination (or the only node).
    Deliver,
    /// Local minimum — no neighbour is closer to target than self.
    /// Caller should trigger repair or use the secondary hop.
    LocalMinimum,
}

/// Choose the next hop toward `target` from `self_id` using the peer `table`.
pub fn next_hop(self_id: &NodeId, target: &NodeId, table: &PeerTable) -> NextHopResult {
    if self_id == target {
        return NextHopResult::Deliver;
    }

    let self_dist = self_id.xor_distance(target);

    let best = table
        .slots
        .iter()
        .filter(|s| !s.should_evict())
        .min_by_key(|s| s.peer_id.xor_distance(target));

    match best {
        None => NextHopResult::LocalMinimum,
        Some(entry) => {
            if entry.peer_id.xor_distance(target) < self_dist {
                NextHopResult::Forward(entry.peer_id)
            } else {
                NextHopResult::LocalMinimum
            }
        }
    }
}

/// Pick up to `n` peers closest to `target` (for FindNode responses).
pub fn closest_peers(target: &NodeId, table: &PeerTable, n: usize) -> Vec<NodeId> {
    let mut peers: Vec<NodeId> = table
        .slots
        .iter()
        .filter(|s| !s.should_evict())
        .map(|s| s.peer_id)
        .collect();

    peers.sort_by_key(|p| p.xor_distance(target));
    peers.truncate(n);
    peers
}

#[cfg(test)]
mod tests {
    use super::*;
    use zona_p2p_types::{PeerTable, SlotEntry, SlotKind};

    fn id(b: u8) -> NodeId { NodeId([b; 32]) }
    fn entry(peer: u8, referrer: u8) -> SlotEntry {
        SlotEntry::new(SlotKind::Greedy, id(peer), id(referrer))
    }

    #[test]
    fn forward_to_closer_peer() {
        let self_id = id(0x10);
        let target  = id(0x01);
        let mut table = PeerTable::new(self_id, 3);
        // id(0x02) is XOR-closer to id(0x01) than id(0x10)
        table.try_insert(entry(0x02, 0xAA));
        table.try_insert(entry(0x20, 0xBB));
        assert_eq!(next_hop(&self_id, &target, &table), NextHopResult::Forward(id(0x02)));
    }

    #[test]
    fn deliver_to_self() {
        let self_id = id(0x55);
        let table   = PeerTable::new(self_id, 3);
        assert_eq!(next_hop(&self_id, &self_id, &table), NextHopResult::Deliver);
    }

    #[test]
    fn local_minimum_with_no_peers() {
        let self_id = id(0x10);
        let target  = id(0x01);
        let table   = PeerTable::new(self_id, 3);
        assert_eq!(next_hop(&self_id, &target, &table), NextHopResult::LocalMinimum);
    }
}
