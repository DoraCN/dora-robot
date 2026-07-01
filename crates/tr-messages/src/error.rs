//! Contract-level errors.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MessageError {
    CodecUnimplemented,
    Decode(String),
    VersionMismatch { expected: u16, got: u16 },
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageError::CodecUnimplemented => {
                write!(f, "codec not implemented (replace PlaceholderCodec)")
            }
            MessageError::Decode(m) => write!(f, "decode error: {m}"),
            MessageError::VersionMismatch { expected, got } => {
                write!(f, "protocol version mismatch: expected {expected}, got {got}")
            }
        }
    }
}

impl std::error::Error for MessageError {}
