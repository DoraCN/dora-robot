//! TCP → follower arm (no zenoh).
//!
//! Listens on a TCP port, receives postcard-encoded `JointTargets` frames,
//! and drives the SO-101 follower in a tight recv loop.  Ctrl‑C to stop.
//!
//! Usage:
//!   cargo run -p tr-so101 --example tcp_follower -- \
//!       /dev/cu.usbmodem5AB01836201 [0.0.0.0:9000]

use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus};
use std::time::{Duration, Instant};
use tr_codec::PostcardCodec;
use tr_messages::{Codec, CommandBody, TeleopCommand};
use tr_transport::Transport;
use tr_transport::backends::TcpTransport;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned().unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let addr = args.get(2).cloned().unwrap_or_else(|| "0.0.0.0:9000".into());

    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let codec = PostcardCodec;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;
    let _guard = rt.enter();

    println!("🔗 Opening follower on {port} ...");
    let mut bus = FeetechBus::new(&port, 1_000_000)?;
    rt.block_on(async { bus.enable_torque(&ids).await })?;
    println!("   torque: ON");

    println!("🔗 TCP listen on {addr} ...");
    let mut transport = TcpTransport::bind_accept(&addr)?;
    println!("   connected.");

    let mut first = true;
    let mut count = 0u64;
    let mut last_written_pos = [0.0_f32; 6];
    let mut last_write = Instant::now();
    const DEDUP_THRESH: f32 = 0.002;
    const MIN_WRITE_DT: Duration = Duration::from_millis(25);
    println!("▶  Receiving (Ctrl‑C to stop)\n");

    loop {
        if let Ok(Some(inbound)) = transport.recv(Duration::from_millis(1)) {
            let cmd: TeleopCommand = codec.decode_command(&inbound.frame)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let joint_rad: Vec<f32> = match &cmd.body {
                CommandBody::Joint(jt) => jt.positions.iter().map(|&p| p as f32).collect(),
                _ => continue,
            };
            if joint_rad.len() < 6 { continue; }

            if !first {
                let max_d = joint_rad.iter().zip(last_written_pos.iter())
                    .map(|(a, b)| (a - b).abs()).fold(0.0_f32, f32::max);
                if max_d < DEDUP_THRESH { continue; }
            }
            if !first {
                let elapsed = last_write.elapsed();
                if elapsed < MIN_WRITE_DT {
                    std::thread::sleep(MIN_WRITE_DT - elapsed);
                }
            }
            last_write = Instant::now();
            last_written_pos.copy_from_slice(&joint_rad);

            let cmds: Vec<(u8, ControlOp)> = ids.iter()
                .zip(joint_rad.iter())
                .map(|(&id, &p)| (id, ControlOp::Position(p)))
                .collect();

            if first {
                rt.block_on(async { bus.sync_write_goals(&cmds).await })?;
                std::thread::sleep(Duration::from_millis(100));
                first = false;
            } else {
                if let Err(e) = rt.block_on(async { bus.sync_write_goals(&cmds).await }) {
                    eprintln!("[warn] write error @ frame {count}: {e}");
                }
            }

            if count < 20 || count % 50 == 0 {
                println!("[tcp] frame={:>4}  j1={:>7.1}°", count, joint_rad[0] * 57.2958);
            }
            count += 1;
        }
    }
}
