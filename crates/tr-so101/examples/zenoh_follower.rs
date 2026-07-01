//! Zenoh → follower arm (buffer + clock replayer).
//!
//! Subscribes to a zenoh key expression, decodes incoming postcard-encoded
//! `JointTargets`, and replays them on the follower SO-101 driven by a steady
//! local clock (20 ms tick).  A `VecDeque` buffer absorbs zenoh delivery
//! jitter — the servo write cadence is **independent** of network timing.
//!
//! Multiple sender/receiver pairs are isolated by **key expression** (e.g.
//! `tr/arm_1/control` vs `tr/arm_2/control`).
//!
//! Usage:
//!   cargo run -p tr-so101 --example zenoh_follower -- \
//!       /dev/cu.usbmodem5AB01836201 [--key tr/csv/control]

use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus};
use std::collections::VecDeque;
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
    let tick_dt = Duration::from_millis(20);

    println!("🔗 Opening zenoh subscriber on {key} ...");
    let mut transport = ZenohTransport::subscriber(&key)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_io()
        .enable_time()
        .build()?;

    let result = rt.block_on(async {
        println!("🔗 Opening follower on {port} ...");
        let mut bus = FeetechBus::new(&port, 1_000_000)?;
        bus.enable_torque(&ids).await?;
        println!("   torque: ON\n");

        let mut buffer: VecDeque<[f32; 6]> = VecDeque::with_capacity(64);
        let mut first = true;
        let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
        let mut tick = tokio::time::interval(tick_dt);
        let mut count = 0u64;

        loop {
            tokio::select! {
                _ = &mut ctrl_c => { println!(); break; }

                // --- Write clock (steady, independent of network) -----------
                _ = tick.tick() => {
                    if let Some(joint_rad) = buffer.pop_front() {
                        if first {
                            let cmds: Vec<(u8, ControlOp)> = ids.iter()
                                .zip(joint_rad.iter())
                                .map(|(&id, &p)| (id, ControlOp::Position(p)))
                                .collect();
                            bus.sync_write_goals(&cmds).await?;
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            first = false;
                        } else {
                            let cmds: Vec<(u8, ControlOp)> = ids.iter()
                                .zip(joint_rad.iter())
                                .map(|(&id, &p)| (id, ControlOp::Position(p)))
                                .collect();
                            if let Err(e) = bus.sync_write_goals(&cmds).await {
                                eprintln!("[warn] write error @ frame {count}: {e}");
                            }
                        }
                        if count < 20 || count % 50 == 0 {
                            println!("[play] frame={:>4}  j1={:>7.1}°", count, joint_rad[0] * 57.2958);
                        }
                        count += 1;
                    }
                    // else: buffer empty → hold current pose, nothing to write
                }
            }

            // Drain zenoh frames into the buffer (non-blocking).
            while let Ok(Some(inbound)) = transport.recv(Duration::from_millis(0)) {
                let cmd: TeleopCommand = codec.decode_command(&inbound.frame)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if let CommandBody::Joint(jt) = &cmd.body {
                    let mut pos = [0f32; 6];
                    for i in 0..6 { pos[i] = jt.positions.get(i).copied().unwrap_or(0.0) as f32; }
                    buffer.push_back(pos);
                }
            }
        }

        println!("\n💤 Disabling torque ({} frames) ...", count);
        bus.disable_torque(&ids).await?;
        println!("👋 Exiting.");
        Ok::<_, anyhow::Error>(())
    });

    drop(transport);
    result?;
    Ok(())
}
