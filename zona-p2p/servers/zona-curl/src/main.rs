//! zona-curl — curl-like CLI over zona-p2p; `serve` mode for local HTTP + hosts file.
//!
//! Invoke: `zona-curl <URL> ...` or `zona-curl fetch <URL> ...` or `zona-curl serve ...`

mod p2p;
mod serve;

use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _};
use clap::Parser;
use url::Url;

use crate::p2p::HttpProxyResponse;

#[derive(Parser, Debug)]
#[command(name = "zona-curl")]
struct FetchArgs {
    /// URL to fetch (http:// or https://).
    url: String,

    #[arg(long = "nodes", value_delimiter = ',', required = true)]
    nodes: Vec<String>,

    /// proxy-server NodeId (64 hex). Optional if `--dns-node` resolves this host to a proxy.
    #[arg(long = "proxy")]
    proxy_node: Option<String>,

    /// zona-dns NodeId (64 hex). Look up URL host in DNS zones → first NodeId used as proxy if `--proxy` omitted.
    #[arg(long = "dns-node")]
    dns_node: Option<String>,

    #[arg(short = 'X', long = "method", default_value = "GET")]
    method: String,

    #[arg(short = 'H', long = "header")]
    headers: Vec<String>,

    #[arg(short = 'd', long = "data")]
    data: Option<String>,

    #[arg(short = 'i', long = "include", default_value_t = false)]
    include: bool,

    #[arg(short = 'v', long = "verbose", default_value_t = false)]
    verbose: bool,

    #[arg(short = 's', long = "silent", default_value_t = false)]
    silent: bool,

    #[arg(short = 'o', long = "output")]
    output: Option<String>,

    #[arg(long = "timeout-ms", default_value_t = 30_000)]
    timeout_ms: u64,
}

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "serve" {
        args.remove(1);
        let s = serve::ServeArgs::try_parse_from(args)?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        return rt.block_on(serve::run_serve(s));
    }

    if args.len() >= 2 && args[1] != "fetch" {
        args.insert(1, "fetch".into());
    }

    let args = FetchArgs::try_parse_from(args)?;
    run_fetch(args)
}

fn run_fetch(args: FetchArgs) -> anyhow::Result<()> {
    if args.proxy_node.is_none() && args.dns_node.is_none() {
        bail!("set --proxy and/or --dns-node");
    }
    if let Some(ref p) = args.proxy_node {
        p2p::validate_hex_node_id(p)?;
    }
    if let Some(ref d) = args.dns_node {
        p2p::validate_hex_node_id(d)?;
    }

    let u = Url::parse(&args.url).context("invalid URL")?;
    let host = u.host_str().ok_or_else(|| anyhow::anyhow!("URL has no host"))?;

    let timeout = Duration::from_millis(args.timeout_ms);
    let client = p2p::build_http_client(timeout)?;

    let keypair = zona_p2p_crypto::NodeKeypair::generate();
    let self_id = hex::encode(keypair.node_id.as_bytes());

    if !args.silent {
        eprintln!("[zona-curl] self  : {}…", &self_id[..16]);
        if let Some(ref p) = args.proxy_node {
            eprintln!("[zona-curl] proxy : {}…", &p[..16.min(p.len())]);
        }
        if let Some(ref d) = args.dns_node {
            eprintln!("[zona-curl] dns   : {}…", &d[..16.min(d.len())]);
        }
        eprintln!("[zona-curl] url   : {}", args.url);
    }

    let t0 = Instant::now();

    let peers = p2p::bootstrap_peers(&client, &self_id, &args.nodes, args.verbose, args.silent)?;
    if !args.silent {
        eprintln!(
            "[zona-curl] bootstrap ok — {} peers ({:.0}ms)",
            peers.len(),
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }

    let mut proxy_hex = args.proxy_node.clone();
    if let Some(ref dns) = args.dns_node {
        let r = p2p::dns_lookup_domain(
            &client, &peers, dns, host, &self_id, args.verbose, args.silent,
        )?;
        if !args.silent {
            eprintln!(
                "[zona-curl] DNS {} → {} node(s)",
                r.domain,
                r.nodes.len()
            );
        }
        if proxy_hex.is_none() && !r.nodes.is_empty() {
            proxy_hex = Some(r.nodes[0].clone());
        }
    }

    let proxy = proxy_hex.ok_or_else(|| {
        anyhow::anyhow!("no proxy — set --proxy or a zona-dns zone for host {host}")
    })?;
    p2p::validate_hex_node_id(&proxy)?;

    let mut req_headers: Vec<[String; 2]> = Vec::new();
    for h in &args.headers {
        if let Some((name, value)) = h.split_once(':') {
            req_headers.push([name.trim().to_string(), value.trim().to_string()]);
        } else {
            bail!("bad header format (expected \"Name: Value\"): {h}");
        }
    }

    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let body_b64: Option<String> = match &args.data {
        None => None,
        Some(d) if d.starts_with('@') => {
            let path = &d[1..];
            let bytes = std::fs::read(path).with_context(|| format!("read {path}"))?;
            Some(B64.encode(bytes))
        }
        Some(d) => Some(B64.encode(d.as_bytes())),
    };

    let (proxy_resp, body_bytes) = p2p::fetch_via_proxy(
        &client,
        &peers,
        &proxy,
        &args.method,
        &args.url,
        req_headers,
        body_b64,
        &self_id,
        args.verbose,
        args.silent,
    )?;

    print_fetch_output(&args, &proxy_resp, &body_bytes, t0)?;
    Ok(())
}

fn print_fetch_output(
    args:     &FetchArgs,
    proxy_resp: &HttpProxyResponse,
    body_bytes: &[u8],
    t0:       Instant,
) -> anyhow::Result<()> {
    if args.include {
        println!("HTTP/1.1 {}", proxy_resp.status);
        for [name, value] in &proxy_resp.headers {
            println!("{name}: {value}");
        }
        println!();
    } else if args.verbose {
        eprintln!("[zona-curl] status : {}", proxy_resp.status);
        for [name, value] in &proxy_resp.headers {
            eprintln!("<  {name}: {value}");
        }
    }

    match &args.output {
        Some(path) => {
            std::fs::write(path, body_bytes).with_context(|| format!("write {path}"))?;
            if !args.silent {
                eprintln!("[zona-curl] saved {} bytes to {path}", body_bytes.len());
            }
        }
        None => {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(body_bytes).context("write stdout")?;
            if body_bytes.last().copied() != Some(b'\n') {
                let _ = stdout.write_all(b"\n");
            }
        }
    }

    if !args.silent {
        eprintln!(
            "[zona-curl] done — {} bytes in {:.0}ms",
            body_bytes.len(),
            t0.elapsed().as_secs_f64() * 1000.0,
        );
    }

    if proxy_resp.status >= 400 {
        std::process::exit(22);
    }
    Ok(())
}
