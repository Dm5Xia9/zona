//! BFT certificate — §11.5.
//!
//! A certificate proves that ≥ 2f+1 validators have signed the same
//! `(height, block_hash)` pair.  Any node that holds a certificate and knows
//! the corresponding `ValidatorSet` can verify finality without participating
//! in the consensus rounds.

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use zona_p2p_types::NodeId;
use crate::validator::ValidatorSet;
use crate::error::BftError;

/// The payload that validators sign in each BFT round.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CertPayload {
    pub epoch:      u64,
    pub height:     u64,
    /// SHA-256 of the finalised block / command-log entry.
    pub block_hash: [u8; 32],
}

impl CertPayload {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("infallible")
    }
}

/// A BFT certificate: quorum of validator signatures over a `CertPayload`.
///
/// Produced after a successful BFT round; stored by validators and distributed
/// to light clients (§11.5, §11.10).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BftCertificate {
    pub payload:    CertPayload,
    /// Vec of `(signer NodeId, 64-byte Ed25519 signature)`.
    pub signatures: Vec<(NodeId, Vec<u8>)>,
}

impl BftCertificate {
    /// Verify this certificate against the given `ValidatorSet`.
    ///
    /// Checks:
    /// 1. Each signature is valid for `payload.to_bytes()`.
    /// 2. Accumulated signer weight ≥ quorum threshold (§11.2).
    pub fn verify(&self, validator_set: &ValidatorSet) -> Result<(), BftError> {
        if self.payload.epoch != validator_set.epoch {
            return Err(BftError::EpochMismatch {
                cert_epoch: self.payload.epoch,
                set_epoch:  validator_set.epoch,
            });
        }
        let msg = self.payload.to_bytes();
        validator_set.verify_quorum(&msg, &self.signatures)
    }

    /// Canonical hash of this certificate — used for deduplication and in
    /// `EpochHeader` commitments.
    pub fn hash(&self) -> [u8; 32] {
        let bytes = bincode::serialize(self).expect("infallible");
        let mut h = Sha256::new();
        h.update(&bytes);
        h.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator::{ValidatorEntry, ValidatorSet};
    use ed25519_dalek::{SigningKey, Signer};
    use rand::rngs::OsRng;
    use zona_p2p_crypto::derive_node_id;

    fn make_cert_with_validators(
        n: usize,
        signing_n: usize,
    ) -> (BftCertificate, ValidatorSet) {
        let mut entries = vec![];
        let mut sks = vec![];
        for _ in 0..n {
            let sk = SigningKey::generate(&mut OsRng);
            let pk = *sk.verifying_key().as_bytes();
            let node_id = derive_node_id(&pk);
            entries.push(ValidatorEntry { node_id, pubkey: pk, weight: 1 });
            sks.push(sk);
        }
        let vs = ValidatorSet::new(1, entries.clone());

        let payload = CertPayload { epoch: 1, height: 42, block_hash: [0xAB; 32] };
        let msg = payload.to_bytes();

        let signatures: Vec<_> = entries.iter().zip(sks.iter()).take(signing_n)
            .map(|(e, sk)| (e.node_id, sk.sign(&msg).to_bytes().to_vec()))
            .collect();

        (BftCertificate { payload, signatures }, vs)
    }

    #[test]
    fn cert_valid_with_quorum() {
        // 4 validators, 3 sign → threshold ⌊8/3⌋+1=3 — should pass.
        let (cert, vs) = make_cert_with_validators(4, 3);
        assert!(cert.verify(&vs).is_ok());
    }

    #[test]
    fn cert_insufficient_quorum() {
        // 4 validators, only 2 sign → below threshold.
        let (cert, vs) = make_cert_with_validators(4, 2);
        assert!(matches!(cert.verify(&vs), Err(BftError::InsufficientQuorum { .. })));
    }

    #[test]
    fn cert_epoch_mismatch() {
        let (cert, mut vs) = make_cert_with_validators(4, 3);
        vs.epoch = 99;
        assert!(matches!(cert.verify(&vs), Err(BftError::EpochMismatch { .. })));
    }
}
