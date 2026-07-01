use std::mem::ManuallyDrop;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;
use tr_transport::error::TransportError;
use tr_transport::qos::Channel;
use tr_transport::transport::{Inbound, LinkState, Transport};

enum Role { Pub, Sub }

pub struct ZenohTransport {
    rt: tokio::runtime::Runtime,
    session: zenoh::Session,
    // Keep the subscriber handle alive so the callback keeps firing.
    // We cannot name the `Subscriber<impl Handler>` type, so we store it
    // erased. `ManuallyDrop` prevents unsubscribe on drop (OK — the
    // subscription lives until process exit).
    _sub: ManuallyDrop<Box<dyn std::any::Any + Send>>,
    sub_rx: Option<Receiver<(Channel, Vec<u8>)>>,
    key_expr: String,
    role: Role,
}

impl ZenohTransport {
    pub fn publisher(key_expr: impl Into<String>) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Runtime::new()?;
        let session = rt.block_on(async {
            zenoh::open(zenoh::Config::default())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        })?;
        Ok(Self {
            rt,
            session,
            _sub: ManuallyDrop::new(Box::new(())),
            sub_rx: None,
            key_expr: key_expr.into(),
            role: Role::Pub,
        })
    }

    pub fn subscriber(key_expr: impl Into<String>) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Runtime::new()?;
        let (tx, rx) = mpsc::channel::<(Channel, Vec<u8>)>();
        let session = rt.block_on(async {
            zenoh::open(zenoh::Config::default())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        })?;
        let key = key_expr.into();
        let sub = rt.block_on(async {
            session
                .declare_subscriber(key.as_str())
                .callback(move |sample| {
                    let payload = sample.payload().to_bytes().to_vec();
                    let _ = tx.send((Channel::Control, payload));
                })
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        })?;
        Ok(Self {
            rt,
            session,
            _sub: ManuallyDrop::new(Box::new(sub)),
            sub_rx: Some(rx),
            key_expr: key,
            role: Role::Sub,
        })
    }
}

impl Transport for ZenohTransport {
    fn send(&mut self, _channel: Channel, payload: &[u8]) -> Result<(), TransportError> {
        if !matches!(self.role, Role::Pub) {
            return Err(TransportError::Unsupported("not a publisher"));
        }
        self.rt
            .block_on(async { self.session.put(self.key_expr.as_str(), payload.to_vec()).await })
            .map_err(|e| {
                TransportError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, TransportError> {
        let rx = self
            .sub_rx
            .as_ref()
            .ok_or(TransportError::Unsupported("not a subscriber"))?;
        match rx.recv_timeout(timeout) {
            Ok((channel, frame)) => Ok(Some(Inbound { channel, frame })),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(TransportError::Closed),
        }
    }

    fn link_state(&self) -> LinkState {
        LinkState::Up
    }
}
