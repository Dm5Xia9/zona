//! hello-server — minimal P2P application server built with `zona-p2p-server`.
//!
//! Registers a catch-all handler that echoes back every packet with a greeting.
//!
//! Environment variables:
//!   ZONA_SERVER_SEED       — 64 hex chars (32 bytes) for deterministic keypair
//!   ZONA_SERVER_SELF_URL   — HTTP URL at which this server is reachable by peers
//!                            (default: http://0.0.0.0:8801)
//!   ZONA_SERVER_HTTP_ADDR  — bind address for the HTTP listener (default: 0.0.0.0:8801)
//!   ZONA_SERVER_BOOTSTRAP  — comma-separated HTTP URLs of seed nodes to bootstrap from
//!                            (e.g. "http://node0:7701,http://node1:7701")

use anyhow::Result;
use tracing::info;
use zona_p2p_crypto::NodeKeypair;
use zona_p2p_server::{CatchAllHandler, NodeServer};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("zona_p2p=info".parse()?)
                .add_directive("hello_server=info".parse()?)
        )
        .init();

    // ── Config from env ───────────────────────────────────────────────────────

    let self_url: String = std::env::var("ZONA_SERVER_SELF_URL")
        .unwrap_or_else(|_| "http://0.0.0.0:8801".into());

    let http_addr: std::net::SocketAddr = std::env::var("ZONA_SERVER_HTTP_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8801".into())
        .parse()?;

    let bootstrap_urls: Vec<String> = std::env::var("ZONA_SERVER_BOOTSTRAP")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // ── Keypair ───────────────────────────────────────────────────────────────

    let keypair = if let Ok(seed_hex) = std::env::var("ZONA_SERVER_SEED") {
        let seed_bytes = hex::decode(&seed_hex)?;
        let seed: [u8; 32] = seed_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("ZONA_SERVER_SEED must be 64 hex chars (32 bytes)"))?;
        NodeKeypair::from_seed(seed)
    } else {
        NodeKeypair::generate()
    };

    let node_id_hex = hex::encode(keypair.node_id.as_bytes());

    // Print the node ID so scripts can capture it.
    println!("SERVER_NODE_ID={node_id_hex}");
    info!(node_id = %node_id_hex, http = %http_addr, self_url = %self_url, "hello-server starting");

    // ── Build server ──────────────────────────────────────────────────────────

    let mut server = NodeServer::new(&self_url, keypair);

    server.register(CatchAllHandler::new(|pkt| {
        let text  = String::from_utf8_lossy(&pkt.payload);
        let reply = format!("Hello from p2p-server! Echo: {text}");
        info!(from = %pkt.from, payload = %text, "handled packet");
        Some(reply.into_bytes())
    }));

    // ── Bootstrap ─────────────────────────────────────────────────────────────

    if !bootstrap_urls.is_empty() {
        info!(urls = ?bootstrap_urls, "bootstrapping...");
        server.bootstrap(bootstrap_urls).await?;
    }

    // ── Serve HTTP ────────────────────────────────────────────────────────────

    let router   = server.make_router();
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    info!(addr = %http_addr, "HTTP relay+API listening");

    axum::serve(listener, router).await?;

    Ok(())
}
