//! Transport tier: carry opaque framed bytes across any medium.
//!
//! This crate is deliberately decoupled from [`tr-messages`]: it moves bytes,
//! not semantics. The teleop/robot tiers serialize the canonical contract; the
//! transport just frames and delivers it with the requested QoS. TCP and UDP
//! backends are real (`std::net`); other media are stubs to be filled in.

pub mod backends;
pub mod error;
pub mod framing;
pub mod qos;
pub mod transport;

pub use error::TransportError;
pub use framing::{FrameDecoder, FrameEncoder};
pub use qos::{Channel, History, Qos, Reliability};
pub use transport::{Inbound, LinkState, Transport};
