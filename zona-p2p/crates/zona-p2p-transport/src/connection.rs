//! Single TCP peer connection: send/receive Envelopes.

use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use zona_p2p_types::{Envelope, NodeId, ProtocolError};
use crate::codec::EnvelopeCodec;

/// A framed connection to a single remote peer.
pub struct PeerConnection {
    pub remote_id: NodeId,
    framed:        Framed<TcpStream, EnvelopeCodec>,
}

impl PeerConnection {
    pub fn new(remote_id: NodeId, stream: TcpStream) -> Self {
        PeerConnection {
            remote_id,
            framed: Framed::new(stream, EnvelopeCodec),
        }
    }

    /// Send an envelope to the peer.
    pub async fn send(&mut self, envelope: Envelope) -> Result<(), ProtocolError> {
        self.framed
            .send(envelope)
            .await
            .map_err(ProtocolError::Io)
    }

    /// Receive the next envelope from the peer.
    pub async fn recv(&mut self) -> Option<Result<Envelope, ProtocolError>> {
        self.framed.next().await.map(|r| r.map_err(ProtocolError::Io))
    }
}
