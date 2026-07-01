//! Real SO-101 leader → TCP sender (live teleop, no zenoh).
//!
//! Connects to the leader arm, reads joints at bus speed (~45 Hz), and sends
//! postcard-encoded `JointTargets` over TCP.  Connects to the follower machine.
//!
//! Usage:
//!   cargo run -p tr-so101 --example leader_teleop_tcp -- \
//!       /dev/cu.usbmodem5AB01836201 [192.168.x.x:9000]

use feetech_servo_sdk::{FeetechBus, MotorBus};
use std::time::Duration;
use tr_codec::PostcardCodec;
use tr_messages::{Codec, CommandBody, ControlMode, JointTargets, MessageHeader, TeleopCommand};
use tr_transport::backends::TcpTransport;
use tr_transport::qos::Channel;
use tr_transport::Transport;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned().unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let addr = args.get(2).cloned().unwrap_or_else(|| "127.0.0.1:9000".into());

    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let codec = PostcardCodec;

    let rt = tokio::runtime::Runtime::new()?;
    let _guard = rt.enter();

    println!("🔗 Opening leader on {port} ...");
    let mut bus = FeetechBus::new(&port, 1_000_000)?;
    rt.block_on(async { bus.disable_torque(&ids).await })?;
    println!("   torque: OFF (backdrivable)");

    println!("🔗 TCP connecting to {addr} ...");
    let mut transport = TcpTransport::connect(&addr)?;
    println!("   connected.\n▶  Publishing (Ctrl‑C to stop)\n");

    let mut seq: u64 = 0;
    let mut last = [0.0_f32; 6];
    loop {
        std::thread::sleep(Duration::from_millis(1));
        let positions = match rt.block_on(async { bus.sync_read_positions(&ids).await }) {
            Ok(p) => { last.copy_from_slice(&p); p }
            Err(e) => { eprintln!("[warn] read: {e}"); last.to_vec() }
        };

        let cmd = TeleopCommand {
            header: MessageHeader::new(0, "leader", ControlMode::JointTargets),
            body: CommandBody::Joint(JointTargets {
                positions: positions.iter().map(|&p| p as f64).collect(),
                velocities: None,
                efforts: None,
            }),
        };
        let bytes = codec.encode_command(&cmd).map_err(|e| anyhow::anyhow!("{e}"))?;
        transport.send(Channel::Control, &bytes)?;

        seq += 1;
        if seq % 100 == 0 { print!("\r   frames: {seq}"); }
    }
}
