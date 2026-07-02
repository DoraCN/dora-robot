//! DORA 1.0 capture node — zenoh → DORA Arrow bridge.
//!
//! Subscribes to zenoh control/observation/command and forwards
//! as DORA outputs: action, observation_state, episode_end.

use arrow::array::Float32Array;
use dora_node_api::DoraNode;
use std::collections::BTreeMap;
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

    let (mut node, _events) = DoraNode::init_from_env()?;

    let mut last_send = std::time::Instant::now();
    let send_interval = std::time::Duration::from_millis(33); // ~30 Hz

    // Main loop: drain MPSC → send DORA outputs.
    // recv_timeout with short timeout keeps the CPU from spinning.
    // When the event stream is done, recv_timeout returns None and we exit.
    loop {
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
                    let mut meta = BTreeMap::new();
                    meta.insert("cmd".into(), dora_node_api::Parameter::String("StartRecord".into()));
                    meta.insert("task".into(), dora_node_api::Parameter::String(task));
                    node.send_output("episode_end".into(), meta, Float32Array::from(Vec::<f32>::new()))?;
                }
                Captured::EpisodeEnd(outcome) => {
                    let mut meta = BTreeMap::new();
                    meta.insert("cmd".into(), dora_node_api::Parameter::String("EndRecord".into()));
                    meta.insert("outcome".into(), dora_node_api::Parameter::String(outcome));
                    node.send_output("episode_end".into(), meta, Float32Array::from(Vec::<f32>::new()))?;
                }
                Captured::EpisodeReRecord => {
                    let mut meta = BTreeMap::new();
                    meta.insert("cmd".into(), dora_node_api::Parameter::String("ReRecord".into()));
                    node.send_output("episode_end".into(), meta, Float32Array::from(Vec::<f32>::new()))?;
                }
                Captured::EpisodeStop => {
                    let mut meta = BTreeMap::new();
                    meta.insert("cmd".into(), dora_node_api::Parameter::String("Stop".into()));
                    node.send_output("episode_end".into(), meta, Float32Array::from(Vec::<f32>::new()))?;
                }
            }
        }
        // No DORA inputs to handle — keep forwarding zenoh data indefinitely.
        // The DORA daemon will kill this process when the dataflow stops.
        std::thread::sleep(Duration::from_millis(1));
    }

    Ok(())
}
