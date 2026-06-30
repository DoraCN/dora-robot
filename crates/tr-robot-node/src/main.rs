//! DORA node: receive canonical commands from the bridge, run IK/retargeting in
//! the driver, drive the robot, and publish feedback.
//!
//! Real DORA wiring (add `dora-node-api`): decode each `command` input with the
//! `Codec`, call `driver.command(&cmd)`, then publish `driver.read_state()` (and
//! force feedback) back onto the bridge.

use tr_messages::{
    CommandBody, ControlMode, JointTargets, MessageHeader, TeleopCommand,
};
use tr_robot::{RobotDriver, RobotModel, SimRobot};

fn main() {
    let model = RobotModel::generic_arm("demo_arm", 6);
    let mut robot = SimRobot::new(model, 0x00C0FFEE, "demo_arm");
    println!("[tr-robot-node] capabilities: {:?}", robot.capabilities());

    let cmd = TeleopCommand {
        header: MessageHeader::new(0x00C0FFEE, "demo_master", ControlMode::JointTargets),
        body: CommandBody::Joint(JointTargets {
            positions: vec![0.1, -0.2, 0.3, 0.0, 0.5, 0.0],
            velocities: None,
            efforts: None,
        }),
    };
    match robot.command(&cmd) {
        Ok(()) => match robot.read_state() {
            Ok(fb) => println!("[tr-robot-node] feedback after command: {:?}", fb.body),
            Err(e) => eprintln!("[tr-robot-node] read_state error: {e}"),
        },
        Err(e) => eprintln!("[tr-robot-node] command error: {e}"),
    }
    println!("[tr-robot-node] (skeleton) connect DORA + tr-bridge to stream commands/feedback.");
}
