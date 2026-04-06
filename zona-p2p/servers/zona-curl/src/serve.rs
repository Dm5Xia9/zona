//! Local HTTP server: browser → Host header → P2P proxy (zona-dns optional).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode, Uri},
    response::Response,
    routing::any,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use clap::Parser;
use rcgen::generate_simple_self_signed;

use crate::p2p::{self, HttpProxyResponse};

#[derive(Parser, Debug)]
pub struct ServeArgs {
    /// Listen address for the local HTTP server (use with Windows hosts file).
    #[arg(long = "listen", default_value = "127.0.0.1:8765")]
    pub listen: String,

    /// Accept HTTPS (TLS) on --listen. Without --tls-cert/--tls-key, a self-signed cert is generated (see --tls-san).
    #[arg(long = "https", default_value_t = false)]
    pub https: bool,

    /// PEM certificate (full chain). Use with --tls-key and --https.
    #[arg(long = "tls-cert")]
    pub tls_cert: Option<PathBuf>,

    /// PEM private key. Use with --tls-cert and --https.
    #[arg(long = "tls-key")]
    pub tls_key: Option<PathBuf>,

    /// Subject Alternative Names for the generated self-signed cert (--https without PEM files). Repeat or use commas.
    #[arg(long = "tls-san", value_delimiter = ',')]
    pub tls_san: Vec<String>,

    #[arg(long = "nodes", value_delimiter = ',', required = true)]
    pub nodes: Vec<String>,

    /// zona-dns NodeId (64 hex). Resolves Host → proxy NodeIds from YAML zones.
    #[arg(long = "dns-node")]
    pub dns_node: Option<String>,

    /// proxy-server NodeId (64 hex). If set, overrides DNS for every request.
    #[arg(long = "proxy")]
    pub proxy_node: Option<String>,

    /// Upstream scheme for requests sent through proxy-server to the internet.
    #[arg(long = "scheme", default_value = "https")]
    pub scheme: String,

    #[arg(long = "timeout-ms", default_value_t = 60_000)]
    pub timeout_ms: u64,

    #[arg(short = 'v', long = "verbose", default_value_t = false)]
    pub verbose: bool,

    #[arg(short = 's', long = "silent", default_value_t = false)]
    pub silent: bool,
}

pub struct ServeState {
    pub client:     reqwest::blocking::Client,
    pub peers:      Vec<(String, String)>,
    pub self_id:    String,
    pub dns_node:   Option<String>,
    pub proxy_node: Option<String>,
    pub scheme:     String,
    pub verbose:    bool,
    pub silent:     bool,
}

pub async fn run_serve(args: ServeArgs) -> anyhow::Result<()> {
    if args.scheme != "http" && args.scheme != "https" {
        anyhow::bail!("--scheme must be http or https");
    }

    if args.proxy_node.is_none() && args.dns_node.is_none() {
        anyhow::bail!("set --proxy and/or --dns-node");
    }
    if let Some(ref p) = args.proxy_node {
        p2p::validate_hex_node_id(p)?;
    }
    if let Some(ref d) = args.dns_node {
        p2p::validate_hex_node_id(d)?;
    }

    if (args.tls_cert.is_some() || args.tls_key.is_some()) && !args.https {
        anyhow::bail!("--tls-cert / --tls-key require --https");
    }
    if (args.tls_cert.is_some()) != (args.tls_key.is_some()) {
        anyhow::bail!("set both --tls-cert and --tls-key, or neither for a generated self-signed cert");
    }

    let timeout = Duration::from_millis(args.timeout_ms);
    let client = p2p::build_http_client(timeout)?;

    let keypair = zona_p2p_crypto::NodeKeypair::generate();
    let self_id = hex::encode(keypair.node_id.as_bytes());

    if !args.silent {
        eprintln!("[zona-curl serve] self  : {}…", &self_id[..16]);
        eprintln!("[zona-curl serve] listen: {}", args.listen);
    }

    let peers = p2p::bootstrap_peers(&client, &self_id, &args.nodes, args.verbose, args.silent)?;
    if !args.silent {
        eprintln!("[zona-curl serve] bootstrap ok — {} peers", peers.len());
    }

    let state = Arc::new(ServeState {
        client,
        peers,
        self_id,
        dns_node:   args.dns_node,
        proxy_node: args.proxy_node,
        scheme:     args.scheme,
        verbose:    args.verbose,
        silent:     args.silent,
    });

    let app = Router::new()
        .fallback(any(handler))
        .with_state(state);

    let addr: SocketAddr = args
        .listen
        .parse()
        .with_context(|| format!("invalid --listen (expected host:port), got {}", args.listen))?;

    if args.https {
        let tls = rustls_config_for_serve(&args).await?;
        if !args.silent {
            eprintln!(
                "[zona-curl serve] HTTPS on {} — e.g. hosts: `127.0.0.1 example.com` then https://example.com:{} (self-signed: trust cert or use mkcert + --tls-cert/--tls-key)",
                addr,
                port_part(&args.listen).unwrap_or("8765")
            );
        }
        axum_server::bind_rustls(addr, tls)
            .serve(app.into_make_service())
            .await
            .context("HTTPS server")?;
    } else {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("bind {}", args.listen))?;
        if !args.silent {
            eprintln!(
                "[zona-curl serve] ready — e.g. hosts: `127.0.0.1 example.com` then http://example.com:{}",
                port_part(&args.listen).unwrap_or("8765")
            );
        }
        axum::serve(listener, app).await.context("HTTP server")?;
    }
    Ok(())
}

async fn rustls_config_for_serve(args: &ServeArgs) -> anyhow::Result<RustlsConfig> {
    match (&args.tls_cert, &args.tls_key) {
        (Some(c), Some(k)) => RustlsConfig::from_pem_file(c, k)
            .await
            .map_err(|e| anyhow::anyhow!("load --tls-cert / --tls-key: {e}")),
        (None, None) => {
            let names = if args.tls_san.is_empty() {
                vec!["localhost".to_string(), "127.0.0.1".to_string()]
            } else {
                args.tls_san.clone()
            };
            let ck = generate_simple_self_signed(names).map_err(|e| anyhow::anyhow!("rcgen: {e}"))?;
            let cert_pem = ck.cert.pem();
            let key_pem = ck.key_pair.serialize_pem();
            RustlsConfig::from_pem(cert_pem.into_bytes(), key_pem.into_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("TLS config: {e}"))
        }
        _ => anyhow::bail!("internal: --tls-cert / --tls-key mismatch"),
    }
}

fn port_part(listen: &str) -> Option<&str> {
    listen.rsplit_once(':').map(|(_, p)| p)
}

async fn handler(
    State(st): State<Arc<ServeState>>,
    req: Request<Body>,
) -> Result<Response<Body>, std::convert::Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    let host = host_from_request(&headers, &uri);

    let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(b) => b.to_vec(),
        Err(e) => return Ok(err_response(StatusCode::BAD_REQUEST, &format!("read body: {e}"))),
    };

    let st2 = Arc::clone(&st);
    let res = tokio::task::spawn_blocking(move || {
        serve_one_bytes(&st2, &host, &uri, &method, &headers, &body_bytes)
    })
    .await;

    match res {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => Ok(err_response(StatusCode::BAD_GATEWAY, &e.to_string())),
        Err(e) => Ok(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("task: {e}"),
        )),
    }
}

fn host_from_request(headers: &HeaderMap, uri: &Uri) -> String {
    if let Some(h) = headers.get(axum::http::header::HOST).and_then(|v| v.to_str().ok()) {
        return h.split(':').next().unwrap_or(h).to_string();
    }
    if let Some(h) = uri.host() {
        return h.to_string();
    }
    "localhost".into()
}

fn serve_one_bytes(
    st:          &ServeState,
    domain:      &str,
    uri:         &Uri,
    method:      &Method,
    headers:     &HeaderMap,
    body_bytes:  &[u8],
) -> anyhow::Result<Response<Body>> {
    let mut proxy_hex = st.proxy_node.clone();
    if let Some(ref dns) = st.dns_node {
        let r = p2p::dns_lookup_domain(
            &st.client, &st.peers, dns, domain, &st.self_id, st.verbose, st.silent,
        )?;
        if proxy_hex.is_none() && !r.nodes.is_empty() {
            proxy_hex = Some(r.nodes[0].clone());
        }
    }
    let proxy = proxy_hex.ok_or_else(|| {
        anyhow::anyhow!("no proxy for host {domain} — set --proxy or a DNS zone for this domain")
    })?;
    p2p::validate_hex_node_id(&proxy)?;

    let pq = uri.path_and_query().map(|x| x.as_str()).unwrap_or("/");
    let upstream_url = format!("{}://{}{}", st.scheme, domain, pq);

    let mut out_h: Vec<[String; 2]> = Vec::new();
    for (k, v) in headers.iter() {
        let name = k.as_str();
        if name.eq_ignore_ascii_case("host")
            || name.eq_ignore_ascii_case("connection")
            || name.eq_ignore_ascii_case("proxy-connection")
            || name.eq_ignore_ascii_case("keep-alive")
            || name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("te")
            || name.eq_ignore_ascii_case("upgrade")
            || name.eq_ignore_ascii_case("proxy-authorization")
        {
            continue;
        }
        if let Ok(s) = v.to_str() {
            out_h.push([name.to_string(), s.to_string()]);
        }
    }

    let body_b64 = if body_bytes.is_empty() {
        None
    } else {
        Some(B64.encode(body_bytes))
    };

    let (proxy_resp, body_out) = p2p::fetch_via_proxy(
        &st.client,
        &st.peers,
        &proxy,
        method.as_str(),
        &upstream_url,
        out_h,
        body_b64,
        &st.self_id,
        st.verbose,
        st.silent,
    )?;

    Ok(build_axum_response(&proxy_resp, body_out)?)
}

fn build_axum_response(pr: &HttpProxyResponse, body: Vec<u8>) -> anyhow::Result<Response<Body>> {
    let mut b = Response::builder().status(pr.status);
    for [k, v] in &pr.headers {
        if k.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        b = b.header(k, v);
    }
    Ok(b.body(Body::from(body))?)
}

fn err_response(code: StatusCode, msg: &str) -> Response<Body> {
    Response::builder()
        .status(code)
        .header(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(msg.to_string()))
        .unwrap()
}
