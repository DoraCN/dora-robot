//! DORA node: the communication middleware.
//!
//! It pumps bytes both ways between the local DORA dataflow and a pluggable
//! [`tr_transport::Transport`] (TCP/UDP/USB/BLE/NearLink, chosen by config):
//!   - dataflow input  `command`  -> transport `Channel::Control`
//!   - transport `Channel::Feedback` -> dataflow output `feedback`
//! and runs the [`tr_session::Session`] handshake/heartbeat on `Channel::Handshake`.

use std::time::Duration;
use tr_messages::{Capabilities, ControlMode};
use tr_session::Session;
use tr_transport::{Channel, FrameEncoder, Qos};

fn local_caps() -> Capabilities {
    Capabilities {
        dof: 6,
        supported_modes: vec![ControlMode::CartesianPose, ControlMode::JointTargets],
        force_feedback: false,
        max_rate_hz: 200,
        frames: vec!["base".into()],
        end_effectors: vec!["tcp".into()],
        gripper: None,
    }
}

fn main() {
    let mut session = Session::new(0x00C0FFEE, local_caps(), Duration::from_millis(100));
    session.begin_handshake();
    println!("[tr-bridge] session {} state {:?}", session.id(), session.state());

    // QoS plan per channel (DDS/ROS2 style).
    println!("[tr-bridge] Control  -> {:?}", Qos::realtime());
    println!("[tr-bridge] Feedback -> {:?}", Qos::realtime());
    println!("[tr-bridge] Handshake-> {:?}", Qos::reliable());

    // Example of framing a (codec-encoded) canonical command for the wire.
    let framed = FrameEncoder::encode(Channel::Control, b"<codec-encoded command>");
    println!("[tr-bridge] framed control bytes: {}", framed.len());

    println!(
        "[tr-bridge] (skeleton) select a Transport (e.g. TcpTransport::connect/bind_accept,\n            \
         UdpTransport::connect, or a USB/BLE/NearLink backend) and pump DORA <-> transport."
    );
}
