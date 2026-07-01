//! [`So101Leader`] — teleop-device (human-driven) role adapter for the SO-101.
//!
//! Torque is **disabled** so a human can backdrive the leader. `poll()` reads the
//! current calibrated joint positions and emits them as a canonical `JointTargets`
//! command. In production the blocking `read_joints` call is replaced by a
//! non-blocking async bridge (task C4); until then the unit tests drive it via a
//! small `block_on` runtime.

use crate::arm::So101Arm;
use feetech_servo_sdk::MotorBus;
use std::time::{SystemTime, UNIX_EPOCH};
use tr_messages::{
    Capabilities, CommandBody, ControlMode, JointTargets, MessageHeader, SessionId, TeleopCommand,
};
use tr_teleop::TeleopDevice;

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// TeleopDevice role: human moves the arm to drive a remote follower.
pub struct So101Leader<B: MotorBus> {
    arm: So101Arm<B>,
    rt: tokio::runtime::Runtime,
    seq: u64,
    session_id: SessionId,
    source_id: String,
}

impl<B: MotorBus> So101Leader<B> {
    pub fn new(
        mut arm: So101Arm<B>,
        session_id: SessionId,
        source_id: impl Into<String>,
    ) -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        // Leader is backdrivable — torque off.
        let _ = rt.block_on(async { arm.set_torque(false).await });
        Self {
            arm,
            rt,
            seq: 0,
            session_id,
            source_id: source_id.into(),
        }
    }
}

impl<B: MotorBus> TeleopDevice for So101Leader<B> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            dof: 6,
            supported_modes: vec![ControlMode::JointTargets],
            force_feedback: false,
            max_rate_hz: 100,
            frames: vec!["base".into()],
            end_effectors: vec!["tcp".into()],
            gripper: None,
        }
    }

    fn poll(&mut self) -> Option<TeleopCommand> {
        let joints_f32 = self
            .rt
            .block_on(async { self.arm.read_joints().await })
            .ok()?;
        let positions: Vec<f64> = joints_f32.into_iter().map(|j| j as f64).collect();
        let mut h = MessageHeader::new(
            self.session_id,
            &self.source_id,
            ControlMode::JointTargets,
        );
        h.seq = self.seq;
        h.stamp_nanos = now_nanos();
        self.seq += 1;
        Some(TeleopCommand {
            header: h,
            body: CommandBody::Joint(JointTargets {
                positions,
                velocities: None,
                efforts: None,
            }),
        })
    }

    fn apply_feedback(&mut self, _fb: &tr_messages::RobotFeedback) {}
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use super::*;
    use crate::arm::So101Arm;
    use crate::config::So101Config;
    use crate::DOF;
    use feetech_servo_sdk::MockBus;

    const IDS: [u8; DOF] = [1, 2, 3, 4, 5, 6];

    #[test]
    fn poll_reads_calibrated_mock_positions() {
        let bus = MockBus::new(&IDS);
        bus.set_servo_position_instant(2, 1.0);
        bus.set_servo_position_instant(5, -0.5);
        let arm = So101Arm::new(bus, So101Config::default());
        let mut leader = So101Leader::new(arm, 42, "test_leader");
        let cmd = leader.poll().expect("should emit a command");
        match cmd.body {
            CommandBody::Joint(jt) => {
                // identity calibration → joint ≈ raw (± mock tick quantisation)
                assert!((jt.positions[1] - 1.0).abs() < 0.01, "j1={}", jt.positions[1]);
                assert!((jt.positions[4] + 0.5).abs() < 0.01, "j5={}", jt.positions[4]);
            }
            _ => panic!("expected Joint command"),
        }
    }

    #[test]
    fn poll_increments_seq() {
        let arm = So101Arm::new(MockBus::new(&IDS), So101Config::default());
        let mut leader = So101Leader::new(arm, 1, "l");
        let h1 = leader.poll().unwrap().header;
        let h2 = leader.poll().unwrap().header;
        assert_eq!(h2.seq, h1.seq + 1);
    }
}
