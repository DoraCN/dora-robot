//! Leader daemon: 主循环直接读臂，zenoh+web 共享单 runtime
//!
//! main thread:  读串口 → 发布 zenoh → 收消息 → sleep
//! rt:           zenoh + web 共享 (multi_thread(1))

use std::io;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use tr_codec::PostcardCodec;
use tr_daemon::config::DaemonConfig;
use tr_daemon::retry::Backoff;
use tr_daemon::web::{self, WebState};
use tr_messages::control::ControlCommand;
use tr_messages::{Codec, CommandBody, ControlMode, JointTargets, MessageHeader, TeleopCommand};
use tr_messages::EpisodeOutcome;
use tr_so101::config::So101Config;
use tr_so101::resolver::{parse_hex_u16, resolve_arm_port, UsbDeviceConfig};
use tr_so101::{FeetechBus, So101Arm};
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
    let config_path = args.iter().position(|a| a == "--config")
        .and_then(|i| args.get(i + 1)).map(|s| s.as_str())
        .unwrap_or("config/leader.toml");

    let toml_str = std::fs::read_to_string(config_path)?;
    let config = DaemonConfig::from_str(&toml_str)?;
    let id = config.arm.id.clone();

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
    eprintln!("[leader] arm={id}  port={port}");

    // ── zenoh + web runtime ─────────────────────
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;

    // web
    let (web_state_tx, _) = tokio::sync::broadcast::channel::<String>(8);
    let (web_cmd_tx, mut web_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let app = web::router(Arc::new(WebState {
        status_tx: web_state_tx.clone(),
        cmd_tx: web_cmd_tx,
        arm_info: format!("arm={id}"),
    }));
    rt.spawn(async {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
        eprintln!("[web] http://localhost:8080");
        axum::serve(listener, app).await.unwrap();
    });

    // web commands → ctrl_tx
    let (ctrl_tx, ctrl_rx) = mpsc::channel::<ControlCommand>();
    let ctrl_tx2 = ctrl_tx.clone();  // for keyboard
    std::thread::spawn(move || loop {
        match web_cmd_rx.blocking_recv() {
            Some(s) => { if let Some(c) = parse_web_cmd(&s) { let _ = ctrl_tx.send(c); } }
            None => break,
        }
    });

    // keyboard
    std::thread::spawn({
        let tx = ctrl_tx2;
        move || loop {
            let mut line = String::new();
            if io::stdin().read_line(&mut line).is_err() { break; }
            if let Some(c) = parse_key(&line) { let _ = tx.send(c); }
        }
    });

    // zenoh session
    let session = rt.block_on(async {
        zenoh::open(zenoh::Config::default()).await.map_err(|e| anyhow::anyhow!("{e}"))
    })?;
    let k_ctrl = format!("tr/{id}/control");
    let k_cmd  = format!("tr/{id}/command");

    // subscriber → mpsc
    let (status_tx, status_rx) = mpsc::channel::<String>();
    rt.block_on(async {
        session.declare_subscriber(format!("tr/{id}/status").as_str())
            .callback(move |sample| {
                let payload = sample.payload().to_bytes().to_vec();
                let _ = status_tx.send(String::from_utf8_lossy(&payload).to_string());
            })
            .await.map_err(|e| anyhow::anyhow!("{e}"))
    })?;

    // ── arm runtime (current_thread, 仅用于 block_on) ──
    let arm_rt = tokio::runtime::Builder::new_current_thread()
        .enable_io().enable_time().build()?;
    let mut arm: Option<So101Arm<tr_so101::FeetechBus>> = None;

    let codec = PostcardCodec;
    let mut fsm = tr_daemon::state::Fsm::new();
    let mut seq: u64 = 0;
    let mut backoff = Backoff::new(1, 30);

    println!("── leader-daemon ──");
    println!("  port: {port}");
    println!("  web:  http://localhost:8080");
    println!("  keys:  o(使能) x(失能) s(采集) f(保存) r(重录) q(停止)");
    println!("────────────────────");

    loop {
        // ── arm connect/reconnect ────────────────
        if arm.is_none() {
            let _guard = rt.enter();
            match FeetechBus::new(&port, config.arm.so101.baud) {
                Ok(bus) => {
                    let mut a = So101Arm::new(bus, So101Config::default());
                    arm_rt.block_on(async { a.set_torque(false).await }).ok();
                    arm = Some(a);
                    backoff.reset();
                    eprintln!("[arm] connected");
                }
                Err(e) => {
                    drop(_guard);
                    eprintln!("[arm] error: {e}");
                    backoff.wait_and_advance();
                    std::thread::sleep(Duration::from_millis(1000));
                    continue;
                }
            }
        }

        // ── 读臂 → 发布 zenoh ─────────────────
        if let Some(ref mut a) = arm {
            match arm_rt.block_on(async { a.read_joints().await }) {
                Ok(joints) => {
                    if fsm.current() != tr_daemon::state::ArmState::Idle {
                        let positions: Vec<f64> = joints.iter().map(|&j| j as f64).collect();
                        let mut h = MessageHeader::new(1, "leader", ControlMode::JointTargets);
                        h.seq = seq;
                        let cmd = TeleopCommand {
                            header: h,
                            body: CommandBody::Joint(JointTargets {
                                positions, velocities: None, efforts: None,
                            }),
                        };
                        seq += 1;
                        if let Ok(bytes) = codec.encode_command(&cmd) {
                            let s = session.clone();
                            let k = k_ctrl.clone();
                            rt.spawn(async move {
                                s.put(k, bytes)
                                    .congestion_control(zenoh::qos::CongestionControl::Drop)
                                    .await.ok();
                            });
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[arm] read error: {e}");
                    arm = None;
                    continue;
                }
            }
        }

        // ── 收控制指令 ────────────────────────
        while let Ok(cmd) = ctrl_rx.try_recv() {
            let (state, _) = fsm.apply(&cmd);
            eprintln!("[ctrl] {cmd:?} → {state:?}");
            // 控制指令 → zenoh
            if let Ok(bytes) = codec.encode_control_command(&cmd) {
                let s = session.clone();
                let k = k_cmd.clone();
                rt.spawn(async move {
                    s.put(k, bytes)
                        .congestion_control(zenoh::qos::CongestionControl::Drop)
                        .await.ok();
                });
            }
        }

        // ── 收从臂状态 → SSE ──────────────────
        while let Ok(json) = status_rx.try_recv() {
            let _ = web_state_tx.send(json);
        }

        std::thread::sleep(Duration::from_millis(25));
    }
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
