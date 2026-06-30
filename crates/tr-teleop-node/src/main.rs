//! DORA node: read a teleop device, emit canonical commands toward the bridge.
//!
//! Real DORA wiring (add `dora-node-api`):
//! ```ignore
//! use dora_node_api::{DoraNode, Event};
//! let (mut node, mut events) = DoraNode::init_from_env()?;
//! while let Some(event) = events.recv() {
//!     if let Event::Input { id, .. } = event {
//!         if id.as_str() == "tick" {
//!             if let Some(cmd) = device.poll() {
//!                 let bytes = codec.encode_command(&cmd)?;            // tr-messages::Codec
//!                 node.send_output("command".into(), Default::default(), bytes.into())?;
//!             }
//!         }
//!     }
//! }
//! ```

use tr_teleop::{DemoCartesianDevice, TeleopDevice};

fn main() {
    let mut device = DemoCartesianDevice::new(0x00C0FFEE, "demo_master");
    println!("[tr-teleop-node] capabilities: {:?}", device.capabilities());
    if let Some(cmd) = device.poll() {
        println!(
            "[tr-teleop-node] sample canonical command (mode {:?}): {:?}",
            cmd.header.control_mode, cmd.body
        );
    }
    println!("[tr-teleop-node] (skeleton) connect DORA + tr-bridge to stream this device.");
}
