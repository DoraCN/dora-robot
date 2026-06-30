//! Kinematics: forward/inverse. Retargeting from canonical Cartesian targets to
//! robot-specific joints happens here.

use crate::error::RobotError;
use tr_messages::Pose;

pub trait Kinematics: Send {
    /// Solve joints that place `ee` at `target`, warm-started from `seed`.
    fn ik(&self, ee: &str, target: &Pose, seed: &[f64]) -> Result<Vec<f64>, RobotError>;
    /// Forward kinematics: pose of the end-effector for the given joints.
    fn fk(&self, joints: &[f64]) -> Result<Pose, RobotError>;
}

/// Placeholder solver: IK echoes the seed, FK returns identity. Replace with a
/// real solver (`k` / KDL / TRAC-IK) backed by `nalgebra`.
#[derive(Debug, Clone, Copy)]
pub struct IdentityKinematics {
    pub dof: usize,
}

impl IdentityKinematics {
    pub fn new(dof: usize) -> Self {
        Self { dof }
    }
}

impl Kinematics for IdentityKinematics {
    fn ik(&self, _ee: &str, _target: &Pose, seed: &[f64]) -> Result<Vec<f64>, RobotError> {
        if seed.len() == self.dof {
            Ok(seed.to_vec())
        } else {
            Ok(vec![0.0; self.dof])
        }
    }
    fn fk(&self, _joints: &[f64]) -> Result<Pose, RobotError> {
        Ok(Pose::default())
    }
}
