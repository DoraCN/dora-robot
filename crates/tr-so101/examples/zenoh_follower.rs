//! Zenoh → follower arm.
//!
//! Subscribes to `tr/csv/control`, decodes incoming postcard-encoded
//! `JointTargets`, and drives the follower SO-101 in real time.
//!
//! Usage:
//!   cargo run -p tr-so101 --example zenoh_follower -- /dev/cu.usbmodem5AB01836201

use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus};
use std::time::Duration;
use tr_codec::PostcardCodec;
use tr_messages::{Codec, CommandBody, TeleopCommand};
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;

fn main() -> anyhow::Result<()> {
    let port = std::env::args().nth(1).unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
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
        println!("   torque: ON");

        let mut first = true;
        let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
        let mut count = 0u64;

        loop {
            tokio::select! {
                _ = &mut ctrl_c => { println!(); break; }
                _ = tokio::time::sleep(Duration::from_millis(5)) => {}
            }
            // Poll zenoh (sync recv) — short timeout so ctrl_c can interrupt.
            match transport.recv(Duration::from_millis(1)) {
                Ok(Some(inbound)) => {
                    let cmd: TeleopCommand = codec.decode_command(&inbound.frame)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    let joint_rad: Vec<f32> = match &cmd.body {
                        CommandBody::Joint(jt) => jt.positions.iter().map(|&p| p as f32).collect(),
                        _ => continue,
                    };
                    if joint_rad.len() < 6 { continue; }

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
                    count += 1;
                    if count % 100 == 0 { print!("\r   frame {count}"); }
                }
                Ok(None) => {} // timeout — loop back to check ctrl_c
                Err(e) => { eprintln!("recv error: {e}"); }
            }
        }

        println!("\n💤 Disabling torque ...");
        bus.disable_torque(&ids).await?;
        println!("👋 Exiting ({count} frames).");
        Ok::<_, anyhow::Error>(())
    })?;

    Ok(())
}
