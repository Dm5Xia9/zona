//! Backend abstraction for the zona-p2p interactive REPL.
//!
//! One trait — `AdminBackend` — is implemented by `SandboxBackend` (in-process
//! simulation) and `DockerBackend` (real Docker containers).
//! Each REPL instance represents a single client.

use serde::{Deserialize, Serialize};

// ── Shared data types ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Short hex prefix (8 chars).
    pub id:      String,
    pub index:   usize,
    pub alive:   bool,
    pub healthy: bool,
    pub slots:   usize,
}

#[derive(Clone, Debug)]
pub struct NetworkStats {
    pub node_count:  usize,
    pub healthy:     usize,
    pub avg_slots:   f64,
    pub drop_prob:   f64,
    pub ticks:       u64,
    pub mode:        String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerSlot {
    /// "G", "L" or "D".
    pub kind:     String,
    pub peer_id:  String,
    pub peer_idx: String,
    pub loss:     f32,
    pub referrer: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboxEntry {
    pub from: String,
    pub text: String,
}

// ── AdminBackend ──────────────────────────────────────────────────────────────

/// All operations available in the interactive REPL.
/// Implemented by `SandboxBackend` (in-process) and `DockerBackend` (Docker).
pub trait AdminBackend: Send {
    fn mode_name(&self) -> &'static str;

    // ── Network topology ─────────────────────────────────────────────────────

    fn node_list(&self) -> Vec<NodeInfo>;
    fn stats(&self) -> NetworkStats;
    fn peer_table(&self, node_ref: &str) -> Option<Vec<PeerSlot>>;

    // ── Simulation controls (sandbox only; no-op in Docker) ──────────────────

    fn step(&mut self, n: usize);
    fn drop_probability(&self) -> f64;
    fn set_drop_probability(&mut self, p: f64);

    // ── Client operations ────────────────────────────────────────────────────

    /// Messages received by this client.
    fn inbox(&self) -> Vec<InboxEntry>;

    /// Send `payload` to `to` (node index / hex prefix / full 64-char hex)
    /// through the P2P relay network and wait for a handler response.
    fn rpc(&mut self, to: &str, payload: &[u8], timeout_ms: u64) -> Result<Vec<u8>, String>;
}
