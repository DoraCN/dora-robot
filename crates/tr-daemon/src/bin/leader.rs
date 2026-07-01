use std::io::{self, Write};
use std::time::Duration;
use tr_codec::PostcardCodec;
use tr_daemon::config::DaemonConfig;
use tr_messages::control::ControlCommand;
use tr_messages::{Codec, EpisodeOutcome};
use tr_so101::config::So101Config;
use tr_so101::resolver::{parse_hex_u16, resolve_arm_port, UsbDeviceConfig};
use tr_so101::{FeetechBus, So101Arm, So101Leader};
use tr_teleop::TeleopDevice;
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .iter().position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("config/leader.toml");

    let toml_str = std::fs::read_to_string(config_path)?;
    let config = DaemonConfig::from_str(&toml_str)?;
    let id = &config.arm.id;

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

    let rt_arm = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;
    let rt_zenoh = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1).enable_io().enable_time().build()?;

    let _guard = rt_arm.enter();
    let bus = FeetechBus::new(&port, config.arm.so101.baud)?;
    let arm = So101Arm::new(bus, So101Config::default());
    let mut leader = So101Leader::new(arm, 1, "leader");
    drop(_guard);

    let k_ctrl   = format!("tr/{id}/control");
    let k_cmd    = format!("tr/{id}/command");
    let k_status = format!("tr/{id}/status");

    let mut t_ctrl  = ZenohTransport::publisher(rt_zenoh.handle(), &k_ctrl)?;
    let mut t_cmd   = ZenohTransport::publisher(rt_zenoh.handle(), &k_cmd)?;
    let _t_status   = ZenohTransport::subscriber(rt_zenoh.handle(), &k_status)?;

    let codec = PostcardCodec;

    println!("── leader-daemon ──");
    println!("  o       TorqueOn");
    println!("  x       TorqueOff");
    println!("  s/Enter StartRecord");
    println!("  f       EndRecord(Success)");
    println!("  r       ReRecord");
    println!("  q       Stop");
    println!("────────────────────");

    // Spawn keyboard reader on a separate thread so it doesn't block the poll loop.
    let (kb_tx, kb_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        loop {
            let mut line = String::new();
            if io::stdin().read_line(&mut line).is_err() { break; }
            let _ = kb_tx.send(line);
        }
    });

    loop {
        if let Some(cmd) = leader.poll() {
            if let Ok(bytes) = codec.encode_command(&cmd) {
                let _ = t_ctrl.send(tr_transport::qos::Channel::Control, &bytes);
            }
        }

        if let Ok(line) = kb_rx.try_recv() {
            let cmd = match line.trim() {
                "o" => Some(ControlCommand::TorqueOn),
                "x" => Some(ControlCommand::TorqueOff),
                "s" | "" => Some(ControlCommand::StartRecord { task: "teleop".into() }),
                "f" => Some(ControlCommand::EndRecord { outcome: EpisodeOutcome::Success }),
                "r" => Some(ControlCommand::ReRecord),
                "q" => Some(ControlCommand::Stop),
                _ => None,
            };

            if let Some(cmd) = cmd {
                if let Ok(bytes) = codec.encode_control_command(&cmd) {
                    let _ = t_cmd.send(tr_transport::qos::Channel::Control, &bytes);
                    println!("  -> {:?}", cmd);
                    let _ = io::stdout().flush();
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
