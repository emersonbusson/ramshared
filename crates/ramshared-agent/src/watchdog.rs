//! Session watchdog of the agent: if the broker stops responding (no `Ack`/command for
//! `deadline`), the session is considered dead and the agent performs cleanup + reconnects (DT-18/DT-27).
//!
//! Pure and with injected clock (`Instant`) to be testable deterministically —
//! `main.rs` passes `Instant::now()`. No I/O here.

use std::time::{Duration, Instant};

/// Tracks the last signal coming from the broker. `expired(now)` indicates the session is dead.
#[derive(Debug, Clone, Copy)]
pub struct Watchdog {
    deadline: Duration,
    last: Instant,
}

impl Watchdog {
    /// Creates the watchdog "touched" at `now` (session start counts as a fresh signal).
    pub fn new(deadline: Duration, now: Instant) -> Self {
        Self {
            deadline,
            last: now,
        }
    }

    /// Registers a signal from the broker (any message, including `Ack`).
    pub fn touch(&mut self, now: Instant) {
        self.last = now;
    }

    /// `true` if `deadline` has passed since the last signal.
    pub fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.last) >= self.deadline
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn fresh_watchdog_not_expired() {
        let t0 = Instant::now();
        let wd = Watchdog::new(Duration::from_secs(90), t0);
        assert!(!wd.expired(t0));
        assert!(!wd.expired(t0 + Duration::from_secs(89)));
    }

    #[test]
    fn expires_after_deadline() {
        let t0 = Instant::now();
        let wd = Watchdog::new(Duration::from_secs(90), t0);
        assert!(wd.expired(t0 + Duration::from_secs(90)));
        assert!(wd.expired(t0 + Duration::from_secs(120)));
    }

    #[test]
    fn touch_resets_the_clock() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(90), t0);
        let t1 = t0 + Duration::from_secs(80);
        wd.touch(t1);
        // 80s + 89s = 169s from start, but only 89s since last touch → still alive.
        assert!(!wd.expired(t1 + Duration::from_secs(89)));
        assert!(wd.expired(t1 + Duration::from_secs(90)));
    }
}
