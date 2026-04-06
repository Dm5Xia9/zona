use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use zona_p2p_types::NodeId;
use crate::identity::derive_node_id;

/// Node's long-term identity keypair (Ed25519).
pub struct NodeKeypair {
    pub signing:   SigningKey,
    pub verifying: VerifyingKey,
    pub node_id:   NodeId,
}

impl NodeKeypair {
    /// Generate a fresh keypair.
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        let node_id = derive_node_id(verifying.as_bytes());
        NodeKeypair { signing, verifying, node_id }
    }

    /// Reconstruct from raw 32-byte seed (alias: `from_seed`).
    pub fn from_seed(seed: [u8; 32]) -> Self { Self::from_bytes(&seed) }

    /// Reconstruct from raw 32-byte seed.
    pub fn from_bytes(seed: &[u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(seed);
        let verifying = signing.verifying_key();
        let node_id = derive_node_id(verifying.as_bytes());
        NodeKeypair { signing, verifying, node_id }
    }

    pub fn raw_public(&self) -> [u8; 32] {
        *self.verifying.as_bytes()
    }
}
