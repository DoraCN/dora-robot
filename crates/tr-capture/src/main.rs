//! DORA 1.0 capture node — zenoh → DORA Arrow bridge.
//!
//! Subscribes to zenoh control/observation/command and forwards
//! as DORA outputs: action, observation_state, episode_end.

use arrow::array::Float32Array;
use dora_node_api::DoraNode;
use std::sync::mpsc;
use std::time::Duration;
use tr_codec::PostcardCodec;
use tr_messages::control::ControlCommand;
use tr_messages::Codec;
use tr_messages::CommandBody;
use tr_transport::Transport;
use tr_transport_zenoh::ZenohTransport;

enum Captured {
    Action(Vec<f32>),
    Observation(Vec<f32>),
    EpisodeStart { task: String },
    EpisodeEnd(String),
    EpisodeReRecord,
    EpisodeStop,
}

fn main() -> eyre::Result<()> {
    let arm_id = std::env::var("TR_ARM_ID").unwrap_or_else(|_| "arm_1".into());
    let (tx, rx) = mpsc::channel::<Captured>();
    let codec = PostcardCodec;

    // DORA provides its own tokio runtime — spawn blocking threads for zenoh I/O.
    let tx_ctrl = tx.clone();
    let tx_obs = tx.clone();
    let tx_cmd = tx;
    let k_ctrl = format!("tr/{arm_id}/control");
    let k_obs = format!("tr/{arm_id}/observation");
    let k_cmd = format!("tr/{arm_id}/command");

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1).enable_io().enable_time().build().unwrap();
        let mut sub = match ZenohTransport::subscriber(rt.handle(), &k_ctrl) {
            Ok(s) => s, Err(e) => { eprintln!("capture ctrl: {e}"); return; }
        };
        loop {
            if let Ok(Some(inbound)) = sub.recv(Duration::from_millis(5)) {
                if let Ok(cmd) = codec.decode_command(&inbound.frame) {
                    if let CommandBody::Joint(jt) = cmd.body {
                        let action: Vec<f32> = jt.positions.iter().map(|p| *p as f32).collect();
                        let _ = tx_ctrl.send(Captured::Action(action));
                    }
                }
            }
        }
    });

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1).enable_io().enable_time().build().unwrap();
        let mut sub = match ZenohTransport::subscriber(rt.handle(), &k_obs) {
            Ok(s) => s, Err(e) => { eprintln!("capture obs: {e}"); return; }
        };
        loop {
            if let Ok(Some(inbound)) = sub.recv(Duration::from_millis(5)) {
                if let Ok(obs) = codec.decode_observation(&inbound.frame) {
                    let _ = tx_obs.send(Captured::Observation(obs));
                }
            }
        }
    });

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1).enable_io().enable_time().build().unwrap();
        let mut sub = match ZenohTransport::subscriber(rt.handle(), &k_cmd) {
            Ok(s) => s, Err(e) => { eprintln!("capture cmd: {e}"); return; }
        };
        loop {
            if let Ok(Some(inbound)) = sub.recv(Duration::from_millis(5)) {
                if let Ok(cmd) = codec.decode_control_command(&inbound.frame) {
                    let msg = match cmd {
                        ControlCommand::StartRecord { task } => Captured::EpisodeStart { task },
                        ControlCommand::EndRecord { outcome } =>
                            Captured::EpisodeEnd(format!("{:?}", outcome)),
                        ControlCommand::ReRecord => Captured::EpisodeReRecord,
                        ControlCommand::Stop => Captured::EpisodeStop,
                        _ => continue,
                    };
                    let _ = tx_cmd.send(msg);
                }
            }
        }
    });

    let (mut node, mut events) = DoraNode::init_from_env()?;

    let mut last_send = std::time::Instant::now();
    let send_interval = std::time::Duration::from_millis(33); // ~30 Hz

    loop {
        // Pump DORA events and drain our mpsc channel
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Captured::Action(a) => {
                    if last_send.elapsed() < send_interval { continue; }
                    node.send_output("action".into(), Default::default(), Float32Array::from(a))?;
                }
                Captured::Observation(o) => {
                    if last_send.elapsed() < send_interval { continue; }
                    node.send_output("observation_state".into(), Default::default(), Float32Array::from(o))?;
                    last_send = std::time::Instant::now();
                }
                Captured::EpisodeStart { task } => {
                    let json = format!(r#"{{"cmd":"StartRecord","task":"{}"}}"#, task);
                    node.send_output_bytes("episode_end".into(), Default::default(), json.len(), json.as_bytes())?;
                }
                Captured::EpisodeEnd(outcome) => {
                    let json = format!(r#"{{"cmd":"EndRecord","outcome":"{}"}}"#, outcome);
                    node.send_output_bytes("episode_end".into(), Default::default(), json.len(), json.as_bytes())?;
                }
                Captured::EpisodeReRecord => {
                    node.send_output_bytes("episode_end".into(), Default::default(), 15, br#"{"cmd":"ReRecord"}"#)?;
                }
                Captured::EpisodeStop => {
                    node.send_output_bytes("episode_end".into(), Default::default(), 13, br#"{"cmd":"Stop"}"#)?;
                }
            }
        }

        match events.recv_timeout(Duration::from_millis(10)) {
            Some(_) => break, // Stop or other termination event
            None => {}         // timeout — continue
        }
    }

    Ok(())
}
