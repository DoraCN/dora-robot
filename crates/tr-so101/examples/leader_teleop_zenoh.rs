//! Real SO-101 leader → zenoh publisher (live teleop sender)
//! + keyboard episode control.
//!
//! Keyboard controls (press key then Enter):
//!   s      Start episode
//!   (Enter) End — Success (save)
//!   f      End — Fail (discard)
//!   r      End — Rerecord (discard, keep recording)
//!   q      Quit (stop publishing)
//!
//! Usage:
//!   cargo run -p tr-so101 --example leader_teleop_zenoh -- /dev/cu.usbmodem5AB01836201

use feetech_servo_sdk::{FeetechBus, MotorBus};
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tr_codec::PostcardCodec;
use tr_messages::{Codec, CommandBody, ControlMode, EpisodeEvent, EpisodeOutcome,
    JointTargets, MessageHeader, TeleopCommand};
use tr_transport::qos::Channel;
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned().unwrap_or_else(|| "/dev/cu.usbmodem5AB01836201".into());
    let key_base = args.iter().position(|a| a == "--key")
        .and_then(|i| args.get(i + 1)).cloned()
        .unwrap_or_else(|| "tr/csv".into());
    let key_control = format!("{key_base}/control");
    let key_episode = format!("{key_base}/episode");

    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let codec = PostcardCodec;

    println!("────────────────────────────────────────────");
    println!("  LEADER → ZENOH  (live teleop)");
    println!("  port   : {port}");
    println!("  keys   : {key_control} / {key_episode}");
    println!("  [s]tart  [Enter]success  [f]ail  [r]erecord  [q]uit");
    println!("────────────────────────────────────────────");

    let rt_zenoh = tokio::runtime::Runtime::new()?;
    let rt_arm = tokio::runtime::Runtime::new()?;

    println!("🔗 zenoh publishers ...");
    let mut transport_ctrl = ZenohTransport::publisher(rt_zenoh.handle(), &key_control)?;
    let mut transport_ep = ZenohTransport::publisher(rt_zenoh.handle(), &key_episode)?;

    // Keyboard thread → reads stdin lines, publishes episode events.
    let quit = Arc::new(AtomicBool::new(false));
    let quit2 = quit.clone();
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut msg = String::new();
        loop {
            msg.clear();
            if stdin.lock().read_line(&mut msg).is_err() { break; }
            let ch = msg.trim().to_lowercase();
            let event = match ch.as_str() {
                "s" => Some(EpisodeEvent::Start),
                "" | "\n" => Some(EpisodeEvent::End { outcome: EpisodeOutcome::Success }),
                "f" => Some(EpisodeEvent::End { outcome: EpisodeOutcome::Fail }),
                "r" => Some(EpisodeEvent::End { outcome: EpisodeOutcome::Rerecord }),
                "q" => {
                    let _ = PostcardCodec.encode_episode(&EpisodeEvent::Stop).map(|b| transport_ep.send(Channel::Episode, &b));
                    quit2.store(true, Ordering::SeqCst); break;
                }
                _ => { println!("  ? {ch}"); continue; }
            };
            if let Some(ev) = event {
                let encoded = PostcardCodec.encode_episode(&ev);
                if let Ok(bytes) = encoded {
                    let _ = transport_ep.send(Channel::Episode, &bytes);
                    eprintln!("  → {ev:?}");
                }
            }
        }
    });

    // Arm read loop.
    let _guard = rt_arm.enter();
    println!("🔗 leader on {port} ...");
    let mut bus = FeetechBus::new(&port, 1_000_000)?;
    rt_arm.block_on(async { bus.disable_torque(&ids).await })?;
    println!("   torque: OFF\n▶  Publishing (keyboard for episode control)\n");

    let mut seq: u64 = 0;
    let mut last = [0.0_f32; 6];
    loop {
        if quit.load(Ordering::SeqCst) { break; }
        std::thread::sleep(Duration::from_millis(1));
        let positions = match rt_arm.block_on(async { bus.sync_read_positions(&ids).await }) {
            Ok(p) => { last.copy_from_slice(&p); p }
            Err(e) => { eprintln!("[warn] read: {e}"); last.to_vec() }
        };
        let cmd = TeleopCommand {
            header: MessageHeader::new(0, "leader", ControlMode::JointTargets),
            body: CommandBody::Joint(JointTargets {
                positions: positions.iter().map(|&p| p as f64).collect(),
                velocities: None, efforts: None,
            }),
        };
        let bytes = codec.encode_command(&cmd).map_err(|e| anyhow::anyhow!("{e}"))?;
        transport_ctrl.send(Channel::Control, &bytes)?;
        seq += 1;
        if seq % 100 == 0 { print!("\r   frames: {seq}"); let _ = io::stdout().flush(); }
    }

    println!("\n👋 leader stopped.");
    Ok(())
}
