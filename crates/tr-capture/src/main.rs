//! DORA capture node — zenoh → DORA Arrow bridge.
//!
//! Subscribes to zenoh control/observation/command and forwards
//! as DORA Arrow outputs: action (Float32Array), observation_state (Float32Array),
//! episode_cmd (JSON bytes).

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
    let rt = tokio::runtime::Runtime::new()?;

    let (tx, rx) = mpsc::channel::<Captured>();
    let handle = rt.handle().clone();
    let codec = PostcardCodec;

    let k_ctrl = format!("tr/{arm_id}/control");
    let k_obs = format!("tr/{arm_id}/observation");
    let k_cmd = format!("tr/{arm_id}/command");

    handle.spawn(run_control_subscriber(
        k_ctrl,
        tx.clone(),
        codec,
        handle.clone(),
    ));
    handle.spawn(run_observation_subscriber(
        k_obs,
        tx.clone(),
        codec,
        handle.clone(),
    ));
    handle.spawn(run_command_subscriber(k_cmd, tx, codec, handle.clone()));

    let (mut node, mut events) = DoraNode::init_from_env()?;

    loop {
        let stopped = match events.recv_timeout(Duration::from_millis(5)) {
            Some(_) => true,
            None => false,
        };
        if stopped {
            break;
        }

        while let Ok(msg) = rx.try_recv() {
            match msg {
                Captured::Action(a) => {
                    let arr = Float32Array::from(a);
                    node.send_output("action".into(), Default::default(), arr)?;
                }
                Captured::Observation(o) => {
                    let arr = Float32Array::from(o);
                    node.send_output("observation_state".into(), Default::default(), arr)?;
                }
                Captured::EpisodeStart { task } => {
                    let json = format!(r#"{{"cmd":"StartRecord","task":"{}"}}"#, task);
                    let b = json.into_bytes();
                    node.send_output_bytes("episode_end".into(), Default::default(), b.len(), &b)?;
                }
                Captured::EpisodeEnd(outcome) => {
                    let json = format!(r#"{{"cmd":"EndRecord","outcome":"{}"}}"#, outcome);
                    let b = json.into_bytes();
                    node.send_output_bytes("episode_end".into(), Default::default(), b.len(), &b)?;
                }
                Captured::EpisodeReRecord => {
                    let b = br#"{"cmd":"ReRecord"}"#;
                    node.send_output_bytes("episode_end".into(), Default::default(), b.len(), b)?;
                }
                Captured::EpisodeStop => {
                    let b = br#"{"cmd":"Stop"}"#;
                    node.send_output_bytes("episode_end".into(), Default::default(), b.len(), b)?;
                }
            }
        }
    }
    Ok(())
}

async fn run_control_subscriber(
    key: String,
    tx: mpsc::Sender<Captured>,
    codec: PostcardCodec,
    handle: tokio::runtime::Handle,
) {
    let mut sub = match ZenohTransport::subscriber(&handle, &key) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("capture ctrl: {e}");
            return;
        }
    };
    loop {
        match Transport::recv(&mut sub, Duration::from_millis(5)) {
            Ok(Some(inbound)) => {
                if let Ok(cmd) = codec.decode_command(&inbound.frame) {
                    if let CommandBody::Joint(jt) = cmd.body {
                        let action: Vec<f32> = jt.positions.iter().map(|p| *p as f32).collect();
                        let _ = tx.send(Captured::Action(action));
                    }
                }
            }
            Ok(None) => {}
            Err(_) => break,
        }
    }
}

async fn run_observation_subscriber(
    key: String,
    tx: mpsc::Sender<Captured>,
    codec: PostcardCodec,
    handle: tokio::runtime::Handle,
) {
    let mut sub = match ZenohTransport::subscriber(&handle, &key) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("capture obs: {e}");
            return;
        }
    };
    loop {
        match Transport::recv(&mut sub, Duration::from_millis(5)) {
            Ok(Some(inbound)) => {
                if let Ok(obs) = codec.decode_observation(&inbound.frame) {
                    let _ = tx.send(Captured::Observation(obs));
                }
            }
            Ok(None) => {}
            Err(_) => break,
        }
    }
}

async fn run_command_subscriber(
    key: String,
    tx: mpsc::Sender<Captured>,
    codec: PostcardCodec,
    handle: tokio::runtime::Handle,
) {
    let mut sub = match ZenohTransport::subscriber(&handle, &key) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("capture cmd: {e}");
            return;
        }
    };
    loop {
        match Transport::recv(&mut sub, Duration::from_millis(5)) {
            Ok(Some(inbound)) => {
                if let Ok(cmd) = codec.decode_control_command(&inbound.frame) {
                    let msg = match cmd {
                        ControlCommand::StartRecord { task } => Captured::EpisodeStart { task },
                        ControlCommand::EndRecord { outcome } => {
                            Captured::EpisodeEnd(format!("{:?}", outcome))
                        }
                        ControlCommand::ReRecord => Captured::EpisodeReRecord,
                        ControlCommand::Stop => Captured::EpisodeStop,
                        _ => continue,
                    };
                    let _ = tx.send(msg);
                }
            }
            Ok(None) => {}
            Err(_) => break,
        }
    }
}
