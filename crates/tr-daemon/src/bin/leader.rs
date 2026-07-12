//! Leader daemon: 三线程架构
//!
//! thread 1 (arm):    读串口 → mpsc 发送位置
//! thread 2 (zenoh):  事件驱动 pub/sub + web server
//! main thread:       收 3 路 mpsc，协调转发 + sleep 节流

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

// ── 单实例检查 ───────────────────────────────

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

// ── Thread 1: arm 串口读取 ─────────────────────

fn arm_spawn(
    port: String,
    baud: u32,
    tx_pos: mpsc::Sender<Vec<f32>>,
    rx_cmd: mpsc::Receiver<ArmCmd>,
) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_io().enable_time().build()
        {
            Ok(r) => r,
            Err(e) => { eprintln!("[arm] runtime: {e}"); return; }
        };

        let mut backoff = Backoff::new(1, 30);
        loop {
            let (mut arm, _guard) = match rt.block_on(async {
                let bus = FeetechBus::new(&port, baud)?;
                let mut arm = So101Arm::new(bus, So101Config::default());
                arm.set_torque(false).await?;  // leader backdrivable
                Ok::<_, anyhow::Error>((arm, ()))
            }) {
                Ok(a) => { backoff.reset(); a }
                Err(e) => {
                    eprintln!("[arm] connect error: {e}");
                    backoff.wait_and_advance();
                    continue;
                }
            };

            eprintln!("[arm] connected");
            loop {
                // 检查控制指令
                if let Ok(cmd) = rx_cmd.try_recv() {
                    match cmd {
                        ArmCmd::EnableTorque  => {
                            rt.block_on(async { arm.set_torque(true).await }).ok();
                            eprintln!("[arm] torque ON");
                        }
                        ArmCmd::DisableTorque => {
                            rt.block_on(async { arm.set_torque(false).await }).ok();
                            eprintln!("[arm] torque OFF");
                        }
                    }
                }

                match rt.block_on(async { arm.read_joints().await }) {
                    Ok(joints) => {
                        if tx_pos.send(joints.to_vec()).is_err() { break; }
                    }
                    Err(e) => {
                        eprintln!("[arm] read error: {e}");
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            eprintln!("[arm] disconnected, reconnecting...");
        }
    });
}

#[derive(Debug, Clone, Copy)]
enum ArmCmd {
    EnableTorque,
    DisableTorque,
}

// ── Thread 2: zenoh + web ──────────────────────

struct ZenohHandle {
    session: zenoh::Session,
    _rt: tokio::runtime::Runtime,
}

fn zenoh_spawn(
    id: String,
    tx_status: mpsc::Sender<String>,
    cmd_tx: mpsc::Sender<ControlCommand>,
    web_state_tx: tokio::sync::broadcast::Sender<String>,
) -> ZenohHandle {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build().unwrap();

    let (web_cmd_tx, mut web_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let app = web::router(Arc::new(WebState {
        status_tx: web_state_tx.clone(),
        cmd_tx: web_cmd_tx,
        arm_info: format!("arm={id}"),
    }));

    // spawn web
    rt.spawn(async {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
        eprintln!("[zenoh] web console → http://localhost:8080");
        axum::serve(listener, app).await.unwrap();
    });

    // web commands → cmd_tx
    std::thread::spawn(move || loop {
        match web_cmd_rx.blocking_recv() {
            Some(s) => {
                if let Some(c) = parse_web_cmd(&s) {
                    let _ = cmd_tx.send(c);
                }
            }
            None => break,
        }
    });

    // zenoh session
    let session = rt.block_on(async {
        zenoh::open(zenoh::Config::default()).await.map_err(|e| anyhow::anyhow!("{e}"))
    }).unwrap();

    // subscriber → mpsc
    let k_status = format!("tr/{id}/status");
    rt.block_on(async {
        session.declare_subscriber(k_status.as_str())
            .callback(move |sample| {
                let payload = sample.payload().to_bytes().to_vec();
                let _ = tx_status.send(String::from_utf8_lossy(&payload).to_string());
            })
            .await.map_err(|e| anyhow::anyhow!("{e}"))
    }).unwrap();

    eprintln!("[zenoh] session ready");
    ZenohHandle { session, _rt: rt }
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

// ── Keyboard ──────────────────────────────────

fn kb_spawn(tx_cmd: mpsc::Sender<ControlCommand>) {
    std::thread::spawn(move || loop {
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() { break; }
        if let Some(cmd) = parse_key(&line) {
            let _ = tx_cmd.send(cmd);
        }
    });
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

// ── 入口 ─────────────────────────────────────

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

    // ── 创建管道 ─────────────────────────────
    let (arm_tx, arm_rx)      = mpsc::channel::<Vec<f32>>();
    let (arm_cmd_tx, arm_cmd_rx) = mpsc::channel::<ArmCmd>();
    let (status_tx, status_rx) = mpsc::channel::<String>();
    let (ctrl_tx, ctrl_rx)     = mpsc::channel::<ControlCommand>();

    let (web_state_tx, _) = tokio::sync::broadcast::channel::<String>(8);

    // ── 启动线程 ─────────────────────────────
    let zh = zenoh_spawn(id.clone(), status_tx, ctrl_tx.clone(), web_state_tx.clone());
    arm_spawn(port.clone(), config.arm.so101.baud, arm_tx, arm_cmd_rx);
    kb_spawn(ctrl_tx.clone());

    let codec = PostcardCodec;
    let k_ctrl = format!("tr/{id}/control");
    let k_cmd  = format!("tr/{id}/command");
    let mut seq: u64 = 0;
    let mut fsm = tr_daemon::state::Fsm::new();

    println!("── leader-daemon ──");
    println!("  arm:   {port}");
    println!("  web:   http://localhost:8080");
    println!("  keys:  o(使能) x(失能) s(采集) f(保存) r(重录) q(停止)");
    println!("────────────────────");

    // ── 主协调循环 ──────────────────────────
    loop {
        let t0 = std::time::Instant::now();

        // ① 控制指令（键盘/web/从臂状态触发）
        while let Ok(cmd) = ctrl_rx.try_recv() {
            let (state, action) = fsm.apply(&cmd);
            eprintln!("[main] ctrl={:?} → {:?}", cmd, state);

            // 力矩控制 → arm 线程
            match action {
                tr_daemon::state::DataflowAction::Launch => {
                    let _ = arm_cmd_tx.send(ArmCmd::EnableTorque);
                }
                tr_daemon::state::DataflowAction::Stop => {
                    let _ = arm_cmd_tx.send(ArmCmd::DisableTorque);
                }
                _ => {}
            }

            // 控制指令 → zenoh
            if let Ok(bytes) = codec.encode_control_command(&cmd) {
                let s = zh.session.clone();
                let k = k_cmd.clone();
                tokio::spawn(async move {
                    s.put(k, bytes).congestion_control(zenoh::qos::CongestionControl::Drop).await.ok();
                });
            }
        }

        // ② arm 位置 → zenoh 发布
        while let Ok(positions) = arm_rx.try_recv() {
            if fsm.current() == tr_daemon::state::ArmState::Idle {
                continue;
            }
            let positions_f64: Vec<f64> = positions.iter().map(|&p| p as f64).collect();
            let mut h = MessageHeader::new(1, "leader", ControlMode::JointTargets);
            h.seq = seq;
            let cmd = TeleopCommand {
                header: h,
                body: CommandBody::Joint(JointTargets {
                    positions: positions_f64, velocities: None, efforts: None,
                }),
            };
            seq += 1;
            if let Ok(bytes) = codec.encode_command(&cmd) {
                let s = zh.session.clone();
                let k = k_ctrl.clone();
                tokio::spawn(async move {
                    s.put(k, bytes).congestion_control(zenoh::qos::CongestionControl::Drop).await.ok();
                });
            }
        }

        // ③ 从臂状态 → SSE
        while let Ok(json) = status_rx.try_recv() {
            let _ = web_state_tx.send(json);
        }

        std::thread::sleep(Duration::from_millis(25));

        if t0.elapsed() > Duration::from_secs(10) {
            eprintln!("[main] tick > 10s, slow?");
        }
    }
}
