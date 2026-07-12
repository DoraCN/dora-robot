use std::io::{self, Write};
use std::sync::mpsc;
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
use std::path::Path;

fn check_single_instance() -> bool {
    let pid_file = "/tmp/dorarobot-leader.pid";
    if let Ok(content) = std::fs::read_to_string(pid_file) {
        if let Ok(pid) = content.trim().parse::<i32>() {
            if Path::new(&format!("/proc/{pid}")).exists() {
                eprintln!("[leader] 已有实例运行 PID={pid}，退出");
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

    // ── 单 runtime 驱动全部 I/O (zenoh + web + arm) ──
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;
    let handle = rt.handle().clone();

    let k_ctrl = format!("tr/{id}/control");
    let k_cmd  = format!("tr/{id}/command");
    let k_status = format!("tr/{id}/status");

    // Web server — 与 zenoh 复用同一个 runtime
    let (status_tx, _) = tokio::sync::broadcast::channel::<String>(8);
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let arm_info = format!("arm={id}");
    let web_state = Arc::new(WebState { status_tx: status_tx.clone(), cmd_tx, arm_info });
    let app = web::router(web_state.clone());

    let (kb_tx, kb_rx) = std::sync::mpsc::channel::<String>();
    let codec = PostcardCodec;
    let mut backoff = Backoff::new(1, 30);

    rt.block_on(async move {
        // ── 启动 web server 在同一个 runtime 上 ──
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
        eprintln!("[leader] web console → http://localhost:8080");
        tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

        // ── 启动 zenoh session ──
        let session = zenoh::open(zenoh::Config::default()).await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // subscriber callback
        let (status_tx_mpsc, status_rx) = mpsc::channel::<String>();
        session.declare_subscriber(k_status.as_str())
            .callback(move |sample| {
                let payload = sample.payload().to_bytes().to_vec();
                let _ = status_tx_mpsc.send(String::from_utf8_lossy(&payload).to_string());
            })
            .await.map_err(|e| anyhow::anyhow!("{e}"))?;

        // ── 键盘线程 ──
        std::thread::spawn(move || loop {
            let mut line = String::new();
            if io::stdin().read_line(&mut line).is_err() { break; }
            let _ = kb_tx.send(line);
        });

        println!("── leader-daemon ──");
        println!("  Web: http://localhost:8080");
        println!("  keyboard: o(使能) x(失能) s(采集) f(保存) r(重录) q(停止)");
        println!("────────────────────");

        // ── Outer recovery loop ──────────────────────
        let mut interval = tokio::time::interval(Duration::from_millis(25));
        loop {
            let (mut leader, _rt_arm) = loop {
                match connect_leader_arm(&port, &config, &handle) {
                    Ok(t) => { backoff.reset(); break t; }
                    Err(e) => {
                        eprintln!("[leader] arm error: {e}");
                        backoff.wait_and_advance();
                    }
                }
            };
            eprintln!("[leader] connected");

            let mut loops: u64 = 0;
            let mut t0 = tokio::time::Instant::now();
            loop {
                interval.tick().await;

                match leader.poll() {
                    Some(cmd) => {
                        if let Ok(bytes) = codec.encode_command(&cmd) {
                            session.put(k_ctrl.as_str(), bytes)
                                .congestion_control(zenoh::qos::CongestionControl::Drop)
                                .await.ok();
                        }
                    }
                    None => {}
                }

                // Follower status → SSE
                match status_rx.try_recv() {
                    Ok(json) => { let _ = status_tx.send(json); }
                    Err(_) => {}
                }

                // Keyboard
                if let Ok(line) = kb_rx.try_recv() {
                    if let Some(cmd) = parse_key(&line) {
                        send_zenoh_cmd(&codec, cmd, session.clone(), &k_cmd);
                    }
                }

                // Web commands
                while let Ok(cmd_str) = cmd_rx.try_recv() {
                    if let Some(cmd) = parse_web_cmd(&cmd_str) {
                        send_zenoh_cmd(&codec, cmd, session.clone(), &k_cmd);
                    }
                }

                loops += 1;
                if t0.elapsed() >= Duration::from_secs(2) {
                    eprintln!("[leader] loops={loops} arm_error={}", leader.poll().is_none());
                    loops = 0;
                    t0 = tokio::time::Instant::now();
                }
            }
        }
        #[allow(unreachable_code)]
        Ok::<_, anyhow::Error>(())
    })?;

    Ok(())
}

fn connect_leader_arm(
    port: &str,
    config: &DaemonConfig,
    handle: &tokio::runtime::Handle,
) -> anyhow::Result<(
    So101Leader<feetech_servo_sdk::driver::FeetechController<tokio_serial::SerialStream>>,
    (),
)> {
    let bus = FeetechBus::new(port, config.arm.so101.baud)?;
    let arm = So101Arm::new(bus, So101Config::default());
    let leader = So101Leader::new(arm, handle.clone(), 1, "leader");
    Ok((leader, ()))
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

fn send_zenoh_cmd(codec: &PostcardCodec, cmd: ControlCommand, session: zenoh::Session, key: &str) {
    println!("  -> {:?}", cmd);
    if let Ok(bytes) = codec.encode_control_command(&cmd) {
        let s = session.clone();
        let k = key.to_string();
        tokio::spawn(async move {
            s.put(k, bytes)
                .congestion_control(zenoh::qos::CongestionControl::Drop)
                .await.ok();
        });
    }
    let _ = io::stdout().flush();
}
