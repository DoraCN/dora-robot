//! Robot -> operator feedback.

use crate::geometry::{Pose, Wrench};
use crate::header::MessageHeader;

#[derive(Debug, Clone, PartialEq)]
pub struct JointState {
    pub positions: Vec<f64>,
    pub velocities: Vec<f64>,
    pub efforts: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndEffectorState {
    pub name: String,
    pub pose: Pose,
    pub wrench: Option<Wrench>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Ok,
    Degraded,
    EStopped,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub health: HealthState,
    pub message: String,
    /// Most recent round-trip estimate, nanoseconds.
    pub rtt_nanos: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FeedbackBody {
    Joint(JointState),
    EndEffector(EndEffectorState),
    /// Force feedback destined for a haptic master (bilateral only).
    Force(Wrench),
    Status(Status),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RobotFeedback {
    pub header: MessageHeader,
    pub body: FeedbackBody,
}
