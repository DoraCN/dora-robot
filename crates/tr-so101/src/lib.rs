//! SO-101 arm — **one hardware type, two roles**.
//!
//! C1 delivers the shared hardware driver [`So101Arm`] (generic over the feetech
//! [`MotorBus`], so the same code runs on real hardware or [`MockBus`]) plus its
//! [`So101Config`]/calibration. The role adapters `So101Leader`
//! (`tr_teleop::TeleopDevice`) and `So101Follower` (`tr_robot::RobotDriver`)
//! arrive in tasks C2/C3.
//!
//! Units: radians; joint order = servo IDs 1..6 (base, shoulder, elbow,
//! wrist_roll, wrist_flex, gripper) — see `constitution.md` C4.

pub mod arm;
pub mod config;

/// Degrees of freedom of an SO-101 (5 arm joints + gripper).
pub const DOF: usize = 6;

pub use arm::So101Arm;
pub use config::{JointCalib, So101Config};

// Re-export the SDK pieces callers need.
pub use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus, ServoError};

#[cfg(feature = "mock")]
pub use feetech_servo_sdk::MockBus;
