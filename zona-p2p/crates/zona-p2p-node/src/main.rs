//! zona-p2p node binary.
//!
//! In sandbox mode (no env vars) just prints the node ID.
//! In Docker mode (ZONA_ADMIN_URL set) runs the full admin HTTP API and
//! performs HTTP-based peer introduction.
//!
//! Environment variables:
//!   ZONA_NODE_SEED      — 64 hex chars (32 bytes) for deterministic keypair
//!   ZONA_ADMIN_URL      — this node's own admin URL (e.g. http://node0:7701)
//!   ZONA_ADMIN_PORT     — port for the HTTP admin server (default: 7701)
//!   ZONA_BOOTSTRAP_PEERS — comma-separated admin URLs to bootstrap from

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::info;

use zona_p2p_crypto::NodeKeypair;
use zona_p2p_node::{admin, Node, NodeConfig};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("zona_p2p=info".parse()?)
        )
        .init();

    // ── Config from env ───────────────────────────────────────────────────────

    let self_url = std::env::var("ZONA_ADMIN_URL").ok();

    if self_url.is_none() {
        // Standalone / sandbox mode.
        let keypair = NodeKeypair::generate();
        let addr: SocketAddr = "0.0.0.0:7700".parse()?;
        let config = NodeConfig::new(addr);
        let node = Node::new(keypair, config);
        info!(node_id = %node.id, "zona-p2p ready (no ZONA_ADMIN_URL — standalone mode)");
        return Ok(());
    }

    let self_url = self_url.unwrap();
    let admin_port: u16 = std::env::var("ZONA_ADMIN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7701);
    let bootstrap_peers: Vec<String> = std::env::var("ZONA_BOOTSTRAP_PEERS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // ── Keypair ───────────────────────────────────────────────────────────────

    let keypair = if let Ok(seed_hex) = std::env::var("ZONA_NODE_SEED") {
        let seed_bytes = hex::decode(&seed_hex)?;
        let seed: [u8; 32] = seed_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("ZONA_NODE_SEED must be 64 hex chars (32 bytes)"))?;
        NodeKeypair::from_seed(seed)
    } else {
        NodeKeypair::generate()
    };

    let addr: SocketAddr = "0.0.0.0:0".parse()?; // addr unused in HTTP-relay mode
    let mut config = NodeConfig::new(addr);
    config.max_greedy = 5;
    let node = Node::new(keypair, config);
    let node_id_hex = hex::encode(node.id.as_bytes());
    info!(node_id = %node.id, admin_url = %self_url, "zona-p2p node starting");

    // ── Shared state ──────────────────────────────────────────────────────────

    let state: admin::Shared = Arc::new(Mutex::new(admin::AdminState {
        node,
        peer_urls: HashMap::new(),
        self_url:  self_url.clone(),
        inbox:     Vec::new(),
    }));

    // ── Bootstrap ────────────────────────────────────────────────────────────────
    // §5.1: After vступление, pull peers from bootstrap node and then do a
    // second-hop introduction to the neighbors of neighbors so the routing table
    // gets populated beyond just one bootstrap peer.
    //
    // Round 1 — introduce to every explicit bootstrap peer.
    // Round 2 — introduce to every neighbor we learned in round 1 (2-hop BFS).

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    // Helper: parse a hex NodeId string into NodeId.
    fn parse_node_id(hex_str: &str) -> Option<zona_p2p_types::NodeId> {
        let bytes = hex::decode(hex_str).ok()?;
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(zona_p2p_types::NodeId::from_bytes(arr))
    }

    // Helper: send introduce and register peers in the shared state.
    async fn do_introduce(
        http: &reqwest::Client,
        target_url: &str,
        body: &admin::IntroduceRequest,
        state: &admin::Shared,
    ) -> Vec<admin::NeighborInfo> {
        let url = format!("{target_url}/api/introduce");
        let Ok(resp) = http.post(&url).json(body).send().await else { return vec![]; };
        let Ok(intro) = resp.json::<admin::IntroduceResponse>().await else { return vec![]; };

        let mut g = state.lock().await;
        use zona_p2p_types::{SlotEntry, SlotKind};

        // Register the peer we just introduced to.
        if let Some(nid) = parse_node_id(&intro.node_id) {
            g.peer_urls.insert(nid, target_url.to_string());
            g.node.table.reset_tick(); // allow competitive insertion
            g.node.table.try_insert(SlotEntry::new(SlotKind::Greedy, nid, nid));
        }
        // Register all neighbors they returned.
        for nb in &intro.neighbors {
            if let Some(nid) = parse_node_id(&nb.node_id) {
                g.peer_urls.insert(nid, nb.admin_url.clone());
                g.node.table.reset_tick();
                g.node.table.try_insert(SlotEntry::new(SlotKind::Greedy, nid, nid));
            }
        }
        info!(peer = %target_url, known = intro.neighbors.len(), "bootstrap: introduced");
        intro.neighbors
    }

    let introduce_body = admin::IntroduceRequest {
        node_id:         node_id_hex.clone(),
        admin_url:       self_url.clone(),
        register_peer:   true,
    };

    // Round 1 — primary bootstrap peers (with retries).
    let mut second_hop: Vec<admin::NeighborInfo> = Vec::new();
    for peer_url in &bootstrap_peers {
        let mut ok = false;
        for attempt in 1u8..=8 {
            let neighbors = do_introduce(&http, peer_url, &introduce_body, &state).await;
            if !neighbors.is_empty() || attempt == 8 {
                second_hop.extend(neighbors);
                ok = true;
                break;
            }
            info!(peer = %peer_url, attempt, "bootstrap: peer not ready, retrying...");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        if !ok {
            info!(peer = %peer_url, "bootstrap: gave up after retries");
        }
    }

    // Round 2 — introduce to every neighbor we learned (2-hop BFS, §5.1).
    // Skip URLs we already bootstrapped to avoid double-work.
    let already_done: std::collections::HashSet<String> = bootstrap_peers.iter().cloned().collect();
    for nb in &second_hop {
        if already_done.contains(&nb.admin_url) { continue; }
        // Best-effort; no retries needed for 2nd hop.
        do_introduce(&http, &nb.admin_url, &introduce_body, &state).await;
    }

    // ── HTTP admin server ─────────────────────────────────────────────────────

    let router = admin::make_router(state);
    let listen_addr = SocketAddr::from(([0, 0, 0, 0], admin_port));
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    info!(addr = %listen_addr, "admin HTTP server listening");
    axum::serve(listener, router).await?;

    Ok(())
}
