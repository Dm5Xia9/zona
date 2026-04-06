//! Lazy descriptor gossip — §4, §8.1.
//!
//! Descriptors are propagated lazily: only on request or on push-based
//! AnnounceDescriptor events, not by constant broadcast.

use zona_p2p_types::{Descriptor, ProtocolError};
use zona_p2p_overlay::DescriptorStore;

/// Push an incoming descriptor into the local store.
///
/// Returns true if the store was updated (so caller can propagate further).
pub fn push_descriptor(
    store:  &mut DescriptorStore,
    desc:   Descriptor,
    height: Option<u64>,
) -> Result<bool, ProtocolError> {
    let updated = store.upsert(desc, height)?;
    if updated {
        tracing::debug!("gossip: descriptor updated in store");
    }
    Ok(updated)
}

/// Collect descriptors to push to a new peer during handshake or PEX.
/// Limits output to `limit` entries (avoid flooding — §4).
pub fn descriptors_for_peer(
    store: &DescriptorStore,
    limit: usize,
) -> Vec<Descriptor> {
    store.all_descriptors().into_iter().take(limit).collect()
}
