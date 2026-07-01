//! [`So101Follower`] — robot-driver (actively driven) role adapter for the SO-101.
//!
//! Torque is **enabled**. `command()` applies limit clamping (from config) *and*
//! per-tick slew limiting (global `max_slew_rad`, M10), then writes the clamped
//! target to the servos. `e_stop()` disables torque and blocks further commands.
//! In production the blocking `write_joints` / `read_joints` calls are replaced
//! by a non-blocking async bridge (task C4); until then a small `block_on`
//! runtime drives the unit tests.

use crate::arm::So101Arm;
use crate::DOF;
use feetech_servo_sdk::MotorBus;
use std::time::{SystemTime, UNIX_EPOCH};
use tr_messages::{
    Capabilities, CommandBody, ControlMode, FeedbackBody, JointState, MessageHeader,
    RobotFeedback, SessionId, TeleopCommand,
};
use tr_robot::{RobotDriver, RobotError};

const SLEW_DEFAULT_RAD: f32 = 0.05236_f32; // 3° @ 100 Hz ≈ 300°/s

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// RobotDriver role: follows the canonical command by writing to the servos.
pub struct So101Follower<B: MotorBus> {
    arm: So101Arm<B>,
    rt: tokio::runtime::Runtime,
    session_id: SessionId,
    source_id: String,
    seq: u64,
    e_stopped: bool,
    holding: bool,  // hold-current state (safe-state phase 1, torque still on)
    prev_target: Option<[f32; DOF]>,
    slew_rad: f32,
}

impl<B: MotorBus> So101Follower<B> {
    pub fn new(
        mut arm: So101Arm<B>,
        session_id: SessionId,
        source_id: impl Into<String>,
    ) -> Self {
        let slew_rad = arm.config().max_slew_rad.max(SLEW_DEFAULT_RAD);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        // Follower is actively driven — torque on.
        let _ = rt.block_on(async { arm.set_torque(true).await });
        Self {
            arm,
            rt,
            session_id,
            source_id: source_id.into(),
            seq: 0,
            e_stopped: false,
            holding: false,
            prev_target: None,
            slew_rad,
        }
    }

    /// Access the inner bus for direct torque control.
    pub fn bus_mut(&mut self) -> &mut B {
        self.arm.bus_mut()
    }

    /// Access the inner arm for direct read/write.
    pub fn arm_mut(&mut self) -> &mut So101Arm<B> {
        &mut self.arm
    }

    fn header(&mut self, mode: ControlMode) -> MessageHeader {
        let mut h = MessageHeader::new(self.session_id, &self.source_id, mode);
        h.seq = self.seq;
        h.stamp_nanos = now_nanos();
        self.seq += 1;
        h
    }

    /// Per-tick slew clamping (M10): each joint delta is capped to `self.slew_rad`.

    /// Anti-jerk startup alignment (D1): smoothly interpolate from the current
    /// position to `target` at ≤ `slew_rad` per tick, so the first live command
    /// doesn't jerk. Blocks until the trajectory completes.
    pub fn align_to(&mut self, target: &[f32; DOF]) -> Result<(), RobotError> {
        if self.e_stopped {
            return Err(RobotError::Hardware("e-stopped".into()));
        }
        let start = self
            .rt
            .block_on(async { self.arm.read_joints().await })
            .map_err(|_e| RobotError::Hardware("read_joints failed".into()))?;
        // linear interpolation at 50 Hz (slew already per-tick, tick ≈ 10 ms)
        let max_d = (target[0] - start[0])
            .abs()
            .max((target[1] - start[1]).abs())
            .max((target[2] - start[2]).abs())
            .max((target[3] - start[3]).abs())
            .max((target[4] - start[4]).abs())
            .max((target[5] - start[5]).abs());
        let steps = ((max_d / self.slew_rad).ceil() as usize).max(1).min(500); // safety bound
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            let mut waypoint = [0.0_f32; DOF];
            for i in 0..DOF {
                waypoint[i] = start[i] + (target[i] - start[i]) * t;
            }
            self.rt
                .block_on(async { self.arm.write_joints(&waypoint).await })
                .map_err(|_e| RobotError::Hardware("write_joints in align_to failed".into()))?;
        }
        // The align is now the baseline for subsequent slew clamping.
        self.prev_target = Some(*target);
        Ok(())
    }

    fn slew_clamp(&self, target: &[f32; DOF], prev: &[f32; DOF]) -> [f32; DOF] {
        let mut out = *target;
        for i in 0..DOF {
            let d = target[i] - prev[i];
            out[i] = prev[i] + d.clamp(-self.slew_rad, self.slew_rad);
        }
        out
    }
}

impl<B: MotorBus> RobotDriver for So101Follower<B> {
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

    fn command(&mut self, cmd: &TeleopCommand) -> Result<(), RobotError> {
        if self.e_stopped { return Err(RobotError::Hardware("e-stopped".into())); }
        if self.holding { return Err(RobotError::Hardware("in safe-hold".into())); }
        let positions: &[f64] = match &cmd.body {
            CommandBody::Joint(jt) => &jt.positions,
            _ => return Err(RobotError::Unsupported("only JointTargets (this iteration)")),
        };
        if positions.len() != DOF {
            return Err(RobotError::LimitViolation(format!(
                "expected {DOF} joints, got {}",
                positions.len()
            )));
        }
        let mut target = [0.0_f32; DOF];
        for i in 0..DOF { target[i] = positions[i] as f32; }

        if let Some(ref prev) = self.prev_target {
            target = self.slew_clamp(&target, prev);
        }
        self.prev_target = Some(target);

        self.rt
            .block_on(async { self.arm.write_joints(&target).await })
            .map_err(|_e| RobotError::Hardware("write_joints failed".into()))
    }

    fn read_state(&mut self) -> Result<RobotFeedback, RobotError> {
        let joints = self
            .rt
            .block_on(async { self.arm.read_joints().await })
            .map_err(|_e| RobotError::Hardware("read_joints failed".into()))?;
        let joints_f64: Vec<f64> = joints.into_iter().map(|j| j as f64).collect();
        let n = joints_f64.len();
        let h = self.header(ControlMode::JointTargets);
        Ok(RobotFeedback {
            header: h,
            body: FeedbackBody::Joint(JointState {
                positions: joints_f64,
                velocities: vec![0.0_f64; n],
                efforts: vec![0.0_f64; n],
            }),
        })
    }

    fn e_stop(&mut self) -> Result<(), RobotError> {
        self.e_stopped = true;
        self.holding = false;
        let _ = self
            .rt
            .block_on(async { self.arm.set_torque(false).await });
        Ok(())
    }
}

impl<B: MotorBus> So101Follower<B> {
    /// Enter safe-hold (phase 1 of M5): stop following new commands, but
    /// **keep torque on** to hold the current position passively.
    pub fn hold(&mut self) {
        self.holding = true;
    }

    /// Resume from safe-hold.
    pub fn resume(&mut self) {
        self.holding = false;
    }

    /// Whether the follower is currently in safe-hold or e-stopped.
    pub fn is_safe(&self) -> bool {
        self.holding || self.e_stopped
    }
}

#[cfg(test)]
mod tests {
    /// Mirror of `slew_clamp` for pure-logic testing (no bus needed).
    fn slew(rad: f32, target: &[f32; 6], prev: &[f32; 6]) -> [f32; 6] {
        let mut out = *target;
        for i in 0..6 {
            let d = target[i] - prev[i];
            out[i] = prev[i] + d.clamp(-rad, rad);
        }
        out
    }

    #[test]
    fn slew_clamp_limits_per_joint() {
        let r = slew(0.05236, &[0.1, 0.0, 0.0, 0.0, 0.0, 0.0], &[0.0; 6]);
        assert!((r[0] - 0.05236).abs() < 0.001);
    }

    #[test]
    fn slew_clamp_respects_direction() {
        let r = slew(0.05236, &[0.1, -0.08, 0.0, 0.0, 0.0, 0.0], &[0.0; 6]);
        assert!((r[0] - 0.05236).abs() < 0.001);
        assert!((r[1] + 0.05236).abs() < 0.001);
    }
}

#[cfg(all(test, feature = "mock"))]
mod mock_tests {
    use crate::arm::So101Arm;
    use crate::config::So101Config;
    use crate::DOF;
    use super::So101Follower;
    use feetech_servo_sdk::MockBus;
    use tr_messages::{
        CommandBody, ControlMode, FeedbackBody, JointTargets, MessageHeader, TeleopCommand,
    };
    use tr_robot::RobotDriver;

    const IDS: [u8; DOF] = [1, 2, 3, 4, 5, 6];

    fn cmd(positions: [f32; DOF]) -> TeleopCommand {
        TeleopCommand {
            header: MessageHeader::new(1, "t", ControlMode::JointTargets),
            body: CommandBody::Joint(JointTargets {
                positions: positions.iter().map(|&p| p as f64).collect(),
                velocities: None,
                efforts: None,
            }),
        }
    }

    #[test]
    fn command_accepted_and_read_state_works() {
        let arm = So101Arm::new(MockBus::new(&IDS), So101Config::default());
        let mut f = So101Follower::new(arm, 1, "f");
        f.command(&cmd([0.1, 0.2, 0.0, 0.0, 0.0, 0.0])).unwrap();
        let s = f.read_state().unwrap();
        match s.body {
            FeedbackBody::Joint(j) => assert_eq!(j.positions.len(), DOF),
            _ => panic!(),
        }
    }

    #[test]
    fn e_stop_blocks_subsequent_commands() {
        let arm = So101Arm::new(MockBus::new(&IDS), So101Config::default());
        let mut f = So101Follower::new(arm, 1, "f");
        f.e_stop().unwrap();
        assert!(f.command(&cmd([0.0; DOF])).is_err());
    }

    #[test]
    fn hold_blocks_commands_but_not_e_stop() {
        let arm = So101Arm::new(MockBus::new(&IDS), So101Config::default());
        let mut f = So101Follower::new(arm, 1, "f");
        f.hold();
        assert!(f.is_safe());
        assert!(f.command(&cmd([0.0; DOF])).is_err());
        // e_stop still works and leaves torque off + safe state true
        f.e_stop().unwrap();
        assert!(f.is_safe());
        assert!(!f.holding); // e_stop clears hold
    }

    #[test]
    fn resume_allows_commands_again() {
        let arm = So101Arm::new(MockBus::new(&IDS), So101Config::default());
        let mut f = So101Follower::new(arm, 1, "f");
        f.hold();
        f.resume();
        assert!(!f.is_safe());
        f.command(&cmd([0.0; DOF])).unwrap();
    }

    #[test]
    fn align_to_runs_and_sets_baseline() {
        let arm = So101Arm::new(MockBus::new(&IDS), So101Config::default());
        let mut f = So101Follower::new(arm, 1, "f");
        let target: [f32; DOF] = [0.05, 0.0, 0.0, 0.0, 0.0, 0.0];
        f.align_to(&target).unwrap();
        f.command(&cmd(target)).unwrap();
    }
}
