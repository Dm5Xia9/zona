//! Interactive REPL for the zona-p2p network.
//!
//! One instance = one client. The client name is set at startup and shown in
//! the prompt. All network access goes through the `AdminBackend` trait.

use std::io::{self, BufRead, Write};
use std::time::Instant;

use crate::backend::*;

// ── ANSI colour helpers ───────────────────────────────────────────────────────

const BOLD:   &str = "\x1b[1m";
const DIM:    &str = "\x1b[2m";
const GREEN:  &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN:   &str = "\x1b[36m";
const RED:    &str = "\x1b[31m";
const R:      &str = "\x1b[0m";

// ── REPL driver ───────────────────────────────────────────────────────────────

pub struct InteractiveRepl {
    backend:     Box<dyn AdminBackend>,
    client_name: String,
}

impl InteractiveRepl {
    pub fn new(backend: Box<dyn AdminBackend>, client_name: impl Into<String>) -> Self {
        InteractiveRepl { backend, client_name: client_name.into() }
    }

    pub fn run(&mut self) {
        let mode = self.backend.mode_name();
        let name = &self.client_name;
        println!("{BOLD}{CYAN}╔══════════════════════════════════════╗{R}");
        println!("{BOLD}{CYAN}║   zona-p2p interactive               ║{R}");
        println!("{BOLD}{CYAN}║   mode:   {mode:<27}  ║{R}");
        println!("{BOLD}{CYAN}║   client: {name:<27}  ║{R}");
        println!("{BOLD}{CYAN}╚══════════════════════════════════════╝{R}");
        println!("Type {BOLD}help{R} for commands.\n");

        let stdin = io::stdin();
        loop {
            print!("{BOLD}{CYAN}{}{R} p2p> ", self.client_name);
            let _ = io::stdout().flush();
            let mut line = String::new();
            if stdin.lock().read_line(&mut line).is_err() || line.is_empty() {
                break;
            }
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if !self.dispatch(line) { break; }
        }
        println!("Bye!");
    }

    fn dispatch(&mut self, line: &str) -> bool {
        let parts: Vec<&str> = line.splitn(4, ' ').collect();
        let cmd = parts[0].to_lowercase();
        match cmd.as_str() {
            "help" | "?"  => self.cmd_help(),
            "nodes"       => self.cmd_nodes(),
            "stats"       => self.cmd_stats(),
            "peers" => {
                let r = parts.get(1).copied().unwrap_or("0");
                self.cmd_peers(r);
            }
            "ping" => {
                if parts.len() < 2 {
                    println!("{RED}usage: ping <node|idx|hex>{R}");
                } else {
                    self.cmd_ping(parts[1]);
                }
            }
            "rpc" => {
                if parts.len() < 3 {
                    println!("{RED}usage: rpc <node|idx|hex> <text>{R}");
                } else {
                    let text = parts[2..].join(" ");
                    self.cmd_rpc(parts[1], &text);
                }
            }
            "inbox" => self.cmd_inbox(),
            "step" => {
                let n = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(10usize);
                self.backend.step(n);
                println!("Stepped {n} ticks.\n");
            }
            "drop" => {
                if let Some(p) = parts.get(1).and_then(|s| s.parse::<f64>().ok()) {
                    self.backend.set_drop_probability(p);
                    println!("Drop probability set to {:.0}%\n", p * 100.0);
                } else {
                    println!("Drop probability: {:.0}%\n", self.backend.drop_probability() * 100.0);
                }
            }
            "quit" | "exit" | "q" => return false,
            other => println!("{RED}Unknown command '{other}'. Type 'help'.{R}"),
        }
        true
    }

    // ── Command handlers ──────────────────────────────────────────────────────

    fn cmd_help(&self) {
        println!("{BOLD}Commands:{R}");
        let cmds = [
            ("nodes",                    "list all P2P nodes"),
            ("stats",                    "network statistics"),
            ("peers [node]",             "routing table of a node"),
            ("ping <node|idx|hex>",      "send 'ping', wait for response, show RTT"),
            ("rpc <node|idx|hex> <msg>", "send packet via P2P relay, wait for response"),
            ("inbox",                    "show messages received by this client"),
            ("step [n]",                 "advance simulation ticks (sandbox only)"),
            ("drop [p]",                 "get/set drop probability 0..1 (sandbox only)"),
            ("quit",                     "exit"),
        ];
        for (cmd, desc) in cmds {
            println!("  {CYAN}{cmd:<35}{R} {desc}");
        }
        println!();
    }

    fn cmd_nodes(&self) {
        let nodes = self.backend.node_list();
        println!("{BOLD}  Idx   ID          Slots  Status{R}");
        println!("{DIM}{}{R}", "─".repeat(42));
        for n in &nodes {
            let status = if !n.alive {
                format!("{RED}dead{R}")
            } else if n.healthy {
                format!("{GREEN}healthy{R}")
            } else {
                format!("{YELLOW}degraded{R}")
            };
            println!("  [{:>2}]  {}…  slots={:<3} {}", n.index, n.id, n.slots, status);
        }
        println!();
    }

    fn cmd_stats(&self) {
        let s = self.backend.stats();
        println!("{BOLD}Network statistics:{R}");
        println!("  Mode:       {CYAN}{}{R}", s.mode);
        println!("  Nodes:      {}/{} healthy", s.healthy, s.node_count);
        println!("  Avg slots:  {:.1}", s.avg_slots);
        if s.drop_prob > 0.0 {
            println!("  Drop prob:  {YELLOW}{:.0}%{R}", s.drop_prob * 100.0);
        }
        if s.ticks > 0 {
            println!("  Ticks:      {}", s.ticks);
        }
        println!();
    }

    fn cmd_peers(&self, node_ref: &str) {
        match self.backend.peer_table(node_ref) {
            None => println!("{RED}Node '{node_ref}' not found.{R}\n"),
            Some(slots) => {
                if slots.is_empty() {
                    println!("  (no peers in routing table)\n");
                } else {
                    println!("{BOLD}  Kind  Peer        Idx    Loss    Referrer{R}");
                    for s in &slots {
                        println!("  {CYAN}{:<5}{R} {}… [{:>3}]   {:.2}    {}…",
                            s.kind, s.peer_id, s.peer_idx, s.loss, s.referrer);
                    }
                    println!();
                }
            }
        }
    }

    fn cmd_ping(&mut self, to: &str) {
        let display = if to.len() == 64 { format!("{}…", &to[..8]) } else { to.to_string() };
        println!("{BOLD}Ping{R} → [{display}]");
        let t0 = Instant::now();
        match self.backend.rpc(to, b"ping", 5_000) {
            Ok(resp) => {
                let ms = t0.elapsed().as_millis();
                let r  = String::from_utf8_lossy(&resp);
                println!("  {GREEN}{BOLD}✓ pong{R}  response: \"{r}\"  ({ms} ms)");
            }
            Err(e) => println!("  {RED}✗ {e}{R}"),
        }
        println!();
    }

    fn cmd_rpc(&mut self, to: &str, text: &str) {
        let display = if to.len() == 64 { format!("{}…", &to[..8]) } else { to.to_string() };
        println!("{BOLD}RPC{R} → [{display}]  \"{text}\"");
        let t0 = Instant::now();
        match self.backend.rpc(to, text.as_bytes(), 5_000) {
            Ok(resp) => {
                let ms = t0.elapsed().as_millis();
                let r  = String::from_utf8(resp.clone())
                    .unwrap_or_else(|_| format!("0x{}", hex::encode(&resp)));
                println!("  {GREEN}{BOLD}✓{R}  \"{r}\"  ({ms} ms)");
            }
            Err(e) => println!("  {RED}✗ {e}{R}"),
        }
        println!();
    }

    fn cmd_inbox(&self) {
        let msgs = self.backend.inbox();
        if msgs.is_empty() {
            println!("  Inbox [{CYAN}{}{R}] is empty.\n", self.client_name);
        } else {
            println!("{BOLD}Inbox [{CYAN}{}{R}{BOLD}]:{R}", self.client_name);
            for (i, m) in msgs.iter().enumerate() {
                println!("  #{i}  from={CYAN}{}{R}  \"{}\",", m.from, m.text);
            }
            println!();
        }
    }
}
