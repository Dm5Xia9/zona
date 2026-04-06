//! zona-p2p-client — transport-agnostic P2P client abstraction.
//!
//! Defines the `NetworkBackend` trait and the interactive REPL.
//! Concrete backends (sandbox, docker) live in `zona-p2p-sim` and implement
//! the trait from this crate.

pub mod backend;
pub mod interactive;
