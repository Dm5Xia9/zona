//! Recovery mode tracking — §5.1.
//!
//! A node in Recovery must NOT act as an authoritative certificate source
//! and must not be counted as an independent channel by light clients.

use zona_p2p_types::PeerTable;

/// The recovery state of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryState {
    /// Normal operation — ≥ 2 live greedy slots with distinct referrer_ids.
    Healthy,
    /// Recovery mode — fewer live greedy slots or all from one referrer.
    Recovery,
}

/// Evaluate the current recovery state from the peer table — §5.1.
pub fn evaluate_recovery(table: &PeerTable) -> RecoveryState {
    if table.in_recovery() {
        return RecoveryState::Recovery;
    }
    // Also check that greedy slots have at least 2 distinct referrer_ids.
    let mut referrers: Vec<_> = table
        .greedy_slots()
        .map(|s| s.referrer_id)
        .collect();
    referrers.dedup();
    if referrers.len() < 2 {
        RecoveryState::Recovery
    } else {
        RecoveryState::Healthy
    }
}
