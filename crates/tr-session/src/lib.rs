//! Session lifecycle shared by both ends of the link.
//!
//! Pure logic (no IO): the bridge/nodes drive it with received messages and a
//! clock. Handles capability negotiation, sequence numbering, a heartbeat
//! deadline watchdog, and the safe-state decision on link loss.

use std::time::{Duration, Instant};
use tr_messages::{Capabilities, Negotiated, SessionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Discovering,
    Handshaking,
    Active,
    /// Link impaired (missed deadline) but not yet given up; robot should hold.
    Degraded,
    Closed,
}

pub struct Session {
    id: SessionId,
    state: SessionState,
    local_caps: Capabilities,
    negotiated: Option<Negotiated>,
    next_seq: u64,
    last_rx: Instant,
    /// Max gap between received messages before declaring `Degraded`.
    deadline: Duration,
    /// Time in `Degraded` before declaring the session `Closed`.
    drop_after: Duration,
}

impl Session {
    pub fn new(id: SessionId, local_caps: Capabilities, deadline: Duration) -> Self {
        Self {
            id,
            state: SessionState::Discovering,
            local_caps,
            negotiated: None,
            next_seq: 0,
            last_rx: Instant::now(),
            deadline,
            drop_after: deadline * 10,
        }
    }

    pub fn id(&self) -> SessionId {
        self.id
    }
    pub fn state(&self) -> SessionState {
        self.state
    }
    pub fn negotiated(&self) -> Option<&Negotiated> {
        self.negotiated.as_ref()
    }
    pub fn local_capabilities(&self) -> &Capabilities {
        &self.local_caps
    }

    pub fn begin_handshake(&mut self) {
        self.state = SessionState::Handshaking;
    }

    /// Apply the peer's advertised capabilities; transitions to `Active` on success.
    pub fn on_peer_capabilities(&mut self, peer: &Capabilities) -> Option<&Negotiated> {
        let negotiated = self.local_caps.negotiate(peer)?;
        self.negotiated = Some(negotiated);
        self.state = SessionState::Active;
        self.last_rx = Instant::now();
        self.negotiated.as_ref()
    }

    /// Allocate the next monotonic sequence number for an outbound message.
    pub fn next_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    /// Record that a message was received (heartbeat / any inbound).
    pub fn on_rx(&mut self, now: Instant) {
        self.last_rx = now;
        if self.state == SessionState::Degraded {
            self.state = SessionState::Active;
        }
    }

    /// Advance the watchdog; returns the (possibly changed) state.
    pub fn tick(&mut self, now: Instant) -> SessionState {
        if matches!(self.state, SessionState::Active | SessionState::Degraded) {
            let idle = now.duration_since(self.last_rx);
            if idle > self.drop_after {
                self.state = SessionState::Closed;
            } else if idle > self.deadline {
                self.state = SessionState::Degraded;
            }
        }
        self.state
    }

    /// The robot tier must latch to a safe hold whenever this is true (never
    /// replay stale commands).
    pub fn should_safe_state(&self) -> bool {
        matches!(self.state, SessionState::Degraded | SessionState::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_messages::ControlMode;

    fn caps() -> Capabilities {
        Capabilities {
            dof: 6,
            supported_modes: vec![ControlMode::CartesianPose],
            force_feedback: false,
            max_rate_hz: 200,
            frames: vec!["base".into()],
            end_effectors: vec!["tcp".into()],
            gripper: None,
        }
    }

    #[test]
    fn watchdog_degrades_then_closes_then_safe_state() {
        let mut s = Session::new(1, caps(), Duration::from_millis(100));
        s.begin_handshake();
        assert!(s.on_peer_capabilities(&caps()).is_some());
        assert_eq!(s.state(), SessionState::Active);

        let t0 = Instant::now();
        assert_eq!(s.tick(t0 + Duration::from_millis(50)), SessionState::Active);
        assert_eq!(
            s.tick(t0 + Duration::from_millis(200)),
            SessionState::Degraded
        );
        assert!(s.should_safe_state());
        assert_eq!(
            s.tick(t0 + Duration::from_secs(5)),
            SessionState::Closed
        );
    }
}
