//! SandboxBackend — fully in-process P2P network using `Sim`.

use zona_p2p_crypto::NodeKeypair;
use zona_p2p_types::{NodeId, SlotKind};

use crate::Sim;
use zona_p2p_client::backend::*;

pub struct SandboxBackend {
    pub sim:   Sim,
    /// Stable ordered list; `None` = killed node (index kept stable).
    pub nodes: Vec<Option<NodeId>>,
    /// Messages received by this client.
    inbox:     Vec<InboxEntry>,
}

impl SandboxBackend {
    /// Create and bootstrap a network of `n` nodes.
    /// The REPL client is conceptually connected to all nodes simultaneously.
    pub fn bootstrap(n: usize) -> Self {
        use rand::SeedableRng;
        use rand::seq::SliceRandom;

        let mut sim = Sim::new(42, 0.0);
        let mut node_ids = Vec::new();
        for _ in 0..n {
            node_ids.push(sim.add_node(NodeKeypair::generate()));
        }
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        for i in 0..n {
            let mut cands: Vec<usize> = (0..n).filter(|&j| j != i).collect();
            cands.shuffle(&mut rng);
            for &j in cands.iter().take(3) {
                sim.introduce(node_ids[i], node_ids[j]);
            }
        }
        for _ in 0..5 {
            sim.run_steps(6);
            sim.repair_all();
        }
        sim.run_steps(10);

        let nodes = node_ids.into_iter().map(Some).collect();
        SandboxBackend { sim, nodes, inbox: Vec::new() }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    pub fn resolve_node(&self, s: &str) -> Option<NodeId> {
        if let Ok(i) = s.parse::<usize>() {
            return self.nodes.get(i).and_then(|s| *s);
        }
        // Full 64-char hex or prefix
        let lower = s.to_lowercase();
        self.nodes.iter().flatten().copied()
            .find(|&nid| hex::encode(nid.as_bytes()).starts_with(&lower))
    }

    fn node_idx_str(&self, nid: NodeId) -> String {
        self.nodes.iter().position(|s| s == &Some(nid))
            .map(|i| i.to_string())
            .unwrap_or("?".into())
    }

    fn short_id(nid: NodeId) -> String {
        hex::encode(&nid.as_bytes()[..4])
    }

    /// First alive node (used as "our" node for routing in rpc).
    fn any_node(&self) -> Option<NodeId> {
        self.nodes.iter().flatten().copied().next()
    }
}

// ── AdminBackend ──────────────────────────────────────────────────────────────

impl AdminBackend for SandboxBackend {
    fn mode_name(&self) -> &'static str { "sandbox" }

    fn node_list(&self) -> Vec<NodeInfo> {
        self.nodes.iter().enumerate().map(|(i, slot)| {
            match slot {
                None => NodeInfo { id: "dead".into(), index: i, alive: false, healthy: false, slots: 0 },
                Some(nid) => {
                    let n = self.sim.node(nid);
                    NodeInfo {
                        id:      Self::short_id(*nid),
                        index:   i,
                        alive:   true,
                        healthy: n.map(|nd| !nd.table.in_recovery()).unwrap_or(false),
                        slots:   n.map(|nd| nd.table.slots.len()).unwrap_or(0),
                    }
                }
            }
        }).collect()
    }

    fn stats(&self) -> NetworkStats {
        let alive   = self.nodes.iter().filter(|s| s.is_some()).count();
        let healthy = self.sim.count_healthy();
        let avg = if alive == 0 { 0.0 } else {
            self.nodes.iter().flatten()
                .filter_map(|n| self.sim.node(n))
                .map(|n| n.table.slots.len() as f64)
                .sum::<f64>() / alive as f64
        };
        NetworkStats {
            node_count: alive,
            healthy,
            avg_slots:  avg,
            drop_prob:  self.sim.drop_prob,
            ticks:      self.sim.ticks,
            mode:       "sandbox".into(),
        }
    }

    fn peer_table(&self, node_ref: &str) -> Option<Vec<PeerSlot>> {
        let nid  = self.resolve_node(node_ref)?;
        let node = self.sim.node(&nid)?;
        Some(node.table.slots.iter().map(|s| PeerSlot {
            kind:     match s.kind {
                SlotKind::Greedy    => "G",
                SlotKind::LongJump  => "L",
                SlotKind::Diversity => "D",
            }.into(),
            peer_id:  Self::short_id(s.peer_id),
            peer_idx: self.node_idx_str(s.peer_id),
            loss:     s.ewma_loss,
            referrer: Self::short_id(s.referrer_id),
        }).collect())
    }

    fn step(&mut self, n: usize) { self.sim.run_steps(n); }
    fn drop_probability(&self) -> f64 { self.sim.drop_prob }
    fn set_drop_probability(&mut self, p: f64) { self.sim.set_drop_prob(p); }

    fn inbox(&self) -> Vec<InboxEntry> { self.inbox.clone() }

    /// Trace route from node[0] to `to` and return a simulated response.
    /// Useful for testing P2P routing without a real server.
    fn rpc(&mut self, to: &str, payload: &[u8], _timeout_ms: u64) -> Result<Vec<u8>, String> {
        let src = self.any_node().ok_or("no nodes")?;
        let dst = self.resolve_node(to)
            .ok_or_else(|| format!("node '{to}' not found"))?;
        let path = self.sim.trace_route(src, dst);
        if path.last() != Some(&dst) {
            let hops: Vec<String> = path.iter()
                .map(|&n| format!("[{}]", self.node_idx_str(n)))
                .collect();
            return Err(format!("no route to '{to}' (stopped at: {})", hops.join(" → ")));
        }
        let text = String::from_utf8_lossy(payload);
        let hops = path.len().saturating_sub(1);
        Ok(format!("[sandbox] routed in {hops} hop(s) — echo: {text}").into_bytes())
    }
}
