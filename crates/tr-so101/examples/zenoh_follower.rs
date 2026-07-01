//! Zenoh → follower arm.
//!
//! Subscribes to a zenoh key expression, decodes incoming postcard-encoded
//! `JointTargets`, and drives the follower SO-101 in a **tight recv loop**
//! — every received frame is written to the servos immediately, with zero
//! artificial delay.  Ctrl‑C to stop.
//!
//! Usage:
//!   cargo run -p tr-so101 --example zenoh_follower -- \
//!       /dev/cu.usbmodem5AB01836201 [--key tr/csv/control]

use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus};
use std::time::Duration;
use tr_codec::PostcardCodec;
use tr_messages::{Codec, CommandBody, TeleopCommand};
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned().unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let key = args.iter().position(|a| a == "--key")
        .and_then(|i| args.get(i + 1)).cloned()
        .unwrap_or_else(|| "tr/csv/control".into());

    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let codec = PostcardCodec;

    println!("🔗 Opening zenoh subscriber on {key} ...");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_io()
        .enable_time()
        .build()?;

    let mut transport = ZenohTransport::subscriber(rt.handle(), &key)?;

    let result = rt.block_on(async {
        println!("🔗 Opening follower on {port} ...");
        let mut bus = FeetechBus::new(&port, 1_000_000)?;
        bus.enable_torque(&ids).await?;
        println!("   torque: ON\n");

        let mut first = true;
        let mut count = 0u64;
        let mut last_written = [0.0_f32; 6];
        const DEDUP_THRESH: f32 = 0.002; // ~0.11° — skip if all joints unchanged

        loop {
            // Tight recv loop — no rate limit, no select, no sleeps between
            // frames.  1 ms timeout yields the CPU when the link is idle.
            if let Ok(Some(inbound)) = transport.recv(Duration::from_millis(1)) {
                let cmd: TeleopCommand = codec.decode_command(&inbound.frame)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let joint_rad: Vec<f32> = match &cmd.body {
                    CommandBody::Joint(jt) => jt.positions.iter().map(|&p| p as f32).collect(),
                    _ => continue,
                };
                if joint_rad.len() < 6 { continue; }

                // Dedup: skip write if all joints unchanged (prevents PID restart jitter).
                if !first {
                    let max_d = joint_rad.iter().zip(last_written.iter())
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0_f32, f32::max);
                    if max_d < DEDUP_THRESH { continue; }
                }
                last_written.copy_from_slice(&joint_rad);

                let cmds: Vec<(u8, ControlOp)> = ids.iter()
                    .zip(joint_rad.iter())
                    .map(|(&id, &p)| (id, ControlOp::Position(p)))
                    .collect();

                if first {
                    bus.sync_write_goals(&cmds).await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    first = false;
                } else {
                    if let Err(e) = bus.sync_write_goals(&cmds).await {
                        eprintln!("[warn] write error @ frame {count}: {e}");
                    }
                }

                if count < 20 || count % 50 == 0 {
                    println!("[play] frame={:>4}  j1={:>7.1}°", count, joint_rad[0] * 57.2958);
                }
                count += 1;
            }
        }
        #[allow(unreachable_code)]
        Ok::<_, anyhow::Error>(())
    });

    drop(transport);
    result?;
    Ok(())
}
