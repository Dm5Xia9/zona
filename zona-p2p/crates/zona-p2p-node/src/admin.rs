//! HTTP API served by each node.
//!
//! Core endpoints (always active):
//!   POST /api/relay       — internal hop-by-hop relay
//!   POST /api/introduce   — register a peer (used during bootstrap)
//!
//! Admin endpoints (only when compiled with `--features admin`):
//!   GET  /api/info        — node ID, health, slot count
//!   GET  /api/peers       — peer table
//!   GET  /api/inbox       — received user messages
//!   POST /api/send        — inject a message (entry point for CLI)

use std::{collections::HashMap, sync::Arc};

use axum::{Router, extract::State, routing::post, Json};
#[cfg(feature = "admin")]
use axum::routing::get;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

use zona_p2p_types::NodeId;
#[cfg(feature = "admin")]
use zona_p2p_types::SlotKind;
use zona_p2p_overlay::routing::xor::{next_hop, NextHopResult};

use crate::node::Node;

// ── Shared state ─────────────────────────────────────────────────────────────

pub struct AdminState {
    pub node:      Node,
    /// Maps NodeId → admin URL of that peer.
    pub peer_urls: HashMap<NodeId, String>,
    /// Our own admin URL (given at startup).
    pub self_url:  String,
    pub inbox:     Vec<InboxEntry>,
}

pub type Shared = Arc<Mutex<AdminState>>;

// ── Wire types ────────────────────────────────────────────────────────────────

/// Stored when a message is delivered to this node's inbox.
/// Always present (relay uses it on delivery).
#[derive(Clone, Serialize, Deserialize)]
pub struct InboxEntry {
    pub from: String,
    pub text: String,
}

// Admin-only wire types (require `--features admin`).

#[cfg(feature = "admin")]
#[derive(Serialize, Deserialize)]
pub struct ApiInfo {
    pub node_id: String,
    pub healthy: bool,
    pub slots:   usize,
}

#[cfg(feature = "admin")]
#[derive(Serialize, Deserialize)]
pub struct ApiPeerSlot {
    pub kind:     String,
    pub peer_id:  String,
    pub peer_idx: String,
    pub loss:     f32,
    pub referrer: String,
}


fn default_register_peer() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
pub struct IntroduceRequest {
    pub node_id:   String,
    pub admin_url: String,
    /// When `false`, return neighbors but do not add the caller to the routing table.
    #[serde(default = "default_register_peer")]
    pub register_peer: bool,
}

#[derive(Serialize, Deserialize)]
pub struct IntroduceResponse {
    pub node_id:   String,
    pub neighbors: Vec<NeighborInfo>,
}

#[derive(Serialize, Deserialize)]
pub struct NeighborInfo {
    pub node_id:   String,
    pub admin_url: String,
}

#[cfg(feature = "admin")]
#[derive(Deserialize)]
pub struct SendRequest {
    pub to_id:       String,
    pub text:        String,
    pub from_client: String,
    /// All node IDs in order (for display index resolution).
    #[serde(default)]
    pub all_nodes:   Vec<String>,
}

#[cfg(feature = "admin")]
#[derive(Serialize, Deserialize)]
pub struct SendResponse {
    pub delivered: bool,
    pub path:      Vec<HopInfo>,
    pub error:     Option<String>,
}

#[cfg(feature = "admin")]
#[derive(Serialize, Deserialize, Clone)]
pub struct HopInfo {
    pub node_id: String,
    pub index:   String,
    pub is_src:  bool,
    pub is_dst:  bool,
}

#[derive(Serialize, Deserialize)]
pub struct RelayRequest {
    pub to_id:       String,
    pub text:        String,
    pub from_client: String,
    pub path:        Vec<String>,   // accumulated hop IDs (full hex)
    pub all_nodes:   Vec<String>,
    /// Node URL to send the response back to (set by NodeServer clients).
    /// Regular nodes carry this field through without inspecting it.
    #[serde(default)] pub return_node_url: Option<String>,
    /// Correlation ID to match the response with a pending send_and_wait.
    #[serde(default)] pub correlation_id:  Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct RelayResponse {
    pub delivered: bool,
    pub path:      Vec<String>,
    pub error:     Option<String>,
    /// Response payload from the destination's handler (synchronous path).
    /// Propagates back through the relay chain without any changes at
    /// intermediate nodes — they just forward the JSON as-is.
    #[serde(default)] pub response: Option<String>,
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn make_router(state: Shared) -> Router {
    // Core P2P endpoints — always active.
    let router = Router::new()
        .route("/api/relay",     post(handle_relay))
        .route("/api/introduce", post(handle_introduce));

    // Admin endpoints — only compiled in with `--features admin`.
    #[cfg(feature = "admin")]
    let router = router
        .route("/api/info",  get(handle_info))
        .route("/api/peers", get(handle_peers))
        .route("/api/inbox", get(handle_inbox))
        .route("/api/send",  post(handle_send));

    router.with_state(state)
}

// ── Admin handlers (feature-gated) ───────────────────────────────────────────

#[cfg(feature = "admin")]
async fn handle_info(State(st): State<Shared>) -> Json<ApiInfo> {
    let g = st.lock().await;
    Json(ApiInfo {
        node_id: hex::encode(g.node.id.as_bytes()),
        healthy: !g.node.table.in_recovery(),
        slots:   g.node.table.slots.len(),
    })
}

#[cfg(feature = "admin")]
async fn handle_peers(State(st): State<Shared>) -> Json<Vec<ApiPeerSlot>> {
    let g  = st.lock().await;
    let ids: Vec<String> = g.peer_urls.keys().map(|k| hex::encode(k.as_bytes())).collect();
    let slots: Vec<ApiPeerSlot> = g.node.table.slots.iter().map(|s| {
        let pid_hex = hex::encode(s.peer_id.as_bytes());
        let idx_str = ids.iter().position(|x| *x == pid_hex)
            .map(|i| i.to_string()).unwrap_or("?".into());
        ApiPeerSlot {
            kind:     match s.kind { SlotKind::Greedy => "G", SlotKind::LongJump => "L", SlotKind::Diversity => "D" }.into(),
            peer_id:  pid_hex[..8.min(pid_hex.len())].to_string(),
            peer_idx: idx_str,
            loss:     s.ewma_loss,
            referrer: hex::encode(&s.referrer_id.as_bytes()[..4]),
        }
    }).collect();
    Json(slots)
}

#[cfg(feature = "admin")]
async fn handle_inbox(State(st): State<Shared>) -> Json<Vec<InboxEntry>> {
    let g = st.lock().await;
    Json(g.inbox.clone())
}

async fn handle_introduce(
    State(st): State<Shared>,
    Json(req): Json<IntroduceRequest>,
) -> Json<IntroduceResponse> {
    let mut g = st.lock().await;
    let self_id = hex::encode(g.node.id.as_bytes());

    // Store the peer's URL (unless lookup-only introduce from an ephemeral client).
    if req.register_peer {
        if let Ok(bytes) = hex::decode(&req.node_id) {
            if let Ok(arr) = bytes.try_into() {
                let raw: [u8; 32] = arr;
                let nid = NodeId::from_bytes(raw);
                g.peer_urls.insert(nid, req.admin_url.clone());

                // Add to routing table as a greedy candidate.
                // Reset tick so this introduction can always compete for a slot (§5.1).
                use zona_p2p_types::{SlotEntry, SlotKind};
                let entry = SlotEntry::new(SlotKind::Greedy, nid, nid);
                g.node.table.reset_tick();
                g.node.table.try_insert(entry);
                info!(peer = %req.admin_url, "introduce: registered peer");
            }
        }
    } else {
        info!(peer = %req.admin_url, "introduce: lookup-only (caller not added to routing table)");
    }

    // Return our own ID + neighbors.
    let neighbors: Vec<NeighborInfo> = g.node.table.slots.iter()
        .take(3)
        .filter_map(|s| {
            let hex_id = hex::encode(s.peer_id.as_bytes());
            g.peer_urls.get(&s.peer_id).map(|url| NeighborInfo {
                node_id:   hex_id,
                admin_url: url.clone(),
            })
        })
        .collect();

    Json(IntroduceResponse { node_id: self_id, neighbors })
}

// ── Core handlers (always active) ────────────────────────────────────────────

#[cfg(feature = "admin")]
async fn handle_send(
    State(st): State<Shared>,
    Json(req): Json<SendRequest>,
) -> Json<SendResponse> {
    let self_id_hex = {
        let g = st.lock().await;
        hex::encode(g.node.id.as_bytes())
    };

    // Save all_nodes before moving into relay request.
    let all_nodes = req.all_nodes.clone();

    let path = vec![self_id_hex.clone()];
    let relay_req = RelayRequest {
        to_id:           req.to_id,
        text:            req.text,
        from_client:     req.from_client,
        path,
        all_nodes:       req.all_nodes,
        return_node_url: None,
        correlation_id:  None,
    };
    let relay_resp = do_relay(st, relay_req).await;

    Json(SendResponse {
        delivered: relay_resp.delivered,
        path:      relay_resp.path.iter().enumerate().map(|(i, id)| {
            // Resolve actual node index from the global all_nodes list.
            let node_idx = all_nodes.iter().position(|n| n == id)
                .map(|idx| idx.to_string())
                .unwrap_or_else(|| i.to_string());
            HopInfo {
                node_id: id[..8.min(id.len())].to_string(),
                index:   node_idx,
                is_src:  i == 0,
                is_dst:  relay_resp.delivered && i + 1 == relay_resp.path.len(),
            }
        }).collect(),
        error:     relay_resp.error,
    })
}

async fn handle_relay(
    State(st): State<Shared>,
    Json(req): Json<RelayRequest>,
) -> Json<RelayResponse> {
    Json(do_relay(st, req).await)
}

async fn do_relay(st: Shared, req: RelayRequest) -> RelayResponse {
    let to_bytes = match hex::decode(&req.to_id) {
        Ok(b) => b,
        Err(e) => return RelayResponse { delivered: false, path: req.path, error: Some(e.to_string()), response: None },
    };
    let to_arr: [u8; 32] = match to_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return RelayResponse { delivered: false, path: req.path, error: Some("bad to_id length".into()), response: None },
    };
    let to_id = NodeId::from_bytes(to_arr);

    // Routing decision — computed while holding the lock, then lock released
    // before any async HTTP calls.
    enum Decision {
        Forward { next_id: NodeId, next_url: Option<String> },
        /// §7.3 fallback: try every unvisited peer (limited fan-out).
        LocalMinimum { alternatives: Vec<(String, String)> }, // (hex_id, admin_url)
    }

    let decision = {
        let mut g = st.lock().await;
        let self_id = g.node.id;

        // Are we the destination?
        if self_id == to_id {
            g.inbox.push(InboxEntry { from: req.from_client.clone(), text: req.text.clone() });
            info!(from = %req.from_client, "message delivered");
            return RelayResponse { delivered: true, path: req.path, error: None, response: None };
        }

        match next_hop(&self_id, &to_id, &g.node.table) {
            NextHopResult::Forward(nid) => {
                let url = g.peer_urls.get(&nid).cloned();
                Decision::Forward { next_id: nid, next_url: url }
            }
            NextHopResult::Deliver => {
                // XOR-closest — deliver here.
                g.inbox.push(InboxEntry { from: req.from_client.clone(), text: req.text.clone() });
                info!(from = %req.from_client, "message delivered (XOR-closest)");
                return RelayResponse { delivered: true, path: req.path, error: None, response: None };
            }
            NextHopResult::LocalMinimum => {
                // §7.3: greedy path stuck; collect all unvisited peers for fan-out.
                let alternatives = g.peer_urls.iter()
                    .filter_map(|(nid, url)| {
                        let hex = hex::encode(nid.as_bytes());
                        if !req.path.contains(&hex) { Some((hex, url.clone())) } else { None }
                    })
                    .collect();
                Decision::LocalMinimum { alternatives }
            }
        }
        // MutexGuard dropped here.
    };

    let http = reqwest::Client::new();

    match decision {
        Decision::Forward { next_id, next_url: Some(next_url) } => {
            let next_id_hex = hex::encode(next_id.as_bytes());
            if req.path.contains(&next_id_hex) {
                return RelayResponse {
                    delivered: false,
                    path: req.path,
                    error: Some("routing loop".into()),
                    response: None,
                };
            }
            let mut new_path = req.path.clone();
            new_path.push(next_id_hex);
            let relay = RelayRequest {
                to_id: req.to_id, text: req.text,
                from_client: req.from_client,
                path: new_path.clone(), all_nodes: req.all_nodes,
                return_node_url: req.return_node_url,
                correlation_id:  req.correlation_id,
            };
            let url = format!("{next_url}/api/relay");
            match http.post(&url).json(&relay).send().await {
                Ok(resp) => resp.json::<RelayResponse>().await
                    .unwrap_or_else(|e| RelayResponse { delivered: false, path: new_path, error: Some(e.to_string()), response: None }),
                Err(e) => RelayResponse { delivered: false, path: new_path, error: Some(e.to_string()), response: None },
            }
        }

        Decision::Forward { next_url: None, .. } => {
            warn!("no admin URL for next hop");
            RelayResponse { delivered: false, path: req.path, error: Some("no URL for next hop".into()), response: None }
        }

        Decision::LocalMinimum { alternatives } => {
            // §7.3: "короткий контролируемый flood" — try every unvisited peer.
            // Loop prevention: each peer we try is added to the path, so they
            // won't relay back to us.
            if alternatives.is_empty() {
                warn!(to = %req.to_id, "local minimum — no unvisited peers left");
                return RelayResponse {
                    delivered: false, path: req.path,
                    error: Some("local minimum — dead end".into()),
                    response: None,
                };
            }
            warn!(to = %req.to_id, count = alternatives.len(),
                  "local minimum — §7.3 fan-out to {} unvisited peers", alternatives.len());

            let mut last = RelayResponse {
                delivered: false, path: req.path.clone(),
                error: Some("local minimum — all alternatives failed".into()),
                response: None,
            };
            for (peer_hex, peer_url) in &alternatives {
                let mut new_path = req.path.clone();
                new_path.push(peer_hex.clone());
                let relay = RelayRequest {
                    to_id: req.to_id.clone(), text: req.text.clone(),
                    from_client: req.from_client.clone(),
                    path: new_path, all_nodes: req.all_nodes.clone(),
                    return_node_url: req.return_node_url.clone(),
                    correlation_id:  req.correlation_id,
                };
                let url = format!("{peer_url}/api/relay");
                match http.post(&url).json(&relay).send().await {
                    Ok(resp) => {
                        if let Ok(r) = resp.json::<RelayResponse>().await {
                            if r.delivered { return r; }
                            last = r;
                        }
                    }
                    Err(_) => {}
                }
            }
            last
        }
    }
}
