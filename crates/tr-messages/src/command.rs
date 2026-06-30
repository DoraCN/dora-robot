//! Operator -> robot canonical commands.

use crate::geometry::{Pose, Twist};
use crate::header::MessageHeader;

/// End-effector pose in a named reference frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CartesianTarget {
    pub frame: String,
    pub pose: Pose,
}

/// Per-joint targets. `velocities`/`efforts` are optional feed-forward terms.
#[derive(Debug, Clone, PartialEq)]
pub struct JointTargets {
    pub positions: Vec<f64>,
    pub velocities: Option<Vec<f64>>,
    pub efforts: Option<Vec<f64>>,
}

/// Normalized gripper command (`position`/`force` in 0.0..=1.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GripperCommand {
    pub position: f64,
    pub force: Option<f64>,
}

/// The semantic payload of a command. Retargeting/IK never appear here — they
/// happen at the edges (teleop adapter or robot driver).
#[derive(Debug, Clone, PartialEq)]
pub enum CommandBody {
    Cartesian(CartesianTarget),
    Joint(JointTargets),
    Twist(Twist),
    Gripper(GripperCommand),
    /// Multiple sub-commands keyed by end-effector name (e.g. "left_arm").
    Composite(Vec<(String, CommandBody)>),
    Custom {
        type_id: u32,
        payload: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeleopCommand {
    pub header: MessageHeader,
    pub body: CommandBody,
}
