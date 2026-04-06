//! Length-prefixed bincode frame codec.

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use zona_p2p_types::Envelope;

/// Maximum frame size — 4 MiB.
const MAX_FRAME: usize = 4 * 1024 * 1024;

pub struct EnvelopeCodec;

impl Decoder for EnvelopeCodec {
    type Item  = Envelope;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Envelope>, Self::Error> {
        if src.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes([src[0], src[1], src[2], src[3]]) as usize;
        if len > MAX_FRAME {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("frame too large: {len}"),
            ));
        }
        if src.len() < 4 + len {
            src.reserve(4 + len - src.len());
            return Ok(None);
        }
        src.advance(4);
        let data = src.split_to(len);
        let envelope: Envelope = bincode::deserialize(&data).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        Ok(Some(envelope))
    }
}

impl Encoder<Envelope> for EnvelopeCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: Envelope, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let data = bincode::serialize(&item).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        if data.len() > MAX_FRAME {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "serialised envelope exceeds MAX_FRAME",
            ));
        }
        dst.put_u32(data.len() as u32);
        dst.put_slice(&data);
        Ok(())
    }
}
