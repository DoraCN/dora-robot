//! DORA node — teleop device on the operator machine.
//!
//! Polls a teleop device (e.g. `So101Leader`), canonical-encodes the command via
//! `tr-codec`, and publishes it through the bridge on output `command`.
//!
//! Dataflow inputs:  `tick` (dora timer) → driver; `feedback` (bridge) → haptics (M3 reserved).
//! Dataflow outputs: `command` (bridge) → codec-encoded JointTargets.

use dora_node_api::{DoraNode, Event};
use futures::StreamExt;
use tr_codec::PostcardCodec;

fn main() -> eyre::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let (_node, mut events) = DoraNode::init_from_env()?;
        let _codec = PostcardCodec;

        // The device is selected by config. A real session constructs
        // So101Leader::new(So101Arm::new(FeetechBus::new(port, 1_000_000)?, cfg), sid, "leader").
        // TODO: select device by env/feature.

        while let Some(event) = events.next().await {
            if let Event::Input { id, .. } = event {
                if id.as_str() == "tick" {
                    // Production:
                    //   let cmd = device.poll();
                    //   let bytes = codec.encode_command(&cmd)?;
                    //   node.send_output("command".into(), Default::default(), bytes.len(), &bytes)?;
                }
            }
        }
        // The event-stream closes itself on Stop.
        Ok(())
    })
}
