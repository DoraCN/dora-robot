//! The pluggable transport trait.

use crate::error::TransportError;
use crate::qos::Channel;
use std::time::Duration;

/// A received frame.
#[derive(Debug, Clone)]
pub struct Inbound {
    pub channel: Channel,
    pub frame: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Degraded,
    Down,
}

/// Moves opaque framed bytes across some medium. Selected at deploy time; the
/// teleop and robot tiers never name a concrete transport.
pub trait Transport: Send {
    /// Send one framed payload on a logical channel.
    fn send(&mut self, channel: Channel, payload: &[u8]) -> Result<(), TransportError>;

    /// Receive the next frame, blocking up to `timeout`. `Ok(None)` on timeout.
    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, TransportError>;

    fn link_state(&self) -> LinkState;
}
