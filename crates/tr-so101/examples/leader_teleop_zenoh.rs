//! Real SO-101 leader → zenoh publisher (live teleop sender).
//!
//! Connects to the leader arm, disables torque (backdrivable), reads joint
//! positions at ~45 Hz, and publishes postcard-encoded `JointTargets` over
//! zenoh.  Optionally also writes a local CSV for recording/debug.
//!
//! Usage:
//!   # publish only
//!   cargo run -p tr-so101 --example leader_teleop_zenoh -- \
//!       /dev/cu.usbmodem5AB01836201 [--key tr/csv/control]
//!
//!   # publish + record CSV
//!   cargo run -p tr-so101 --example leader_teleop_zenoh -- \
//!       /dev/cu.usbmodem5AB01836201 --output logs/my_session.csv

use feetech_servo_sdk::{FeetechBus, MotorBus};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
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

    // -- Leader arm ----------------------------------------------------------
    println!("🔗 Opening leader on {port} ...");
    let rt = tokio::runtime::Runtime::new()?;
    let mut bus = FeetechBus::new(&port, 1_000_000)?;
    rt.block_on(async { bus.disable_torque(&ids).await })?;
    println!("   torque: OFF (backdrivable)");

    // -- Zenoh ----------------------------------------------------------------
    println!("🔗 Opening zenoh publisher on {key} ...");
    let mut transport = ZenohTransport::publisher(&key)?;

    // -- Optional CSV ---------------------------------------------------------
    let mut csv: Option<BufWriter<File>> = csv_path
        .as_ref()
        .map(|p| BufWriter::new(File::create(p).expect("create csv")));
    if csv.is_some() {
        writeln!(csv.as_mut().unwrap(), "seq stamp_nanos j1 j2 j3 j4 j5 j6")?;
    }

    println!("\n▶  Publishing (Ctrl‑C to stop)\n");
    let mut seq: u64 = 0;
    let _t0 = Instant::now();

    loop {
        // Yield to Ctrl‑C (synchronous check via tiny sleep)
        std::thread::sleep(Duration::from_millis(1));

        let positions = rt.block_on(async { bus.sync_read_positions(&ids).await })?;
        let stamp = now_nanos();

        // Build canonical JointTargets
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

        // CSV
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
    // Note: Ctrl‑C kills the process; the loop never exits cleanly.
    // The bus and zenoh session are closed by the OS on process termination.
}
