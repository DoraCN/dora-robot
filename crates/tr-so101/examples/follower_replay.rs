//! Follower replay tool — reads a recorded leader trajectory CSV and replays it
//! on a follower SO-101, timing the playback to match the original recording.
//!
//! CSV format (space-separated, as written by `leader_teleop_debug`):
//!   seq stamp_nanos j1 j2 j3 j4 j5 j6
//!
//! On Ctrl‑C the follower is smoothly parked to a safe pose and torque is
//! disabled.
//!
//! Usage:
//!   cargo run -p tr-so101 --example follower_replay -- \
//!       /dev/cu.usbmodem5AB01836201 [logs/leader_latest.csv]

use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Safe park pose (degrees) — roughly the SO-101's relaxed home position.
const SAFE_PARK: [f32; 6] = [0.0, -105.0, 90.0, 74.0, 0.0, 0.0];

/// One frame of a recorded leader trajectory (joints in radians).
#[derive(Debug, Clone)]
struct Frame {
    stamp_nanos: u64,
    positions: [f32; 6],
}

fn parse_csv(path: &PathBuf) -> io::Result<Vec<Frame>> {
    let f = File::open(path)?;
    let mut frames = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        if line.is_empty() || line.starts_with("seq") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 8 {
            continue;
        }
        let stamp: u64 = parts[1]
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut pos = [0.0_f32; 6];
        for (i, s) in parts[2..8].iter().enumerate() {
            pos[i] = s
                .parse()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        }
        frames.push(Frame {
            stamp_nanos: stamp,
            positions: pos,
        });
    }
    Ok(frames)
}

/// Smooth linear interpolation from current to target over `duration` seconds at 50 Hz.
async fn move_smoothly(
    bus: &mut FeetechBus,
    ids: &[u8],
    target_deg: &[f32],
    duration_sec: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let freq = 50.0;
    let steps = (duration_sec * freq) as usize;
    let dt = Duration::from_secs_f32(1.0 / freq);
    let start = bus.sync_read_positions(ids).await?;
    let target_rad: Vec<f32> = target_deg.iter().map(|d| d.to_radians()).collect();

    for s in 1..=steps {
        let t = s as f32 / steps as f32;
        let cmds: Vec<(u8, ControlOp)> = ids
            .iter()
            .zip(start.iter())
            .zip(target_rad.iter())
            .map(|((&id, &start), &end)| {
                (id, ControlOp::Position(start + (end - start) * t))
            })
            .collect();
        bus.sync_write_goals(&cmds).await?;
        sleep(dt).await;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let port = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let csv_path = args
        .get(2)
        .cloned()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("logs/leader_latest.csv"));

    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];

    // -- Load trajectory ----------------------------------------------------
    println!("📂 Loading {} ...", csv_path.display());
    let frames = parse_csv(&csv_path)?;
    if frames.is_empty() {
        eprintln!("❌ CSV is empty or malformed.");
        return Ok(());
    }
    println!("   {} frames loaded.", frames.len());

    // -- Open follower bus --------------------------------------------------
    println!("🔗 Opening follower on {port} ...");
    let mut bus = FeetechBus::new(&port, 1_000_000)?;
    bus.enable_torque(&ids).await?;
    println!("   torque: ON");

    // -- Anti-jerk: move to first frame -------------------------------------
    println!("🔄 Aligning to first frame ...");
    let first = &frames[0].positions;
    let start_cmds: Vec<(u8, ControlOp)> = ids
        .iter()
        .zip(first.iter())
        .map(|(&id, &p)| (id, ControlOp::Position(p)))
        .collect();
    bus.sync_write_goals(&start_cmds).await?;
    sleep(Duration::from_millis(100)).await;

    // -- Replay loop (selectable Ctrl‑C) ------------------------------------
    println!("▶  Replaying {} frames (Ctrl‑C to stop + park) ...", frames.len());
    let t0 = Instant::now();
    let base_stamp = frames[0].stamp_nanos;

    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
    let mut idx = 0_usize;

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                println!();
                break;
            }
            _ = sleep(Duration::from_millis(1)), if idx < frames.len() => {
                let frame = &frames[idx];
                let target_elapsed = Duration::from_nanos(frame.stamp_nanos.saturating_sub(base_stamp));
                let elapsed = t0.elapsed();
                if elapsed >= target_elapsed {
                    let cmds: Vec<(u8, ControlOp)> = ids
                        .iter()
                        .zip(frame.positions.iter())
                        .map(|(&id, &p)| (id, ControlOp::Position(p)))
                        .collect();
                    if let Err(e) = bus.sync_write_goals(&cmds).await {
                        eprintln!("[warn] write error @ frame {idx}: {e}");
                    }
                    idx += 1;
                    if idx % 100 == 0 {
                        print!("\r   frame {idx}/{}", frames.len());
                        let _ = io::stdout().flush();
                    }
                }
            }
            _ = sleep(Duration::from_millis(10)) => {}
        }
    }

    println!();

    // -- Safe parking (always run after the replay loop exits) --------------
    println!("\n🛑 Parking safely ...");
    move_smoothly(&mut bus, &ids, &SAFE_PARK, 3.0).await?;
    println!("🏠 Parked at safe pose.");

    bus.disable_torque(&ids).await?;
    println!("💤 Torque OFF. Exiting.");
    Ok(())
}
