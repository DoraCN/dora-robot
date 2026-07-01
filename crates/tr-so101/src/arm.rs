//! [`So101Arm`] — the shared SO-101 hardware driver, generic over the feetech
//! [`MotorBus`] so the *same* code runs on real hardware ([`feetech_servo_sdk::FeetechBus`])
//! or a hardware-free [`feetech_servo_sdk::MockBus`].
//!
//! Read/write go through per-joint calibration (raw servo rad ↔ canonical joint
//! rad); writes are clamped to per-joint limits. Async (the SDK is async/Tokio).

use crate::config::So101Config;
use crate::DOF;
use feetech_servo_sdk::{ControlOp, MotorBus, ServoError};

/// One SO-101 arm instance (owns its bus + calibration). Used by the leader
/// (torque off, read) and follower (torque on, write) role adapters (C2/C3).
pub struct So101Arm<B: MotorBus> {
    bus: B,
    cfg: So101Config,
}

impl<B: MotorBus> So101Arm<B> {
    pub fn new(bus: B, cfg: So101Config) -> Self {
        Self { bus, cfg }
    }

    pub fn config(&self) -> &So101Config {
        &self.cfg
    }

    /// Mutable access to the underlying bus (advanced ops / tests).
    pub fn bus_mut(&mut self) -> &mut B {
        &mut self.bus
    }

    /// Enable (`true`) / disable (`false`) torque on all joints.
    pub async fn set_torque(&mut self, on: bool) -> Result<(), ServoError> {
        if on {
            self.bus.enable_torque(&self.cfg.ids).await
        } else {
            self.bus.disable_torque(&self.cfg.ids).await
        }
    }

    /// Read calibrated joint positions (radians), in joint order (ids 1..6).
    pub async fn read_joints(&mut self) -> Result<[f32; DOF], ServoError> {
        let raw = self.bus.sync_read_positions(&self.cfg.ids).await?;
        let mut out = [0.0_f32; DOF];
        for (i, slot) in out.iter_mut().enumerate() {
            let r = raw.get(i).copied().unwrap_or(0.0);
            *slot = self.cfg.joints[i].raw_to_joint(r);
        }
        Ok(out)
    }

    /// Write calibrated joint targets (radians); each joint is clamped to its limits.
    pub async fn write_joints(&mut self, joints: &[f32; DOF]) -> Result<(), ServoError> {
        let mut cmds: Vec<(u8, ControlOp)> = Vec::with_capacity(DOF);
        for i in 0..DOF {
            let j = self.cfg.joints[i].clamp(joints[i]);
            let raw = self.cfg.joints[i].joint_to_raw(j);
            cmds.push((self.cfg.ids[i], ControlOp::Position(raw)));
        }
        self.bus.sync_write_goals(&cmds).await
    }
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use super::*;
    use crate::config::So101Config;
    use feetech_servo_sdk::MockBus;

    /// Minimal current-thread runtime (avoids the `tokio-macros` dependency).
    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(f)
    }

    const IDS: [u8; DOF] = [1, 2, 3, 4, 5, 6];

    #[test]
    fn torque_toggle_ok() {
        block_on(async {
            let mut arm = So101Arm::new(MockBus::new(&IDS), So101Config::default());
            arm.set_torque(true).await.unwrap();
            arm.set_torque(false).await.unwrap();
        });
    }

    #[test]
    fn read_reflects_calibrated_positions() {
        block_on(async {
            let bus = MockBus::new(&IDS);
            bus.set_servo_position_instant(1, 0.5);
            bus.set_servo_position_instant(3, -0.3);
            let mut arm = So101Arm::new(bus, So101Config::default());
            let j = arm.read_joints().await.unwrap();
            // identity calibration → joint ≈ raw (within MockBus tick quantization ~0.0016 rad).
            assert!((j[0] - 0.5).abs() < 0.01, "j[0]={}", j[0]);
            assert!((j[2] + 0.3).abs() < 0.01, "j[2]={}", j[2]);
        });
    }

    #[test]
    fn write_joints_runs_and_clamps() {
        block_on(async {
            let mut arm = So101Arm::new(MockBus::new(&IDS), So101Config::default());
            arm.set_torque(true).await.unwrap();
            // joint 0 well past the ±π limit → clamped, write still succeeds.
            arm.write_joints(&[100.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                .await
                .unwrap();
        });
    }
}
