//! Epoch headers and chain-of-commitments — §11.10, §12.1.
//!
//! Each finalised block (or at least once per epoch) contains a commitment to
//! the **next** validator set so that light clients can verify the chain of
//! epochs without trusting a single peer (§11.10 chek-list point 2).
//!
//! Light clients walk the chain: genesis → epoch 0 header → epoch 1 header →
//! … → current, verifying at each step that the commitment in the parent block
//! matches the hash of the child `ValidatorSet`.

use serde::{Deserialize, Serialize};
use crate::validator::ValidatorSet;
use crate::error::BftError;

/// A header published (at most) once per epoch, carried in the finalised log.
///
/// Contains:
/// - the epoch number and the hash of *this* epoch's `ValidatorSet` (so
///   receivers can verify they have the right set),
/// - a commitment to the *next* epoch's `ValidatorSet` hash (so light clients
///   can verify the transition without fetching the full next set immediately).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpochHeader {
    pub epoch:                u64,
    /// SHA-256(canonical serialisation of this epoch's ValidatorSet) — §12.1.
    pub validator_set_hash:   [u8; 32],
    /// Commitment to the *next* epoch's `ValidatorSet` hash.
    /// `None` for the very last epoch (no successor).
    pub next_validator_commitment: Option<[u8; 32]>,
    /// Finalised height at which this header was published.
    pub published_at_height:  u64,
}

impl EpochHeader {
    /// Build an `EpochHeader` from the current and (optionally) next
    /// `ValidatorSet`.
    pub fn new(
        current:         &ValidatorSet,
        next:            Option<&ValidatorSet>,
        published_at_height: u64,
    ) -> Self {
        EpochHeader {
            epoch:                        current.epoch,
            validator_set_hash:           current.set_hash,
            next_validator_commitment:    next.map(|vs| vs.set_hash),
            published_at_height,
        }
    }

    /// Verify that `current_vs` matches this header's `validator_set_hash`.
    pub fn verify_current_set(&self, current_vs: &ValidatorSet) -> Result<(), BftError> {
        if current_vs.set_hash != self.validator_set_hash {
            return Err(BftError::ValidatorSetHashMismatch {
                expected: self.validator_set_hash,
                got:      current_vs.set_hash,
            });
        }
        Ok(())
    }

    /// Verify that `next_vs` matches the committed `next_validator_commitment`.
    ///
    /// Call this when transitioning to the next epoch to ensure the light
    /// client is not fed a fraudulent validator set (§11.10 point 3).
    pub fn verify_next_set(&self, next_vs: &ValidatorSet) -> Result<(), BftError> {
        match self.next_validator_commitment {
            None => Err(BftError::NoNextEpochCommitment),
            Some(committed) => {
                if next_vs.set_hash != committed {
                    Err(BftError::ValidatorSetHashMismatch {
                        expected: committed,
                        got:      next_vs.set_hash,
                    })
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// Verify a chain of epoch headers from a trusted anchor to a target header.
///
/// Each header in `chain` must:
/// 1. Have `epoch` increasing by exactly 1 from the previous.
/// 2. Match the `next_validator_commitment` of the previous header.
///
/// `chain[0]` must match `anchor_set_hash` (the genesis / checkpoint hash
/// the caller already trusts).
pub fn verify_epoch_chain(
    anchor_set_hash: [u8; 32],
    chain:           &[EpochHeader],
) -> Result<(), BftError> {
    if chain.is_empty() {
        return Ok(());
    }

    // First header must match anchor.
    if chain[0].validator_set_hash != anchor_set_hash {
        return Err(BftError::ValidatorSetHashMismatch {
            expected: anchor_set_hash,
            got:      chain[0].validator_set_hash,
        });
    }

    for window in chain.windows(2) {
        let parent = &window[0];
        let child  = &window[1];

        // Epoch must be consecutive.
        if child.epoch != parent.epoch + 1 {
            return Err(BftError::NonConsecutiveEpoch {
                expected: parent.epoch + 1,
                got:      child.epoch,
            });
        }

        // Child's set hash must match parent's commitment.
        match parent.next_validator_commitment {
            None => return Err(BftError::NoNextEpochCommitment),
            Some(committed) => {
                if child.validator_set_hash != committed {
                    return Err(BftError::ValidatorSetHashMismatch {
                        expected: committed,
                        got:      child.validator_set_hash,
                    });
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator::{ValidatorEntry, ValidatorSet};
    use zona_p2p_types::NodeId;

    fn dummy_vs(epoch: u64) -> ValidatorSet {
        let entry = ValidatorEntry {
            node_id: NodeId([epoch as u8; 32]),
            pubkey:  [epoch as u8; 32],
            weight:  1,
        };
        ValidatorSet::new(epoch, vec![entry])
    }

    #[test]
    fn chain_of_two_epochs_ok() {
        let vs0 = dummy_vs(0);
        let vs1 = dummy_vs(1);
        let h0 = EpochHeader::new(&vs0, Some(&vs1), 0);
        let h1 = EpochHeader::new(&vs1, None, 100);

        assert!(verify_epoch_chain(vs0.set_hash, &[h0, h1]).is_ok());
    }

    #[test]
    fn chain_wrong_commitment_fails() {
        let vs0 = dummy_vs(0);
        let vs1 = dummy_vs(1);
        let vs2 = dummy_vs(2); // not committed in h0
        let h0 = EpochHeader::new(&vs0, Some(&vs1), 0);
        let h_wrong = EpochHeader::new(&vs2, None, 100);

        assert!(verify_epoch_chain(vs0.set_hash, &[h0, h_wrong]).is_err());
    }
}
