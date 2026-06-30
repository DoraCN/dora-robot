//! Length-prefixed framing for stream transports.
//!
//! Wire layout: `[u32 BE payload_len][u8 channel][payload bytes]`.

use crate::qos::Channel;

/// Stateless frame encoder.
pub struct FrameEncoder;

impl FrameEncoder {
    pub fn encode(channel: Channel, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + payload.len());
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.push(channel.as_u8());
        out.extend_from_slice(payload);
        out
    }
}

/// Incremental frame decoder for byte streams (TCP, serial, ...).
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Pull the next complete frame, if one is buffered.
    pub fn next_frame(&mut self) -> Option<(Channel, Vec<u8>)> {
        const HEADER: usize = 5;
        if self.buf.len() < HEADER {
            return None;
        }
        let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        let total = HEADER + len;
        if self.buf.len() < total {
            return None;
        }
        let channel = Channel::from_u8(self.buf[4])?;
        let payload = self.buf[HEADER..total].to_vec();
        self.buf.drain(0..total);
        Some((channel, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_across_split_reads() {
        let frame = FrameEncoder::encode(Channel::Control, b"hello");
        let mut dec = FrameDecoder::new();
        // feed byte-by-byte to prove the decoder reassembles partial reads.
        for b in &frame {
            assert!(dec.next_frame().is_none());
            dec.feed(&[*b]);
        }
        let (ch, payload) = dec.next_frame().expect("a frame");
        assert_eq!(ch, Channel::Control);
        assert_eq!(payload, b"hello");
        assert!(dec.next_frame().is_none());
    }
}
