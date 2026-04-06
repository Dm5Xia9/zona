use ed25519_dalek::{Signer, Verifier, SigningKey, VerifyingKey, Signature};
use zona_p2p_types::{Descriptor, Envelope, ProtocolError, NodeId};
use crate::identity::derive_node_id;

/// Sign a descriptor's canonical payload and fill `descriptor.signature`.
pub fn sign_descriptor(desc: &mut Descriptor, signing: &SigningKey) {
    let payload = desc.signing_payload();
    let sig: Signature = signing.sign(&payload);
    desc.signature = sig.to_bytes().to_vec();
}

/// Verify a descriptor's signature against its embedded public key.
pub fn verify_descriptor(desc: &Descriptor) -> Result<(), ProtocolError> {
    let vk = VerifyingKey::from_bytes(&desc.pubkey)
        .map_err(|_| ProtocolError::InvalidSignature { node_id: desc.node_id.to_string() })?;

    // Check NodeId = SHA-256(pubkey).
    let expected_id = derive_node_id(&desc.pubkey);
    if expected_id != desc.node_id {
        return Err(ProtocolError::InvalidSignature { node_id: desc.node_id.to_string() });
    }

    let sig_bytes: [u8; 64] = desc.signature.as_slice().try_into()
        .map_err(|_| ProtocolError::InvalidSignature { node_id: desc.node_id.to_string() })?;
    let sig = Signature::from_bytes(&sig_bytes);
    let payload = desc.signing_payload();
    vk.verify(&payload, &sig)
        .map_err(|_| ProtocolError::InvalidSignature { node_id: desc.node_id.to_string() })
}

/// Sign an envelope's canonical payload and fill `envelope.signature` — §10.4.
pub fn sign_envelope(env: &mut Envelope, signing: &SigningKey) {
    let payload = env.signing_payload();
    let sig: Signature = signing.sign(&payload);
    env.signature = sig.to_bytes().to_vec();
}

/// Verify an envelope's signature using the sender's raw public key — §10.4.
///
/// Returns `Ok(())` if the signature is valid or the envelope is unsigned
/// (empty signature slice — allowed for loopback/test messages).
/// Returns `Err` if the signature is present but invalid.
pub fn verify_envelope(env: &Envelope, sender_pubkey: &[u8; 32]) -> Result<(), ProtocolError> {
    if env.signature.is_empty() {
        return Ok(());
    }
    let vk = VerifyingKey::from_bytes(sender_pubkey)
        .map_err(|_| ProtocolError::EnvelopeSignatureInvalid {
            sender_id: env.from.to_string(),
        })?;
    let arr: [u8; 64] = env.signature.as_slice().try_into().map_err(|_| {
        ProtocolError::EnvelopeSignatureInvalid { sender_id: env.from.to_string() }
    })?;
    let sig = Signature::from_bytes(&arr);
    let payload = env.signing_payload();
    vk.verify(&payload, &sig).map_err(|_| ProtocolError::EnvelopeSignatureInvalid {
        sender_id: env.from.to_string(),
    })
}

/// Sign an arbitrary byte payload; returns 64-byte signature as Vec.
pub fn sign_bytes(payload: &[u8], signing: &SigningKey) -> Vec<u8> {
    let sig: Signature = signing.sign(payload);
    sig.to_bytes().to_vec()
}

/// Verify an arbitrary byte payload against a given public key.
pub fn verify_bytes(
    payload:   &[u8],
    sig_bytes: &[u8],
    pubkey:    &[u8; 32],
    signer_id: &NodeId,
) -> Result<(), ProtocolError> {
    let vk = VerifyingKey::from_bytes(pubkey)
        .map_err(|_| ProtocolError::InvalidSignature { node_id: signer_id.to_string() })?;
    let arr: [u8; 64] = sig_bytes.try_into()
        .map_err(|_| ProtocolError::InvalidSignature { node_id: signer_id.to_string() })?;
    let sig = Signature::from_bytes(&arr);
    vk.verify(payload, &sig)
        .map_err(|_| ProtocolError::InvalidSignature { node_id: signer_id.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::NodeKeypair;
    use zona_p2p_types::{reach_spec::ReachList, Descriptor};

    fn make_desc(kp: &NodeKeypair) -> Descriptor {
        Descriptor {
            node_id:     kp.node_id,
            pubkey:      kp.raw_public(),
            reach:       ReachList(vec![]),
            version:     1,
            provisional: false,
            signature:   vec![0u8; 64],
        }
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let kp = NodeKeypair::generate();
        let mut desc = make_desc(&kp);
        sign_descriptor(&mut desc, &kp.signing);
        assert!(verify_descriptor(&desc).is_ok(), "verify failed");
    }

    #[test]
    fn tampered_descriptor_fails() {
        let kp = NodeKeypair::generate();
        let mut desc = make_desc(&kp);
        sign_descriptor(&mut desc, &kp.signing);
        desc.version = 999;
        assert!(verify_descriptor(&desc).is_err());
    }
}
