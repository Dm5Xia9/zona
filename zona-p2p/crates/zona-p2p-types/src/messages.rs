use serde::{Deserialize, Serialize};
use crate::{NodeId, Descriptor};

/// Priority class for a message — §7.3, §12.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageClass {
    /// Routine gossip or routing.
    Normal,
    /// User message; irreversible effect → requires 2-of-3 route quorum.
    CriticalUser,
}

/// Wire-level envelope wrapping all protocol messages — §7.3.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    /// Destination NodeId.
    pub to:         NodeId,
    /// Originating NodeId.
    pub from:       NodeId,
    /// Unique per-message id (for dedup / idempotency).
    pub message_id: u64,
    /// Hops remaining; decremented at each relay.
    pub ttl:        u8,
    pub class:      MessageClass,
    /// If true, sender has forked this message over two independent first-hops
    /// (§7.3 CriticalUser route_fork). Receiver deduplicates by message_id.
    pub route_fork: bool,
    pub body:       MessageKind,
    /// Ed25519 signature by `from` over the canonical payload (all fields
    /// except `signature` itself), serialised with bincode — §10.4.
    /// Empty `Vec` = unsigned (e.g. loopback / test messages).
    pub signature:  Vec<u8>,
}

impl Envelope {
    /// Fields covered by the signature: everything except `signature`.
    pub fn signing_payload(&self) -> Vec<u8> {
        bincode::serialize(&(
            &self.to,
            &self.from,
            self.message_id,
            self.ttl,
            &self.class,
            self.route_fork,
            &self.body,
        ))
        .expect("bincode serialize is infallible for Envelope fields")
    }
}

/// All protocol message types.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessageKind {
    /// Handshake — send own descriptor.
    Hello { descriptor: Descriptor },

    /// Find the `count` peers closest to `target` — §7, rate-limited §12.2.
    FindNode { target: NodeId, count: u8 },

    /// Response to FindNode.
    FoundNodes { peers: Vec<PexCandidate> },

    /// Peer exchange: offer candidates — §10.7.
    Pex { candidates: Vec<PexCandidate> },

    /// Liveness probe — §4.
    Ping { nonce: u64 },

    /// Liveness response.
    Pong { nonce: u64 },

    /// Publish (or update) a descriptor — §3.
    AnnounceDescriptor { descriptor: Descriptor },

    /// User payload — delivered to the final destination.
    UserData { payload: Vec<u8> },

    /// Invite token from inviting node P to new node N — §8.
    Invite { token: Vec<u8> },
}

/// A single PEX candidate — §10.7.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PexCandidate {
    pub descriptor:  Descriptor,
    /// Who is recommending this peer.
    pub referrer_id: NodeId,
}

/// Protocol error codes sent as responses — §12.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    InviteQuotaExceeded,
    FindRateLimitExceeded,
    PingRateLimitExceeded,
    CertUnavailableUseMultisource,
    InconsistentReply,
    FinalityPaused,
    FlowControlled,
}
