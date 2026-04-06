//! Cryptographic primitives for zona-p2p.
//!
//! - Ed25519 keypair generation (§6.1, §12.2)
//! - NodeId = SHA-256(public key) — 256-bit
//! - Sign / verify Descriptor and Envelope payloads
//! - Invite token creation and verification

pub mod keypair;
pub mod identity;
pub mod signing;
pub mod invite;

pub use keypair::NodeKeypair;
pub use identity::derive_node_id;
pub use signing::{sign_descriptor, verify_descriptor, sign_envelope, verify_envelope};
pub use invite::{InviteToken, create_invite, verify_invite};
