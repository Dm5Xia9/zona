use sha2::{Sha256, Digest};
use zona_p2p_types::NodeId;

/// Derive NodeId = SHA-256(pubkey) — §6.1, §12.2.
pub fn derive_node_id(pubkey_bytes: &[u8]) -> NodeId {
    let mut hasher = Sha256::new();
    hasher.update(pubkey_bytes);
    let result = hasher.finalize();
    let mut id = [0u8; 32];
    id.copy_from_slice(&result);
    NodeId(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_pubkey_same_node_id() {
        let pk = [42u8; 32];
        assert_eq!(derive_node_id(&pk), derive_node_id(&pk));
    }

    #[test]
    fn different_pubkey_different_node_id() {
        let pk1 = [1u8; 32];
        let pk2 = [2u8; 32];
        assert_ne!(derive_node_id(&pk1), derive_node_id(&pk2));
    }
}
