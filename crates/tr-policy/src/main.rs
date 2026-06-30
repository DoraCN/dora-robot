//! DORA node: autonomous policy replay.
//!
//! Loads a trained policy and emits canonical commands straight into the robot
//! node — the teleop tier is simply unplugged, which proves the canonical
//! contract is the real seam between perception/policy and the robot.
//!
//! Production: take observations in, run inference (LeRobot/torch via `tch`/`ort`
//! or a Python sidecar), and emit the action as a canonical command.

use tr_messages::{
    CartesianTarget, CommandBody, ControlMode, MessageHeader, Pose, TeleopCommand, Vec3,
};

fn infer_action(step: u64) -> TeleopCommand {
    let mut pose = Pose::default();
    pose.position = Vec3::new(0.4, 0.0, 0.3 + 0.001 * step as f64);
    TeleopCommand {
        header: MessageHeader::new(1, "policy", ControlMode::CartesianPose),
        body: CommandBody::Cartesian(CartesianTarget {
            frame: "base".into(),
            pose,
        }),
    }
}

fn main() {
    let action = infer_action(0);
    println!("[tr-policy] inferred action: {:?}", action.body);
    println!("[tr-policy] (skeleton) load a trained policy and stream actions to tr-robot-node.");
}
