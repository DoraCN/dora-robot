//! DORA node — robot (follower) on the robot machine.
//!
//! Receives canonical commands (JointTargets → `tr-codec` decoded), drives the
//! follower, and emits both *canonical* feedback (encoded, for the bridge) and
//! *Arrow* `action`/`observation_state` (plain Arrow, for the Python recorder).
//!
//! Dataflow inputs : `command` (bridge) → canonical JointTargets
//!                   `tick` (dora timer) → read-state / emit observation
//! Dataflow outputs: `feedback` (bridge) — canonical RobotFeedback (M3 reserved)
//!                   `action` (recorder) — Arrow Float32Array(6)
//!                   `observation_state` (recorder) — Arrow Float32Array(6)

use arrow::array::Float32Array;
use dora_node_api::{DoraNode, Event};
use futures::StreamExt;
use tr_codec::PostcardCodec;

fn main() -> eyre::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let (_node, mut events) = DoraNode::init_from_env()?;
        let _codec = PostcardCodec;

        // Prove the arrow and codec types compile / link.
        let _ = Float32Array::from(vec![0.0_f32]);

        // The follower is selected by config. A real session constructs
        // So101Follower::new(So101Arm::new(FeetechBus::new(port, 1_000_000)?, cfg), sid, "follower").
        // TODO: select driver by env/feature.

        while let Some(event) = events.next().await {
            match event {
                Event::Input { id, data, .. } if id.as_str() == "command" => {
                    // Production:
                    //   let cmd = codec.decode_command(&data.to_byte_slice())?;
                    //   follower.command(&cmd)?;
                    //   let action_j: Vec<f32> = convert_cmd_to_f32(&cmd);
                    //   node.send_output("action".into(), Default::default(),
                    //                    Float32Array::from(action_j))?;
                    let _ = (id, data); // compile-check wiring
                }
                Event::Input { id, .. } if id.as_str() == "tick" => {
                    // Production:
                    //   let fb = follower.read_state()?;
                    //   let obs = Float32Array::from(fb_joint_positions_as_f32_vec);
                    //   node.send_output("observation_state".into(), Default::default(), obs)?;
                    //   let fb_enc = codec.encode_feedback(&fb)?;
                    //   node.send_output("feedback".into(), Default::default(), fb_enc.len(), &fb_enc)?;  // M3 reserved
                    let _ = id; // compile-check wiring
                }
                _ => {}
            }
        }
        Ok(())
    })
}
