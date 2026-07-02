//! `tr-daemon` — teleoperation daemon library.
//!
//! Provides config loading, state machine, and DORA dataflow management
//! shared between the follower-daemon and leader-daemon binaries.

pub mod config;
pub mod dora;
pub mod retry;
pub mod state;
pub mod web;
