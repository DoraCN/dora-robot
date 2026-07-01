//! Follower-daemon state machine.
//!
//! Three stable states: Idle → Ready → Recording.
//! TorqueOff returns to Idle from any state.
//! Transitions may trigger DORA dataflow lifecycle actions.

use tr_messages::control::ControlCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmState {
    Idle,
    Ready,
    Recording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataflowAction {
    Launch,
    Stop,
    None,
}

#[derive(Debug)]
pub struct Fsm {
    state: ArmState,
}

impl Fsm {
    pub fn new() -> Self {
        Self { state: ArmState::Idle }
    }

    pub fn current(&self) -> ArmState {
        self.state
    }

    /// Apply a control command. Returns the new state and (if applicable)
    /// the dataflow action the caller must take *after* the transition.
    pub fn apply(&mut self, cmd: &ControlCommand) -> (ArmState, DataflowAction) {
        let action = match (self.state, cmd) {
            // ── TorqueOn ────────────────────────────────────────────
            (ArmState::Idle, ControlCommand::TorqueOn) => {
                self.state = ArmState::Ready;
                DataflowAction::Launch
            }

            // ── TorqueOff (valid from any state) ────────────────────
            (_, ControlCommand::TorqueOff) => {
                self.state = ArmState::Idle;
                DataflowAction::Stop
            }

            // ── StartRecord ─────────────────────────────────────────
            (ArmState::Ready, ControlCommand::StartRecord { .. }) => {
                self.state = ArmState::Recording;
                DataflowAction::None
            }

            // ── EndRecord ───────────────────────────────────────────
            (ArmState::Recording, ControlCommand::EndRecord { .. }) => {
                self.state = ArmState::Ready;
                DataflowAction::None
            }

            // ── ReRecord ────────────────────────────────────────────
            (ArmState::Recording, ControlCommand::ReRecord) => {
                // stays in Recording; recorder internally resets
                DataflowAction::None
            }

            // ── Stop ────────────────────────────────────────────────
            (ArmState::Recording, ControlCommand::Stop) => {
                self.state = ArmState::Ready;
                DataflowAction::None
            }

            // ── Invalid transitions ─────────────────────────────────
            _ => DataflowAction::None,
        };

        (self.state, action)
    }
}

impl Default for Fsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_messages::episode::EpisodeOutcome;

    #[test]
    fn initial_state_is_idle() {
        let fsm = Fsm::new();
        assert_eq!(fsm.current(), ArmState::Idle);
    }

    // ── TorqueOn ──────────────────────────────────────────────

    #[test]
    fn torque_on_from_idle() {
        let mut fsm = Fsm::new();
        let (state, action) = fsm.apply(&ControlCommand::TorqueOn);
        assert_eq!(state, ArmState::Ready);
        assert_eq!(action, DataflowAction::Launch);
    }

    #[test]
    fn torque_on_from_ready_is_noop() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn); // Idle → Ready
        let (state, action) = fsm.apply(&ControlCommand::TorqueOn);
        assert_eq!(state, ArmState::Ready);
        assert_eq!(action, DataflowAction::None);
    }

    #[test]
    fn torque_on_from_recording_is_noop() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn); // Idle → Ready
        fsm.apply(&ControlCommand::StartRecord {
            task: "test".into(),
        }); // Ready → Recording
        let (state, action) = fsm.apply(&ControlCommand::TorqueOn);
        assert_eq!(state, ArmState::Recording);
        assert_eq!(action, DataflowAction::None);
    }

    // ── TorqueOff ─────────────────────────────────────────────

    #[test]
    fn torque_off_from_idle() {
        let mut fsm = Fsm::new();
        let (state, action) = fsm.apply(&ControlCommand::TorqueOff);
        assert_eq!(state, ArmState::Idle);
        assert_eq!(action, DataflowAction::Stop);
    }

    #[test]
    fn torque_off_from_ready() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn);
        let (state, action) = fsm.apply(&ControlCommand::TorqueOff);
        assert_eq!(state, ArmState::Idle);
        assert_eq!(action, DataflowAction::Stop);
    }

    #[test]
    fn torque_off_from_recording() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn);
        fsm.apply(&ControlCommand::StartRecord {
            task: "test".into(),
        });
        let (state, action) = fsm.apply(&ControlCommand::TorqueOff);
        assert_eq!(state, ArmState::Idle);
        assert_eq!(action, DataflowAction::Stop);
    }

    // ── StartRecord ──────────────────────────────────────────

    #[test]
    fn start_record_from_ready() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn);
        let (state, action) = fsm.apply(&ControlCommand::StartRecord {
            task: "pick cube".into(),
        });
        assert_eq!(state, ArmState::Recording);
        assert_eq!(action, DataflowAction::None);
    }

    #[test]
    fn start_record_from_idle_is_noop() {
        let mut fsm = Fsm::new();
        let (state, action) = fsm.apply(&ControlCommand::StartRecord {
            task: "pick cube".into(),
        });
        assert_eq!(state, ArmState::Idle);
        assert_eq!(action, DataflowAction::None);
    }

    // ── EndRecord ────────────────────────────────────────────

    #[test]
    fn end_record_from_recording() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn);
        fsm.apply(&ControlCommand::StartRecord {
            task: "test".into(),
        });
        let (state, action) = fsm.apply(&ControlCommand::EndRecord {
            outcome: EpisodeOutcome::Success,
        });
        assert_eq!(state, ArmState::Ready);
        assert_eq!(action, DataflowAction::None);
    }

    #[test]
    fn end_record_from_ready_is_noop() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn);
        let (state, action) = fsm.apply(&ControlCommand::EndRecord {
            outcome: EpisodeOutcome::Success,
        });
        assert_eq!(state, ArmState::Ready);
        assert_eq!(action, DataflowAction::None);
    }

    // ── ReRecord ─────────────────────────────────────────────

    #[test]
    fn rerecord_from_recording() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn);
        fsm.apply(&ControlCommand::StartRecord {
            task: "test".into(),
        });
        let (state, action) = fsm.apply(&ControlCommand::ReRecord);
        assert_eq!(state, ArmState::Recording);
        assert_eq!(action, DataflowAction::None);
    }

    #[test]
    fn rerecord_from_idle_is_noop() {
        let mut fsm = Fsm::new();
        let (state, action) = fsm.apply(&ControlCommand::ReRecord);
        assert_eq!(state, ArmState::Idle);
        assert_eq!(action, DataflowAction::None);
    }

    // ── Stop ─────────────────────────────────────────────────

    #[test]
    fn stop_from_recording() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn);
        fsm.apply(&ControlCommand::StartRecord {
            task: "test".into(),
        });
        let (state, action) = fsm.apply(&ControlCommand::Stop);
        assert_eq!(state, ArmState::Ready);
        assert_eq!(action, DataflowAction::None);
    }

    #[test]
    fn stop_from_idle_is_noop() {
        let mut fsm = Fsm::new();
        let (state, action) = fsm.apply(&ControlCommand::Stop);
        assert_eq!(state, ArmState::Idle);
        assert_eq!(action, DataflowAction::None);
    }

    // ── Full cycle ───────────────────────────────────────────

    #[test]
    fn full_cycle_idle_to_recording_and_back() {
        let mut fsm = Fsm::new();
        assert_eq!(fsm.current(), ArmState::Idle);

        let (s, a) = fsm.apply(&ControlCommand::TorqueOn);
        assert_eq!(s, ArmState::Ready);
        assert_eq!(a, DataflowAction::Launch);

        let (s, a) = fsm.apply(&ControlCommand::StartRecord {
            task: "task".into(),
        });
        assert_eq!(s, ArmState::Recording);
        assert_eq!(a, DataflowAction::None);

        let (s, a) = fsm.apply(&ControlCommand::EndRecord {
            outcome: EpisodeOutcome::Success,
        });
        assert_eq!(s, ArmState::Ready);
        assert_eq!(a, DataflowAction::None);

        let (s, a) = fsm.apply(&ControlCommand::TorqueOff);
        assert_eq!(s, ArmState::Idle);
        assert_eq!(a, DataflowAction::Stop);
    }

    #[test]
    fn cycle_with_rerecord() {
        let mut fsm = Fsm::new();
        fsm.apply(&ControlCommand::TorqueOn);
        fsm.apply(&ControlCommand::StartRecord {
            task: "t".into(),
        });
        assert_eq!(fsm.current(), ArmState::Recording);

        fsm.apply(&ControlCommand::ReRecord);
        assert_eq!(fsm.current(), ArmState::Recording); // stays

        fsm.apply(&ControlCommand::Stop);
        assert_eq!(fsm.current(), ArmState::Ready);
    }
}
