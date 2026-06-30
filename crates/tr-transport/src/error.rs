//! Transport errors.

use std::fmt;

#[derive(Debug)]
pub enum TransportError {
    Io(std::io::Error),
    /// Backend exists but the operation is not implemented in the skeleton.
    Unsupported(&'static str),
    Closed,
    BadFrame(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Io(e) => write!(f, "io error: {e}"),
            TransportError::Unsupported(s) => write!(f, "unsupported: {s}"),
            TransportError::Closed => write!(f, "transport closed"),
            TransportError::BadFrame(s) => write!(f, "bad frame: {s}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        TransportError::Io(e)
    }
}
