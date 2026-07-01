use std::mem::ManuallyDrop;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use tr_transport::error::TransportError;
use tr_transport::qos::Channel;
use tr_transport::transport::{Inbound, LinkState, Transport};

#[allow(dead_code)]
enum Role { Pub, Sub }

pub struct ZenohTransport {
    sub_rx: Option<Receiver<(Channel, Vec<u8>)>>,
    pub_tx: Option<Sender<Vec<u8>>>,
    _keep: ManuallyDrop<KeepAlive>,
    #[allow(dead_code)]
    role: Role,
}

#[allow(dead_code)]
struct KeepAlive {
    session: zenoh::Session,
    sub: Option<Box<dyn std::any::Any + Send>>,
}

impl ZenohTransport {
    /// Create a PUBLISHING endpoint.
    ///
    /// A background task is spawned on `handle` that drains an internal MPSC
    /// channel and publishes each payload to zenoh.  `send()` pushes to that
    /// internal channel — **no `block_on`** — so `send` can be called from
    /// inside or outside a runtime without nesting.
    pub fn publisher(handle: &tokio::runtime::Handle, key_expr: impl Into<String>) -> anyhow::Result<Self> {
        let key = key_expr.into();
        let session = handle.block_on(async {
            zenoh::open(zenoh::Config::default()).await.map_err(|e| anyhow::anyhow!("{e}"))
        })?;
        let (tx, rx) = mpsc::channel::<Vec<u8>>();

        // Spawn the background publisher drain.
        let session2 = session.clone();
        let key2 = key.clone();
        handle.spawn(async move {
            while let Ok(payload) = rx.recv() {
                let _ = session2
                    .put(key2.as_str(), payload)
                    .congestion_control(zenoh::qos::CongestionControl::Drop)
                    .await;
            }
        });

        Ok(Self {
            sub_rx: None,
            pub_tx: Some(tx),
            _keep: ManuallyDrop::new(KeepAlive { session, sub: None }),
            role: Role::Pub,
        })
    }

    /// Create a SUBSCRIBING endpoint (callback → MPSC → `recv()`).
    pub fn subscriber(handle: &tokio::runtime::Handle, key_expr: impl Into<String>) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel::<(Channel, Vec<u8>)>();
        let session = handle.block_on(async {
            zenoh::open(zenoh::Config::default()).await.map_err(|e| anyhow::anyhow!("{e}"))
        })?;
        let key = key_expr.into();
        let sub = handle.block_on(async {
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
            sub_rx: Some(rx),
            pub_tx: None,
            _keep: ManuallyDrop::new(KeepAlive { session, sub: Some(Box::new(sub)) }),
            role: Role::Sub,
        })
    }
}

impl Transport for ZenohTransport {
    fn send(&mut self, _channel: Channel, payload: &[u8]) -> Result<(), TransportError> {
        let tx = self.pub_tx.as_ref().ok_or(TransportError::Unsupported("not a publisher"))?;
        tx.send(payload.to_vec()).map_err(|_| TransportError::Closed)
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, TransportError> {
        let rx = self.sub_rx.as_ref().ok_or(TransportError::Unsupported("not a subscriber"))?;
        match rx.recv_timeout(timeout) {
            Ok((channel, frame)) => Ok(Some(Inbound { channel, frame })),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(TransportError::Closed),
        }
    }

    fn link_state(&self) -> LinkState { LinkState::Up }
}
