//! Core Node actor — wires together crypto, overlay, transport and repair.

use tracing::{info, warn, debug};

use zona_p2p_types::{
    NodeId, PeerTable, SlotEntry, SlotKind, Envelope, MessageKind, MessageClass,
    PexCandidate,
};
use zona_p2p_crypto::{NodeKeypair, sign_descriptor, sign_envelope, verify_envelope};
use zona_p2p_overlay::{
    routing::xor::{next_hop, NextHopResult},
    routing::critical::{QuorumTracker, QuorumResult, pick_fork_hops, fork_envelopes},
    peers::pex::PexManager,
    repair::{repair_targets, long_jump_target, REPAIR_TARGETS_PER_TICK},
    limits::RateLimiter,
    descriptor_store::DescriptorStore,
    recovery::{evaluate_recovery, RecoveryState},
};
use zona_p2p_types::{reach_spec::ReachList, Descriptor};
use crate::config::NodeConfig;
use crate::metrics::Metrics;

/// The main node object.
///
/// In a real deployment this would own tokio tasks managing connections.
/// Here it exposes the protocol logic as pure methods so the simulator
/// can drive it deterministically without real sockets.
pub struct Node {
    pub id:       NodeId,
    pub keypair:  NodeKeypair,
    pub config:   NodeConfig,
    pub table:    PeerTable,
    pub store:    DescriptorStore,
    pub pex:      PexManager,
    pub limits:   RateLimiter,
    pub metrics:  Metrics,
    /// Quorum tracker for CriticalUser messages (receiver side — §12.4).
    pub quorum:   QuorumTracker,
    msg_counter:  u64,
}

impl Node {
    pub fn new(keypair: NodeKeypair, config: NodeConfig) -> Self {
        let id = keypair.node_id;
        let table = PeerTable::new(id, config.max_greedy);
        Node {
            id,
            keypair,
            config,
            table,
            store:       DescriptorStore::new(),
            pex:         PexManager::new(id),
            limits:      RateLimiter::new(),
            metrics:     Metrics::default(),
            quorum:      QuorumTracker::new(),
            msg_counter: 0,
        }
    }

    /// Build and sign this node's own descriptor.
    pub fn own_descriptor(&self, reach: ReachList) -> Descriptor {
        let mut d = Descriptor {
            node_id:     self.id,
            pubkey:      self.keypair.raw_public(),
            reach,
            version:     1,
            provisional: false,
            signature:   vec![0u8; 64],
        };
        sign_descriptor(&mut d, &self.keypair.signing);
        d
    }

    /// Next message id (monotone).
    fn next_msg_id(&mut self) -> u64 {
        self.msg_counter += 1;
        self.msg_counter
    }

    /// Verify and process an incoming envelope — pure logic, no I/O.
    ///
    /// Performs signature verification (§10.4) before dispatching.
    /// Returns a list of `(destination, envelope)` pairs to send.
    pub fn handle_envelope(
        &mut self,
        from:     NodeId,
        envelope: Envelope,
    ) -> Vec<(NodeId, Envelope)> {
        // Signature verification (§10.4): skip unsigned (empty sig) envelopes
        // only for Hello/bootstrap; reject invalid signatures from known peers.
        if !envelope.signature.is_empty() {
            if let Some(cached) = self.store.get(&from) {
                if let Err(e) = verify_envelope(&envelope, &cached.descriptor.pubkey) {
                    warn!(?e, sender = %from, "dropping envelope: bad signature");
                    return vec![];
                }
            }
            // Unknown sender with a signature present — accept for Hello only.
            // Other message types require a known sender descriptor.
            else if !matches!(envelope.body, MessageKind::Hello { .. }) {
                warn!(sender = %from, "dropping non-Hello from unknown peer (no descriptor)");
                return vec![];
            }
        }

        let mut out = Vec::new();

        match &envelope.body {
            MessageKind::Hello { descriptor } => {
                let desc = descriptor.clone();
                match self.store.upsert(desc.clone(), None) {
                    Ok(updated) => {
                        if updated {
                            debug!(peer = %from, "Hello: stored descriptor");
                        }
                        // Admit peer as a candidate (referrer = itself for bootstrap).
                        let entry = SlotEntry::new(SlotKind::Greedy, from, from);
                        self.table.try_insert(entry);
                    }
                    Err(e) => warn!(?e, "Hello: invalid descriptor"),
                }
                // Respond with our own Hello.
                let reach = ReachList(vec![]);
                let own = self.own_descriptor(reach);
                let mut reply = self.make_envelope(from, MessageKind::Hello { descriptor: own });
                sign_envelope(&mut reply, &self.keypair.signing);
                out.push((from, reply));
            }

            MessageKind::Ping { nonce } => {
                let nonce = *nonce;
                let mut reply = self.make_envelope(from, MessageKind::Pong { nonce });
                sign_envelope(&mut reply, &self.keypair.signing);
                out.push((from, reply));
            }

            MessageKind::Pong { .. } => {
                if let Some(slot) = self.table.slots.iter_mut().find(|s| s.peer_id == from) {
                    slot.record_probe(true);
                }
            }

            MessageKind::FindNode { target, count } => {
                let target = *target;
                let count = *count as usize;
                if let Err(e) = self.limits.check_find(&from, true) {
                    warn!(?e, "FindNode rate limited");
                    self.metrics.messages_dropped += 1;
                    return out;
                }
                let descs = self.store.all_descriptors();
                let peers = zona_p2p_overlay::routing::xor::closest_peers(
                    &target, &self.table, count,
                );
                let candidates: Vec<PexCandidate> = peers
                    .into_iter()
                    .filter_map(|pid| {
                        descs.iter().find(|d| d.node_id == pid).map(|d| PexCandidate {
                            descriptor:  d.clone(),
                            referrer_id: self.id,
                        })
                    })
                    .collect();
                let mut reply =
                    self.make_envelope(from, MessageKind::FoundNodes { peers: candidates });
                sign_envelope(&mut reply, &self.keypair.signing);
                out.push((from, reply));
            }

            MessageKind::FoundNodes { peers } => {
                let candidates = peers.clone();
                for c in candidates {
                    let _ = self.store.upsert(c.descriptor.clone(), None);
                    let entry = SlotEntry::new(
                        SlotKind::Greedy,
                        c.descriptor.node_id,
                        c.referrer_id,
                    );
                    self.table.try_insert(entry);
                }
                debug!(peer = %from, "FoundNodes: integrated responses");
            }

            MessageKind::Pex { candidates } => {
                let n = self.pex.apply_candidates(candidates.clone(), &mut self.table);
                debug!(peer = %from, n, "PEX applied");
                self.metrics.pex_rounds += 1;
            }

            MessageKind::AnnounceDescriptor { descriptor } => {
                let desc = descriptor.clone();
                match self.store.upsert(desc, None) {
                    Ok(true)  => debug!(peer = %from, "AnnounceDescriptor: stored"),
                    Ok(false) => {}
                    Err(e)    => warn!(?e, "AnnounceDescriptor invalid"),
                }
            }

            MessageKind::UserData { payload } => {
                let is_critical = envelope.class == MessageClass::CriticalUser;

                if envelope.to == self.id {
                    // We are the destination.
                    if is_critical {
                        // §12.4: require quorum of ≥ 2 matching copies.
                        let result = self.quorum.record(envelope.message_id, payload.clone());
                        match result {
                            QuorumResult::Pending => {
                                debug!(msg_id = envelope.message_id,
                                    "CriticalUser: waiting for quorum");
                            }
                            QuorumResult::Accepted => {
                                info!(msg_id = envelope.message_id,
                                    "CriticalUser: quorum reached — delivered");
                            }
                            QuorumResult::Inconsistent => {
                                warn!(msg_id = envelope.message_id,
                                    "CriticalUser: INCONSISTENT_REPLY — rejected");
                                self.metrics.messages_dropped += 1;
                            }
                        }
                    } else {
                        info!(msg_id = envelope.message_id, "UserData delivered to self");
                    }
                } else {
                    self.forward_user_data(envelope, &mut out);
                }
            }

            MessageKind::Invite { .. } => {
                if let Err(e) = self.limits.check_invite(&from) {
                    warn!(?e, "Invite quota exceeded");
                }
            }
        }

        out
    }

    /// Forward a UserData envelope with LocalMinimum fallback — §7.3.
    ///
    /// Strategy:
    /// 1. Try greedy XOR hop.
    /// 2. On LocalMinimum, attempt the L-slot peer (small-world bypass).
    /// 3. If still stuck, emit a bounded FIND_NODE repair toward the target.
    /// 4. Drop the original message.
    fn forward_user_data(
        &mut self,
        envelope: Envelope,
        out:      &mut Vec<(NodeId, Envelope)>,
    ) {
        let target = envelope.to;

        let hop = match next_hop(&self.id, &target, &self.table) {
            NextHopResult::Forward(n) => Some(n),
            NextHopResult::Deliver    => {
                // Shouldn't happen (caller already checked), but handle safely.
                info!(msg_id = envelope.message_id, "UserData: deliver at relay");
                return;
            }
            NextHopResult::LocalMinimum => None,
        };

        if let Some(next) = hop {
            self.send_user_hop(envelope, next, out);
            return;
        }

        // LocalMinimum reached — try L-slot bypass first (§7.3).
        if let Some(l_slot) = self.table.long_jump_slot() {
            let l_peer = l_slot.peer_id;
            if l_peer != self.id {
                debug!(target = %target, via = %l_peer, "LocalMinimum: trying L-slot bypass");
                self.send_user_hop(envelope.clone(), l_peer, out);
                // Also emit a repair FIND_NODE so future hops improve.
                self.emit_find_node_repair(target, out);
                return;
            }
        }

        // No bypass available — emit a repair FIND_NODE and drop the message.
        warn!(target = %target, "UserData: local minimum, no L-slot — dropping and repairing");
        self.emit_find_node_repair(target, out);
        self.metrics.messages_dropped += 1;
    }

    fn send_user_hop(
        &mut self,
        mut envelope: Envelope,
        next:         NodeId,
        out:          &mut Vec<(NodeId, Envelope)>,
    ) {
        envelope.ttl = envelope.ttl.saturating_sub(1);
        if envelope.ttl == 0 {
            warn!(msg_id = envelope.message_id, "UserData TTL exhausted");
            self.metrics.messages_dropped += 1;
            return;
        }
        envelope.to = next;
        sign_envelope(&mut envelope, &self.keypair.signing);
        out.push((next, envelope));
        self.metrics.messages_forwarded += 1;
    }

    /// Emit a single bounded FIND_NODE toward `target` to repair routing — §7.3.
    fn emit_find_node_repair(&mut self, target: NodeId, out: &mut Vec<(NodeId, Envelope)>) {
        // Route the FIND_NODE itself greedily (best we can do).
        let next = match next_hop(&self.id, &target, &self.table) {
            NextHopResult::Forward(n) => n,
            _ => match self.table.slots.first() {
                Some(s) => s.peer_id,
                None    => return,
            },
        };
        let env = self.make_find(next, target);
        out.push((next, env));
    }

    /// Send a CriticalUser message with route_fork — §7.3, §12.4.
    ///
    /// Picks two first-hops with different `referrer_id` and forks the message
    /// over both.  The receiver uses `QuorumTracker` to accept only when ≥ 2
    /// matching copies arrive.
    ///
    /// Returns the pair of outbound `(destination, envelope)` entries, or a
    /// single entry if the table does not have two independent peers.
    pub fn send_critical(
        &mut self,
        to:      NodeId,
        payload: Vec<u8>,
    ) -> Vec<(NodeId, Envelope)> {
        let msg_id = self.next_msg_id();
        let base = Envelope {
            to,
            from:       self.id,
            message_id: msg_id,
            ttl:        20,
            class:      MessageClass::CriticalUser,
            route_fork: false,
            body:       MessageKind::UserData { payload },
            signature:  vec![],
        };

        let mut out = Vec::new();

        match pick_fork_hops(&self.table) {
            Some((hop_a, hop_b)) => {
                let (mut env_a, mut env_b) = fork_envelopes(base, hop_a, hop_b);
                sign_envelope(&mut env_a, &self.keypair.signing);
                sign_envelope(&mut env_b, &self.keypair.signing);
                out.push((hop_a, env_a));
                out.push((hop_b, env_b));
            }
            None => {
                // Only one peer available — send single copy.
                let mut single = base;
                if let Some(peer) = self.table.slots.first() {
                    let peer_id = peer.peer_id;
                    single.to = peer_id;
                    sign_envelope(&mut single, &self.keypair.signing);
                    out.push((peer_id, single));
                }
            }
        }

        out
    }

    // ── Produce a ping envelope for a peer (called by keep-alive timer).
    pub fn make_ping(&mut self, peer: NodeId) -> Envelope {
        let mut env = self.make_envelope(peer, MessageKind::Ping { nonce: self.msg_counter });
        sign_envelope(&mut env, &self.keypair.signing);
        env
    }

    /// Produce a FIND_NODE envelope for repair.
    pub fn make_find(&mut self, peer: NodeId, target: NodeId) -> Envelope {
        let mut env =
            self.make_envelope(peer, MessageKind::FindNode { target, count: 3 });
        sign_envelope(&mut env, &self.keypair.signing);
        env
    }

    fn make_envelope(&mut self, to: NodeId, body: MessageKind) -> Envelope {
        Envelope {
            to,
            from:       self.id,
            message_id: self.next_msg_id(),
            ttl:        20,
            class:      MessageClass::Normal,
            route_fork: false,
            body,
            signature:  vec![],
        }
    }

    /// Evaluate and return the current recovery state.
    pub fn recovery_state(&self) -> RecoveryState {
        evaluate_recovery(&self.table)
    }

    /// Run a repair tick — §4, §5.1, §10.7.
    ///
    /// Emits:
    /// - FIND_NODE toward each pseudo-random repair target.
    /// - FIND_NODE toward the L-slot target if the L-slot is currently empty,
    ///   so the table can discover a suitable long-jump peer.
    pub fn repair_tick(&mut self) -> Vec<(NodeId, Envelope)> {
        let mut out = Vec::new();
        self.metrics.repair_ticks += 1;

        // Regular repair targets.
        let targets = repair_targets(self.config.epoch_seed, &self.id, REPAIR_TARGETS_PER_TICK);
        for target in targets {
            let next = match next_hop(&self.id, &target, &self.table) {
                NextHopResult::Forward(n) => n,
                _ => match self.table.slots.first() {
                    Some(s) => s.peer_id,
                    None    => continue,
                },
            };
            let env = self.make_find(next, target);
            out.push((next, env));
        }

        // L-slot seeding: if no L-slot peer yet, emit a FIND_NODE toward the
        // pseudo-random long-jump target — whoever is closest will fill the slot.
        if self.table.long_jump_slot().is_none() {
            let lj_target = long_jump_target(self.config.epoch_seed, &self.id);
            if let Some(next) = self.table.slots.first().map(|s| s.peer_id) {
                let env = self.make_find(next, lj_target);
                out.push((next, env));
                debug!(target = %lj_target, "repair_tick: seeding L-slot via FIND_NODE");
            }
        }

        out
    }

    /// Inverted connect mechanism — §3.
    ///
    /// Returns a list of `(NodeId, ReachList)` for peers whose descriptor we
    /// hold but who are **not** currently in our peer table as live slots.
    /// The caller (transport layer) should initiate outgoing connections to
    /// these peers — this is the "sеть сама подключается" behaviour.
    pub fn pending_inverted_connects(&self) -> Vec<(NodeId, ReachList)> {
        let live: std::collections::HashSet<NodeId> =
            self.table.slots.iter().filter(|s| !s.should_evict()).map(|s| s.peer_id).collect();

        self.store
            .all_descriptors()
            .into_iter()
            .filter(|d| d.node_id != self.id && !live.contains(&d.node_id))
            .map(|d| (d.node_id, d.reach))
            .collect()
    }
}
