//! Kinematic simulation driver — a runnable reference `RobotDriver`.

use crate::driver::RobotDriver;
use crate::error::RobotError;
use crate::kinematics::{IdentityKinematics, Kinematics};
use crate::model::RobotModel;
use std::time::{SystemTime, UNIX_EPOCH};
use tr_messages::{
    Capabilities, CommandBody, ControlMode, FeedbackBody, HealthState, JointState, MessageHeader,
    RobotFeedback, SessionId, Status, TeleopCommand,
};

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

pub struct SimRobot {
    model: RobotModel,
    kin: IdentityKinematics,
    joints: Vec<f64>,
    health: HealthState,
    session_id: SessionId,
    source_id: String,
    seq: u64,
}

impl SimRobot {
    pub fn new(model: RobotModel, session_id: SessionId, source_id: impl Into<String>) -> Self {
        let dof = model.dof as usize;
        Self {
            kin: IdentityKinematics::new(dof),
            joints: vec![0.0; dof],
            model,
            health: HealthState::Ok,
            session_id,
            source_id: source_id.into(),
            seq: 0,
        }
    }

    fn apply_body(&mut self, body: &CommandBody) -> Result<(), RobotError> {
        match body {
            CommandBody::Joint(jt) => {
                if jt.positions.len() != self.joints.len() {
                    return Err(RobotError::LimitViolation(format!(
                        "expected {} joints, got {}",
                        self.joints.len(),
                        jt.positions.len()
                    )));
                }
                self.joints.copy_from_slice(&jt.positions);
                self.model.clamp_positions(&mut self.joints);
                Ok(())
            }
            CommandBody::Cartesian(ct) => {
                let mut q = self.kin.ik("tcp", &ct.pose, &self.joints)?;
                self.model.clamp_positions(&mut q);
                self.joints = q;
                Ok(())
            }
            CommandBody::Twist(_) => Ok(()), // a mobile base would integrate here
            CommandBody::Gripper(_) => Ok(()),
            CommandBody::Composite(parts) => {
                for (_ee, sub) in parts {
                    self.apply_body(sub)?;
                }
                Ok(())
            }
            CommandBody::Custom { .. } => Err(RobotError::Unsupported("custom command")),
        }
    }

    fn header(&mut self, mode: ControlMode) -> MessageHeader {
        let mut h = MessageHeader::new(self.session_id, self.source_id.clone(), mode);
        h.seq = self.seq;
        h.stamp_nanos = now_nanos();
        self.seq += 1;
        h
    }
}

impl RobotDriver for SimRobot {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            dof: self.model.dof,
            supported_modes: vec![ControlMode::CartesianPose, ControlMode::JointTargets],
            force_feedback: false,
            max_rate_hz: 200,
            frames: vec!["base".into()],
            end_effectors: self.model.end_effectors.clone(),
            gripper: None,
        }
    }

    fn command(&mut self, cmd: &TeleopCommand) -> Result<(), RobotError> {
        if self.health == HealthState::EStopped {
            return Err(RobotError::Hardware("e-stopped".into()));
        }
        self.apply_body(&cmd.body)
    }

    fn read_state(&mut self) -> Result<RobotFeedback, RobotError> {
        let n = self.joints.len();
        let positions = self.joints.clone();
        let header = self.header(ControlMode::JointTargets);
        Ok(RobotFeedback {
            header,
            body: FeedbackBody::Joint(JointState {
                positions,
                velocities: vec![0.0; n],
                efforts: vec![0.0; n],
            }),
        })
    }

    fn e_stop(&mut self) -> Result<(), RobotError> {
        self.health = HealthState::EStopped;
        Ok(())
    }
}

impl SimRobot {
    /// Build a one-off status feedback (health + RTT estimate).
    pub fn status(&mut self, rtt_nanos: u64) -> RobotFeedback {
        let header = self.header(ControlMode::JointTargets);
        RobotFeedback {
            header,
            body: FeedbackBody::Status(Status {
                health: self.health,
                message: self.model.name.clone(),
                rtt_nanos,
            }),
        }
    }
}
