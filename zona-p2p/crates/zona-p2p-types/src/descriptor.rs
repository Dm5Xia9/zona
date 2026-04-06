use serde::{Deserialize, Serialize};
use crate::{NodeId, reach_spec::ReachList};

/// Signed node descriptor D_N — §2.2, §8.
///
/// `signature` covers all other fields serialised with bincode.
/// Provisional descriptors have a short TTL and `provisional = true`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Descriptor {
    pub node_id:     NodeId,
    /// Raw Ed25519 public key (32 bytes).
    pub pubkey:      [u8; 32],
    pub reach:       ReachList,
    /// Monotonically increasing version; higher wins in merge (§8.1).
    pub version:     u64,
    /// If true, consider short-lived until confirmed in BFT state.
    pub provisional: bool,
    /// Ed25519 signature over the canonical encoding of all fields above.
    pub signature:   Vec<u8>,
}

impl Descriptor {
    /// Bytes to sign: everything except `signature` field, serialised.
    pub fn signing_payload(&self) -> Vec<u8> {
        // We serialise a tuple of the signable fields to avoid including `signature`.
        bincode::serialize(&(
            &self.node_id,
            &self.pubkey,
            &self.reach,
            self.version,
            self.provisional,
        ))
        .expect("bincode serialize is infallible for these types")
    }
}
