//! In-process loopback transport for tests (no zenoh/network).
//!
//! [`LoopbackTransport::pair`] returns two connected endpoints backed by `std`
//! channels — reliable in-process delivery. Used by the **fast-gate** end-to-end
//! tests (leader ↔ follower in one process) so they need no network dependency
//! (see `docs/specs/001-so101-teleop-record/plan.md` §9).

use crate::error::TransportError;
use crate::qos::Channel;
use crate::transport::{Inbound, LinkState, Transport};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

/// One endpoint of an in-process duplex loopback link.
pub struct LoopbackTransport {
    tx: Sender<(Channel, Vec<u8>)>,
    rx: Receiver<(Channel, Vec<u8>)>,
}

impl LoopbackTransport {
    /// Two connected endpoints (e.g. operator ↔ robot bridges in one process).
    /// What endpoint A `send`s, endpoint B `recv`s, and vice-versa.
    pub fn pair() -> (Self, Self) {
        let (tx_a, rx_a) = mpsc::channel(); // A.send -> B.recv
        let (tx_b, rx_b) = mpsc::channel(); // B.send -> A.recv
        (
            LoopbackTransport { tx: tx_a, rx: rx_b },
            LoopbackTransport { tx: tx_b, rx: rx_a },
        )
    }
}

impl Transport for LoopbackTransport {
    fn send(&mut self, channel: Channel, payload: &[u8]) -> Result<(), TransportError> {
        self.tx
            .send((channel, payload.to_vec()))
            .map_err(|_| TransportError::Closed)
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, TransportError> {
        match self.rx.recv_timeout(timeout) {
            Ok((channel, frame)) => Ok(Some(Inbound { channel, frame })),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(TransportError::Closed),
        }
    }

    fn link_state(&self) -> LinkState {
        LinkState::Up
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplex_roundtrip() {
        let (mut a, mut b) = LoopbackTransport::pair();
        a.send(Channel::Control, b"cmd").unwrap();
        let got = b.recv(Duration::from_millis(50)).unwrap().unwrap();
        assert_eq!(got.channel, Channel::Control);
        assert_eq!(got.frame, b"cmd");

        b.send(Channel::Episode, b"end").unwrap();
        let got = a.recv(Duration::from_millis(50)).unwrap().unwrap();
        assert_eq!(got.channel, Channel::Episode);
        assert_eq!(got.frame, b"end");
    }

    #[test]
    fn recv_times_out_when_empty() {
        let (_a, mut b) = LoopbackTransport::pair();
        assert!(b.recv(Duration::from_millis(1)).unwrap().is_none());
    }

    #[test]
    fn recv_errors_when_peer_dropped() {
        let (a, mut b) = LoopbackTransport::pair();
        drop(a);
        assert!(matches!(
            b.recv(Duration::from_millis(1)),
            Err(TransportError::Closed) | Ok(None)
        ));
    }
}
