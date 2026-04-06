//! zona-p2p HTTP Proxy Server.
//!
//! A P2P node that accepts `HTTP_PROXY:` packets from the overlay network,
//! performs the actual outbound HTTP/HTTPS request, and returns the response
//! back through the relay chain.
//!
//! # Usage
//!
//! ```text
//! proxy-server \
//!   --url  http://this-host:8801 \   # own URL visible to other nodes
//!   --listen 0.0.0.0:8801 \          # local bind address
//!   --nodes http://seed1,http://seed2
//! ```
//!
//! On startup the proxy prints its NodeId (64 hex chars).
//! Pass that NodeId to `zona-curl --proxy <id>`.

use std::time::Duration;

use anyhow::Context as _;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use zona_p2p_crypto::NodeKeypair;
use zona_p2p_server::{AppPacket, NodeServer, PrefixHandler};

// ── Wire types shared with zona-curl ─────────────────────────────────────────

/// Prefix that marks a packet as an HTTP proxy request.
pub const PROXY_PREFIX: &[u8] = b"HTTP_PROXY:";

/// HTTP proxy request — serialised as JSON, prefixed with `HTTP_PROXY:`.
#[derive(Debug, Clone, Deserialize)]
pub struct HttpProxyRequest {
    /// HTTP method (GET, POST, …).
    pub method:  String,
    /// Full URL including scheme (http:// or https://).
    pub url:     String,
    /// Request headers: `[[name, value], …]`.
    #[serde(default)]
    pub headers: Vec<[String; 2]>,
    /// Request body — standard base64 encoded, or `null` for no body.
    pub body:    Option<String>,
}

/// HTTP proxy response — serialised as JSON in `RelayResponse.response`.
#[derive(Debug, Serialize)]
pub struct HttpProxyResponse {
    pub status:  u16,
    pub headers: Vec<[String; 2]>,
    /// Response body — standard base64 encoded.
    pub body:    String,
    /// Set when a proxy-level error occurs (connection refused, timeout, …).
    pub error:   Option<String>,
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name    = "proxy-server",
    about   = "zona-p2p HTTP proxy — routes HTTP/HTTPS requests from the overlay to the internet"
)]
struct Args {
    /// Own HTTP URL visible to other P2P nodes (e.g. http://proxy.example.com:8801).
    #[arg(long, env = "PROXY_URL")]
    url: String,

    /// Local bind address (e.g. 0.0.0.0:8801).
    #[arg(long, env = "PROXY_LISTEN", default_value = "0.0.0.0:8801")]
    listen: String,

    /// Bootstrap P2P node URLs (comma-separated or repeated).
    #[arg(long = "nodes", value_delimiter = ',', env = "PROXY_NODES")]
    nodes: Vec<String>,

    /// Timeout for outbound HTTP requests (seconds).
    #[arg(long, default_value = "30")]
    upstream_timeout_secs: u64,
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "proxy_server=info,zona_p2p=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    let keypair = NodeKeypair::generate();
    let node_id_hex = hex::encode(keypair.node_id.as_bytes());
    // Machine-readable line for Docker/scripts (same idea as hello-server SERVER_NODE_ID).
    println!("PROXY_NODE_ID={node_id_hex}");

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  zona-p2p HTTP Proxy Server                              ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  NodeId  : {node_id_hex} ║");
    println!("║  URL     : {:<48} ║", args.url);
    println!("║  Listen  : {:<48} ║", args.listen);
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();
    println!("Pass the NodeId to zona-curl:");
    println!("  zona-curl --proxy {node_id_hex} --nodes {} <URL>", args.url);
    println!();

    let upstream_timeout = Duration::from_secs(args.upstream_timeout_secs);

    let mut server = NodeServer::new(&args.url, keypair);

    // Register the HTTP proxy handler.
    server.register(PrefixHandler::new(PROXY_PREFIX, move |packet: AppPacket| {
        let json_bytes = packet.payload.get(PROXY_PREFIX.len()..)?;

        let req: HttpProxyRequest = match serde_json::from_slice(json_bytes) {
            Ok(r) => r,
            Err(e) => {
                warn!("proxy: bad request JSON: {e}");
                let r = make_error(400, format!("parse error: {e}"));
                return Some(serde_json::to_vec(&r).unwrap());
            }
        };

        info!(method = %req.method, url = %req.url, "proxy: forwarding request");

        // Run the blocking HTTP request outside the tokio thread pool.
        let resp = tokio::task::block_in_place(|| do_upstream_request(&req, upstream_timeout));
        Some(serde_json::to_vec(&resp).expect("response serialisation is infallible"))
    }));

    // Bootstrap into the P2P network.
    if !args.nodes.is_empty() {
        server.bootstrap(args.nodes).await.context("bootstrap failed")?;
    } else {
        info!("no bootstrap nodes provided — waiting for peers to connect");
    }

    let router   = server.make_router();
    let listener = tokio::net::TcpListener::bind(&args.listen)
        .await
        .with_context(|| format!("bind {}", args.listen))?;

    info!(listen = %args.listen, "proxy server ready");
    axum::serve(listener, router).await?;
    Ok(())
}

// ── Upstream HTTP request ─────────────────────────────────────────────────────

fn do_upstream_request(req: &HttpProxyRequest, timeout: Duration) -> HttpProxyResponse {
    let connect_timeout = Duration::from_secs(15).min(timeout);
    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .user_agent("zona-p2p-proxy/1.0")
        .build()
    {
        Ok(c) => c,
        Err(e) => return make_error(500, format!("build client: {e}")),
    };

    let method = match reqwest::Method::from_bytes(req.method.to_uppercase().as_bytes()) {
        Ok(m) => m,
        Err(_) => return make_error(400, format!("invalid method: {}", req.method)),
    };

    let mut builder = client.request(method, &req.url);

    for [name, value] in &req.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    if let Some(body_b64) = &req.body {
        match B64.decode(body_b64) {
            Ok(bytes) => { builder = builder.body(bytes); }
            Err(e)    => return make_error(400, format!("bad body base64: {e}")),
        }
    }

    match builder.send() {
        Err(e) => {
            warn!(url = %req.url, "upstream error: {e:#}");
            make_error(502, format!("upstream: {e:#}"))
        }
        Ok(resp) => {
            let status = resp.status().as_u16();
            let headers: Vec<[String; 2]> = resp
                .headers()
                .iter()
                .map(|(k, v)| [k.to_string(), v.to_str().unwrap_or("").to_string()])
                .collect();

            match resp.bytes() {
                Ok(bytes) => {
                    info!(url = %req.url, status, body_len = bytes.len(), "proxy: upstream ok");
                    HttpProxyResponse {
                        status,
                        headers,
                        body:  B64.encode(&bytes),
                        error: None,
                    }
                }
                Err(e) => {
                    warn!(url = %req.url, "read body: {e}");
                    HttpProxyResponse { status, headers, body: String::new(), error: Some(format!("read body: {e}")) }
                }
            }
        }
    }
}

fn make_error(status: u16, msg: impl Into<String>) -> HttpProxyResponse {
    HttpProxyResponse {
        status,
        headers: vec![],
        body:    String::new(),
        error:   Some(msg.into()),
    }
}
