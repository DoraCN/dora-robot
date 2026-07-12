use std::io::{self, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;
use tr_codec::PostcardCodec;
use tr_daemon::config::DaemonConfig;
use tr_daemon::retry::Backoff;
use tr_daemon::web::{self, WebState};
use tr_messages::control::ControlCommand;
use tr_messages::Codec;
use tr_messages::EpisodeOutcome;
use tr_so101::config::So101Config;
use tr_so101::resolver::{parse_hex_u16, resolve_arm_port, UsbDeviceConfig};
use tr_so101::{FeetechBus, So101Arm, So101Leader};
use tr_teleop::TeleopDevice;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .iter().position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("config/leader.toml");

    let toml_str = std::fs::read_to_string(config_path)?;
    let config = DaemonConfig::from_str(&toml_str)?;
    let id = config.arm.id.clone();

    eprintln!("[leader] arm={id}  config={config_path}");

    let device = UsbDeviceConfig {
        vid: parse_hex_u16(&config.arm.so101.vid)?,
        pid: parse_hex_u16(&config.arm.so101.pid)?,
        serial: config.arm.so101.serial.clone(),
    };
    let cli_port = args.iter().position(|a| a == "--port")
        .and_then(|i| args.get(i + 1).cloned());
    let port = match cli_port {
        Some(p) => p,
        None => resolve_arm_port(&device)?,
    };
    eprintln!("[leader] USB -> {port}");

    // ── zenoh: 1 worker thread, 1 session, direct pub/sub ──
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;

    let k_ctrl = format!("tr/{id}/control");
    let k_cmd  = format!("tr/{id}/command");
    let k_status = format!("tr/{id}/status");

    let session = rt.block_on(async {
        zenoh::open(zenoh::Config::default()).await.map_err(|e| anyhow::anyhow!("{e}"))
    })?;

    // Subscriber status → mpsc（callback on zenoh worker thread）
    let (status_tx_mpsc, status_rx) = mpsc::channel::<String>();
    rt.block_on(async {
        session.declare_subscriber(k_status.as_str())
            .callback(move |sample| {
                let payload = sample.payload().to_bytes().to_vec();
                let _ = status_tx_mpsc.send(String::from_utf8_lossy(&payload).to_string());
            })
            .await.map_err(|e| anyhow::anyhow!("{e}"))
    })?;

    // Web server
    let (status_tx_sse, _) = tokio::sync::broadcast::channel::<String>(8);
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let arm_info = format!("arm={id}");
    let web_state = Arc::new(WebState { status_tx: status_tx_sse, cmd_tx, arm_info });
    let app = web::router(web_state.clone());
    let web_rt = tokio::runtime::Builder::new_current_thread()
        .enable_io().enable_time().build()?;
    web_rt.spawn(async {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
        eprintln!("[leader] web console → http://localhost:8080");
        axum::serve(listener, app).await.unwrap();
    });

    let codec = PostcardCodec;
    let mut backoff = Backoff::new(1, 30);

    // Keyboard input thread
    let (kb_tx, kb_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || loop {
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() { break; }
        let _ = kb_tx.send(line);
    });

    println!("── leader-daemon ──");
    println!("  Web: http://localhost:8080");
    println!("  keyboard: o(使能) x(失能) s(采集) f(保存) r(重录) q(停止)");
    println!("────────────────────");

    // ── Outer recovery loop ────────────────────────────────
    loop {
        let (mut leader, _rt_arm) =
            match connect_leader_arm(&port, &config) {
                Ok(t) => { backoff.reset(); t }
                Err(e) => {
                    eprintln!("[leader] arm error: {e}");
                    backoff.wait_and_advance();
                    continue;
                }
            };

        eprintln!("[leader] connected");

        // ── Inner control loop ──────────────────────────────
        loop {
            match leader.poll() {
                Some(cmd) => {
                    if let Ok(bytes) = codec.encode_command(&cmd) {
                        let s = session.clone();
                        let k = k_ctrl.clone();
                        rt.block_on(async {
                            s.put(k, bytes)
                                .congestion_control(zenoh::qos::CongestionControl::Drop)
                                .await
                        }).ok();
                    }
                }
                None => {}
            }

            // Follower status → SSE
            match status_rx.recv_timeout(Duration::from_millis(5)) {
                Ok(json) => {
                    let _ = web_state.status_tx.send(json);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(_) => {}
            }

            // Keyboard
            if let Ok(line) = kb_rx.try_recv() {
                if let Some(cmd) = parse_key(&line) {
                    send_zenoh_cmd(&codec, cmd, &session, &k_cmd, &rt);
                }
            }

            // Web commands
            while let Ok(cmd_str) = cmd_rx.try_recv() {
                if let Some(cmd) = parse_web_cmd(&cmd_str) {
                    send_zenoh_cmd(&codec, cmd, &session, &k_cmd, &rt);
                }
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn connect_leader_arm(
    port: &str,
    config: &DaemonConfig,
) -> anyhow::Result<(
    So101Leader<feetech_servo_sdk::driver::FeetechController<tokio_serial::SerialStream>>,
    tokio::runtime::Runtime,
)> {
    let rt_arm = tokio::runtime::Builder::new_current_thread()
        .enable_io().enable_time().build()?;

    let _guard = rt_arm.enter();
    let bus = FeetechBus::new(port, config.arm.so101.baud)?;
    let arm = So101Arm::new(bus, So101Config::default());
    let leader = So101Leader::new(arm, 1, "leader");
    drop(_guard);

    Ok((leader, rt_arm))
}

fn parse_key(line: &str) -> Option<ControlCommand> {
    match line.trim() {
        "o" => Some(ControlCommand::TorqueOn),
        "x" => Some(ControlCommand::TorqueOff),
        "s" => Some(ControlCommand::StartRecord { task: "teleop".into() }),
        "f" => Some(ControlCommand::EndRecord { outcome: EpisodeOutcome::Success }),
        "r" => Some(ControlCommand::ReRecord),
        "q" => Some(ControlCommand::Stop),
        _ => None,
    }
}

fn parse_web_cmd(cmd: &str) -> Option<ControlCommand> {
    match cmd {
        "TorqueOn" => Some(ControlCommand::TorqueOn),
        "TorqueOff" => Some(ControlCommand::TorqueOff),
        "StartRecord" => Some(ControlCommand::StartRecord { task: "teleop".into() }),
        "EndRecord" => Some(ControlCommand::EndRecord { outcome: EpisodeOutcome::Success }),
        "ReRecord" => Some(ControlCommand::ReRecord),
        "Stop" => Some(ControlCommand::Stop),
        _ => None,
    }
}

fn send_zenoh_cmd(codec: &PostcardCodec, cmd: ControlCommand, session: &zenoh::Session, key: &str, rt: &tokio::runtime::Runtime) {
    println!("  -> {:?}", cmd);
    if let Ok(bytes) = codec.encode_control_command(&cmd) {
        let k = key.to_string();
        let s = session.clone();
        rt.block_on(async {
            s.put(k, bytes)
                .congestion_control(zenoh::qos::CongestionControl::Drop)
                .await
        }).ok();
    }
    let _ = io::stdout().flush();
}
