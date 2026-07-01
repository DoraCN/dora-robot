//! Capability descriptors and handshake negotiation (WebRTC-style offer/answer).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::header::ControlMode;

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GripperSpec {
    pub max_force: f64,
    pub stroke: f64,
}

/// Advertised by both the teleop device and the robot driver before streaming.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Capabilities {
    pub dof: u32,
    pub supported_modes: Vec<ControlMode>,
    pub force_feedback: bool,
    pub max_rate_hz: u32,
    pub frames: Vec<String>,
    pub end_effectors: Vec<String>,
    pub gripper: Option<GripperSpec>,
}

/// The agreed parameters for a session, computed from both ends' capabilities.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Negotiated {
    pub mode: ControlMode,
    pub rate_hz: u32,
    pub force_feedback: bool,
}

/// Mode preference order used by [`Capabilities::negotiate`].
/// Cartesian first because it is the most device/robot-agnostic.
const MODE_PREFERENCE: [ControlMode; 6] = [
    ControlMode::CartesianPose,
    ControlMode::JointTargets,
    ControlMode::Twist,
    ControlMode::Composite,
    ControlMode::Gripper,
    ControlMode::Custom,
];

impl Capabilities {
    /// Compute the intersection of two capability sets, or `None` if the ends
    /// share no common control mode.
    pub fn negotiate(&self, other: &Capabilities) -> Option<Negotiated> {
        let mode = MODE_PREFERENCE.into_iter().find(|m| {
            self.supported_modes.contains(m) && other.supported_modes.contains(m)
        })?;
        Some(Negotiated {
            mode,
            rate_hz: self.max_rate_hz.min(other.max_rate_hz),
            force_feedback: self.force_feedback && other.force_feedback,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(modes: Vec<ControlMode>, rate: u32, ff: bool) -> Capabilities {
        Capabilities {
            dof: 6,
            supported_modes: modes,
            force_feedback: ff,
            max_rate_hz: rate,
            frames: vec!["base".into()],
            end_effectors: vec!["tcp".into()],
            gripper: None,
        }
    }

    #[test]
    fn picks_common_mode_min_rate_and_anded_ff() {
        let teleop = caps(
            vec![ControlMode::JointTargets, ControlMode::CartesianPose],
            1000,
            true,
        );
        let robot = caps(vec![ControlMode::CartesianPose], 250, false);
        let n = teleop.negotiate(&robot).unwrap();
        assert_eq!(n.mode, ControlMode::CartesianPose);
        assert_eq!(n.rate_hz, 250);
        assert!(!n.force_feedback);
    }

    #[test]
    fn no_common_mode_is_none() {
        let a = caps(vec![ControlMode::JointTargets], 100, false);
        let b = caps(vec![ControlMode::Twist], 100, false);
        assert!(a.negotiate(&b).is_none());
    }
}
