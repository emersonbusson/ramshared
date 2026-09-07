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
    paused_at: Option<Instant>,
}

impl Watchdog {
    pub fn new(deadline: Duration, now: Instant) -> Result<Self, WatchdogError> {
        if deadline <= Duration::from_millis(10) {
            return Err(WatchdogError::TooShort);
        }
        if deadline > Duration::from_secs(86400) {
            return Err(WatchdogError::TooLong);
        }
        Ok(Self {
            deadline,
            last: now,
            paused_at: None,
        })
    }

    /// Creates a watchdog with transparent fallback clamp (resilient constructor).
    pub fn new_clamped(deadline: Duration, now: Instant) -> Self {
        let clamped = deadline.clamp(Duration::from_millis(10), Duration::from_secs(86400));
        Self {
            deadline: clamped,
            last: now,
            paused_at: None,
        }
    }

    /// Signals liveness, resetting the timer to `now`.
    pub fn touch(&mut self, now: Instant) {
        if now >= self.last {
            self.last = now;
        }
    }

    /// Pauses the watchdog. No expiration will occur while paused.
    pub fn pause(&mut self, now: Instant) {
        if self.paused_at.is_none() && now >= self.last {
            self.paused_at = Some(now);
        }
    }

    /// Resumes the watchdog, compensating the missed time.
    #[allow(clippy::collapsible_if)]
    pub fn resume(&mut self, now: Instant) {
        if let Some(paused) = self.paused_at.take() {
            if now > paused {
                self.last += now.saturating_duration_since(paused);
            }
        }
    }

    /// `true` if `deadline` has passed since the last signal.
    pub fn expired(&self, now: Instant) -> bool {
        let effective_now = match self.paused_at {
            Some(p) if now > p => p,
            _ => now,
        };
        if effective_now < self.last {
            return false;
        }
        effective_now.saturating_duration_since(self.last) >= self.deadline
    }

    /// Checks if `deadline` has passed since the last signal.
    pub fn check(&self, now: Instant) -> Result<(), WatchdogError> {
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
            Watchdog::new(Duration::from_millis(5), t0),
            Err(WatchdogError::TooShort)
        );
        assert_eq!(
            Watchdog::new(Duration::from_millis(10), t0),
            Err(WatchdogError::TooShort)
        );
        assert_eq!(
            Watchdog::new(Duration::from_secs(100_000), t0),
            Err(WatchdogError::TooLong)
        );
        assert!(Watchdog::new(Duration::from_millis(20), t0).is_ok());
    }

    #[test]
    fn does_not_expire_before_deadline() {
        let t0 = Instant::now();
        let wd = Watchdog::new(Duration::from_secs(90), t0).expect("valid");
        assert!(!wd.expired(t0));
        assert!(!wd.expired(t0 + Duration::from_secs(89)));
        assert_eq!(wd.check(t0), Ok(()));
        assert_eq!(wd.check(t0 + Duration::from_secs(89)), Ok(()));
    }

    #[test]
    fn expires_at_and_after_deadline() {
        let t0 = Instant::now();
        let wd = Watchdog::new(Duration::from_secs(90), t0).expect("valid");
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
        let mut wd = Watchdog::new(Duration::from_secs(90), t0).expect("valid");
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
        let mut wd = Watchdog::new(Duration::from_secs(90), t0).expect("valid");
        let t_past = t0.checked_sub(Duration::from_secs(10)).unwrap_or(t0);

        assert!(!wd.expired(t_past));
        wd.touch(t_past);
        assert_eq!(wd.last, t0);
    }

    #[test]
    fn watchdog_pause_during_system_suspend_and_resumption() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(90), t0).expect("valid");

        let t_pause = t0 + Duration::from_secs(50);
        wd.pause(t_pause);

        // Simulating long system suspend (2 hours)
        let t_suspend_during = t_pause + Duration::from_secs(3600);
        assert!(!wd.expired(t_suspend_during));

        let t_resume = t_pause + Duration::from_secs(7200);
        wd.resume(t_resume);

        // The time during pause should be compensated,
        // 50 seconds have elapsed before pause, 40 seconds remaining.
        assert!(!wd.expired(t_resume));
        assert!(!wd.expired(t_resume + Duration::from_secs(39)));
        assert!(wd.expired(t_resume + Duration::from_secs(40)));
    }
}
