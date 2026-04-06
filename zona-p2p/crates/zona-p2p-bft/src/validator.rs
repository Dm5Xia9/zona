//! Validator set for an epoch — §11.2, §11.7, §12.1.
//!
//! `ValidatorSet` holds the weighted list of validators for a BFT epoch and
//! can verify that a quorum of `2f+1` signatures is present on a certificate.

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use zona_p2p_types::NodeId;
use crate::error::BftError;

/// A single entry in the validator set.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorEntry {
    /// Validator's node identity.
    pub node_id: NodeId,
    /// Raw Ed25519 public key (32 bytes).
    pub pubkey:  [u8; 32],
    /// Weight (proportional stake / vote weight). All weights sum to total.
    pub weight:  u64,
}

/// The complete validator set for an epoch.
///
/// Referendum: §12.1 default committee size n = 21, f ≤ 6.
/// Quorum threshold = ⌊(2 × total_weight) / 3⌋ + 1.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorSet {
    pub epoch:   u64,
    pub entries: Vec<ValidatorEntry>,
    /// SHA-256 digest of the canonical serialisation — used in EpochHeader
    /// commitments (§12.1).
    pub set_hash: [u8; 32],
}

impl ValidatorSet {
    /// Build a `ValidatorSet` from entries, computing `set_hash`.
    pub fn new(epoch: u64, entries: Vec<ValidatorEntry>) -> Self {
        let set_hash = compute_set_hash(epoch, &entries);
        ValidatorSet { epoch, entries, set_hash }
    }

    pub fn total_weight(&self) -> u64 {
        self.entries.iter().map(|e| e.weight).sum()
    }

    /// Quorum threshold: smallest integer > 2/3 of total weight.
    pub fn quorum_threshold(&self) -> u64 {
        let total = self.total_weight();
        // ⌊2·total / 3⌋ + 1
        (2 * total) / 3 + 1
    }

    /// Look up a validator by NodeId.
    pub fn get(&self, node_id: &NodeId) -> Option<&ValidatorEntry> {
        self.entries.iter().find(|e| &e.node_id == node_id)
    }

    /// Verify that `signatures` (pairs of (signer_id, 64-byte sig)) form a
    /// valid quorum over `message` — §11.2, §11.5.
    ///
    /// Each signer must be in the set; duplicate signers are counted once;
    /// accumulated weight must reach the quorum threshold.
    pub fn verify_quorum(
        &self,
        message:    &[u8],
        signatures: &[(NodeId, Vec<u8>)],
    ) -> Result<(), BftError> {
        use ed25519_dalek::{VerifyingKey, Signature, Verifier};

        let mut weight_accumulated: u64 = 0;
        let mut seen = std::collections::HashSet::new();

        for (signer_id, sig_bytes) in signatures {
            if !seen.insert(signer_id) {
                continue; // deduplicate
            }
            let entry = self.get(signer_id).ok_or_else(|| BftError::UnknownValidator {
                node_id: signer_id.to_string(),
            })?;

            let vk = VerifyingKey::from_bytes(&entry.pubkey)
                .map_err(|_| BftError::InvalidSignature { node_id: signer_id.to_string() })?;

            let arr: [u8; 64] = sig_bytes.as_slice().try_into().map_err(|_| {
                BftError::InvalidSignature { node_id: signer_id.to_string() }
            })?;
            let sig = Signature::from_bytes(&arr);
            vk.verify(message, &sig)
                .map_err(|_| BftError::InvalidSignature { node_id: signer_id.to_string() })?;

            weight_accumulated += entry.weight;
        }

        if weight_accumulated < self.quorum_threshold() {
            return Err(BftError::InsufficientQuorum {
                accumulated: weight_accumulated,
                required:    self.quorum_threshold(),
            });
        }

        Ok(())
    }
}

fn compute_set_hash(epoch: u64, entries: &[ValidatorEntry]) -> [u8; 32] {
    let payload = bincode::serialize(&(epoch, entries)).expect("infallible");
    let mut h = Sha256::new();
    h.update(&payload);
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{SigningKey, Signer};
    use rand::rngs::OsRng;
    use zona_p2p_crypto::derive_node_id;

    fn make_validator() -> (ValidatorEntry, SigningKey) {
        let sk = SigningKey::generate(&mut OsRng);
        let pk = *sk.verifying_key().as_bytes();
        let node_id = derive_node_id(&pk);
        (ValidatorEntry { node_id, pubkey: pk, weight: 1 }, sk)
    }

    #[test]
    fn quorum_threshold_21_validators() {
        let entries: Vec<_> = (0..21)
            .map(|_| make_validator().0)
            .collect();
        let vs = ValidatorSet::new(0, entries);
        assert_eq!(vs.quorum_threshold(), 15); // ⌊42/3⌋+1 = 14+1 = 15
    }

    #[test]
    fn verify_quorum_ok() {
        let (entry, sk) = make_validator();
        let vs = ValidatorSet::new(0, vec![entry.clone()]);
        // Single validator with weight=1, threshold=1 — quorum with one sig.
        let msg = b"test message";
        let sig = sk.sign(msg).to_bytes().to_vec();
        assert!(vs.verify_quorum(msg, &[(entry.node_id, sig)]).is_ok());
    }

    #[test]
    fn verify_quorum_insufficient() {
        let (e1, _) = make_validator();
        let (e2, sk2) = make_validator();
        let vs = ValidatorSet::new(0, vec![e1, e2.clone()]);
        // Two validators, threshold = 2, only one sig — should fail.
        let msg = b"test";
        let sig = sk2.sign(msg).to_bytes().to_vec();
        assert!(matches!(
            vs.verify_quorum(msg, &[(e2.node_id, sig)]),
            Err(BftError::InsufficientQuorum { .. })
        ));
    }
}
