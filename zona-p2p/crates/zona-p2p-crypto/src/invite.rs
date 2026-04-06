//! Invite token — §8: Sign_P(new_NodeId, expiry, rights).

use serde::{Deserialize, Serialize};
use zona_p2p_types::{NodeId, ProtocolError};
use crate::signing::{sign_bytes, verify_bytes};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteToken {
    /// Invited node's future NodeId.
    pub invitee_id: NodeId,
    /// Unix timestamp after which invite is invalid.
    pub expires_at: u64,
    /// Inviting node's NodeId.
    pub issuer_id:  NodeId,
    /// Issuer's public key (for verification without lookup).
    pub issuer_pk:  [u8; 32],
    /// Signature of (invitee_id, expires_at, issuer_id).
    pub signature:  Vec<u8>,
}

/// Create a signed invite from node P for a new node with `invitee_id`.
pub fn create_invite(
    issuer_kp:  &crate::keypair::NodeKeypair,
    invitee_id: NodeId,
    expires_at: u64,
) -> InviteToken {
    let payload = invite_payload(&invitee_id, expires_at, &issuer_kp.node_id);
    let sig = sign_bytes(&payload, &issuer_kp.signing);
    InviteToken {
        invitee_id,
        expires_at,
        issuer_id: issuer_kp.node_id,
        issuer_pk: issuer_kp.raw_public(),
        signature: sig,
    }
}

/// Verify the invite's signature.
pub fn verify_invite(token: &InviteToken, now_unix: u64) -> Result<(), ProtocolError> {
    if now_unix > token.expires_at {
        return Err(ProtocolError::InvalidSignature { node_id: token.invitee_id.to_string() });
    }
    let payload = invite_payload(&token.invitee_id, token.expires_at, &token.issuer_id);
    verify_bytes(&payload, &token.signature, &token.issuer_pk, &token.issuer_id)
}


fn invite_payload(invitee_id: &NodeId, expires_at: u64, issuer_id: &NodeId) -> Vec<u8> {
    bincode::serialize(&(invitee_id, expires_at, issuer_id))
        .expect("infallible serialise")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::NodeKeypair;

    #[test]
    fn invite_roundtrip() {
        let issuer = NodeKeypair::generate();
        let invitee = NodeKeypair::generate();
        let token = create_invite(&issuer, invitee.node_id, 9_999_999_999);
        assert!(verify_invite(&token, 0).is_ok());
    }

    #[test]
    fn expired_invite_fails() {
        let issuer = NodeKeypair::generate();
        let invitee = NodeKeypair::generate();
        let token = create_invite(&issuer, invitee.node_id, 100);
        assert!(verify_invite(&token, 101).is_err());
    }
}
