//! Canonical teleoperation message contract.
//!
//! This crate is the single decoupling boundary between the teleop tier, the
//! communication tier, and the robot tier. It is intentionally `std`-only so the
//! whole workspace compiles offline; in production add `serde` + a wire codec
//! (`prost` / `flatbuffers` / `postcard`) behind a feature.

pub mod capability;
pub mod codec;
pub mod command;
pub mod episode;
pub mod error;
pub mod feedback;
pub mod geometry;
pub mod header;

pub use capability::{Capabilities, GripperSpec, Negotiated};
pub use codec::{Codec, PlaceholderCodec};
pub use command::{CartesianTarget, CommandBody, GripperCommand, JointTargets, TeleopCommand};
pub use episode::{EpisodeEvent, EpisodeOutcome};
pub use error::MessageError;
pub use feedback::{
    EndEffectorState, FeedbackBody, HealthState, JointState, RobotFeedback, Status,
};
pub use geometry::{Pose, Quat, Twist, Vec3, Wrench};
pub use header::{ControlMode, MessageHeader, SessionId, PROTOCOL_VERSION};
