use std::time::{Duration, Instant};
use tr_codec::PostcardCodec;
use tr_daemon::config::DaemonConfig;
use tr_daemon::dora::DoraFlow;
use tr_daemon::retry::Backoff;
use tr_daemon::state::{ArmState, DataflowAction, Fsm};
use tr_messages::Codec;
use tr_messages::control::{ControlCommand, DaemonStatus};
use tr_robot::RobotDriver;
use tr_so101::config::So101Config;
use tr_so101::resolver::{UsbDeviceConfig, parse_hex_u16, resolve_arm_port};
use tr_so101::{FeetechBus, MotorBus, So101Arm, So101Follower};
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;

fn main() -> anyhow::Result<()> {
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

        let mut fsm = Fsm::new();
        let mut dora: Option<DoraFlow> = None;
        let mut frames: u64 = 0;
        eprintln!("[follower] state=IDLE");

        'inner: loop {
            match t_cmd.recv(Duration::from_millis(5)) {
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
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[follower] cmd err, recovery: {e}");
                    handle_recovery(&mut t_st, &mut last_status, &mut dora);
                    break 'inner;
                }
            }

            match t_ctrl.recv(Duration::from_millis(5)) {
                Ok(Some(inbound)) => {
                    if fsm.current() != ArmState::Idle {
                        if let Ok(cmd) = codec.decode_command(&inbound.frame) {
                            if follower.command(&cmd).is_err() {
                                eprintln!("[follower] bus write error, recovery");
                                handle_recovery(&mut t_st, &mut last_status, &mut dora);
                                break 'inner;
                            }
                            frames += 1;
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[follower] ctrl err, recovery: {e}");
                    handle_recovery(&mut t_st, &mut last_status, &mut dora);
                    break 'inner;
                }
            }

            let obs_res = rt_arm
                .block_on(async { follower.arm_mut().read_joints().await.map(|a| a.to_vec()) });
            match obs_res {
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

            if let Some(ref d) = dora {
                if !d.alive() {
                    eprintln!("[follower] DORA crashed, recovery");
                    dora = None;
                    fsm.apply(&ControlCommand::TorqueOff);
                    handle_recovery(&mut t_st, &mut last_status, &mut dora);
                    break 'inner;
                }
            }

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
