//! Device adapters.
//!
//! [`DemoCartesianDevice`] is a runnable example that emits a slowly moving
//! Cartesian target. The others are stubs implementing the trait; fill them in
//! with the SDKs noted in `Cargo.toml`.

use crate::TeleopDevice;
use std::time::{SystemTime, UNIX_EPOCH};
use tr_messages::{
    Capabilities, CartesianTarget, CommandBody, ControlMode, MessageHeader, Pose, SessionId,
    TeleopCommand, Vec3,
};

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn cartesian_caps(rate_hz: u32, force_feedback: bool) -> Capabilities {
    Capabilities {
        dof: 6,
        supported_modes: vec![ControlMode::CartesianPose],
        force_feedback,
        max_rate_hz: rate_hz,
        frames: vec!["base".into()],
        end_effectors: vec!["tcp".into()],
        gripper: None,
    }
}

/// Emits a Cartesian target that wiggles along X — useful for end-to-end tests.
pub struct DemoCartesianDevice {
    session_id: SessionId,
    source_id: String,
    seq: u64,
}

impl DemoCartesianDevice {
    pub fn new(session_id: SessionId, source_id: impl Into<String>) -> Self {
        Self {
            session_id,
            source_id: source_id.into(),
            seq: 0,
        }
    }
}

impl TeleopDevice for DemoCartesianDevice {
    fn capabilities(&self) -> Capabilities {
        cartesian_caps(200, false)
    }

    fn poll(&mut self) -> Option<TeleopCommand> {
        let phase = (self.seq as f64) * 0.01;
        let mut header =
            MessageHeader::new(self.session_id, self.source_id.clone(), ControlMode::CartesianPose);
        header.seq = self.seq;
        header.stamp_nanos = now_nanos();
        self.seq += 1;

        let mut pose = Pose::default();
        pose.position = Vec3::new(0.4 + 0.05 * phase.sin(), 0.0, 0.3);
        Some(TeleopCommand {
            header,
            body: CommandBody::Cartesian(CartesianTarget {
                frame: "base".into(),
                pose,
            }),
        })
    }
}

macro_rules! stub_device {
    ($name:ident, $modes:expr, $ff:expr, $doc:literal) => {
        #[doc = $doc]
        pub struct $name;
        impl $name {
            pub fn new() -> Self {
                Self
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl TeleopDevice for $name {
            fn capabilities(&self) -> Capabilities {
                Capabilities {
                    dof: 6,
                    supported_modes: $modes,
                    force_feedback: $ff,
                    max_rate_hz: 100,
                    frames: vec!["base".into()],
                    end_effectors: vec!["tcp".into()],
                    gripper: None,
                }
            }
            fn poll(&mut self) -> Option<TeleopCommand> {
                None // TODO: read the real device.
            }
        }
    };
}

stub_device!(
    KeyboardDevice,
    vec![ControlMode::Twist, ControlMode::CartesianPose],
    false,
    "Keyboard adapter stub (jog/twist)."
);
stub_device!(
    GamepadDevice,
    vec![ControlMode::Twist, ControlMode::CartesianPose],
    false,
    "Gamepad/remote adapter stub (use `gilrs`)."
);
stub_device!(
    VrDevice,
    vec![ControlMode::CartesianPose, ControlMode::Composite],
    true,
    "VR headset+controllers adapter stub (OpenXR; controller pose -> Cartesian)."
);
stub_device!(
    IsomorphicArmDevice,
    vec![ControlMode::JointTargets, ControlMode::CartesianPose],
    true,
    "Isomorphic master arm adapter stub (joint mirroring + bilateral haptics)."
);
