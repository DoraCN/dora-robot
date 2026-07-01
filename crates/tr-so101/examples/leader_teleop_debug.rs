//! Leader recording tool — reads the SO-101 leader at ~100 Hz, prints positions
//! to stdout, and **always appends** each frame to `logs/leader_latest.csv`.
//!
//! CSV format (space-separated, radians):
//!   seq stamp_nanos j1 j2 j3 j4 j5 j6
//!
//! Usage:
//!   cargo run -p tr-so101 --example leader_teleop_debug
//!   cargo run -p tr-so101 --example leader_teleop_debug -- /dev/cu.usbmodem5AB01836201 --output my_path.csv

use feetech_servo_sdk::{FeetechBus, MotorBus};
use std::f32::consts::PI;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::interval;

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let port = args.get(1).cloned().unwrap_or_else(|| {
        "/dev/cu.usbmodem5AB01836201".into()
    });
    let csv_path: PathBuf = args
        .iter()
        .position(|a| a == "--output")
        .and_then(|i| args.get(i + 1).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("logs/leader_latest.csv"));

    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let baud = 1_000_000;
    let tick_ms = 10_u64; // ~100 Hz

    println!("────────────────────────────────────────────");
    println!("  SO-101 LEADER RECORD ({tick_ms} ms)");
    println!("  port    : {port}");
    println!("  ids     : {ids:?}  baud: {baud}");
    println!("  output  : {}", csv_path.display());
    println!("  Ctrl‑C to stop");
    println!("────────────────────────────────────────────");

    let mut bus = FeetechBus::new(&port, baud)?;
    bus.disable_torque(&ids).await?;
    println!("  torque  : OFF (backdrivable)\n");

    // Always open the CSV output file.
    let mut csv = BufWriter::new(File::create(&csv_path)?);

    // Print header
    println!(
        "{:>6} {:>12} {:>8}{:>8}{:>8}{:>8}{:>8}{:>8}",
        "seq", "stamp_ns", "j1°", "j2°", "j3°", "j4°", "j5°", "j6°",
    );

    let mut seq: u64 = 0;
    let _t0 = Instant::now();
    let mut tick = interval(Duration::from_millis(tick_ms));
    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let positions = match bus.sync_read_positions(&ids).await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("[warn] read error: {e}");
                        continue;
                    }
                };
                let stamp = now_nanos();

                // -- console -------------------------------------------------
                print!(
                    "{:>6} {:>12}",
                    seq, stamp,
                );
                for &p in &positions {
                    print!("{:>8.1}", p * 180.0 / PI);
                }
                println!();
                let _ = io::stdout().flush();

                // -- csv -----------------------------------------------------
                write!(csv, "{seq} {stamp}")?;
                for &p in &positions {
                    write!(csv, " {:.6}", p)?;
                }
                writeln!(csv)?;

                seq += 1;
            }
            res = &mut ctrl_c => {
                res?;
                let elapsed = Instant::now().duration_since(_t0).as_secs_f64();
                println!("\n🛑 Ctrl‑C — {seq} readings ({elapsed:.1} s).");
                csv.flush()?;
                println!("💾 saved → {}", csv_path.display());
                break;
            }
        }
    }

    Ok(())
}
