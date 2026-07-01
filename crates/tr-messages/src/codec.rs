//! Pluggable wire codec.
//!
//! The skeleton ships [`PlaceholderCodec`], which deliberately returns
//! [`MessageError::CodecUnimplemented`]. Replace it in production with a real
//! implementation backed by `prost` (Protobuf), `flatbuffers`, or `postcard`.
//! The [`crate::header::PROTOCOL_VERSION`] field gates incompatibilities.

use crate::command::TeleopCommand;
use crate::episode::EpisodeEvent;
use crate::error::MessageError;
use crate::feedback::RobotFeedback;

pub trait Codec: Send + Sync {
    fn encode_command(&self, cmd: &TeleopCommand) -> Result<Vec<u8>, MessageError>;
    fn decode_command(&self, bytes: &[u8]) -> Result<TeleopCommand, MessageError>;
    fn encode_feedback(&self, fb: &RobotFeedback) -> Result<Vec<u8>, MessageError>;
    fn decode_feedback(&self, bytes: &[u8]) -> Result<RobotFeedback, MessageError>;
    fn encode_episode(&self, ev: &EpisodeEvent) -> Result<Vec<u8>, MessageError>;
    fn decode_episode(&self, bytes: &[u8]) -> Result<EpisodeEvent, MessageError>;
}

/// Placeholder; wire it to a real serializer before going on-air.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlaceholderCodec;

impl Codec for PlaceholderCodec {
    fn encode_command(&self, _cmd: &TeleopCommand) -> Result<Vec<u8>, MessageError> {
        Err(MessageError::CodecUnimplemented)
    }
    fn decode_command(&self, _bytes: &[u8]) -> Result<TeleopCommand, MessageError> {
        Err(MessageError::CodecUnimplemented)
    }
    fn encode_feedback(&self, _fb: &RobotFeedback) -> Result<Vec<u8>, MessageError> {
        Err(MessageError::CodecUnimplemented)
    }
    fn decode_feedback(&self, _bytes: &[u8]) -> Result<RobotFeedback, MessageError> {
        Err(MessageError::CodecUnimplemented)
    }
    fn encode_episode(&self, _ev: &EpisodeEvent) -> Result<Vec<u8>, MessageError> {
        Err(MessageError::CodecUnimplemented)
    }
    fn decode_episode(&self, _bytes: &[u8]) -> Result<EpisodeEvent, MessageError> {
        Err(MessageError::CodecUnimplemented)
    }
}
