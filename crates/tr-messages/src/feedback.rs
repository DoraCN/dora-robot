//! Robot -> operator feedback.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::geometry::{Pose, Wrench};
use crate::header::MessageHeader;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct JointState {
    pub positions: Vec<f64>,
    pub velocities: Vec<f64>,
    pub efforts: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EndEffectorState {
    pub name: String,
    pub pose: Pose,
    pub wrench: Option<Wrench>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum HealthState {
    Ok,
    Degraded,
    EStopped,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Status {
    pub health: HealthState,
    pub message: String,
    /// Most recent round-trip estimate, nanoseconds.
    pub rtt_nanos: u64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FeedbackBody {
    Joint(JointState),
    EndEffector(EndEffectorState),
    /// Force feedback destined for a haptic master (bilateral only).
    Force(Wrench),
    Status(Status),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RobotFeedback {
    pub header: MessageHeader,
    pub body: FeedbackBody,
}
