//! Minimal geometry primitives (SI units: metres, radians, newtons).

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

/// Unit quaternion (w, x, y, z). `Default` is the identity rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Quat {
    fn default() -> Self {
        Self {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

/// Rigid-body pose: position + orientation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pose {
    pub position: Vec3,
    pub orientation: Quat,
}

/// Spatial velocity.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Twist {
    pub linear: Vec3,
    pub angular: Vec3,
}

/// Force + torque (used for bilateral force feedback).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Wrench {
    pub force: Vec3,
    pub torque: Vec3,
}
