//! TCP transport with length-prefixed frame codec.
//!
//! Frame layout:
//!   [4 bytes BE: length] [length bytes: bincode(Envelope)]
//!
//! One framed stream per peer connection (§11.6 — two listeners for overlay
//! vs consensus can be added later; here we use a single stream and rely on
//! MessageKind for dispatch).

pub mod codec;
pub mod connection;
pub mod listener;

pub use connection::PeerConnection;
pub use listener::Listener;
