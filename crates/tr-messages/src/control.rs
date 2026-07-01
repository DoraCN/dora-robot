//! Daemon control commands and status.
//!
//! Carried on `tr/<id>/command` (postcard) and `tr/<id>/status` (JSON).

use crate::episode::EpisodeOutcome;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// State-machine command sent from leader to follower-daemon.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ControlCommand {
    /// Enable torque + start DORA dataflow (Idle → Ready).
    TorqueOn,
    /// Disable torque + stop DORA dataflow (any → Idle).
    TorqueOff,
    /// Begin a new recording episode (Ready → Recording).
    StartRecord { task: String },
    /// Finish the current episode with an outcome (Recording → Ready).
    EndRecord { outcome: EpisodeOutcome },
    /// Discard current episode, immediately start a new one (Recording → Recording).
    ReRecord,
    /// Stop recording, return to Ready (Recording → Ready).
    Stop,
}

/// Daemon status published by follower-daemon every second (JSON).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DaemonStatus {
    pub state: String,
    pub torque_on: bool,
    pub recording: bool,
    pub episode: Option<u32>,
    pub frame_count: u64,
    pub fps: f32,
    pub error: Option<String>,
}

impl DaemonStatus {
    /// Build a status snapshot for the given FSM state.
    pub fn new(state: impl Into<String>, torque_on: bool) -> Self {
        Self {
            state: state.into(),
            torque_on,
            recording: false,
            episode: None,
            frame_count: 0,
            fps: 0.0,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ControlCommand ──────────────────────────────────────────────

    #[test]
    fn cmd_torque_on() {
        let cmd = ControlCommand::TorqueOn;
        assert_eq!(cmd, ControlCommand::TorqueOn);
    }

    #[test]
    fn cmd_torque_off() {
        let cmd = ControlCommand::TorqueOff;
        assert_ne!(cmd, ControlCommand::TorqueOn);
    }

    #[test]
    fn cmd_start_record() {
        let cmd = ControlCommand::StartRecord {
            task: "pick cube".into(),
        };
        assert!(matches!(cmd, ControlCommand::StartRecord { .. }));
    }

    #[test]
    fn cmd_end_record() {
        let cmd = ControlCommand::EndRecord {
            outcome: EpisodeOutcome::Success,
        };
        assert!(matches!(
            cmd,
            ControlCommand::EndRecord {
                outcome: EpisodeOutcome::Success
            }
        ));
    }

    #[test]
    fn cmd_rerecord() {
        assert_eq!(ControlCommand::ReRecord, ControlCommand::ReRecord);
    }

    #[test]
    fn cmd_stop() {
        let cmd = ControlCommand::Stop;
        assert_ne!(cmd, ControlCommand::TorqueOn);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn cmd_serde_roundtrip_torque_on() {
        let cmd = ControlCommand::TorqueOn;
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ControlCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn cmd_serde_roundtrip_torque_off() {
        let cmd = ControlCommand::TorqueOff;
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ControlCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn cmd_serde_roundtrip_start_record() {
        let cmd = ControlCommand::StartRecord {
            task: "pick cube".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ControlCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn cmd_serde_roundtrip_end_record_success() {
        let cmd = ControlCommand::EndRecord {
            outcome: EpisodeOutcome::Success,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ControlCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn cmd_serde_roundtrip_end_record_fail() {
        let cmd = ControlCommand::EndRecord {
            outcome: EpisodeOutcome::Fail,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ControlCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn cmd_serde_roundtrip_rerecord() {
        let cmd = ControlCommand::ReRecord;
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ControlCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn cmd_serde_roundtrip_stop() {
        let cmd = ControlCommand::Stop;
        let json = serde_json::to_string(&cmd).unwrap();
        let back: ControlCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    // ── DaemonStatus ────────────────────────────────────────────────

    #[test]
    fn status_new_idle() {
        let s = DaemonStatus::new("IDLE", false);
        assert_eq!(s.state, "IDLE");
        assert!(!s.torque_on);
        assert!(!s.recording);
        assert_eq!(s.episode, None);
    }

    #[test]
    fn status_new_ready() {
        let s = DaemonStatus::new("READY", true);
        assert_eq!(s.state, "READY");
        assert!(s.torque_on);
        assert!(!s.recording);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn status_json_roundtrip() {
        let s = DaemonStatus {
            state: "RECORDING".into(),
            torque_on: true,
            recording: true,
            episode: Some(3),
            frame_count: 1420,
            fps: 30.0,
            error: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: DaemonStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.state, "RECORDING");
        assert!(back.torque_on);
        assert!(back.recording);
        assert_eq!(back.episode, Some(3));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn status_json_roundtrip_with_error() {
        let s = DaemonStatus {
            state: "OFFLINE".into(),
            torque_on: false,
            recording: false,
            episode: None,
            frame_count: 0,
            fps: 0.0,
            error: Some("USB disconnected".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: DaemonStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.error, Some("USB disconnected".into()));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn status_json_state_values() {
        for state in ["IDLE", "READY", "RECORDING", "OFFLINE"] {
            let s = DaemonStatus::new(state, false);
            let json = serde_json::to_string(&s).unwrap();
            assert!(json.contains(state), "missing state {state} in JSON");
        }
    }
}
