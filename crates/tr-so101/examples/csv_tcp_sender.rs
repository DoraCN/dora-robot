//! CSV → TCP sender (no zenoh).
//!
//! Reads a recorded leader trajectory CSV and sends each frame over TCP
//! as a postcard-encoded `JointTargets`.  Connects to the follower's TCP port.
//!
//! Usage:
//!   cargo run -p tr-so101 --example csv_tcp_sender -- [logs/leader_latest.csv] [192.168.x.x:9000]

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};
use tr_codec::PostcardCodec;
use tr_messages::{Codec, CommandBody, ControlMode, JointTargets, MessageHeader, TeleopCommand};
use tr_transport::qos::Channel;
use tr_transport::Transport;
use tr_transport::backends::TcpTransport;

fn parse_csv(path: &str) -> anyhow::Result<Vec<(u64, [f32; 6])>> {
    let mut frames = Vec::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if line.is_empty() || line.starts_with("seq") { continue; }
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 8 { continue; }
        let stamp: u64 = p[1].parse()?;
        let mut pos = [0f32; 6];
        for i in 0..6 { pos[i] = p[2 + i].parse()?; }
        frames.push((stamp, pos));
    }
    Ok(frames)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let csv = args.get(1).cloned().unwrap_or_else(|| "logs/leader_latest.csv".into());
    let addr = args.get(2).cloned().unwrap_or_else(|| "127.0.0.1:9000".into());

    println!("📂 Loading {} ...", csv);
    let frames = parse_csv(&csv)?;
    if frames.is_empty() { anyhow::bail!("empty CSV"); }
    println!("   {} frames loaded.", frames.len());

    println!("🔗 TCP connecting to {addr} ...");
    let mut transport = TcpTransport::connect(&addr)?;
    let codec = PostcardCodec;

    let t0 = Instant::now();
    let base_stamp = frames[0].0;

    println!("▶  Sending {} frames (Ctrl‑C to stop)", frames.len());
    for (i, &(stamp, positions)) in frames.iter().enumerate() {
        let target = Duration::from_nanos(stamp.saturating_sub(base_stamp));
        let elapsed = t0.elapsed();
        if elapsed < target {
            std::thread::sleep(target - elapsed);
        }
        let cmd = TeleopCommand {
            header: MessageHeader::new(0, "csv", ControlMode::JointTargets),
            body: CommandBody::Joint(JointTargets {
                positions: positions.iter().map(|&p| p as f64).collect(),
                velocities: None,
                efforts: None,
            }),
        };
        let bytes = codec.encode_command(&cmd).map_err(|e| anyhow::anyhow!("{e}"))?;
        transport.send(Channel::Control, &bytes)?;
        if i % 100 == 0 { print!("\r   frame {i}/{}", frames.len()); }
    }
    println!("\n✅ Done.");
    Ok(())
}
