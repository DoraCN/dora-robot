//! Teleop tier.
//!
//! Every input device implements [`TeleopDevice`], mapping its native input into
//! a **canonical command** (device-side retargeting lives here). Bilateral
//! devices render [`RobotFeedback`] in `apply_feedback`.

pub mod devices;

use tr_messages::{Capabilities, RobotFeedback, TeleopCommand};

pub trait TeleopDevice: Send {
    /// What this device can emit (modes, rate, force-feedback support).
    fn capabilities(&self) -> Capabilities;

    /// Produce the next canonical command, or `None` if no new input.
    fn poll(&mut self) -> Option<TeleopCommand>;

    /// Render feedback (e.g. drive haptics on a master arm). No-op by default.
    fn apply_feedback(&mut self, _feedback: &RobotFeedback) {}
}

pub use devices::{DemoCartesianDevice, GamepadDevice, IsomorphicArmDevice, KeyboardDevice, VrDevice};
