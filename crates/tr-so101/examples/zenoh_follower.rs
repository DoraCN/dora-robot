//! Zenoh → follower arm.
//!
//! Subscribes to a zenoh control key, decodes `JointTargets`, drives the
//! SO-101 follower.  Optional `--record` mode outputs frames + episode events
//! on stdout for the Python pipe recorder.
//!
//! --record stdout protocol:
//!   D j1 j2 j3 j4 j5 j6     data frame (radians)
//!   @START / @SUCCESS / @FAIL / @RERECORD / @STOP
//!
//! Usage:
//!   # vanilla teleop
//!   cargo run -p tr-so101 --example zenoh_follower -- /dev/cu.usbmodemxxx
//!
//!   # with recording
//!   cargo run ... --record | python -m tr_lerobot.pipe_recorder

use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus};
use std::io::{self, Write};
use std::mem::ManuallyDrop;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tr_codec::PostcardCodec;
use tr_messages::{Codec, CommandBody, EpisodeEvent, TeleopCommand};
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
    let do_record = args.iter().any(|a| a == "--record");

    let ids: [u8; 6] = [1, 2, 3, 4, 5, 6];
    let codec = PostcardCodec;

    if do_record {
        eprintln!("  [zenoh_follower] record mode ON — stdout protocol");
    }
    eprintln!("  keys: {key_control} / {key_episode}");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;

    // Control subscription.
    let mut transport_ctrl = ZenohTransport::subscriber(rt.handle(), &key_control)?;

    // Episode subscription (raw zenoh session, kept alive via ManuallyDrop).
    let (ep_tx, ep_rx) = mpsc::channel::<EpisodeEvent>();
    let _ep_keep: Option<ManuallyDrop<Box<dyn std::any::Any + Send>>> = if do_record {
        let session = rt.block_on(async {
            zenoh::open(zenoh::Config::default()).await.map_err(|e| anyhow::anyhow!("{e}"))
        })?;
        let sub = rt.block_on(async {
            session.declare_subscriber(key_episode.as_str())
                .callback({
                    let codec = PostcardCodec;
                    let tx = ep_tx.clone();
                    move |sample| {
                        let payload = sample.payload().to_bytes().to_vec();
                        if let Ok(ev) = codec.decode_episode(&payload) {
                            let _ = tx.send(ev);
                        }
                    }
                })
                .await.map_err(|e| anyhow::anyhow!("{e}"))
        })?;
        Some(ManuallyDrop::new(Box::new((session, sub))))
    } else {
        None
    };

    // Output pipe — line-buffered directly (no BufWriter, lost on Ctrl‑C).
    let mut stdout = io::stdout();

    let result = rt.block_on(async {
        eprintln!("🔗 follower on {port} ...");
        let mut bus = FeetechBus::new(&port, 1_000_000)?;
        bus.enable_torque(&ids).await?;
        eprintln!("   torque: ON\n");

        let mut first = true;
        let mut count = 0u64;
        let mut last_written_pos = [0.0_f32; 6];
        let mut last_write = Instant::now();
        const DEDUP_THRESH: f32 = 0.002;
        const MIN_WRITE_DT: Duration = Duration::from_millis(25);

        let mut should_stop = false;

        loop {
            // Drain episode events.
            while let Ok(ev) = ep_rx.try_recv() {
                if do_record {
                    let line = match ev {
                        EpisodeEvent::Start => "@START",
                        EpisodeEvent::End { outcome: tr_messages::EpisodeOutcome::Success } => "@SUCCESS",
                        EpisodeEvent::End { outcome: tr_messages::EpisodeOutcome::Fail } => "@FAIL",
                        EpisodeEvent::End { outcome: tr_messages::EpisodeOutcome::Rerecord } => "@RERECORD",
                        EpisodeEvent::Stop => "@STOP",
                    };
                    let is_stop = matches!(ev, EpisodeEvent::Stop);
                    writeln!(stdout, "{line}")?;
                    stdout.flush()?;
                    if is_stop { should_stop = true; }
                }
            }
            if should_stop { break; }

            if let Ok(Some(inbound)) = transport_ctrl.recv(Duration::from_millis(1)) {
                let cmd: TeleopCommand = codec.decode_command(&inbound.frame)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let joint_rad: Vec<f32> = match &cmd.body {
                    CommandBody::Joint(jt) => jt.positions.iter().map(|&p| p as f32).collect(),
                    _ => continue,
                };
                if joint_rad.len() < 6 { continue; }

                // ── recording pipe ─────────────────────────────
                if do_record {
                    write!(stdout, "D")?;
                    for v in &joint_rad { write!(stdout, " {:.6}", v)?; }
                    writeln!(stdout)?;
                    stdout.flush()?;
                }
                // ───────────────────────────────────────────────

                if !first {
                    let max_d = joint_rad.iter().zip(last_written_pos.iter())
                        .map(|(a,b)| (a-b).abs()).fold(0.0_f32, f32::max);
                    if max_d < DEDUP_THRESH { continue; }
                }
                if !first {
                    let elapsed = last_write.elapsed();
                    if elapsed < MIN_WRITE_DT {
                        tokio::time::sleep(MIN_WRITE_DT - elapsed).await;
                    }
                }
                last_write = Instant::now();
                last_written_pos.copy_from_slice(&joint_rad);

                let cmds: Vec<(u8, ControlOp)> = ids.iter()
                    .zip(joint_rad.iter())
                    .map(|(&id, &p)| (id, ControlOp::Position(p))).collect();

                if first {
                    bus.sync_write_goals(&cmds).await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    first = false;
                } else {
                    if let Err(e) = bus.sync_write_goals(&cmds).await {
                        eprintln!("[warn] write error @ frame {count}: {e}");
                    }
                }
                if count < 20 || count % 50 == 0 {
                    eprintln!("[play] frame={:>4}  j1={:>7.1}°", count, joint_rad[0] * 57.2958);
                }
                count += 1;
            }
        }
        #[allow(unreachable_code)]
        Ok::<_, anyhow::Error>(())
    });

    if do_record {
        let _ = writeln!(stdout, "@STOP");
        let _ = stdout.flush();
    }
    result?;
    Ok(())
}
