use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid signature on descriptor from {node_id}")]
    InvalidSignature { node_id: String },

    #[error("invalid envelope signature from {sender_id}")]
    EnvelopeSignatureInvalid { sender_id: String },

    #[error("descriptor version {received} is older than known {known}")]
    StaleDescriptor { received: u64, known: u64 },

    #[error("invite quota exceeded")]
    InviteQuotaExceeded,

    #[error("rate limit exceeded: {op}")]
    RateLimitExceeded { op: &'static str },

    #[error("peer table full and no slot can be replaced this tick")]
    PeerTableFull,

    #[error("codec error: {0}")]
    Codec(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialisation error: {0}")]
    Serialise(String),

    #[error("unknown sender: no descriptor for {sender_id}")]
    UnknownSender { sender_id: String },
}
