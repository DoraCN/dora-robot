//! Message header shared by every command and feedback message.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Negotiated, per-session identifier.
pub type SessionId = u64;

/// Wire-format protocol version. Bumped on any breaking schema change; gated by
/// [`MessageHeader::protocol_version`].
pub const PROTOCOL_VERSION: u16 = 1;

/// The semantic space a command/stream is expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ControlMode {
    /// End-effector Cartesian pose (default for heterogeneous master/slave & VR).
    CartesianPose,
    /// Per-joint targets (isomorphic master arm with matched DoF).
    JointTargets,
    /// Linear + angular velocity (mobile bases, velocity servoing).
    Twist,
    /// Gripper open/close.
    Gripper,
    /// Multiple of the above keyed by end-effector (dual-arm, humanoid).
    Composite,
    /// Device-specific escape hatch.
    Custom,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MessageHeader {
    pub protocol_version: u16,
    pub session_id: SessionId,
    /// Logical sender, e.g. "vr_left", "arm0".
    pub source_id: String,
    /// Monotonic per source; a gap indicates loss.
    pub seq: u64,
    /// Sender clock in nanoseconds (see architecture §9 time sync).
    pub stamp_nanos: u64,
    pub control_mode: ControlMode,
}

impl MessageHeader {
    pub fn new(session_id: SessionId, source_id: impl Into<String>, mode: ControlMode) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            session_id,
            source_id: source_id.into(),
            seq: 0,
            stamp_nanos: 0,
            control_mode: mode,
        }
    }
}
