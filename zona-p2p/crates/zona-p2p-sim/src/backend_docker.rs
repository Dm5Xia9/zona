//! DockerBackend — talks to real Docker containers via the node HTTP admin API.
//!
//! Each node container exposes an HTTP admin API on port 7701.
//! One REPL instance = one client; it connects to all known nodes automatically.

use serde::{Deserialize, Serialize};

use zona_p2p_client::backend::*;

// ── Wire types (must match zona-p2p-node/src/admin.rs) ───────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct ApiInfo {
    node_id: String,
    healthy: bool,
    slots:   usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiPeerSlot {
    kind:     String,
    peer_id:  String,
    peer_idx: String,
    loss:     f32,
    referrer: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiInboxEntry {
    from: String,
    text: String,
}

// ── DockerNode bookkeeping ────────────────────────────────────────────────────

#[derive(Clone)]
struct DockerNode {
    index:     usize,
    node_id:   String,
    admin_url: String,
    alive:     bool,
    healthy:   bool,
    slots:     usize,
}

pub struct DockerBackend {
    nodes:  Vec<DockerNode>,
    client: reqwest::blocking::Client,
}

impl DockerBackend {
    /// Connect to a list of admin URLs and query their current state.
    pub fn connect(admin_urls: Vec<String>) -> anyhow::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        let mut nodes = Vec::new();
        for (index, url) in admin_urls.iter().enumerate() {
            let info: ApiInfo = client
                .get(format!("{url}/api/info"))
                .send()?
                .json()?;
            nodes.push(DockerNode {
                index,
                node_id:   info.node_id,
                admin_url: url.clone(),
                alive:     true,
                healthy:   info.healthy,
                slots:     info.slots,
            });
        }

        Ok(DockerBackend { nodes, client })
    }

    fn refresh(&mut self) {
        for n in &mut self.nodes {
            if !n.alive { continue; }
            if let Ok(resp) = self.client.get(format!("{}/api/info", n.admin_url)).send() {
                if let Ok(info) = resp.json::<ApiInfo>() {
                    n.healthy = info.healthy;
                    n.slots   = info.slots;
                }
            }
        }
    }

    fn short_id(id: &str) -> String { id[..8.min(id.len())].to_string() }

    fn resolve_node_index(&self, node_ref: &str) -> Option<usize> {
        if let Ok(i) = node_ref.parse::<usize>() {
            if i < self.nodes.len() { return Some(i); }
        }
        self.nodes.iter().position(|n| n.node_id.starts_with(node_ref))
    }

    /// Resolve `to` to a full hex NodeId.
    /// - node index ("0", "2", …)
    /// - full 64-char hex: passed as-is (node doesn't have to be in admin list)
    /// - hex prefix: first match in known nodes
    fn resolve_node_id(&self, to: &str) -> Result<String, String> {
        if let Ok(idx) = to.parse::<usize>() {
            return self.nodes.get(idx)
                .ok_or_else(|| format!("node index {idx} out of range (have {} nodes)", self.nodes.len()))
                .map(|n| n.node_id.clone());
        }
        if to.len() == 64 && to.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(to.to_lowercase());
        }
        let lower = to.to_lowercase();
        self.nodes.iter()
            .find(|n| n.node_id.starts_with(&lower))
            .map(|n| n.node_id.clone())
            .ok_or_else(|| format!("no known node matching prefix '{to}'"))
    }

    /// Pick the first alive node as the relay entry point.
    fn gateway(&self) -> Result<&DockerNode, String> {
        self.nodes.iter().find(|n| n.alive)
            .ok_or_else(|| "no alive nodes".into())
    }
}

// ── AdminBackend ──────────────────────────────────────────────────────────────

impl AdminBackend for DockerBackend {
    fn mode_name(&self) -> &'static str { "docker" }

    fn node_list(&self) -> Vec<NodeInfo> {
        self.nodes.iter().map(|n| NodeInfo {
            id:      Self::short_id(&n.node_id),
            index:   n.index,
            alive:   n.alive,
            healthy: n.healthy,
            slots:   n.slots,
        }).collect()
    }

    fn stats(&self) -> NetworkStats {
        let alive   = self.nodes.iter().filter(|n| n.alive).count();
        let healthy = self.nodes.iter().filter(|n| n.alive && n.healthy).count();
        let avg = if alive == 0 { 0.0 } else {
            self.nodes.iter().filter(|n| n.alive)
                .map(|n| n.slots as f64).sum::<f64>() / alive as f64
        };
        NetworkStats {
            node_count: alive,
            healthy,
            avg_slots:  avg,
            drop_prob:  0.0,
            ticks:      0,
            mode:       "docker".into(),
        }
    }

    fn peer_table(&self, node_ref: &str) -> Option<Vec<PeerSlot>> {
        let n = self.nodes.iter().find(|n| n.alive && (
            n.index.to_string() == node_ref ||
            n.node_id.starts_with(node_ref)
        ))?;
        let slots: Vec<ApiPeerSlot> = self.client
            .get(format!("{}/api/peers", n.admin_url))
            .send().ok()?
            .json().ok()?;
        Some(slots.into_iter().map(|s| PeerSlot {
            kind:     s.kind,
            peer_id:  s.peer_id,
            peer_idx: s.peer_idx,
            loss:     s.loss,
            referrer: s.referrer,
        }).collect())
    }

    fn step(&mut self, _n: usize) { self.refresh(); }
    fn drop_probability(&self) -> f64 { 0.0 }
    fn set_drop_probability(&mut self, _p: f64) {
        eprintln!("drop probability: not supported in Docker mode");
    }

    /// Combined inbox from all alive nodes.
    fn inbox(&self) -> Vec<InboxEntry> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for n in self.nodes.iter().filter(|n| n.alive) {
            let entries: Vec<ApiInboxEntry> = self.client
                .get(format!("{}/api/inbox", n.admin_url))
                .send().ok()
                .and_then(|r| r.json().ok())
                .unwrap_or_default();
            for e in entries {
                let key = format!("{}|{}", e.from, e.text);
                if seen.insert(key) {
                    result.push(InboxEntry { from: e.from, text: e.text });
                }
            }
        }
        result
    }

    /// Send `payload` to `to` via P2P relay through the gateway node.
    fn rpc(&mut self, to: &str, payload: &[u8], timeout_ms: u64) -> Result<Vec<u8>, String> {
        let to_id       = self.resolve_node_id(to)?;
        let gw          = self.gateway()?;
        let relay_url   = format!("{}/api/relay", gw.admin_url);
        let from_client = gw.node_id.clone();

        let text = String::from_utf8_lossy(payload).into_owned();
        let body = serde_json::json!({
            "to_id":       to_id,
            "text":        text,
            "from_client": from_client,
            "path":        [from_client],
            "all_nodes":   [],
        });

        let json: serde_json::Value = self.client
            .post(&relay_url)
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .json(&body)
            .send()
            .map_err(|e| format!("HTTP error: {e}"))?
            .json()
            .map_err(|e| format!("bad JSON: {e}"))?;

        if json["delivered"].as_bool() != Some(true) {
            let err = json["error"].as_str().unwrap_or("not delivered");
            return Err(format!("relay failed: {err}"));
        }

        match json["response"].as_str() {
            Some(r) => Ok(r.as_bytes().to_vec()),
            None    => Err("packet delivered but server returned no response".into()),
        }
    }
}
