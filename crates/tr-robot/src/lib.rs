//! Robot tier.
//!
//! A [`RobotDriver`] consumes canonical commands and drives hardware; retargeting
//! / inverse kinematics live here via [`Kinematics`], using a [`RobotModel`].

pub mod driver;
pub mod error;
pub mod kinematics;
pub mod model;
pub mod sim;

pub use driver::RobotDriver;
pub use error::RobotError;
pub use kinematics::{IdentityKinematics, Kinematics};
pub use model::{JointLimit, RobotModel};
pub use sim::SimRobot;
