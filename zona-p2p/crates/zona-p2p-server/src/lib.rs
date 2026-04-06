//! `zona-p2p-server` — framework for building application servers on the P2P overlay.
//!
//! A `NodeServer` is a full P2P node (using HTTP relay transport, same as
//! `zona-p2p-node`) with registered application packet handlers. Clients send
//! packets to their own node; the HTTP relay network delivers them here.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use zona_p2p_crypto::NodeKeypair;
//! use zona_p2p_server::{NodeServer, CatchAllHandler};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let keypair   = NodeKeypair::generate();
//!     let self_url  = "http://my-server:8801";
//!     let http_addr: std::net::SocketAddr = "0.0.0.0:8801".parse()?;
//!
//!     let mut server = NodeServer::new(self_url, keypair);
//!     server.register(CatchAllHandler::new(|pkt| {
//!         let msg = format!("echo: {}", String::from_utf8_lossy(&pkt.payload));
//!         Some(msg.into_bytes())
//!     }));
//!
//!     server.bootstrap(vec!["http://seed-node:7701".to_string()]).await?;
//!
//!     let router   = server.make_router();
//!     let listener = tokio::net::TcpListener::bind(http_addr).await?;
//!     axum::serve(listener, router).await?;
//!     Ok(())
//! }
//! ```

pub mod handler;
pub mod http_api;
pub mod packet;
pub mod server;

pub use handler::{CatchAllHandler, PacketHandler, PrefixHandler};
pub use packet::AppPacket;
pub use server::{NodeServer, send_and_wait};
