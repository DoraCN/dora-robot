//! The pluggable robot driver trait.

use crate::error::RobotError;
use tr_messages::{Capabilities, RobotFeedback, TeleopCommand};

/// Implemented per robot. Consumes canonical commands (running IK/retargeting
/// internally) and exposes state + e-stop. The teleop/comm tiers never name a
/// concrete driver.
pub trait RobotDriver: Send {
    fn capabilities(&self) -> Capabilities;

    /// Apply a canonical command (post-negotiation). Implementations enforce
    /// joint/velocity/workspace limits.
    fn command(&mut self, cmd: &TeleopCommand) -> Result<(), RobotError>;

    /// Read current state as a feedback message (joint state, EE pose, wrench).
    fn read_state(&mut self) -> Result<RobotFeedback, RobotError>;

    /// Latch a safe state immediately (honored locally, even if the link is down).
    fn e_stop(&mut self) -> Result<(), RobotError>;
}
