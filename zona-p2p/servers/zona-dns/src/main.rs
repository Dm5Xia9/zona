//! zona-dns — P2P node that maps domain names to NodeId lists (YAML), for zona-curl routing.
//!
//! Wire format (relay, sync): `ZONA_DNS:` + JSON `{"domain":"example.com"}`
//! Response JSON: `{"domain":"example.com","nodes":["64hex",...]}`
//!
//! HTTP: `GET /api/dns/lookup?domain=example.com`

use std::{collections::HashMap, sync::Arc};

use anyhow::Context as _;
use axum::{
    Json,
    extract::{Query, State},
    routing::get,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use zona_p2p_crypto::NodeKeypair;
use zona_p2p_server::{AppPacket, NodeServer, PrefixHandler};

pub const DNS_PREFIX: &[u8] = b"ZONA_DNS:";

#[derive(Debug, Deserialize)]
struct DnsQueryPayload {
    domain: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DnsLookupResponse {
    domain: String,
    nodes:  Vec<String>,
}

#[derive(Deserialize)]
struct LookupQuery {
    domain: String,
}

type ZoneTable = HashMap<String, Vec<String>>;

#[derive(Clone)]
struct AppState {
    zones: Arc<ZoneTable>,
}

#[derive(Parser, Debug)]
#[command(name = "zona-dns", about = "zona-p2p domain → NodeId list (YAML zones)")]
struct Args {
    #[arg(long, env = "ZONA_DNS_URL")]
    url: String,

    #[arg(long, env = "ZONA_DNS_LISTEN", default_value = "0.0.0.0:8811")]
    listen: String,

    /// Optional second bind for `GET /api/dns/lookup` (same process; avoids axum state clash with relay router).
    #[arg(long = "http-listen", env = "ZONA_DNS_HTTP_LISTEN", default_value = "127.0.0.1:8822")]
    http_listen: String,

    #[arg(long = "nodes", value_delimiter = ',', env = "ZONA_DNS_NODES")]
    nodes: Vec<String>,

    /// Path to YAML: domain -> list of 64-hex NodeIds
    #[arg(long = "zones", env = "ZONA_DNS_ZONES")]
    zones_file: String,
}

fn normalize_domain(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

fn load_zones(path: &str) -> anyhow::Result<ZoneTable> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    let loaded: HashMap<String, Vec<String>> = serde_yaml::from_str(&raw)
        .with_context(|| format!("parse YAML {path}"))?;
    let mut out = ZoneTable::new();
    for (k, v) in loaded {
        let dom = normalize_domain(&k);
        let mut ids = Vec::new();
        for h in v {
            let h = h.trim();
            if h.len() != 64 {
                warn!(domain = %dom, id = %h, "skip invalid hex (expected 64 chars)");
                continue;
            }
            if hex::decode(h).ok().filter(|b| b.len() == 32).is_some() {
                ids.push(h.to_ascii_lowercase());
            } else {
                warn!(domain = %dom, id = %h, "skip invalid hex");
            }
        }
        out.insert(dom, ids);
    }
    Ok(out)
}

fn lookup(zones: &ZoneTable, domain: &str) -> DnsLookupResponse {
    let dom = normalize_domain(domain);
    let nodes = zones.get(&dom).cloned().unwrap_or_default();
    DnsLookupResponse { domain: dom, nodes }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zona_dns=info,zona_p2p_server=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();
    let zones = Arc::new(load_zones(&args.zones_file)?);
    info!(loaded = zones.len(), path = %args.zones_file, "zones loaded");

    let keypair = NodeKeypair::generate();
    let node_id_hex = hex::encode(keypair.node_id.as_bytes());
    println!("ZONA_DNS_NODE_ID={node_id_hex}");
    println!("zona-dns URL={} listen={}", args.url, args.listen);

    let state = AppState { zones: Arc::clone(&zones) };
    let zones_for_handler = Arc::clone(&zones);

    let mut server = NodeServer::new(&args.url, keypair);
    server.register(PrefixHandler::new(DNS_PREFIX, move |packet: AppPacket| {
        let json_bytes = packet.payload.get(DNS_PREFIX.len()..)?;
        let q: DnsQueryPayload = serde_json::from_slice(json_bytes).ok()?;
        let r = lookup(&zones_for_handler, &q.domain);
        Some(serde_json::to_vec(&r).expect("dns response json"))
    }));

    if !args.nodes.is_empty() {
        server.bootstrap(args.nodes).await.context("bootstrap")?;
    }

    let router = server.make_router();

    let listener = tokio::net::TcpListener::bind(&args.listen)
        .await
        .with_context(|| format!("bind {}", args.listen))?;
    info!(listen = %args.listen, "zona-dns P2P HTTP ready");

    let http_router = axum::Router::new()
        .route("/api/dns/lookup", get(http_lookup))
        .with_state(state);

    let http_bind = args.http_listen.clone();
    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(&http_bind).await {
            Ok(l) => {
                info!(listen = %http_bind, "zona-dns HTTP lookup API");
                if let Err(e) = axum::serve(l, http_router).await {
                    tracing::error!("http listener: {e}");
                }
            }
            Err(e) => warn!(listen = %http_bind, "skip HTTP API: {e}"),
        }
    });

    axum::serve(listener, router).await?;
    Ok(())
}

async fn http_lookup(State(st): State<AppState>, Query(q): Query<LookupQuery>) -> Json<DnsLookupResponse> {
    Json(lookup(&st.zones, &q.domain))
}
