//! NodeServer — P2P node with HTTP relay transport + application handler dispatch.
//!
//! # Flow (request → response)
//!
//! ```text
//!  Client POST /api/packet
//!        │
//!        ▼
//!  send_and_wait()  ──────────────────────────────────────────────────────╮
//!    saves oneshot {cid → tx}                                             │
//!    POST {first_hop}/api/relay  [to_id=srv, return_node_url=self, cid]  │
//!        │                                                                │
//!        ▼ (relayed through zona-p2p-node hops)                          │
//!  Server's /api/relay  (do_relay, to_id==self)                          │
//!    → find matching handler                                              │
//!    → call handler(AppPacket) → response bytes                          │
//!    → POST directly to return_node_url/api/relay                        │
//!          [to_id=from_client, correlation_id=cid]                       │
//!        │                                                                │
//!        ▼                                                                │
//!  Sender's /api/relay  (to_id==self)                                    │
//!    → cid matches pending → tx.send(response)                           │
//!        │                                                                │
//!        ╰──────────────────────────────────────────────────────────────►╯
//!  send_and_wait() resolves → HTTP response to client
//! ```

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

use zona_p2p_crypto::NodeKeypair;
use zona_p2p_node::{Node, NodeConfig};
use zona_p2p_overlay::routing::xor::{next_hop, NextHopResult};
use zona_p2p_types::{NodeId, SlotEntry, SlotKind};

use crate::{handler::PacketHandler, packet::AppPacket};

// ── Correlation counter ────────────────────────────────────────────────────────

static NEXT_CID: AtomicU64 = AtomicU64::new(1);

fn next_cid() -> u64 {
    NEXT_CID.fetch_add(1, Ordering::Relaxed)
}

// ── State ─────────────────────────────────────────────────────────────────────

pub struct ServerState {
    pub node:      Node,
    /// Maps NodeId → admin/relay HTTP URL of that peer.
    pub peer_urls: HashMap<NodeId, String>,
    /// This node's own HTTP base URL (e.g. `http://hello-server:8801`).
    pub self_url:  String,
    /// Pending send_and_wait calls: correlation_id → response channel.
    pub pending:   HashMap<u64, oneshot::Sender<Vec<u8>>>,
    /// Registered application handlers. Arc so they can be cloned out of the lock.
    pub handlers:  Vec<Arc<dyn PacketHandler>>,
}

/// Arc-wrapped mutable state shared between axum handlers and the NodeServer.
pub type Shared = Arc<Mutex<ServerState>>;

// ── Relay wire types (mirrors zona-p2p-node/src/admin.rs) ─────────────────────

/// Must stay in sync with `RelayRequest` in `zona-p2p-node/src/admin.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RelayRequest {
    pub to_id:          String,
    pub text:           String,
    pub from_client:    String,
    pub path:           Vec<String>,
    #[serde(default)]
    pub all_nodes:      Vec<String>,
    /// HTTP URL of the node that initiated the request; used to route response.
    #[serde(default)]
    pub return_node_url: Option<String>,
    /// Opaque token to match a response with a waiting send_and_wait().
    #[serde(default)]
    pub correlation_id:  Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RelayResponse {
    pub delivered: bool,
    pub path:      Vec<String>,
    pub error:     Option<String>,
    /// Synchronous response from the destination's handler.
    /// Propagates back through the relay chain automatically.
    #[serde(default)] pub response: Option<String>,
}

fn default_register_peer() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IntroduceRequest {
    pub node_id:   String,
    pub admin_url: String,
    /// When `false`, return neighbors but do not add the caller to `peer_urls` / routing slots.
    /// Ephemeral clients (e.g. zona-curl) must set this to `false` so XOR routing does not
    /// select them as the next hop toward a destination (would cause a routing loop).
    #[serde(default = "default_register_peer")]
    pub register_peer: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IntroduceResponse {
    pub node_id:   String,
    pub neighbors: Vec<NeighborInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NeighborInfo {
    pub node_id:   String,
    pub admin_url: String,
}

// ── NodeServer ────────────────────────────────────────────────────────────────

/// A P2P node that participates in the HTTP relay network and dispatches
/// incoming application packets to registered handlers.
pub struct NodeServer {
    state: Shared,
}

impl NodeServer {
    /// Create a new server with the given keypair.
    ///
    /// `self_url` is the HTTP base URL at which this node will be reachable
    /// by other nodes (e.g. `http://hello-server:8801`).
    pub fn new(self_url: impl Into<String>, keypair: NodeKeypair) -> Self {
        let self_url = self_url.into();
        let mut config = NodeConfig::new("0.0.0.0:0".parse().unwrap());
        config.max_greedy = 5;
        let node = Node::new(keypair, config);
        let state = Arc::new(Mutex::new(ServerState {
            node,
            peer_urls: HashMap::new(),
            self_url,
            pending:  HashMap::new(),
            handlers: Vec::new(),
        }));
        NodeServer { state }
    }

    /// Register an application packet handler (call before `run()`).
    pub fn register(&mut self, handler: impl PacketHandler + 'static) {
        self.state
            .try_lock()
            .expect("register() called while server is already running")
            .handlers
            .push(Arc::new(handler));
    }

    /// Bootstrap the server by introducing itself to each seed HTTP URL.
    ///
    /// Performs a 2-hop BFS (same strategy as `zona-p2p-node`): contacts
    /// every seed, then contacts the neighbors returned by those seeds.
    pub async fn bootstrap(&self, seeds: Vec<String>) -> anyhow::Result<()> {
        let (node_id_hex, self_url) = {
            let g = self.state.lock().await;
            (hex::encode(g.node.id.as_bytes()), g.self_url.clone())
        };
        let body = IntroduceRequest {
            node_id:         node_id_hex,
            admin_url:       self_url,
            register_peer:   true,
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let mut second_hop: Vec<NeighborInfo> = Vec::new();
        for seed_url in &seeds {
            match do_introduce(&http, seed_url, &body, &self.state).await {
                Ok(nb) => second_hop.extend(nb),
                Err(e) => {
                    warn!(seed = %seed_url, "bootstrap attempt 1 failed: {e}, retrying...");
                    // Retry with back-off (e.g. node might still be starting).
                    let mut ok = false;
                    for attempt in 2u8..=8 {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        if let Ok(nb) = do_introduce(&http, seed_url, &body, &self.state).await {
                            second_hop.extend(nb);
                            ok = true;
                            break;
                        }
                        info!(seed = %seed_url, attempt, "still retrying bootstrap...");
                    }
                    if !ok { warn!(seed = %seed_url, "bootstrap: gave up after retries"); }
                }
            }
        }

        let seeds_set: std::collections::HashSet<&str> =
            seeds.iter().map(String::as_str).collect();
        for nb in second_hop {
            if seeds_set.contains(nb.admin_url.as_str()) { continue; }
            let _ = do_introduce(&http, &nb.admin_url, &body, &self.state).await;
        }

        let known = self.state.lock().await.peer_urls.len();
        info!(known_peers = known, "bootstrap complete");
        Ok(())
    }

    /// Return a clone of the shared state (for axum routing).
    pub fn shared(&self) -> Shared {
        self.state.clone()
    }

    /// Build the axum `Router` for this server.
    ///
    /// Exposes:
    /// - `POST /api/relay`     — P2P relay hop (called by other nodes)
    /// - `POST /api/introduce` — peer registration (called during bootstrap)
    /// - `POST /api/packet`    — client entry point (blocking RPC)
    pub fn make_router(&self) -> axum::Router {
        use axum::routing::post;
        axum::Router::new()
            .route("/api/relay",     post(http_relay))
            .route("/api/introduce", post(http_introduce))
            .route("/api/packet",    post(crate::http_api::handle_packet))
            .with_state(self.state.clone())
    }
}

// ── Bootstrap helper ──────────────────────────────────────────────────────────

async fn do_introduce(
    http:   &reqwest::Client,
    target: &str,
    body:   &IntroduceRequest,
    state:  &Shared,
) -> anyhow::Result<Vec<NeighborInfo>> {
    let url = format!("{target}/api/introduce");
    let resp = http.post(&url).json(body).send().await?;
    let intro: IntroduceResponse = resp.json().await?;

    let mut g = state.lock().await;
    if let Some(nid) = parse_node_id(&intro.node_id) {
        g.peer_urls.insert(nid, target.to_string());
        g.node.table.reset_tick();
        g.node.table.try_insert(SlotEntry::new(SlotKind::Greedy, nid, nid));
    }
    for nb in &intro.neighbors {
        if let Some(nid) = parse_node_id(&nb.node_id) {
            g.peer_urls.insert(nid, nb.admin_url.clone());
            g.node.table.reset_tick();
            g.node.table.try_insert(SlotEntry::new(SlotKind::Greedy, nid, nid));
        }
    }
    info!(peer = %target, neighbors = intro.neighbors.len(), "introduce ok");
    Ok(intro.neighbors)
}

// ── Axum handlers ─────────────────────────────────────────────────────────────

async fn http_introduce(
    axum::extract::State(st): axum::extract::State<Shared>,
    axum::Json(req):           axum::Json<IntroduceRequest>,
) -> axum::Json<IntroduceResponse> {
    let mut g = st.lock().await;

    let self_id = hex::encode(g.node.id.as_bytes());

    if req.register_peer {
        if let Some(nid) = parse_node_id(&req.node_id) {
            g.peer_urls.insert(nid, req.admin_url.clone());
            g.node.table.reset_tick();
            g.node.table.try_insert(SlotEntry::new(SlotKind::Greedy, nid, nid));
            info!(peer = %req.admin_url, "introduce: registered peer");
        }
    } else {
        info!(peer = %req.admin_url, "introduce: lookup-only (caller not added to routing table)");
    }

    let neighbors: Vec<NeighborInfo> = g.node.table.slots.iter()
        .take(3)
        .filter_map(|s| {
            g.peer_urls.get(&s.peer_id).map(|url| NeighborInfo {
                node_id:   hex::encode(s.peer_id.as_bytes()),
                admin_url: url.clone(),
            })
        })
        .collect();

    axum::Json(IntroduceResponse { node_id: self_id, neighbors })
}

async fn http_relay(
    axum::extract::State(st): axum::extract::State<Shared>,
    axum::Json(req):           axum::Json<RelayRequest>,
) -> axum::Json<RelayResponse> {
    axum::Json(do_relay(st, req).await)
}

// ── Core relay logic ──────────────────────────────────────────────────────────

pub(crate) async fn do_relay(st: Shared, req: RelayRequest) -> RelayResponse {
    // Parse destination.
    let to_id = match parse_node_id(&req.to_id) {
        Some(id) => id,
        None => return RelayResponse { response: None,
            delivered: false, path: req.path,
            error: Some(format!("bad to_id: {}", req.to_id)),
        },
    };

    // Take routing decision while holding lock, then drop it before I/O.
    enum Decision {
        /// Packet is for this node.
        Deliver,
        /// Forward to a specific peer.
        Forward { next_url: String, next_id_hex: String },
        /// Greedy routing stuck — try all unvisited peers.
        LocalMinimum { alts: Vec<(String, String)> },
        /// No route at all.
        NoRoute(String),
    }

    let (decision, self_id_hex, self_url) = {
        let g = st.lock().await;
        let self_id = g.node.id;
        let self_hex = hex::encode(self_id.as_bytes());
        let self_url = g.self_url.clone();

        let dec = if self_id == to_id {
            Decision::Deliver
        } else {
            match next_hop(&self_id, &to_id, &g.node.table) {
                NextHopResult::Forward(nid) => {
                    match g.peer_urls.get(&nid) {
                        Some(url) => Decision::Forward {
                            next_url:     url.clone(),
                            next_id_hex:  hex::encode(nid.as_bytes()),
                        },
                        None => Decision::NoRoute(
                            format!("no URL for next hop {}", hex::encode(nid.as_bytes()))
                        ),
                    }
                }
                NextHopResult::Deliver => Decision::Deliver, // XOR-closest
                NextHopResult::LocalMinimum => {
                    let alts = g.peer_urls.iter()
                        .filter_map(|(nid, url)| {
                            let hex = hex::encode(nid.as_bytes());
                            if !req.path.contains(&hex) { Some((hex, url.clone())) } else { None }
                        })
                        .collect();
                    Decision::LocalMinimum { alts }
                }
            }
        };
        (dec, self_hex, self_url)
    };

    match decision {
        Decision::Deliver => deliver(st, req, &self_id_hex, &self_url).await,

        Decision::Forward { next_url, next_id_hex } => {
            if req.path.contains(&next_id_hex) {
                return RelayResponse {
                    delivered: false, path: req.path,
                    error: Some("routing loop".into()), response: None,
                };
            }
            forward_relay(&next_url, req, next_id_hex).await
        }

        Decision::LocalMinimum { alts } => {
            if alts.is_empty() {
                warn!(to = %req.to_id, "local minimum — no unvisited peers");
                return RelayResponse {
                    delivered: false, path: req.path,
                    error: Some("local minimum — dead end".into()), response: None,
                };
            }
            warn!(to = %req.to_id, count = alts.len(), "local minimum — fan-out");
            fan_out(alts, req).await
        }

        Decision::NoRoute(err) => {
            warn!("{err}");
            RelayResponse { delivered: false, path: req.path, error: Some(err), response: None }
        }
    }
}

/// Deliver a packet that has reached its final destination (this node).
async fn deliver(
    st:          Shared,
    req:         RelayRequest,
    self_id_hex: &str,
    _self_url:   &str,
) -> RelayResponse {
    // 1. Check if this resolves a pending send_and_wait.
    if let Some(cid) = req.correlation_id {
        let tx = st.lock().await.pending.remove(&cid);
        if let Some(tx) = tx {
            let _ = tx.send(req.text.into_bytes());
            return RelayResponse { delivered: true, path: req.path, error: None, response: None };
        }
    }

    // 2. Dispatch to the first matching handler.
    let payload = req.text.as_bytes().to_vec();

    // Clone the handler Arc to call it outside the lock (avoids deadlock).
    let handler: Option<Arc<dyn PacketHandler>> = {
        let g = st.lock().await;
        g.handlers.iter().find(|h| h.matches(&payload)).cloned()
    };

    let mut sync_response: Option<String> = None;

    if let Some(h) = handler {
        let app = AppPacket {
            from:    parse_node_id(&req.from_client).unwrap_or(NodeId::ZERO),
            payload: payload.clone(),
        };
        let resp_bytes = h.handle(app);

        match (resp_bytes, req.return_node_url.as_ref()) {
            (Some(bytes), Some(return_url)) => {
                // Async path: caller has a NodeServer listener — POST response back.
                let resp_relay = RelayRequest {
                    to_id:           req.from_client.clone(),
                    text:            String::from_utf8_lossy(&bytes).into_owned(),
                    from_client:     self_id_hex.to_string(),
                    path:            vec![self_id_hex.to_string()],
                    all_nodes:       vec![],
                    return_node_url: None,
                    correlation_id:  req.correlation_id,
                };
                let http = reqwest::Client::new();
                let url = format!("{return_url}/api/relay");
                if let Err(e) = http.post(&url).json(&resp_relay).send().await {
                    warn!(return_url = %return_url, "failed to send async response: {e}");
                }
            }
            (Some(bytes), None) => {
                // Sync path: no return URL — embed response in RelayResponse so it
                // propagates back through the relay chain to the original caller.
                sync_response = Some(String::from_utf8_lossy(&bytes).into_owned());
            }
            _ => {
                info!(from = %req.from_client, "delivered (no handler matched or no response)");
            }
        }
    }

    RelayResponse { delivered: true, path: req.path, error: None, response: sync_response }
}

/// Forward a relay request to the next hop.
async fn forward_relay(next_url: &str, req: RelayRequest, next_id_hex: String) -> RelayResponse {
    let mut new_path = req.path.clone();
    new_path.push(next_id_hex);
    let relay = RelayRequest {
        path: new_path.clone(),
        ..req
    };
    let http = reqwest::Client::new();
    let url = format!("{next_url}/api/relay");
    match http.post(&url).json(&relay).send().await {
        Ok(resp) => resp.json::<RelayResponse>().await
            .unwrap_or_else(|e| RelayResponse { delivered: false, path: new_path, error: Some(e.to_string()), response: None }),
        Err(e) => RelayResponse { delivered: false, path: new_path, error: Some(e.to_string()), response: None },
    }
}

/// §7.3 fan-out: try all unvisited peers when greedy path is stuck.
async fn fan_out(alts: Vec<(String, String)>, req: RelayRequest) -> RelayResponse {
    let http = reqwest::Client::new();
    let mut last = RelayResponse {
        delivered: false, path: req.path.clone(),
        error: Some("local minimum — all alternatives failed".into()),
        response: None,
    };
    for (peer_hex, peer_url) in &alts {
        let mut new_path = req.path.clone();
        new_path.push(peer_hex.clone());
        let relay = RelayRequest { path: new_path, ..req.clone() };
        let url = format!("{peer_url}/api/relay");
        if let Ok(resp) = http.post(&url).json(&relay).send().await {
            if let Ok(r) = resp.json::<RelayResponse>().await {
                if r.delivered { return r; }
                last = r;
            }
        }
    }
    last
}

// ── Public send_and_wait ──────────────────────────────────────────────────────

/// Send `payload` bytes to `to_id` and wait for a response (or timeout).
///
/// Used by [`crate::http_api`] to implement the blocking `POST /api/packet`.
pub async fn send_and_wait(
    state:   Shared,
    to_id:   NodeId,
    payload: Vec<u8>,
    timeout: Duration,
) -> anyhow::Result<Vec<u8>> {
    let (tx, rx) = oneshot::channel::<Vec<u8>>();
    let cid = next_cid();

    let (first_hop_url, self_id_hex, self_url, to_id_hex) = {
        let mut g = state.lock().await;
        g.pending.insert(cid, tx);

        let self_id  = g.node.id;
        let self_hex = hex::encode(self_id.as_bytes());
        let self_url = g.self_url.clone();
        let to_hex   = hex::encode(to_id.as_bytes());

        // If the destination IS this node, dispatch locally and skip the network.
        if self_id == to_id {
            let handler = g.handlers.iter().find(|h| h.matches(&payload)).cloned();
            drop(g);
            state.lock().await.pending.remove(&cid);
            return match handler {
                Some(h) => {
                    let app = AppPacket { from: to_id, payload: payload.clone() };
                    h.handle(app).ok_or_else(|| anyhow::anyhow!("handler returned no response"))
                }
                None => Err(anyhow::anyhow!("no handler matched")),
            };
        }

        let hop_url = match next_hop(&self_id, &to_id, &g.node.table) {
            NextHopResult::Forward(nid) => g.peer_urls.get(&nid).cloned(),
            NextHopResult::Deliver => {
                // XOR-closest but not exact — still treat as remote, pick any peer.
                g.peer_urls.values().next().cloned()
            }
            NextHopResult::LocalMinimum => {
                // Fan-out: just pick any peer as the first hop.
                g.peer_urls.values().next().cloned()
            }
        };
        (hop_url, self_hex, self_url, to_hex)
    };

    let Some(first_url) = first_hop_url else {
        state.lock().await.pending.remove(&cid);
        return Err(anyhow::anyhow!("no route to {to_id_hex}"));
    };

    let relay = RelayRequest {
        to_id:           to_id_hex,
        text:            String::from_utf8_lossy(&payload).into_owned(),
        from_client:     self_id_hex.clone(),
        path:            vec![self_id_hex],
        all_nodes:       vec![],
        return_node_url: Some(self_url),
        correlation_id:  Some(cid),
    };

    let http = reqwest::Client::new();
    let url = format!("{first_url}/api/relay");
    if let Err(e) = http.post(&url).json(&relay).send().await {
        state.lock().await.pending.remove(&cid);
        return Err(anyhow::anyhow!("relay send failed: {e}"));
    }

    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(resp))  => Ok(resp),
        Ok(Err(_))    => Err(anyhow::anyhow!("response channel closed")),
        Err(_elapsed) => {
            state.lock().await.pending.remove(&cid);
            Err(anyhow::anyhow!("timeout waiting for response"))
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_node_id(s: &str) -> Option<NodeId> {
    let bytes = hex::decode(s).ok()?;
    let arr: [u8; 32] = bytes.try_into().ok()?;
    Some(NodeId::from_bytes(arr))
}
