use serde::{Deserialize, Serialize};
use crate::NodeId;

/// How to establish a session with a node — §2.1, §2.2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachSpec {
    /// Direct TCP/TLS or QUIC endpoint.
    Direct {
        addr: std::net::SocketAddr,
    },
    /// Session via relay node: connect to relay, then open channel.
    Relayed {
        relay:          NodeId,
        circuit_handle: u64,
    },
}

/// Ordered list of ReachSpec variants with priority (first = highest). §3.1.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReachList(pub Vec<ReachSpec>);

impl ReachList {
    pub fn direct(addr: std::net::SocketAddr) -> Self {
        ReachList(vec![ReachSpec::Direct { addr }])
    }

    pub fn relayed(relay: NodeId, circuit_handle: u64) -> Self {
        ReachList(vec![ReachSpec::Relayed { relay, circuit_handle }])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for ReachList {
    fn default() -> Self {
        ReachList(vec![])
    }
}
