//! TCP listener that accepts connections and yields TcpStreams.

use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use zona_p2p_types::ProtocolError;

pub struct Listener {
    inner:     TcpListener,
    pub local: SocketAddr,
}

impl Listener {
    pub async fn bind(addr: SocketAddr) -> Result<Self, ProtocolError> {
        let inner = TcpListener::bind(addr).await?;
        let local = inner.local_addr()?;
        tracing::info!(%local, "transport listener started");
        Ok(Listener { inner, local })
    }

    /// Accept the next incoming TCP stream.
    pub async fn accept(&self) -> Result<(TcpStream, SocketAddr), ProtocolError> {
        self.inner.accept().await.map_err(ProtocolError::Io)
    }
}
