//! Quick sanity check: connect to a real SO-101 leader arm over the Feetech
//! bus and read the current calibrated joint positions.
//!
//! Usage:
//!   cargo run --example read_leader -- /dev/cu.usbmodem5AB01836201

use feetech_servo_sdk::{FeetechBus, MotorBus};
use std::f32::consts::PI;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // -- 1. Parameter --------------------------------------------------------
    let port = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let baud = 1_000_000;

    // -- 2. Open the bus -----------------------------------------------------
    println!("🔗 Opening {port} @ {baud} baud ...");
    let mut bus = FeetechBus::new(&port, baud)?;
    println!("✅ bus opened");

    // -- 3. Disable torque (leader is backdrivable) --------------------------
    println!("⚡ disabling torque (leader) ...");
    bus.disable_torque(&ids).await?;

    // -- 4. Read and print ---------------------------------------------------
    println!("📡 sync_read_positions ...");
    let positions = bus.sync_read_positions(&ids).await?;
    println!();
    println!("{:>4} {:>10} {:>10}", "ID", "rad", "deg");
    println!("{}", "-".repeat(28));
    for (i, &pos) in positions.iter().enumerate() {
        println!(
            " {:>3} {:>10.4} {:>10.1}",
            ids[i],
            pos,
            pos * 180.0 / PI,
        );
    }
    println!("{}", "-".repeat(28));

    // -- 5. Optional: read individual servos ---------------------------------
    println!();
    println!("per-servo read (id 3, example):");
    let p3 = bus.read_position(3).await?;
    println!("  id 3 → {:.4} rad  ({:.1}°)", p3, p3 * 180.0 / PI);

    println!();
    println!("✅ Done — data successfully acquired from the SO-101 leader.");
    Ok(())
}
