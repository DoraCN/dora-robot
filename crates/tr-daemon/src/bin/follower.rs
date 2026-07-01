use std::time::{Duration, Instant};
use tr_codec::PostcardCodec;
use tr_daemon::config::DaemonConfig;
use tr_daemon::dora::DoraFlow;
use tr_daemon::state::{ArmState, DataflowAction, Fsm};
use tr_messages::control::{ControlCommand, DaemonStatus};
use tr_messages::Codec;
use tr_so101::config::So101Config;
use tr_so101::resolver::{parse_hex_u16, resolve_arm_port, UsbDeviceConfig};
use tr_so101::{FeetechBus, MotorBus, So101Arm, So101Follower};
use tr_robot::RobotDriver;
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
    let id = &config.arm.id;

    eprintln!("[follower] arm={id}  config={config_path}");

    let device = UsbDeviceConfig {
        vid: parse_hex_u16(&config.arm.so101.vid)?,
        pid: parse_hex_u16(&config.arm.so101.pid)?,
        serial: config.arm.so101.serial.clone(),
    };
    // --port overrides USB resolver (for testing)
    let cli_port = args.iter().position(|a| a == "--port")
        .and_then(|i| args.get(i + 1).cloned());
    let port = match cli_port {
        Some(p) => p,
        None => resolve_arm_port(&device)?,
    };
    eprintln!("[follower] USB -> {port}");

    let rt_arm = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;
    let rt_zenoh = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;

    let _guard = rt_arm.enter();
    let mut bus = FeetechBus::new(&port, config.arm.so101.baud)?;
    let ids_arr = ids_to_array(&config.arm.so101.ids);
    rt_arm.block_on(async { bus.disable_torque(&ids_arr).await })?;
    let arm = So101Arm::new(bus, So101Config::default());
    let mut follower = So101Follower::new(arm, 1, "follower");
    // Re-disable after construction (follower enables torque by default)
    rt_arm.block_on(async { follower.bus_mut().disable_torque(&ids_arr).await })?;
    drop(_guard);

    let k_ctrl   = format!("tr/{id}/control");
    let k_cmd    = format!("tr/{id}/command");
    let k_obs    = format!("tr/{id}/observation");
    let k_status = format!("tr/{id}/status");

    let mut t_ctrl  = ZenohTransport::subscriber(rt_zenoh.handle(), &k_ctrl)?;
    let mut t_cmd   = ZenohTransport::subscriber(rt_zenoh.handle(), &k_cmd)?;
    let mut t_obs   = ZenohTransport::publisher(rt_zenoh.handle(), &k_obs)?;
    let mut t_st    = ZenohTransport::publisher(rt_zenoh.handle(), &k_status)?;

    let mut fsm = Fsm::new();
    let mut dora: Option<DoraFlow> = None;
    let mut last_status = Instant::now();
    let mut frames: u64 = 0;
    let codec = PostcardCodec;

    eprintln!("[follower] state=IDLE");

    loop {
        match t_cmd.recv(Duration::from_millis(5)) {
            Ok(Some(inbound)) => {
                if let Ok(cmd) = codec.decode_control_command(&inbound.frame) {
                    eprintln!("[follower] cmd={:?}", cmd);
                    let (_, action) = fsm.apply(&cmd);
                    match action {
                        DataflowAction::Launch => {
                            if dora.is_none() {
                                match DoraFlow::launch(&config) {
                                    Ok(df) => dora = Some(df),
                                    Err(e) => eprintln!("[follower] DORA: {e}"),
                                }
                            }
                            rt_arm.block_on(
                                async { follower.bus_mut().enable_torque(&ids_arr).await },
                            ).ok();
                        }
                        DataflowAction::Stop => {
                            if let Some(df) = dora.take() {
                                let _ = df.stop();
                            }
                            rt_arm.block_on(
                                async { follower.bus_mut().disable_torque(&ids_arr).await },
                            ).ok();
                        }
                        DataflowAction::None => {}
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("[follower] cmd err: {e}");
                fsm.apply(&ControlCommand::TorqueOff);
                if let Some(df) = dora.take() { let _ = df.stop(); }
            }
        }

        match t_ctrl.recv(Duration::from_millis(5)) {
            Ok(Some(inbound)) => {
                if fsm.current() != ArmState::Idle {
                    if let Ok(cmd) = codec.decode_command(&inbound.frame) {
                        let _ = follower.command(&cmd);
                        frames += 1;
                    }
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("[follower] ctrl err: {e}"),
        }

        let obs: Vec<f32> = rt_arm.block_on(async {
            follower.arm_mut().read_joints().await
                .map(|a| a.to_vec())
                .unwrap_or_else(|_| vec![0.0; 6])
        });
        if let Ok(b) = codec.encode_observation(&obs) {
            let _ = t_obs.send(tr_transport::qos::Channel::Control, &b);
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

fn ids_to_array(ids: &[u8]) -> [u8; 6] {
    let mut arr = [1u8; 6];
    for (i, &id) in ids.iter().take(6).enumerate() {
        arr[i] = id;
    }
    arr
}
