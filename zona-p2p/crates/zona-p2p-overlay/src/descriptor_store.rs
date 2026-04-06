//! Local descriptor cache — §8.1.
//!
//! Merge rules (§8.1):
//! 1. Prefer the record associated with a higher known finalised BFT height.
//! 2. At equal height, prefer higher `version`.
//! 3. Tie-break: lower content hash (deterministic).

use std::collections::HashMap;
use sha2::{Sha256, Digest};
use zona_p2p_types::{NodeId, Descriptor, ProtocolError};
use zona_p2p_crypto::verify_descriptor;

/// A cached descriptor entry with optional finalisation metadata.
#[derive(Clone, Debug)]
pub struct CachedDescriptor {
    pub descriptor:        Descriptor,
    /// BFT-finalised height at which this descriptor was confirmed.
    /// `None` = provisional (gossip-only).
    pub finalised_height:  Option<u64>,
}

/// Local store of peer descriptors with §8.1 merge semantics.
pub struct DescriptorStore {
    entries: HashMap<NodeId, CachedDescriptor>,
}

impl DescriptorStore {
    pub fn new() -> Self {
        DescriptorStore { entries: HashMap::new() }
    }

    /// Insert or update a descriptor if it is valid and preferred by §8.1 rules.
    /// Returns true if the store was updated.
    pub fn upsert(
        &mut self,
        descriptor: Descriptor,
        finalised_height: Option<u64>,
    ) -> Result<bool, ProtocolError> {
        // Verify signature first.
        verify_descriptor(&descriptor)?;

        let key = descriptor.node_id;
        let incoming = CachedDescriptor { descriptor, finalised_height };

        let should_replace = match self.entries.get(&key) {
            None => true,
            Some(existing) => self.prefer_incoming(&incoming, existing),
        };

        if should_replace {
            self.entries.insert(key, incoming);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get a descriptor by NodeId.
    pub fn get(&self, node_id: &NodeId) -> Option<&CachedDescriptor> {
        self.entries.get(node_id)
    }

    /// All known descriptors (for PEX building, etc.).
    pub fn all_descriptors(&self) -> Vec<Descriptor> {
        self.entries.values().map(|c| c.descriptor.clone()).collect()
    }

    /// §8.1 merge logic: does `incoming` win over `existing`?
    fn prefer_incoming(&self, incoming: &CachedDescriptor, existing: &CachedDescriptor) -> bool {
        // Rule 1: higher finalised height wins.
        match (incoming.finalised_height, existing.finalised_height) {
            (Some(a), Some(b)) if a != b => return a > b,
            (Some(_), None) => return true,
            (None, Some(_)) => return false,
            _ => {}
        }
        // Rule 2: higher version wins.
        if incoming.descriptor.version != existing.descriptor.version {
            return incoming.descriptor.version > existing.descriptor.version;
        }
        // Rule 3: deterministic tie-break — lower content hash wins.
        let h_new = content_hash(&incoming.descriptor);
        let h_old = content_hash(&existing.descriptor);
        h_new < h_old
    }
}

fn content_hash(d: &Descriptor) -> [u8; 32] {
    let payload = d.signing_payload();
    let mut h = Sha256::new();
    h.update(&payload);
    h.finalize().into()
}

impl Default for DescriptorStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zona_p2p_types::reach_spec::ReachList;
    use zona_p2p_crypto::{NodeKeypair, sign_descriptor};

    fn signed_desc(kp: &NodeKeypair, version: u64) -> Descriptor {
        let mut d = Descriptor {
            node_id:     kp.node_id,
            pubkey:      kp.raw_public(),
            reach:       ReachList(vec![]),
            version,
            provisional: false,
            signature:   vec![0u8; 64],
        };
        sign_descriptor(&mut d, &kp.signing);
        d
    }

    #[test]
    fn higher_version_wins() {
        let kp = NodeKeypair::generate();
        let mut store = DescriptorStore::new();
        let d1 = signed_desc(&kp, 1);
        let d2 = signed_desc(&kp, 2);
        store.upsert(d1, None).unwrap();
        assert!(store.upsert(d2, None).unwrap());
        let cached = store.get(&kp.node_id).unwrap();
        assert_eq!(cached.descriptor.version, 2);
    }

    #[test]
    fn lower_version_rejected() {
        let kp = NodeKeypair::generate();
        let mut store = DescriptorStore::new();
        let d2 = signed_desc(&kp, 2);
        let d1 = signed_desc(&kp, 1);
        store.upsert(d2, None).unwrap();
        // Older version should not replace.
        assert!(!store.upsert(d1, None).unwrap());
    }

    #[test]
    fn finalised_height_takes_priority() {
        let kp = NodeKeypair::generate();
        let mut store = DescriptorStore::new();
        let d1 = signed_desc(&kp, 5); // version 5 gossip
        let d2 = signed_desc(&kp, 1); // version 1 but height 10
        store.upsert(d1, None).unwrap();
        // BFT-finalised record at height 10 should win even with older version.
        assert!(store.upsert(d2, Some(10)).unwrap());
        assert_eq!(store.get(&kp.node_id).unwrap().descriptor.version, 1);
    }
}
