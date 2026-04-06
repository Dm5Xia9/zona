//! BFT node state — §11, §12.6.
//!
//! `BftState` tracks the highest finalised height, the current validator set
//! and finality-pause conditions.  It is the single source of truth for
//! "what has been committed" on this node.

use serde::{Deserialize, Serialize};
use crate::certificate::BftCertificate;
use crate::epoch::EpochHeader;
use crate::validator::ValidatorSet;
use crate::error::BftError;

/// Finality status — §12.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinalityStatus {
    /// Normal operation.
    Active,
    /// Finality paused — new critical commands are queued or rejected
    /// (§11.10, §12.6).
    Paused,
}

/// The BFT node state.
///
/// Updated via `apply_certificate()`.  Consumers (descriptor store, overlay)
/// query `finalised_height` and `finality_status`.
#[derive(Clone, Debug)]
pub struct BftState {
    /// Highest finalised height for which we hold a valid certificate.
    pub finalised_height: u64,
    /// SHA-256 of the finalised block at `finalised_height`.
    pub block_hash:       [u8; 32],
    /// Current epoch.
    pub epoch:            u64,
    /// Validator set for the current epoch.
    pub validator_set:    ValidatorSet,
    pub finality_status:  FinalityStatus,
    /// How many consecutive divergence detections (§12.6 trigger 1).
    divergence_count:     u32,
}

impl BftState {
    /// Initialise from genesis — §11.7, §12.1.
    pub fn genesis(validator_set: ValidatorSet, genesis_hash: [u8; 32]) -> Self {
        BftState {
            finalised_height: 0,
            block_hash:       genesis_hash,
            epoch:            validator_set.epoch,
            validator_set,
            finality_status:  FinalityStatus::Active,
            divergence_count: 0,
        }
    }

    /// Apply a BFT certificate:
    /// 1. Verify it against the current validator set.
    /// 2. Accept it only if `height > finalised_height`.
    /// 3. Update state accordingly.
    ///
    /// Returns `Ok(true)` if the state was advanced, `Ok(false)` if the
    /// certificate is for an already-finalised height (idempotent), or
    /// `Err` if the certificate is invalid.
    pub fn apply_certificate(&mut self, cert: &BftCertificate) -> Result<bool, BftError> {
        if self.finality_status == FinalityStatus::Paused {
            return Err(BftError::FinalityPaused);
        }

        cert.verify(&self.validator_set)?;

        if cert.payload.height <= self.finalised_height {
            return Ok(false); // already finalised or stale
        }

        self.finalised_height = cert.payload.height;
        self.block_hash       = cert.payload.block_hash;
        Ok(true)
    }

    /// Rotate to a new epoch's validator set, verifying the transition via the
    /// `EpochHeader` commitment (§11.10, §12.1).
    pub fn advance_epoch(
        &mut self,
        new_set:     ValidatorSet,
        header:      &EpochHeader,
    ) -> Result<(), BftError> {
        // The current epoch's header must commit to new_set.
        header.verify_next_set(&new_set)?;

        // New epoch must follow the current one.
        if new_set.epoch != self.epoch + 1 {
            return Err(BftError::NonConsecutiveEpoch {
                expected: self.epoch + 1,
                got:      new_set.epoch,
            });
        }

        self.epoch         = new_set.epoch;
        self.validator_set = new_set;
        Ok(())
    }

    /// Report a divergence observation (e.g. two peers reported different
    /// `(height, hash)` at the same logical height).
    ///
    /// Automatically triggers `FINALITY_PAUSED` when the threshold from
    /// §12.6 is reached.
    pub fn record_divergence(&mut self) {
        self.divergence_count += 1;
        // §12.6 trigger 1: pause on ≥ 1 divergence between independent peers.
        if self.divergence_count >= 1 {
            if self.finality_status == FinalityStatus::Active {
                tracing::warn!(
                    count = self.divergence_count,
                    "BFT: divergence detected — entering FINALITY_PAUSED"
                );
                self.finality_status = FinalityStatus::Paused;
            }
        }
    }

    /// Resume finality after divergence is resolved (e.g. partition healed,
    /// fork_choice applied, canonical chain confirmed).
    pub fn resume_finality(&mut self) {
        self.divergence_count = 0;
        self.finality_status  = FinalityStatus::Active;
        tracing::info!("BFT: finality resumed");
    }

    /// Whether a client should consider height `h` final — §12.6, §11.10.
    ///
    /// In normal mode requires `h ≤ finalised_height`.
    /// When paused, nothing is newly final.
    pub fn is_final(&self, height: u64) -> bool {
        self.finality_status == FinalityStatus::Active
            && height <= self.finalised_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::certificate::{BftCertificate, CertPayload};
    use crate::validator::{ValidatorEntry, ValidatorSet};
    use ed25519_dalek::{SigningKey, Signer};
    use rand::rngs::OsRng;
    use zona_p2p_crypto::derive_node_id;

    fn make_vs_with_signers(n: usize) -> (ValidatorSet, Vec<SigningKey>) {
        let mut entries = vec![];
        let mut sks     = vec![];
        for _ in 0..n {
            let sk = SigningKey::generate(&mut OsRng);
            let pk = *sk.verifying_key().as_bytes();
            let node_id = derive_node_id(&pk);
            entries.push(ValidatorEntry { node_id, pubkey: pk, weight: 1 });
            sks.push(sk);
        }
        (ValidatorSet::new(0, entries), sks)
    }

    fn make_cert(vs: &ValidatorSet, sks: &[SigningKey], height: u64) -> BftCertificate {
        let payload = CertPayload { epoch: vs.epoch, height, block_hash: [height as u8; 32] };
        let msg = payload.to_bytes();
        // Sign with enough validators for quorum.
        let threshold = vs.quorum_threshold() as usize;
        let signatures: Vec<_> = vs.entries.iter().zip(sks.iter()).take(threshold)
            .map(|(e, sk)| (e.node_id, sk.sign(&msg).to_bytes().to_vec()))
            .collect();
        BftCertificate { payload, signatures }
    }

    #[test]
    fn apply_valid_certificate() {
        let (vs, sks) = make_vs_with_signers(4);
        let genesis_hash = [0u8; 32];
        let mut state = BftState::genesis(vs.clone(), genesis_hash);

        let cert = make_cert(&vs, &sks, 1);
        assert!(state.apply_certificate(&cert).unwrap());
        assert_eq!(state.finalised_height, 1);
    }

    #[test]
    fn stale_certificate_returns_false() {
        let (vs, sks) = make_vs_with_signers(4);
        let mut state = BftState::genesis(vs.clone(), [0u8; 32]);

        let cert = make_cert(&vs, &sks, 5);
        state.apply_certificate(&cert).unwrap();

        // Re-applying same height returns false (already finalised).
        let cert2 = make_cert(&vs, &sks, 5);
        assert!(!state.apply_certificate(&cert2).unwrap());
    }

    #[test]
    fn finality_paused_blocks_new_certs() {
        let (vs, sks) = make_vs_with_signers(4);
        let mut state = BftState::genesis(vs.clone(), [0u8; 32]);
        state.record_divergence();

        let cert = make_cert(&vs, &sks, 1);
        assert!(matches!(state.apply_certificate(&cert), Err(BftError::FinalityPaused)));
    }

    #[test]
    fn resume_allows_certs_again() {
        let (vs, sks) = make_vs_with_signers(4);
        let mut state = BftState::genesis(vs.clone(), [0u8; 32]);
        state.record_divergence();
        state.resume_finality();

        let cert = make_cert(&vs, &sks, 1);
        assert!(state.apply_certificate(&cert).is_ok());
    }
}
