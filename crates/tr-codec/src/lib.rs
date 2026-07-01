//! Postcard-based [`tr_messages::Codec`] implementation.
//!
//! Postcard is a compact `serde`-based wire format (`no_std`, no schema) —
//! chosen for the initial Rust ↔ Rust inter-machine bridge path (M1).
//! The canonical types gain `Serialize` / `Deserialize` when the
//! `tr-messages` `serde` feature is on (see `constitution.md` C8/K3).

use tr_messages::{
    Codec, EpisodeEvent, MessageError, RobotFeedback, TeleopCommand,
};

pub struct PostcardCodec;

impl Codec for PostcardCodec {
    fn encode_command(&self, cmd: &TeleopCommand) -> Result<Vec<u8>, MessageError> {
        postcard::to_stdvec(cmd).map_err(|e| MessageError::Decode(e.to_string()))
    }
    fn decode_command(&self, bytes: &[u8]) -> Result<TeleopCommand, MessageError> {
        postcard::from_bytes(bytes).map_err(|e| MessageError::Decode(e.to_string()))
    }
    fn encode_feedback(&self, fb: &RobotFeedback) -> Result<Vec<u8>, MessageError> {
        postcard::to_stdvec(fb).map_err(|e| MessageError::Decode(e.to_string()))
    }
    fn decode_feedback(&self, bytes: &[u8]) -> Result<RobotFeedback, MessageError> {
        postcard::from_bytes(bytes).map_err(|e| MessageError::Decode(e.to_string()))
    }
    fn encode_episode(&self, ev: &EpisodeEvent) -> Result<Vec<u8>, MessageError> {
        postcard::to_stdvec(ev).map_err(|e| MessageError::Decode(e.to_string()))
    }
    fn decode_episode(&self, bytes: &[u8]) -> Result<EpisodeEvent, MessageError> {
        postcard::from_bytes(bytes).map_err(|e| MessageError::Decode(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_messages::{
        CartesianTarget, CommandBody, ControlMode, EpisodeOutcome, FeedbackBody,
        JointState, JointTargets, MessageHeader, Pose, Vec3,
    };

    fn codec() -> PostcardCodec {
        PostcardCodec
    }

    fn header(sid: u64, mode: ControlMode) -> MessageHeader {
        MessageHeader::new(sid, "test", mode)
    }

    #[test]
    fn command_roundtrip_joint() {
        let cmd = TeleopCommand {
            header: header(1, ControlMode::JointTargets),
            body: CommandBody::Joint(JointTargets {
                positions: vec![0.1, -0.2, 0.3, 0.0, 0.5, 0.0],
                velocities: Some(vec![0.0; 6]),
                efforts: None,
            }),
        };
        let bytes = codec().encode_command(&cmd).unwrap();
        let decoded = codec().decode_command(&bytes).unwrap();
        assert_eq!(decoded.body, cmd.body);
    }

    #[test]
    fn command_roundtrip_cartesian() {
        let cmd = TeleopCommand {
            header: header(2, ControlMode::CartesianPose),
            body: CommandBody::Cartesian(CartesianTarget {
                frame: "base".into(),
                pose: Pose {
                    position: Vec3::new(0.4, 0.1, 0.3),
                    ..Default::default()
                },
            }),
        };
        let bytes = codec().encode_command(&cmd).unwrap();
        let decoded = codec().decode_command(&bytes).unwrap();
        assert_eq!(decoded.body, cmd.body);
    }

    #[test]
    fn feedback_roundtrip() {
        let fb = RobotFeedback {
            header: header(3, ControlMode::JointTargets),
            body: FeedbackBody::Joint(JointState {
                positions: vec![0.09, -0.19, 0.31, 0.0, 0.49, 0.0],
                velocities: vec![0.0; 6],
                efforts: vec![0.0; 6],
            }),
        };
        let bytes = codec().encode_feedback(&fb).unwrap();
        let decoded = codec().decode_feedback(&bytes).unwrap();
        assert_eq!(decoded.body, fb.body);
    }

    #[test]
    fn episode_roundtrip_start() {
        let ev = EpisodeEvent::Start;
        let bytes = codec().encode_episode(&ev).unwrap();
        let decoded = codec().decode_episode(&bytes).unwrap();
        assert_eq!(decoded, ev);
    }

    #[test]
    fn episode_roundtrip_end_fail() {
        let ev = EpisodeEvent::End {
            outcome: EpisodeOutcome::Fail,
        };
        let bytes = codec().encode_episode(&ev).unwrap();
        let decoded = codec().decode_episode(&bytes).unwrap();
        assert_eq!(decoded, ev);
    }
}
