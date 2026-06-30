//! Transport backends.
//!
//! [`TcpTransport`] and [`UdpTransport`] are real (`std::net`). [`SerialTransport`],
//! [`BleTransport`] and [`NearLinkTransport`] are stubs that implement the trait
//! but return [`TransportError::Unsupported`]; fill them in with the dependencies
//! noted in `Cargo.toml`.

use crate::error::TransportError;
use crate::framing::{FrameDecoder, FrameEncoder};
use crate::qos::Channel;
use crate::transport::{Inbound, LinkState, Transport};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

fn clamp_timeout(t: Duration) -> Duration {
    if t.is_zero() {
        Duration::from_millis(1)
    } else {
        t
    }
}

// ---------------------------------------------------------------- TCP --------

/// Reliable stream transport. Good for handshake + control over WiFi/Ethernet/4G/5G.
pub struct TcpTransport {
    stream: TcpStream,
    decoder: FrameDecoder,
}

impl TcpTransport {
    pub fn connect(addr: impl ToSocketAddrs) -> Result<Self, TransportError> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            decoder: FrameDecoder::new(),
        })
    }

    /// Bind, accept exactly one peer (server side of the bridge).
    pub fn bind_accept(addr: impl ToSocketAddrs) -> Result<Self, TransportError> {
        let listener = TcpListener::bind(addr)?;
        let (stream, _peer) = listener.accept()?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            decoder: FrameDecoder::new(),
        })
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, channel: Channel, payload: &[u8]) -> Result<(), TransportError> {
        let frame = FrameEncoder::encode(channel, payload);
        self.stream.write_all(&frame)?;
        Ok(())
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, TransportError> {
        self.stream.set_read_timeout(Some(clamp_timeout(timeout)))?;
        loop {
            if let Some((channel, frame)) = self.decoder.next_frame() {
                return Ok(Some(Inbound { channel, frame }));
            }
            let mut tmp = [0u8; 8192];
            match self.stream.read(&mut tmp) {
                Ok(0) => return Err(TransportError::Closed),
                Ok(n) => self.decoder.feed(&tmp[..n]),
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return Ok(None)
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn link_state(&self) -> LinkState {
        LinkState::Up
    }
}

// ---------------------------------------------------------------- UDP --------

/// Datagram transport. Good for low-latency control/feedback; one datagram = one frame.
pub struct UdpTransport {
    socket: UdpSocket,
}

impl UdpTransport {
    /// Bind `local` and pin the default peer to `remote`.
    pub fn connect(
        local: impl ToSocketAddrs,
        remote: impl ToSocketAddrs,
    ) -> Result<Self, TransportError> {
        let socket = UdpSocket::bind(local)?;
        socket.connect(remote)?;
        Ok(Self { socket })
    }
}

impl Transport for UdpTransport {
    fn send(&mut self, channel: Channel, payload: &[u8]) -> Result<(), TransportError> {
        let frame = FrameEncoder::encode(channel, payload);
        self.socket.send(&frame)?;
        Ok(())
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, TransportError> {
        self.socket.set_read_timeout(Some(clamp_timeout(timeout)))?;
        let mut buf = [0u8; 65536];
        match self.socket.recv(&mut buf) {
            Ok(n) => {
                let mut dec = FrameDecoder::new();
                dec.feed(&buf[..n]);
                match dec.next_frame() {
                    Some((channel, frame)) => Ok(Some(Inbound { channel, frame })),
                    None => Err(TransportError::BadFrame("truncated datagram".into())),
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn link_state(&self) -> LinkState {
        LinkState::Up
    }
}

// -------------------------------------------------------------- stubs --------

macro_rules! stub_transport {
    ($name:ident, $what:literal) => {
        #[doc = concat!("Stub transport for ", $what, ". Implement with the dep noted in Cargo.toml.")]
        #[derive(Debug, Default)]
        pub struct $name;
        impl $name {
            pub fn new() -> Self {
                Self
            }
        }
        impl Transport for $name {
            fn send(&mut self, _channel: Channel, _payload: &[u8]) -> Result<(), TransportError> {
                Err(TransportError::Unsupported($what))
            }
            fn recv(&mut self, _timeout: Duration) -> Result<Option<Inbound>, TransportError> {
                Err(TransportError::Unsupported($what))
            }
            fn link_state(&self) -> LinkState {
                LinkState::Down
            }
        }
    };
}

stub_transport!(SerialTransport, "USB/serial (use `serialport`/`nusb`)");
stub_transport!(BleTransport, "Bluetooth LE (use `btleplug`)");
stub_transport!(NearLinkTransport, "NearLink/星闪 (vendor SDK)");
