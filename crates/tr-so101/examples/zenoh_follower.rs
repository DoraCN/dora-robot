//! Zenoh → follower arm.
//!
//! Subscribes to `tr/csv/control`, decodes incoming postcard-encoded
//! `JointTargets`, and drives the follower SO-101 in real time.
//! Frames are consumed in a tight draining loop (no artificial delay between
//! frames); per-frame timing is logged for debugging.
//!
//! Usage:
//!   cargo run -p tr-so101 --example zenoh_follower -- /dev/cu.usbmodem5AB01836201

use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus};
use std::time::{Duration, Instant};
use tr_codec::PostcardCodec;
use tr_messages::{Codec, CommandBody, TeleopCommand};
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;

fn main() -> anyhow::Result<()> {
    let port = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let codec = PostcardCodec;

    println!("🔗 Opening zenoh subscriber ...");
    let mut transport = ZenohTransport::subscriber("tr/csv/control")?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_io()
        .enable_time()
        .build()?;

    rt.block_on(async move {
        println!("🔗 Opening follower on {port} ...");
        let mut bus = FeetechBus::new(&port, 1_000_000)?;
        bus.enable_torque(&ids).await?;
        println!("   torque: ON\n");

        let mut first = true;
        let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
        let mut count = 0u64;
        let mut last_recv = Instant::now();

        loop {
            // Yield briefly so ctrl_c can be checked; drain pending frames in a
            // tight loop afterwards (no artificial delay between frames).
            tokio::select! {
                _ = &mut ctrl_c => { println!(); break; }
                _ = tokio::time::sleep(Duration::from_millis(1)) => {}
            }

            while let Ok(Some(inbound)) = transport.recv(Duration::from_millis(0)) {
                let now = Instant::now();
                let dt_ms = now.duration_since(last_recv).as_millis();
                last_recv = now;

                let cmd: TeleopCommand = codec
                    .decode_command(&inbound.frame)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let joint_rad: Vec<f32> = match &cmd.body {
                    CommandBody::Joint(jt) => jt.positions.iter().map(|&p| p as f32).collect(),
                    _ => continue,
                };
                if joint_rad.len() < 6 {
                    continue;
                }

                if first {
                    let cmds: Vec<(u8, ControlOp)> = ids
                        .iter()
                        .zip(joint_rad.iter())
                        .map(|(&id, &p)| (id, ControlOp::Position(p)))
                        .collect();
                    bus.sync_write_goals(&cmds).await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    first = false;
                } else {
                    let cmds: Vec<(u8, ControlOp)> = ids
                        .iter()
                        .zip(joint_rad.iter())
                        .map(|(&id, &p)| (id, ControlOp::Position(p)))
                        .collect();
                    if let Err(e) = bus.sync_write_goals(&cmds).await {
                        eprintln!("[warn] write error @ frame {count}: {e}");
                    }
                }

                // Log timing for the first 20 frames and every 50th thereafter.
                if count < 20 || count % 50 == 0 {
                    println!("[recv] frame={:>4}  dt={:>4}ms  j1={:>7.1}°",
                        count, dt_ms, joint_rad[0] * 57.2958);
                }

                count += 1;
            }
        }

        println!("\n💤 Disabling torque ({} frames) ...", count);
        bus.disable_torque(&ids).await?;
        println!("👋 Exiting.");
        Ok::<_, anyhow::Error>(())
    })?;

    Ok(())
}
