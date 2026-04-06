//! Application-level packet — what handlers see.

use zona_p2p_types::NodeId;

/// An application packet delivered to a handler.
///
/// `msg_id` is intentionally hidden — the framework uses it internally
/// to correlate responses back to waiting senders.
#[derive(Clone, Debug)]
pub struct AppPacket {
    /// NodeId of the original sender.
    pub from:    NodeId,
    /// Raw payload bytes.
    pub payload: Vec<u8>,
}

