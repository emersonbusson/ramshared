//! Session watchdog of the agent: if the broker stops responding (no `Ack`/command for
//! `deadline`), the session is considered dead and the agent performs cleanup + reconnects (DT-18/DT-27).
//!
//! Pure and with injected clock (`Instant`) to be testable deterministically —
//! `main.rs` passes `Instant::now()`. No I/O here.

use std::fmt;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub enum WatchdogError {
    HeartbeatTimeout(Duration),
    SupervisorTerminated,
    TooShort(Duration),
}

impl std::error::Error for WatchdogError {}

impl fmt::Display for WatchdogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeartbeatTimeout(d) => write!(
                f,
                "watchdog: broker silent for {}s; closing session",
                d.as_secs()
            ),
            Self::SupervisorTerminated => write!(f, "watchdog: supervisor terminated"),
            Self::TooShort(d) => write!(f, "watchdog: deadline too short ({}ms)", d.as_millis()),
        }
    }
}

/// Tracks the last signal coming from the broker. `expired(now)` indicates the session is dead.
#[derive(Debug, Clone, Copy)]
pub struct Watchdog {
    deadline: Duration,
    last: Instant,
}

impl Watchdog {
    /// Creates the watchdog "touched" at `now` (session start counts as a fresh signal).
    pub fn new(deadline: Duration, now: Instant) -> Result<Self, WatchdogError> {
        if deadline <= Duration::from_millis(10) {
            return Err(WatchdogError::TooShort(deadline));
        }
        Ok(Self {
            deadline,
            last: now,
        })
    }

    /// Registers a signal from the broker (any message, including `Ack`).
    pub fn touch(&mut self, now: Instant) {
        self.last = now;
    }

    /// Checks if `deadline` has passed since the last signal.
    pub fn check(&self, now: Instant) -> Result<(), WatchdogError> {
        if now.duration_since(self.last) >= self.deadline {
            Err(WatchdogError::HeartbeatTimeout(self.deadline))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn fresh_watchdog_not_expired() {
        let t0 = Instant::now();
        let wd = Watchdog::new(Duration::from_secs(90), t0).expect("valid deadline");
        assert_eq!(wd.check(t0), Ok(()));
        assert_eq!(wd.check(t0 + Duration::from_secs(89)), Ok(()));
    }

    #[test]
    fn expires_after_deadline() {
        let t0 = Instant::now();
        let wd = Watchdog::new(Duration::from_secs(90), t0).expect("valid deadline");
        assert_eq!(
            wd.check(t0 + Duration::from_secs(90)),
            Err(WatchdogError::HeartbeatTimeout(Duration::from_secs(90)))
        );
        assert_eq!(
            wd.check(t0 + Duration::from_secs(120)),
            Err(WatchdogError::HeartbeatTimeout(Duration::from_secs(90)))
        );
    }

    #[test]
    fn touch_resets_the_clock() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(90), t0).expect("valid deadline");
        let t1 = t0 + Duration::from_secs(80);
        wd.touch(t1);
        // 80s + 89s = 169s from start, but only 89s since last touch → still alive.
        assert_eq!(wd.check(t1 + Duration::from_secs(89)), Ok(()));
        assert_eq!(
            wd.check(t1 + Duration::from_secs(90)),
            Err(WatchdogError::HeartbeatTimeout(Duration::from_secs(90)))
        );
    }

    #[test]
    fn deadline_too_short() {
        let t0 = Instant::now();
        let err = Watchdog::new(Duration::from_millis(5), t0).expect_err("should fail");
        assert_eq!(err, WatchdogError::TooShort(Duration::from_millis(5)));
    }
}
