//! Exponential backoff retry utility.
//!
//! Used by the follower daemon to recover from bus/zenoh/DORA errors.

use std::time::{Duration, Instant};

pub struct Backoff {
    base: Duration,
    max: Duration,
    current: u32, // number of consecutive failures
}

impl Backoff {
    pub fn new(base_secs: u64, max_secs: u64) -> Self {
        Self {
            base: Duration::from_secs(base_secs),
            max: Duration::from_secs(max_secs),
            current: 0,
        }
    }

    /// Current backoff delay.
    pub fn delay(&self) -> Duration {
        if self.current == 0 {
            return Duration::from_secs(0);
        }
        let d = self.base.as_secs() * (1u64 << (self.current - 1));
        let d = if d == 0 { 1 } else { d };
        Duration::from_secs(d).min(self.max)
    }

    /// Sleep for the current backoff duration, then increment failure count.
    pub fn wait_and_advance(&mut self) {
        self.current += 1;
        let d = self.delay();
        if d > Duration::from_secs(0) {
            std::thread::sleep(d);
        }
    }

    /// Reset after successful recovery.
    pub fn reset(&mut self) {
        self.current = 0;
    }

    /// Whether a retry should be attempted now based on `last_attempt` time.
    pub fn should_retry(&self, last_attempt: Instant) -> bool {
        let d = self.delay();
        if d == Duration::from_secs(0) {
            return true; // first failure, retry immediately
        }
        last_attempt.elapsed() >= d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_growth() {
        let b = Backoff { base: Duration::from_secs(1), max: Duration::from_secs(30), current: 1 };
        assert_eq!(b.delay(), Duration::from_secs(1));
        let b = Backoff { base: Duration::from_secs(1), max: Duration::from_secs(30), current: 2 };
        assert_eq!(b.delay(), Duration::from_secs(2));
        let b = Backoff { base: Duration::from_secs(1), max: Duration::from_secs(30), current: 3 };
        assert_eq!(b.delay(), Duration::from_secs(4));
        let b = Backoff { base: Duration::from_secs(1), max: Duration::from_secs(30), current: 4 };
        assert_eq!(b.delay(), Duration::from_secs(8));
    }

    #[test]
    fn capped_at_max() {
        let b = Backoff { base: Duration::from_secs(1), max: Duration::from_secs(4), current: 4 };
        assert_eq!(b.delay(), Duration::from_secs(4));
        let b = Backoff { base: Duration::from_secs(1), max: Duration::from_secs(4), current: 10 };
        assert_eq!(b.delay(), Duration::from_secs(4));
    }

    #[test]
    fn reset_works() {
        let mut b = Backoff::new(1, 30);
        b.current = 5;
        b.reset();
        assert_eq!(b.current, 0);
        assert_eq!(b.delay(), Duration::from_secs(0));
    }

    #[test]
    fn should_retry_first_time() {
        let b = Backoff::new(1, 30);
        assert!(b.should_retry(Instant::now() - Duration::from_secs(10))); // any past time
    }

    #[test]
    fn should_retry_after_delay() {
        let b = Backoff { base: Duration::from_secs(1), max: Duration::from_secs(30), current: 3 };
        assert!(!b.should_retry(Instant::now())); // just now, should wait 4s
        assert!(b.should_retry(Instant::now() - Duration::from_secs(5))); // 5s ago, should retry
    }
}
