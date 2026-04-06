use thiserror::Error;

#[derive(Debug, Error)]
pub enum BftError {
    #[error("invalid signature from validator {node_id}")]
    InvalidSignature { node_id: String },

    #[error("unknown validator {node_id}")]
    UnknownValidator { node_id: String },

    #[error("insufficient quorum: accumulated {accumulated}, required {required}")]
    InsufficientQuorum { accumulated: u64, required: u64 },

    #[error("certificate epoch {cert_epoch} does not match validator set epoch {set_epoch}")]
    EpochMismatch { cert_epoch: u64, set_epoch: u64 },

    #[error("validator set hash mismatch: expected {}, got {}", hex::encode(expected), hex::encode(got))]
    ValidatorSetHashMismatch { expected: [u8; 32], got: [u8; 32] },

    #[error("epoch {got} is not consecutive after {expected}")]
    NonConsecutiveEpoch { expected: u64, got: u64 },

    #[error("no next-epoch commitment in this epoch header")]
    NoNextEpochCommitment,

    #[error("finality is paused — new certificates are not accepted")]
    FinalityPaused,
}
