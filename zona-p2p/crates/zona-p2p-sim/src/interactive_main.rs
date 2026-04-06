//! Entry point for the `sim-interactive` binary.
//!
//! Usage:
//!   sim-interactive [<n>]                              — sandbox mode, n nodes (default 8)
//!   sim-interactive --docker [--nodes <n>] [--base-port <p>] [--client <name>]

use zona_p2p_sim::{backend_sandbox::SandboxBackend, backend_docker::DockerBackend};
use zona_p2p_client::interactive::InteractiveRepl;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("zona_p2p_sim=info".parse().unwrap())
        )
        .init();

    let raw: Vec<String> = std::env::args().skip(1).collect();

    let mut docker                   = false;
    let mut nodes_arg: Option<usize> = None;
    let mut base_port: u16           = 17701;
    let mut client_name              = "me".to_string();

    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--docker"               => { docker = true; }
            "--nodes" | "-n" | "-N" => {
                i += 1;
                nodes_arg = raw.get(i).and_then(|s| s.parse().ok());
            }
            "--base-port" => {
                i += 1;
                if let Some(p) = raw.get(i).and_then(|s| s.parse().ok()) { base_port = p; }
            }
            "--client" => {
                i += 1;
                if let Some(name) = raw.get(i) { client_name = name.clone(); }
            }
            other => {
                if let Ok(n) = other.parse::<usize>() { nodes_arg = Some(n); }
            }
        }
        i += 1;
    }

    if docker {
        let n = nodes_arg.unwrap_or(6);
        let admin_urls: Vec<String> = (0..n)
            .map(|i| format!("http://localhost:{}", base_port + i as u16))
            .collect();

        println!("Connecting to Docker nodes...");
        for url in &admin_urls { println!("  {url}"); }

        let backend = match DockerBackend::connect(admin_urls) {
            Ok(b)  => b,
            Err(e) => {
                eprintln!("Failed to connect: {e}");
                eprintln!("Make sure containers are running: ./scripts/docker-p2p.ps1 -N {n}");
                std::process::exit(1);
            }
        };

        InteractiveRepl::new(Box::new(backend), client_name).run();
    } else {
        let n = nodes_arg.unwrap_or(8);
        println!("Bootstrapping sandbox with {n} nodes...");
        let backend = SandboxBackend::bootstrap(n);
        InteractiveRepl::new(Box::new(backend), client_name).run();
    }
}
