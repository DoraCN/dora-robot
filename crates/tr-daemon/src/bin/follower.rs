use std::time::{Duration, Instant};
use tr_codec::PostcardCodec;
use tr_daemon::config::DaemonConfig;
use tr_daemon::dora::DoraFlow;
use tr_daemon::retry::Backoff;
use tr_daemon::state::{ArmState, DataflowAction, Fsm};
use tr_messages::{Codec, CommandBody};
use tr_messages::control::{ControlCommand, DaemonStatus};

use tr_so101::config::So101Config;
use tr_so101::resolver::{UsbDeviceConfig, parse_hex_u16, resolve_arm_port};
use tr_so101::{FeetechBus, MotorBus, So101Arm, So101Follower};
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;
use std::path::Path;

fn check_single_instance() -> bool {
    let pid_file = "/tmp/dorarobot-follower.pid";
    if let Ok(content) = std::fs::read_to_string(pid_file) {
        if let Ok(pid) = content.trim().parse::<i32>() {
            if Path::new(&format!("/proc/{pid}")).exists() {
                eprintln!("[follower] 已有实例运行 PID={pid}，退出");
                return false;
            }
        }
    }
    let _ = std::fs::write(pid_file, std::process::id().to_string());
    true
}

fn main() -> anyhow::Result<()> {
    if !check_single_instance() {
        return Ok(());
    }
    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("config/follower.toml");

    let toml_str = std::fs::read_to_string(config_path)?;
    let config = DaemonConfig::from_str(&toml_str)?;
    let id = config.arm.id.clone();

    eprintln!("[follower] arm={id}  config={config_path}");

    let device = UsbDeviceConfig {
        vid: parse_hex_u16(&config.arm.so101.vid)?,
        pid: parse_hex_u16(&config.arm.so101.pid)?,
        serial: config.arm.so101.serial.clone(),
    };
    let cli_port = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1).cloned());
    let port = match cli_port {
        Some(p) => p,
        None => resolve_arm_port(&device)?,
    };
    eprintln!("[follower] USB -> {port}");

    let ids_arr = ids_to_array(&config.arm.so101.ids);

    let rt_zenoh = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_io()
        .enable_time()
        .build()?;

    let k_status = format!("tr/{id}/status");
    let mut t_st = ZenohTransport::publisher(rt_zenoh.handle(), &k_status)?;

    let codec = PostcardCodec;
    let mut backoff = Backoff::new(1, 30);
    let mut last_status = Instant::now();

    // ── Main loop with recovery ──────────────────────────────
    loop {
        let (mut follower, rt_arm, mut t_ctrl, mut t_cmd, mut t_obs) =
            match connect_arm(&port, &config, &id, &ids_arr, &rt_zenoh) {
                Ok(t) => {
                    backoff.reset();
                    t
                }
                Err(e) => {
                    eprintln!("[follower] arm error: {e}");
                    pub_offline_status(&mut t_st, &mut last_status);
                    backoff.wait_and_advance();
                    continue;
                }
            };

        // M1 已验证方案：消息驱动 + 25ms 限速 + 0.002 rad 去重
        const MIN_WRITE_DT: Duration = Duration::from_millis(25);
        const DEDUP_THRESH: f32 = 0.002;

        let mut fsm = Fsm::new();
        let mut dora: Option<DoraFlow> = None;
        let mut frames: u64 = 0;
        let mut first_write = true;
        let mut last_write = Instant::now();
        let mut last_written = [0.0_f32; 6];
        let mut read_counter: u8 = 0;
        eprintln!("[follower] state=IDLE  M1-style (25ms+dedup)");

        'inner: loop {
            // ── ① FSM commands (non-blocking drain) ──────────
            loop {
                match t_cmd.recv(Duration::ZERO) {
                    Ok(Some(inbound)) => {
                        if let Ok(cmd) = codec.decode_control_command(&inbound.frame) {
                            eprintln!("[follower] cmd={:?}", cmd);
                            let (_, action) = fsm.apply(&cmd);
                            handle_dataflow_action(
                                &mut dora,
                                &config,
                                &rt_arm,
                                &mut follower,
                                &ids_arr,
                                action,
                            );
                            first_write = true; // reset on torque toggle
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("[follower] cmd err, recovery: {e}");
                        handle_recovery(&mut t_st, &mut last_status, &mut dora);
                        break 'inner;
                    }
                }
            }

            // ── ② Joint command (M1: recv one at a time) ────
            match t_ctrl.recv(Duration::from_millis(5)) {
                Ok(Some(inbound)) => {
                    if fsm.current() != ArmState::Idle {
                        if let Ok(cmd) = codec.decode_command(&inbound.frame) {
                            let positions: Vec<f32> = match &cmd.body {
                                CommandBody::Joint(jt) => {
                                    jt.positions.iter().map(|&p| p as f32).collect()
                                }
                                _ => continue,
                            };
                            if positions.len() < 6 {
                                continue;
                            }

                            // ── 去重：与上次写入的位置比较 ──
                            if !first_write {
                                let max_d = positions.iter().zip(last_written.iter())
                                    .map(|(a, b)| (a - b).abs())
                                    .fold(0.0_f32, f32::max);
                                if max_d < DEDUP_THRESH {
                                    continue; // 位置几乎没变，跳过写入
                                }
                            }

                            // ── 速率限制：距上次写入至少 25ms ──
                            if !first_write {
                                let elapsed = last_write.elapsed();
                                if elapsed < MIN_WRITE_DT {
                                    std::thread::sleep(MIN_WRITE_DT - elapsed);
                                }
                            }

                            // ── 写入（绕过 slew_clamp，与 M1 一致） ──
                            let mut target = [0.0_f32; 6];
                            target.copy_from_slice(&positions[..6]);
                            let ok = rt_arm.block_on(async {
                                follower.arm_mut().write_joints(&target).await.is_ok()
                            });
                            if ok {
                                last_write = Instant::now();
                                last_written = target;
                                first_write = false;
                                frames += 1;
                            } else {
                                eprintln!("[follower] bus write error, recovery");
                                handle_recovery(&mut t_st, &mut last_status, &mut dora);
                                break 'inner;
                            }
                        }
                    }
                }
                Ok(None) => {} // timeout, no message
                Err(e) => {
                    eprintln!("[follower] ctrl err, recovery: {e}");
                    handle_recovery(&mut t_st, &mut last_status, &mut dora);
                    break 'inner;
                }
            }

            // ── ③ Read at low rate (~12Hz, every ~3 loops) ───
            read_counter += 1;
            if read_counter >= 3 {
                read_counter = 0;
                match rt_arm.block_on(
                    async { follower.arm_mut().read_joints().await.map(|a| a.to_vec()) },
                ) {
                    Ok(obs) => {
                        if let Ok(b) = codec.encode_observation(&obs) {
                            let _ = t_obs.send(tr_transport::qos::Channel::Control, &b);
                        }
                    }
                    Err(e) => {
                        eprintln!("[follower] bus read error, recovery: {e}");
                        handle_recovery(&mut t_st, &mut last_status, &mut dora);
                        break 'inner;
                    }
                }
            }

            // ── DORA alive check ──────────────────────────────
            if let Some(ref d) = dora {
                if !d.alive() {
                    eprintln!("[follower] DORA crashed, recovery");
                    dora = None;
                    fsm.apply(&ControlCommand::TorqueOff);
                    handle_recovery(&mut t_st, &mut last_status, &mut dora);
                    break 'inner;
                }
            }

            // ── Status publish (1Hz) ──────────────────────────
            if last_status.elapsed() >= Duration::from_secs(1) {
                last_status = Instant::now();
                let state_str = match fsm.current() {
                    ArmState::Idle => "IDLE",
                    ArmState::Ready => "READY",
                    ArmState::Recording => "RECORDING",
                };
                let st = DaemonStatus {
                    state: state_str.into(),
                    torque_on: fsm.current() != ArmState::Idle,
                    recording: fsm.current() == ArmState::Recording,
                    episode: None,
                    frame_count: frames,
                    fps: 0.0,
                    error: None,
                };
                if let Ok(json) = serde_json::to_vec(&st) {
                    let _ = t_st.send(tr_transport::qos::Channel::Control, &json);
                }
            }

            std::thread::sleep(Duration::from_micros(500));
        }
    }
}

type ArmHandle = (
    So101Follower<feetech_servo_sdk::driver::FeetechController<tokio_serial::SerialStream>>,
    tokio::runtime::Runtime,
    ZenohTransport,
    ZenohTransport,
    ZenohTransport,
);

fn connect_arm(
    port: &str,
    config: &DaemonConfig,
    id: &str,
    ids_arr: &[u8; 6],
    rt_zenoh: &tokio::runtime::Runtime,
) -> anyhow::Result<ArmHandle> {
    let rt_arm = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_io()
        .enable_time()
        .build()?;

    let _guard = rt_arm.enter();
    let mut bus = FeetechBus::new(port, config.arm.so101.baud)?;
    rt_arm.block_on(async { bus.disable_torque(ids_arr).await })?;
    let arm = So101Arm::new(bus, So101Config::default());
    let mut follower = So101Follower::new(arm, 1, "follower");
    rt_arm.block_on(async { follower.bus_mut().disable_torque(ids_arr).await })?;
    drop(_guard);

    let t_ctrl = ZenohTransport::subscriber(rt_zenoh.handle(), &format!("tr/{id}/control"))?;
    let t_cmd = ZenohTransport::subscriber(rt_zenoh.handle(), &format!("tr/{id}/command"))?;
    let t_obs = ZenohTransport::publisher(rt_zenoh.handle(), &format!("tr/{id}/observation"))?;

    Ok((follower, rt_arm, t_ctrl, t_cmd, t_obs))
}

fn handle_dataflow_action(
    dora: &mut Option<DoraFlow>,
    config: &DaemonConfig,
    rt_arm: &tokio::runtime::Runtime,
    follower: &mut So101Follower<
        feetech_servo_sdk::driver::FeetechController<tokio_serial::SerialStream>,
    >,
    ids_arr: &[u8; 6],
    action: DataflowAction,
) {
    match action {
        DataflowAction::Launch => {
            if dora.is_none() {
                match DoraFlow::launch(config) {
                    Ok(df) => *dora = Some(df),
                    Err(e) => eprintln!("[follower] DORA: {e}"),
                }
            }
            rt_arm
                .block_on(async { follower.bus_mut().enable_torque(ids_arr).await })
                .ok();
        }
        DataflowAction::Stop => {
            if let Some(df) = dora.take() {
                let _ = df.stop();
            }
            rt_arm
                .block_on(async { follower.bus_mut().disable_torque(ids_arr).await })
                .ok();
        }
        DataflowAction::None => {}
    }
}

fn handle_recovery(
    t_st: &mut ZenohTransport,
    last_status: &mut Instant,
    dora: &mut Option<DoraFlow>,
) {
    if let Some(df) = dora.take() {
        let _ = df.stop();
    }
    pub_offline_status(t_st, last_status);
}

fn pub_offline_status(t_st: &mut ZenohTransport, last_status: &mut Instant) {
    *last_status = Instant::now();
    let st = DaemonStatus {
        state: "OFFLINE".into(),
        torque_on: false,
        recording: false,
        episode: None,
        frame_count: 0,
        fps: 0.0,
        error: Some("reconnecting...".into()),
    };
    if let Ok(json) = serde_json::to_vec(&st) {
        let _ = t_st.send(tr_transport::qos::Channel::Control, &json);
    }
}

fn ids_to_array(ids: &[u8]) -> [u8; 6] {
    let mut arr = [1u8; 6];
    for (i, &id) in ids.iter().take(6).enumerate() {
        arr[i] = id;
    }
    arr
}
