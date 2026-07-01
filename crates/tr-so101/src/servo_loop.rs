//! [`ServoLoop`] — non-blocking async↔sync bridge (C4).
//!
//! Wraps a [`So101Arm`] inside a background Tokio task that continuously reads
//! positions at a fixed rate and publishes the latest via a `watch` channel.
//! Goal commands are sent through an MPSC channel and consumed by the task.
//!
//! Callers (leader `poll`, follower `command`) never block on the bus — they
//! only read the latest watch value or push a goal onto the channel.

use crate::arm::So101Arm;
use crate::DOF;
use feetech_servo_sdk::MotorBus;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Duration;

/// Non-blocking owner of the servo IO loop.  Keep it alive as long as you need
/// the bus; `stop()` recovers the wrapped `So101Arm`.
pub struct ServoLoop<B: MotorBus> {
    pos_rx: watch::Receiver<[f32; DOF]>,
    goal_tx: mpsc::UnboundedSender<[f32; DOF]>,
    task: JoinHandle<So101Arm<B>>,
}

impl<B: MotorBus + Send + 'static> ServoLoop<B> {
    /// Spawn the background loop.  The arm is moved into a Tokio task that
    /// reads positions at `rate_hz` (e.g. 100 Hz) and publishes them.
    pub fn spawn(mut arm: So101Arm<B>, rate_hz: f64) -> Self {
        let (pos_tx, pos_rx) = watch::channel([0.0_f32; DOF]);
        let (goal_tx, mut goal_rx) = mpsc::unbounded_channel();

        let period = Duration::from_secs_f64(1.0 / rate_hz);
        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            let mut pending: Option<[f32; DOF]> = None;
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        if let Ok(j) = arm.read_joints().await {
                            let _ = pos_tx.send_if_modified(|prev| { *prev = j; true });
                        }
                        if let Some(g) = pending.take() {
                            let _ = arm.write_joints(&g).await;
                        }
                    }
                    goal = goal_rx.recv() => {
                        match goal {
                            Some(g) => pending = Some(g),
                            None => break, // goal_tx dropped → shut down
                        }
                    }
                }
            }
            arm
        });

        Self {
            pos_rx,
            goal_tx,
            task,
        }
    }

    /// Return the latest position snapshot (non-blocking).
    pub fn latest(&self) -> [f32; DOF] {
        *self.pos_rx.borrow()
    }

    /// Post a new goal target (non-blocking).  The loop picks the latest goal
    /// and writes it on the next tick.
    pub fn set_goal(&self, goal: [f32; DOF]) {
        // Unbounded — ignore a full channel (not possible for unbounded).
        let _ = self.goal_tx.send(goal);
    }

    /// Shut down the background task and recover the underlying `So101Arm`.
    pub async fn stop(self) -> So101Arm<B> {
        drop(self.goal_tx); // signals the task to exit
        self.task.await.expect("servo loop panicked")
    }
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use super::*;
    use crate::config::So101Config;
    use feetech_servo_sdk::MockBus;

    /// Minimal current-thread runtime — same helper as in `arm.rs`.
    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn servo_loop_spawn_latest_set_and_stop() {
        block_on(async {
            let bus = MockBus::new(&[1, 2, 3, 4, 5, 6]);
            let arm = So101Arm::new(bus, So101Config::default());
            let sl = ServoLoop::spawn(arm, 100.0);
            // latest should be available immediately (initial 0.0 from watch init)
            let pos = sl.latest();
            assert!((pos[0] - 0.0_f32).abs() < 0.01, "pos[0]={}", pos[0]);
            // posting a goal must not panic
            sl.set_goal([0.5, 0.0, 0.0, 0.0, 0.0, 0.0]);
            // stop and recover the arm
            let _arm = sl.stop().await;
        });
    }
}
