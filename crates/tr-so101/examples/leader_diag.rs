//! SO-101 hardware diagnostic — read joints and print.
//!
//! Minimal tool: open the bus, disable torque, loop at bus speed printing
//! joint positions.  No network, no zenoh, no CSV — just raw hardware read.
//!
//! Usage:
//!   cargo run -p tr-so101 --example leader_diag -- /dev/cu.usbmodem5AB01836201

use feetech_servo_sdk::{FeetechBus, MotorBus};
use std::f32::consts::PI;
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::args().nth(1).unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];

    println!("────────────────────────────");
    println!("  SO-101 DIAG  (read only)");
    println!("  port : {port}");
    println!("  Ctrl‑C to stop");
    println!("────────────────────────────");

    let rt = tokio::runtime::Runtime::new()?;
    let _guard = rt.enter();
    let mut bus = FeetechBus::new(&port, 1_000_000)?;
    rt.block_on(async { bus.disable_torque(&ids).await })?;
    println!("  torque: OFF\n");

    println!("{:>6} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "seq", "j1°", "j2°", "j3°", "j4°", "j5°", "j6°");

    let mut seq: u64 = 0;
    loop {
        // Tiny sleep to avoid hot-looping the CPU
        std::thread::sleep(std::time::Duration::from_millis(1));

        let positions = rt.block_on(async { bus.sync_read_positions(&ids).await })?;
        print!("{:>6} ", seq);
        for &p in &positions { print!("{:>8.1}", p * 180.0 / PI); }
        println!();
        let _ = io::stdout().flush();
        seq += 1;
    }
}
