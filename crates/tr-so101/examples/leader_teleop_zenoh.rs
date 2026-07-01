//! Real SO-101 leader → zenoh publisher (live teleop sender).
//!
//! Connects to the leader arm, disables torque (backdrivable), reads joint
//! positions at bus speed (~45 Hz), and publishes postcard-encoded
//! `JointTargets` over zenoh.  Optionally also writes a local CSV for
//! recording/debug.  Ctrl‑C to stop.
//!
//! Two **separate** tokio runtimes isolate the serial bus from zenoh I/O.
//!
//! Usage:
//!   cargo run -p tr-so101 --example leader_teleop_zenoh -- \
//!       /dev/cu.usbmodem5AB01836201 [--key tr/csv/control] [--output logs/session.csv]

use feetech_servo_sdk::{FeetechBus, MotorBus};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tr_codec::PostcardCodec;
use tr_messages::{
    Codec, CommandBody, ControlMode, JointTargets, MessageHeader, TeleopCommand,
};
use tr_transport::qos::Channel;
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned()
        .unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let key = args.iter().position(|a| a == "--key")
        .and_then(|i| args.get(i + 1)).cloned()
        .unwrap_or_else(|| "tr/csv/control".into());
    let csv_path: Option<PathBuf> = args.iter().position(|a| a == "--output")
        .and_then(|i| args.get(i + 1)).map(PathBuf::from);

    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let codec = PostcardCodec;

    println!("────────────────────────────────────────────");
    println!("  LEADER → ZENOH  (live teleop)");
    println!("  port    : {port}");
    println!("  key     : {key}");
    if let Some(ref p) = csv_path { println!("  csv     : {}", p.display()); }
    println!("  Ctrl‑C to stop");
    println!("────────────────────────────────────────────");

    // -- Two separate runtimes — zenoh and serial bus do not share IO drivers ----
    let rt_zenoh = tokio::runtime::Runtime::new()?;

    println!("🔗 Opening zenoh publisher on {key} ...");
    let mut transport = ZenohTransport::publisher(rt_zenoh.handle(), &key)?;

    // Serial bus gets its own runtime — same proven pattern as leader_diag.
    let rt_arm = tokio::runtime::Runtime::new()?;
    let _guard = rt_arm.enter();

    println!("🔗 Opening leader on {port} ...");
    let mut bus = FeetechBus::new(&port, 1_000_000)?;
    rt_arm.block_on(async { bus.disable_torque(&ids).await })?;
    println!("   torque: OFF (backdrivable)\n▶  Publishing (Ctrl‑C to stop)\n");

    // Optional CSV
    let mut csv: Option<BufWriter<File>> = csv_path
        .as_ref()
        .map(|p| BufWriter::new(File::create(p).expect("create csv")));
    if csv.is_some() {
        writeln!(csv.as_mut().unwrap(), "seq stamp_nanos j1 j2 j3 j4 j5 j6")?;
    }

    let mut seq: u64 = 0;
    loop {
        std::thread::sleep(Duration::from_millis(1));
        let positions = rt_arm.block_on(async { bus.sync_read_positions(&ids).await })?;
        let stamp = now_nanos();

        let cmd = TeleopCommand {
            header: MessageHeader::new(0, "leader", ControlMode::JointTargets),
            body: CommandBody::Joint(JointTargets {
                positions: positions.iter().map(|&p| p as f64).collect(),
                velocities: None,
                efforts: None,
            }),
        };
        let encoded = codec.encode_command(&cmd).map_err(|e| anyhow::anyhow!("{e}"))?;
        transport.send(Channel::Control, &encoded)?;

        if let Some(ref mut w) = csv {
            write!(w, "{seq} {stamp}")?;
            for &p in &positions { write!(w, " {:.6}", p)?; }
            writeln!(w)?;
        }

        seq += 1;
        if seq % 100 == 0 {
            print!("\r   frames: {seq}");
            let _ = std::io::stdout().flush();
        }
    }
}
