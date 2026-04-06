use std::net::SocketAddr;
use zona_p2p_types::NodeId;

/// Bootstrap peer reference (address + expected NodeId for verification).
#[derive(Clone, Debug)]
pub struct BootstrapPeer {
    pub addr:    SocketAddr,
    pub node_id: NodeId,
}

/// Runtime configuration for a Node instance.
#[derive(Clone, Debug)]
pub struct NodeConfig {
    /// Overlay listener address.
    pub listen_addr:    SocketAddr,
    /// Bootstrap peers to contact on startup.
    pub bootstrap:      Vec<BootstrapPeer>,
    /// Number of greedy G-slots (1..=4; typically 3).
    pub max_greedy:     usize,
    /// Epoch seed for repair target computation — rotated by consensus.
    pub epoch_seed:     u64,
    /// Keep-alive interval in seconds.
    pub keepalive_secs: u64,
    /// Repair tick interval in seconds.
    pub repair_secs:    u64,
}

impl NodeConfig {
    pub fn new(listen_addr: SocketAddr) -> Self {
        NodeConfig {
            listen_addr,
            bootstrap:      vec![],
            max_greedy:     3,
            epoch_seed:     0,
            keepalive_secs: 30,
            repair_secs:    60,
        }
    }
}
