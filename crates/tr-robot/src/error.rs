//! Robot tier errors.

use std::fmt;

#[derive(Debug)]
pub enum RobotError {
    Unsupported(&'static str),
    IkFailed,
    LimitViolation(String),
    Hardware(String),
}

impl fmt::Display for RobotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RobotError::Unsupported(s) => write!(f, "unsupported: {s}"),
            RobotError::IkFailed => write!(f, "inverse kinematics failed"),
            RobotError::LimitViolation(s) => write!(f, "limit violation: {s}"),
            RobotError::Hardware(s) => write!(f, "hardware error: {s}"),
        }
    }
}

impl std::error::Error for RobotError {}
