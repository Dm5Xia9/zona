//! P2P bootstrap, DNS lookup, and HTTP proxy fetch (blocking).

use std::time::{Duration, Instant};

use anyhow::{bail, Context as _};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

// ── Proxy (proxy-server) ──────────────────────────────────────────────────────

pub const PROXY_PREFIX: &str = "HTTP_PROXY:";

#[derive(Serialize)]
pub struct HttpProxyRequest<'a> {
    pub method:  &'a str,
    pub url:     &'a str,
    pub headers: Vec<[String; 2]>,
    pub body:    Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HttpProxyResponse {
    pub status:  u16,
    pub headers: Vec<[String; 2]>,
    pub body:    String,
    pub error:   Option<String>,
}

// ── DNS (zona-dns) ────────────────────────────────────────────────────────────

pub const DNS_PREFIX: &str = "ZONA_DNS:";

#[derive(Serialize)]
pub struct DnsQueryPayload<'a> {
    pub domain: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct DnsLookupResponse {
    pub domain: String,
    pub nodes:  Vec<String>,
}

// ── Relay / introduce ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct IntroduceRequest {
    pub node_id:       String,
    pub admin_url:     String,
    pub register_peer: bool,
}

#[derive(Deserialize)]
pub struct IntroduceResponse {
    pub node_id:   String,
    pub neighbors: Vec<NeighborInfo>,
}

#[derive(Deserialize)]
pub struct NeighborInfo {
    pub node_id:   String,
    pub admin_url: String,
}

#[derive(Serialize)]
pub struct RelayRequest {
    pub to_id:           String,
    pub text:            String,
    pub from_client:     String,
    pub path:            Vec<String>,
    pub all_nodes:       Vec<String>,
    pub return_node_url: Option<String>,
    pub correlation_id:  Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct RelayResponse {
    pub delivered: bool,
    pub path:      Vec<String>,
    pub error:     Option<String>,
    #[serde(default)]
    pub response:  Option<String>,
}

pub fn bootstrap_peers(
    client:   &Client,
    self_id:  &str,
    seeds:    &[String],
    verbose:  bool,
    silent:   bool,
) -> anyhow::Result<Vec<(String, String)>> {
    let introduce_body = IntroduceRequest {
        node_id:       self_id.to_string(),
        admin_url:     "http://zona-curl-ephemeral".to_string(),
        register_peer: false,
    };

    let mut peers: Vec<(String, String)> = Vec::new();
    'bootstrap: for seed_url in seeds {
        if verbose {
            eprintln!("[zona-curl] bootstrap → {seed_url}");
        }
        let url = format!("{seed_url}/api/introduce");
        match client.post(&url).json(&introduce_body).send() {
            Err(e) => {
                if !silent { eprintln!("[zona-curl] {seed_url}: {e}"); }
            }
            Ok(resp) => {
                match resp.json::<IntroduceResponse>() {
                    Err(e) => {
                        if !silent { eprintln!("[zona-curl] {seed_url}: bad introduce response: {e}"); }
                    }
                    Ok(intro) => {
                        if verbose {
                            eprintln!(
                                "[zona-curl] connected to {}… ({} neighbors)",
                                &intro.node_id[..8.min(intro.node_id.len())],
                                intro.neighbors.len()
                            );
                        }
                        peers.push((intro.node_id, seed_url.clone()));
                        for nb in intro.neighbors {
                            peers.push((nb.node_id, nb.admin_url));
                        }
                        break 'bootstrap;
                    }
                }
            }
        }
    }

    if peers.is_empty() {
        bail!("failed to connect to any P2P bootstrap node");
    }
    Ok(peers)
}

/// Relay `text` to `to_id_hex`, return parsed JSON response string from `RelayResponse.response`.
pub fn relay_sync(
    client:     &Client,
    peers:      &[(String, String)],
    to_id_hex:  &str,
    payload:    String,
    self_id:    &str,
    verbose:    bool,
    silent:     bool,
) -> anyhow::Result<RelayResponse> {
    let relay_req = RelayRequest {
        to_id:           to_id_hex.to_string(),
        text:            payload,
        from_client:     self_id.to_string(),
        path:            vec![self_id.to_string()],
        all_nodes:       Vec::new(),
        return_node_url: None,
        correlation_id:  None,
    };

    let t1 = Instant::now();
    for (peer_id, peer_url) in peers {
        if verbose {
            eprintln!("[zona-curl] relay via {}…", &peer_id[..8.min(peer_id.len())]);
        }
        let url = format!("{peer_url}/api/relay");
        match client.post(&url).json(&relay_req).send() {
            Err(e) => {
                if !silent { eprintln!("[zona-curl] relay via {peer_url}: {e}"); }
            }
            Ok(resp) => match resp.json::<RelayResponse>() {
                Err(e) => {
                    if !silent { eprintln!("[zona-curl] bad relay response from {peer_url}: {e}"); }
                }
                Ok(r) => {
                    if verbose {
                        let path_str: Vec<&str> = r.path.iter()
                            .map(|s| &s[..8.min(s.len())])
                            .collect();
                        eprintln!(
                            "[zona-curl] path  : {} ({:.0}ms)",
                            path_str.join(" → "),
                            t1.elapsed().as_secs_f64() * 1000.0,
                        );
                    }
                    return Ok(r);
                }
            },
        }
    }
    bail!("all peers failed to relay the request")
}

pub fn dns_lookup_domain(
    client:     &Client,
    peers:      &[(String, String)],
    dns_node:   &str,
    domain:     &str,
    self_id:    &str,
    verbose:    bool,
    silent:     bool,
) -> anyhow::Result<DnsLookupResponse> {
    let payload = format!(
        "{}{}",
        DNS_PREFIX,
        serde_json::to_string(&DnsQueryPayload { domain })?
    );
    let relay = relay_sync(client, peers, dns_node, payload, self_id, verbose, silent)?;
    if !relay.delivered {
        bail!(
            "DNS relay not delivered: {:?} {}",
            relay.path,
            relay.error.unwrap_or_default()
        );
    }
    let text = relay.response.ok_or_else(|| anyhow::anyhow!("DNS returned empty response"))?;
    let trimmed = text.trim();
    serde_json::from_str(trimmed).context("parse DnsLookupResponse")
}

/// Validate 64-hex NodeId string.
pub fn validate_hex_node_id(s: &str) -> anyhow::Result<()> {
    let b = hex::decode(s).context("not valid hex")?;
    if b.len() != 32 {
        bail!("NodeId must be 64 hex chars (32 bytes)");
    }
    Ok(())
}

/// Build proxy payload and relay; return decoded body bytes and status/headers for printing.
pub fn fetch_via_proxy(
    client:      &Client,
    peers:       &[(String, String)],
    proxy_node:  &str,
    method:      &str,
    url:         &str,
    headers:     Vec<[String; 2]>,
    body_b64:    Option<String>,
    self_id:     &str,
    verbose:     bool,
    silent:      bool,
) -> anyhow::Result<(HttpProxyResponse, Vec<u8>)> {
    let proxy_req = HttpProxyRequest {
        method:  &method.to_uppercase(),
        url,
        headers,
        body:    body_b64,
    };
    let payload = format!(
        "{}{}",
        PROXY_PREFIX,
        serde_json::to_string(&proxy_req)?
    );

    let relay = relay_sync(client, peers, proxy_node, payload, self_id, verbose, silent)?;
    if !relay.delivered {
        bail!(
            "packet not delivered (path: {:?}): {}",
            relay.path,
            relay.error.unwrap_or_default()
        );
    }

    let response_text = relay.response
        .ok_or_else(|| anyhow::anyhow!("proxy returned no response — wrong NodeId?"))?;

    let mut trimmed = response_text.trim();
    if let Some(s) = trimmed.strip_prefix('\u{FEFF}') {
        trimmed = s.trim();
    }
    if trimmed.is_empty() {
        bail!("empty proxy response");
    }

    let proxy_resp: HttpProxyResponse = serde_json::from_str(trimmed).map_err(|e| {
        let preview: String = trimmed.chars().take(160).collect();
        anyhow::anyhow!("parse HttpProxyResponse: {e} (preview={preview:?})")
    })?;

    if let Some(ref err) = proxy_resp.error {
        bail!("proxy error (status {}): {err}", proxy_resp.status);
    }

    let body_bytes = B64.decode(&proxy_resp.body).context("decode response body base64")?;
    Ok((proxy_resp, body_bytes))
}

pub fn build_http_client(timeout: Duration) -> anyhow::Result<Client> {
    Client::builder().timeout(timeout).build().context("build reqwest client")
}
