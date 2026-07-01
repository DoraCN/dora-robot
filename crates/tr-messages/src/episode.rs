//! Episode lifecycle events — the operator marks an episode at its end.
//!
//! Carried on a reliable channel (operator → robot). The robot side forwards the
//! decoded outcome to the (Python) recorder, which turns it into a save/discard
//! call on lerobot's writer. **Persistence itself is lerobot's domain, not this
//! project** (see `docs/specs/001-so101-teleop-record/spec.md` §5/§6).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// What to do with the just-ended episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EpisodeOutcome {
    /// Keep it — the recorder calls the writer's *save*.
    Success,
    /// Drop it — the recorder calls the writer's *discard*.
    Fail,
    /// Drop and re-record — treated as *discard* for persistence.
    Rerecord,
}

impl EpisodeOutcome {
    /// `true` when the episode should be kept (saved), `false` when discarded.
    pub fn keep(self) -> bool {
        matches!(self, EpisodeOutcome::Success)
    }
}

/// Episode boundary signal from the operator-control node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EpisodeEvent {
    /// Begin a new episode.
    Start,
    /// End the current episode with an outcome.
    End { outcome: EpisodeOutcome },
    /// End the entire recording session (follower disables torque + exits).
    Stop,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_semantics() {
        assert!(EpisodeOutcome::Success.keep());
        assert!(!EpisodeOutcome::Fail.keep());
        assert!(!EpisodeOutcome::Rerecord.keep());
    }

    #[test]
    fn event_match() {
        let e = EpisodeEvent::End {
            outcome: EpisodeOutcome::Fail,
        };
        match e {
            EpisodeEvent::End { outcome } => assert_eq!(outcome, EpisodeOutcome::Fail),
            EpisodeEvent::Start => unreachable!(),
        }
    }
}
