//! PacketHandler trait and built-in implementations.

use crate::packet::AppPacket;

// ── Core trait ────────────────────────────────────────────────────────────────

/// An application handler registered on a `NodeServer`.
///
/// When a `UserData` packet arrives and is addressed to this node,
/// the server iterates registered handlers in order and calls the first
/// whose `matches()` returns `true`.
pub trait PacketHandler: Send + Sync {
    /// Return `true` if this handler wants to process the packet.
    fn matches(&self, payload: &[u8]) -> bool;

    /// Process the packet and optionally return response bytes.
    /// The response is automatically routed back to `packet.from`.
    fn handle(&self, packet: AppPacket) -> Option<Vec<u8>>;
}

// ── PrefixHandler — match by byte prefix ─────────────────────────────────────

/// Matches packets whose payload starts with a fixed byte prefix
/// (e.g. `b"json:"` or `b"\x01"`).
pub struct PrefixHandler {
    prefix: Vec<u8>,
    func:   Box<dyn Fn(AppPacket) -> Option<Vec<u8>> + Send + Sync>,
}

impl PrefixHandler {
    /// Create a handler that fires when `payload` starts with `prefix`.
    pub fn new(
        prefix: impl Into<Vec<u8>>,
        func:   impl Fn(AppPacket) -> Option<Vec<u8>> + Send + Sync + 'static,
    ) -> Self {
        PrefixHandler { prefix: prefix.into(), func: Box::new(func) }
    }
}

impl PacketHandler for PrefixHandler {
    fn matches(&self, payload: &[u8]) -> bool {
        payload.starts_with(&self.prefix)
    }

    fn handle(&self, packet: AppPacket) -> Option<Vec<u8>> {
        (self.func)(packet)
    }
}

// ── CatchAllHandler ───────────────────────────────────────────────────────────

/// Matches every packet — useful as a fallback.
pub struct CatchAllHandler {
    func: Box<dyn Fn(AppPacket) -> Option<Vec<u8>> + Send + Sync>,
}

impl CatchAllHandler {
    pub fn new(func: impl Fn(AppPacket) -> Option<Vec<u8>> + Send + Sync + 'static) -> Self {
        CatchAllHandler { func: Box::new(func) }
    }
}

impl PacketHandler for CatchAllHandler {
    fn matches(&self, _payload: &[u8]) -> bool { true }
    fn handle(&self, packet: AppPacket) -> Option<Vec<u8>> { (self.func)(packet) }
}
