//! Watchdog timer: checks whether the supervisor or daemon has signaled heartbeat within the deadline.
//! SPEC §14.1, DT-9. Pure time arithmetic, no threads.

use std::fmt;
use std::time::{Duration, Instant};

/// Errors that can occur when initializing or evaluating a Watchdog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchdogError {
    /// Heartbeat timed out past the deadline.
    HeartbeatTimeout(Duration),
    /// Supervisor process terminated unexpectedly.
    SupervisorTerminated,
    /// The deadline is too short (less than the baseline timer resolution of 10ms).
    TooShort,
    /// The deadline is too long (exceeds practical limit of 1 day).
    TooLong,
}

impl fmt::Display for WatchdogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeartbeatTimeout(d) => {
                write!(
                    f,
                    "watchdog: broker silent for {}s; closing session",
                    d.as_secs()
                )
            }
            Self::SupervisorTerminated => write!(f, "watchdog: supervisor terminated"),
            Self::TooShort => write!(f, "watchdog deadline too short (< 10ms)"),
            Self::TooLong => write!(f, "watchdog deadline too long (> 1 day)"),
        }
    }
}

impl std::error::Error for WatchdogError {}

#[derive(Debug, PartialEq, Eq)]
pub struct Watchdog {
    deadline: Duration,
    last: Instant,
    failures: u32,
    threshold: u32,
}

impl Watchdog {
    pub fn new(deadline: Duration, now: Instant, threshold: u32) -> Result<Self, WatchdogError> {
        if deadline <= Duration::from_millis(10) {
            return Err(WatchdogError::TooShort);
        }
        if deadline > Duration::from_secs(86400) {
            return Err(WatchdogError::TooLong);
        }
        Ok(Self {
            deadline,
            last: now,
            failures: 0,
            threshold,
        })
    }

    /// Creates a watchdog with transparent fallback clamp (resilient constructor).
    pub fn new_clamped(deadline: Duration, now: Instant, threshold: u32) -> Self {
        let clamped = deadline.clamp(Duration::from_millis(10), Duration::from_secs(86400));
        Self {
            deadline: clamped,
            last: now,
            failures: 0,
            threshold,
        }
    }

    /// Signals liveness, resetting the timer to `now`.
    pub fn touch(&mut self, now: Instant) {
        if now >= self.last {
            self.last = now;
            self.failures = 0;
        }
    }

    /// `true` if `deadline` has passed since the last signal.
    pub fn expired(&mut self, now: Instant) -> bool {
        if now < self.last {
            return false;
        }
        if now.saturating_duration_since(self.last) >= self.deadline {
            let passed_intervals = now.saturating_duration_since(self.last).as_millis() / self.deadline.as_millis();
            self.failures = self.failures.saturating_add(passed_intervals as u32);
            self.last += Duration::from_millis((passed_intervals * self.deadline.as_millis()) as u64);
            if self.failures >= self.threshold {
                return true;
            }
        }
        self.failures >= self.threshold
    }

    /// Checks if `deadline` has passed since the last signal.
    pub fn check(&mut self, now: Instant) -> Result<(), WatchdogError> {
        if self.expired(now) {
            Err(WatchdogError::HeartbeatTimeout(self.deadline))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_extremes() {
        let t0 = Instant::now();
        assert_eq!(
            Watchdog::new(Duration::from_millis(5), t0, 1),
            Err(WatchdogError::TooShort)
        );
        assert_eq!(
            Watchdog::new(Duration::from_millis(10), t0, 1),
            Err(WatchdogError::TooShort)
        );
        assert_eq!(
            Watchdog::new(Duration::from_secs(100_000), t0, 1),
            Err(WatchdogError::TooLong)
        );
        assert!(Watchdog::new(Duration::from_millis(20), t0, 1).is_ok());
    }

    #[test]
    fn does_not_expire_before_deadline() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(90), t0, 1).expect("valid");
        assert!(!wd.expired(t0));
        assert!(!wd.expired(t0 + Duration::from_secs(89)));
        assert_eq!(wd.check(t0), Ok(()));
        assert_eq!(wd.check(t0 + Duration::from_secs(89)), Ok(()));
    }

    #[test]
    fn expires_at_and_after_deadline() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(90), t0, 1).expect("valid");
        assert!(wd.expired(t0 + Duration::from_secs(90)));
        assert!(wd.expired(t0 + Duration::from_secs(120)));
        assert_eq!(
            wd.check(t0 + Duration::from_secs(90)),
            Err(WatchdogError::HeartbeatTimeout(Duration::from_secs(90)))
        );
    }

    #[test]
    fn touch_resets_the_clock() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(90), t0, 1).expect("valid");
        let t1 = t0 + Duration::from_secs(50);
        wd.touch(t1);

        // 50s from original t0 would normally expire at 90s (40s after t1).
        // Since we touched at t1, 90s from t1 is t1 + 90s.
        assert!(!wd.expired(t0 + Duration::from_secs(90)));
        assert!(!wd.expired(t1 + Duration::from_secs(89)));
        assert!(wd.expired(t1 + Duration::from_secs(90)));
    }

    #[test]
    fn time_drift_backwards_safe() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(90), t0, 1).expect("valid");
        let t_past = t0.checked_sub(Duration::from_secs(10)).unwrap_or(t0);

        assert!(!wd.expired(t_past));
        wd.touch(t_past);
        assert_eq!(wd.last, t0);
    }

    #[test]
    fn threshold_accumulates_failures() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(10), t0, 3).expect("valid");

        // Interval 1
        assert!(!wd.expired(t0 + Duration::from_secs(10)));
        // Interval 2
        assert!(!wd.expired(t0 + Duration::from_secs(20)));
        // Interval 3 -> Trigger!
        assert!(wd.expired(t0 + Duration::from_secs(30)));
    }

    #[test]
    fn threshold_resets_on_touch() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(10), t0, 3).expect("valid");

        // Interval 1
        assert!(!wd.expired(t0 + Duration::from_secs(10)));
        // Touch halfway through interval 2
        wd.touch(t0 + Duration::from_secs(15));

        // Interval 1 from touch
        assert!(!wd.expired(t0 + Duration::from_secs(25)));
        // Interval 2 from touch
        assert!(!wd.expired(t0 + Duration::from_secs(35)));
        // Interval 3 from touch -> Trigger!
        assert!(wd.expired(t0 + Duration::from_secs(45)));
    }

    #[test]
    fn jump_multiple_intervals_adds_proportionally() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(10), t0, 4).expect("valid");

        // Jump 2.5 intervals -> Adds 2 failures
        assert!(!wd.expired(t0 + Duration::from_secs(25)));
        // Wait 2 more intervals -> Adds 2 failures -> Trigger!
        assert!(wd.expired(t0 + Duration::from_secs(45)));
    }
}
