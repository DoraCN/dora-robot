//! QoS profiles (DDS / ROS 2 inspired) and logical channels.

/// Logical streams multiplexed over one or more transports. Media (cameras)
/// gets its own channel so it cannot starve the control loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Channel {
    Handshake = 0,
    Control = 1,
    Feedback = 2,
    Telemetry = 3,
    Media = 4,
}

impl Channel {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0 => Some(Channel::Handshake),
            1 => Some(Channel::Control),
            2 => Some(Channel::Feedback),
            3 => Some(Channel::Telemetry),
            4 => Some(Channel::Media),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reliability {
    /// Newest-wins; a dropped high-rate control sample beats a stale queued one.
    BestEffort,
    /// Retransmit until delivered (handshake, mode change, e-stop).
    Reliable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    KeepLast(u32),
    KeepAll,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Qos {
    pub reliability: Reliability,
    pub history: History,
    pub deadline_hz: Option<u32>,
    pub priority: u8,
}

impl Qos {
    /// High-rate control / haptic feedback: newest wins.
    pub fn realtime() -> Self {
        Self {
            reliability: Reliability::BestEffort,
            history: History::KeepLast(1),
            deadline_hz: None,
            priority: 200,
        }
    }
    /// Handshake, mode change, e-stop: must arrive.
    pub fn reliable() -> Self {
        Self {
            reliability: Reliability::Reliable,
            history: History::KeepAll,
            deadline_hz: None,
            priority: 255,
        }
    }
}
