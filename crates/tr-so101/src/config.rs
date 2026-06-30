//! SO-101 configuration: servo IDs + per-joint calibration & limits.
//!
//! Calibration maps **raw servo radians ↔ canonical joint radians** so a leader
//! and a follower (two separate physical arms) agree on one joint frame. Physical
//! zeroing is done with the SDK's `set_mid`/`sync_set_middle_positions`; these
//! per-joint `sign`/`offset` handle any residual mismatch.

use crate::DOF;
use std::f32::consts::PI;

/// Per-joint calibration + limits (radians).
#[derive(Debug, Clone, Copy)]
pub struct JointCalib {
    /// Direction: `+1.0` or `-1.0`.
    pub sign: f32,
    /// Raw-zero offset (radians) subtracted on read / added on write.
    pub offset_rad: f32,
    /// Lower joint limit (radians), applied on write.
    pub lower: f32,
    /// Upper joint limit (radians), applied on write.
    pub upper: f32,
}

impl JointCalib {
    /// Raw servo radians → canonical joint radians.
    pub fn raw_to_joint(self, raw: f32) -> f32 {
        self.sign * (raw - self.offset_rad)
    }

    /// Canonical joint radians → raw servo radians.
    pub fn joint_to_raw(self, joint: f32) -> f32 {
        self.sign * joint + self.offset_rad
    }

    /// Clamp a canonical joint target to `[lower, upper]`.
    pub fn clamp(self, joint: f32) -> f32 {
        joint.clamp(self.lower, self.upper)
    }
}

impl Default for JointCalib {
    fn default() -> Self {
        Self {
            sign: 1.0,
            offset_rad: 0.0,
            lower: -PI,
            upper: PI,
        }
    }
}

/// One SO-101 instance's config (servo IDs + per-joint calibration).
#[derive(Debug, Clone)]
pub struct So101Config {
    pub ids: [u8; DOF],
    pub joints: [JointCalib; DOF],
}

impl Default for So101Config {
    fn default() -> Self {
        Self {
            ids: [1, 2, 3, 4, 5, 6],
            joints: [JointCalib::default(); DOF],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_calib_is_inverse() {
        let c = JointCalib::default();
        let raw = 0.37_f32;
        assert!((c.joint_to_raw(c.raw_to_joint(raw)) - raw).abs() < 1e-6);
    }

    #[test]
    fn signed_offset_calib_is_inverse() {
        let c = JointCalib {
            sign: -1.0,
            offset_rad: 0.2,
            lower: -PI,
            upper: PI,
        };
        let raw = 0.37_f32;
        assert!((c.joint_to_raw(c.raw_to_joint(raw)) - raw).abs() < 1e-6);
        let joint = 0.5_f32;
        assert!((c.raw_to_joint(c.joint_to_raw(joint)) - joint).abs() < 1e-6);
    }

    #[test]
    fn clamp_to_limits() {
        let c = JointCalib {
            sign: 1.0,
            offset_rad: 0.0,
            lower: -1.0,
            upper: 1.0,
        };
        assert_eq!(c.clamp(5.0), 1.0);
        assert_eq!(c.clamp(-5.0), -1.0);
        assert_eq!(c.clamp(0.3), 0.3);
    }
}
