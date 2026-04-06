//! Light-client state and multi-channel certificate verification — §11.10, §12.7.
//!
//! A light client does not participate in BFT rounds.  It maintains a local
//! "best trusted header" and advances it only when the conditions of §11.10
//! are satisfied:
//!
//! **(a)** Two independent channels (different `referrer_id`) report the same
//!         `(height, hash)`.
//! **(b)** One channel + a trusted external checkpoint.
//! **(c)** Two channels + an external checkpoint (most conservative).
//!
//! Additionally the client verifies the chain of `EpochHeader` commitments
//! from the genesis anchor (§11.10 check-list).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zona_p2p_types::NodeId;
use crate::certificate::BftCertificate;
use crate::epoch::{EpochHeader, verify_epoch_chain};
use crate::validator::ValidatorSet;
use crate::error::BftError;

/// A trusted genesis or checkpoint anchor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Anchor {
    pub height:           u64,
    pub block_hash:       [u8; 32],
    /// SHA-256 of the `ValidatorSet` at this anchor — used to start chain
    /// verification.
    pub validator_set_hash: [u8; 32],
}

/// An incoming certificate observation from one channel.
#[derive(Clone, Debug)]
pub struct CertObservation {
    pub cert:        BftCertificate,
    /// `referrer_id` of the peer that delivered this certificate.
    pub referrer_id: NodeId,
}

/// Light-client state — §11.10, §12.7.
pub struct LightClient {
    /// Genesis / checkpoint anchor — never modified after init.
    pub anchor: Anchor,
    /// The highest header the client currently trusts.
    pub best_height:    u64,
    pub best_hash:      [u8; 32],
    /// Collected observations for the current candidate height.
    /// Key: (height, block_hash) → set of distinct referrer_ids that confirmed it.
    pending: HashMap<(u64, [u8; 32]), std::collections::HashSet<NodeId>>,
    /// Validator set for the current epoch (needed for cert verification).
    pub validator_set: ValidatorSet,
    /// Whether we have an external checkpoint for the current round.
    has_external_checkpoint: bool,
}

impl LightClient {
    /// Create from a genesis anchor and the genesis validator set.
    pub fn from_genesis(anchor: Anchor, validator_set: ValidatorSet) -> Self {
        let best_height = anchor.height;
        let best_hash   = anchor.block_hash;
        LightClient {
            anchor,
            best_height,
            best_hash,
            pending: HashMap::new(),
            validator_set,
            has_external_checkpoint: false,
        }
    }

    /// Record that an external (out-of-band) checkpoint is available — §12.7.
    pub fn provide_external_checkpoint(&mut self) {
        self.has_external_checkpoint = true;
    }

    /// Submit a certificate observation from one channel.
    ///
    /// The client verifies the cert cryptographically and then checks whether
    /// the §11.10 acceptance conditions are met:
    ///
    /// - 2 independent channels (different `referrer_id`) with the same
    ///   `(height, hash)` → **Accepted**.
    /// - 1 channel + external checkpoint → **Accepted**.
    /// - Otherwise → **Pending**.
    ///
    /// Returns `Ok(true)` when the best header was advanced.
    pub fn observe(&mut self, obs: CertObservation) -> Result<bool, BftError> {
        // A node in Recovery must not count as an authoritative channel (§5.1).
        // The caller is responsible for setting the `referrer_id` appropriately;
        // we cannot detect Recovery here without the overlay state.

        // Cryptographic verification.
        obs.cert.verify(&self.validator_set)?;

        let height    = obs.cert.payload.height;
        let hash      = obs.cert.payload.block_hash;

        // Must be strictly higher than our current best.
        if height <= self.best_height {
            return Ok(false);
        }

        let key = (height, hash);
        let refs = self.pending.entry(key).or_default();
        refs.insert(obs.referrer_id);

        // Check acceptance conditions (§11.10, §12.7).
        let channel_count = refs.len();
        let accepted = channel_count >= 2
            || (channel_count >= 1 && self.has_external_checkpoint);

        if accepted {
            self.best_height             = height;
            self.best_hash               = hash;
            self.has_external_checkpoint = false; // consume the checkpoint
            // Evict older pending entries.
            self.pending.retain(|(h, _), _| *h > height);
            tracing::info!(height, "LightClient: best header advanced");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Verify a chain of epoch headers from the anchor to the current epoch.
    ///
    /// Delegates to `verify_epoch_chain` (§11.10 check-list).
    pub fn verify_epoch_chain(&self, chain: &[EpochHeader]) -> Result<(), BftError> {
        verify_epoch_chain(self.anchor.validator_set_hash, chain)
    }

    /// Advance to a new epoch by verifying the transition commitment.
    pub fn advance_epoch(
        &mut self,
        new_set:  ValidatorSet,
        header:   &EpochHeader,
    ) -> Result<(), BftError> {
        header.verify_next_set(&new_set)?;
        if new_set.epoch != self.validator_set.epoch + 1 {
            return Err(BftError::NonConsecutiveEpoch {
                expected: self.validator_set.epoch + 1,
                got:      new_set.epoch,
            });
        }
        self.validator_set = new_set;
        Ok(())
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
    use zona_p2p_types::NodeId;

    fn make_vs_sks(n: usize) -> (ValidatorSet, Vec<SigningKey>) {
        let mut entries = vec![];
        let mut sks = vec![];
        for _ in 0..n {
            let sk = SigningKey::generate(&mut OsRng);
            let pk = *sk.verifying_key().as_bytes();
            entries.push(ValidatorEntry {
                node_id: derive_node_id(&pk),
                pubkey:  pk,
                weight:  1,
            });
            sks.push(sk);
        }
        (ValidatorSet::new(0, entries), sks)
    }

    fn make_obs(
        vs: &ValidatorSet,
        sks: &[SigningKey],
        height: u64,
        referrer: u8,
    ) -> CertObservation {
        let payload = CertPayload { epoch: 0, height, block_hash: [height as u8; 32] };
        let msg = payload.to_bytes();
        let threshold = vs.quorum_threshold() as usize;
        let signatures: Vec<_> = vs.entries.iter().zip(sks.iter()).take(threshold)
            .map(|(e, sk)| (e.node_id, sk.sign(&msg).to_bytes().to_vec()))
            .collect();
        CertObservation {
            cert: BftCertificate { payload, signatures },
            referrer_id: NodeId([referrer; 32]),
        }
    }

    fn make_lc(vs: ValidatorSet) -> LightClient {
        LightClient::from_genesis(
            Anchor { height: 0, block_hash: [0u8; 32], validator_set_hash: vs.set_hash },
            vs,
        )
    }

    #[test]
    fn two_channels_advance_best() {
        let (vs, sks) = make_vs_sks(4);
        let mut lc = make_lc(vs.clone());

        assert!(!lc.observe(make_obs(&vs, &sks, 1, 0xAA)).unwrap());
        assert!( lc.observe(make_obs(&vs, &sks, 1, 0xBB)).unwrap());
        assert_eq!(lc.best_height, 1);
    }

    #[test]
    fn one_channel_plus_checkpoint_advances() {
        let (vs, sks) = make_vs_sks(4);
        let mut lc = make_lc(vs.clone());
        lc.provide_external_checkpoint();

        assert!(lc.observe(make_obs(&vs, &sks, 1, 0xAA)).unwrap());
        assert_eq!(lc.best_height, 1);
    }

    #[test]
    fn same_referrer_does_not_advance() {
        let (vs, sks) = make_vs_sks(4);
        let mut lc = make_lc(vs.clone());

        lc.observe(make_obs(&vs, &sks, 1, 0xAA)).unwrap();
        // Same referrer again — still pending.
        assert!(!lc.observe(make_obs(&vs, &sks, 1, 0xAA)).unwrap());
        assert_eq!(lc.best_height, 0);
    }
}
