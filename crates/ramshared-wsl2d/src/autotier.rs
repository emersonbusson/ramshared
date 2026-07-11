use std::fmt;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct BudgetInput {
    pub budget: u64,
    pub current_usage: u64,
    pub cuda_committed: u64,
    pub sampled_at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct AutotierConfig {
    pub safety_margin_bytes: u64,
    pub max_sample_age: Duration,
    pub constrained_samples: u32,
    pub recovery_samples: u32,
}

impl Default for AutotierConfig {
    fn default() -> Self {
        Self {
            safety_margin_bytes: 256 * 1024 * 1024,
            max_sample_age: Duration::from_secs(5),
            constrained_samples: 3,
            recovery_samples: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutotierState {
    Available,
    Constrained,
    Demoting,
    Parked,
    Recovering,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyDecision {
    pub state: AutotierState,
    pub allow_commit: bool,
    pub usable_budget: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyError(&'static str);

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PolicyError {}

pub fn backend_release_allowed(
    swapoff_attempted: bool,
    swapoff_confirmed: bool,
    used_kb: u64,
) -> bool {
    used_kb == 0 && (!swapoff_attempted || swapoff_confirmed)
}

pub fn usable_budget(input: BudgetInput, config: &AutotierConfig) -> u64 {
    let external_usage = input.current_usage.saturating_sub(input.cuda_committed);
    input
        .budget
        .saturating_sub(external_usage)
        .saturating_sub(config.safety_margin_bytes)
}

pub fn commit_allowed(
    input: BudgetInput,
    committed: u64,
    chunk: u64,
    config: &AutotierConfig,
) -> Result<u64, PolicyError> {
    if Instant::now().saturating_duration_since(input.sampled_at) > config.max_sample_age {
        return Err(PolicyError("WDDM budget sample is stale"));
    }
    let usable = usable_budget(input, config);
    if committed.saturating_add(chunk) > usable {
        return Err(PolicyError("next CUDA chunk exceeds usable WDDM budget"));
    }
    Ok(usable)
}

pub struct AutotierPolicy {
    config: AutotierConfig,
    state: AutotierState,
    constrained_streak: u32,
    recovery_streak: u32,
}

impl AutotierPolicy {
    pub fn new(config: AutotierConfig) -> Self {
        Self {
            config,
            state: AutotierState::Available,
            constrained_streak: 0,
            recovery_streak: 0,
        }
    }

    pub fn observe(
        &mut self,
        input: BudgetInput,
        committed: u64,
        next_chunk: u64,
    ) -> PolicyDecision {
        let usable = usable_budget(input, &self.config);
        let allowed = commit_allowed(input, committed, next_chunk, &self.config).is_ok();
        match self.state {
            AutotierState::Available if !allowed => {
                self.constrained_streak = self.constrained_streak.saturating_add(1);
                if committed.saturating_add(next_chunk) > usable && committed > usable
                    || self.constrained_streak >= self.config.constrained_samples
                {
                    self.state = AutotierState::Constrained;
                }
            }
            AutotierState::Available => self.constrained_streak = 0,
            AutotierState::Parked if allowed => {
                self.recovery_streak = self.recovery_streak.saturating_add(1);
                if self.recovery_streak >= self.config.recovery_samples {
                    self.state = AutotierState::Recovering;
                }
            }
            AutotierState::Parked => self.recovery_streak = 0,
            _ => {}
        }
        PolicyDecision {
            state: self.state,
            allow_commit: allowed && self.state == AutotierState::Available,
            usable_budget: usable,
        }
    }

    pub fn mark_demoting(&mut self) -> AutotierState {
        if self.state == AutotierState::Constrained {
            self.state = AutotierState::Demoting;
        }
        self.state
    }

    pub fn mark_parked(&mut self, used_kb: u64) -> Result<AutotierState, PolicyError> {
        if used_kb != 0 {
            return Err(PolicyError(
                "cannot park VRAM tier while swap is referenced",
            ));
        }
        if matches!(self.state, AutotierState::Demoting | AutotierState::Parked) {
            self.state = AutotierState::Parked;
            self.recovery_streak = 0;
            Ok(self.state)
        } else {
            Err(PolicyError("park transition requires demoting state"))
        }
    }

    pub fn mark_available(&mut self, tier_empty: bool) -> Result<AutotierState, PolicyError> {
        if !tier_empty {
            return Err(PolicyError("recovery requires an empty VRAM tier"));
        }
        if matches!(
            self.state,
            AutotierState::Recovering | AutotierState::Available
        ) {
            self.state = AutotierState::Available;
            self.constrained_streak = 0;
            Ok(self.state)
        } else {
            Err(PolicyError(
                "available transition requires recovering state",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AutotierConfig, AutotierPolicy, AutotierState, BudgetInput};
    use std::time::{Duration, Instant};

    fn input(budget: u64, usage: u64, committed: u64) -> BudgetInput {
        BudgetInput {
            budget,
            current_usage: usage,
            cuda_committed: committed,
            sampled_at: Instant::now(),
        }
    }

    #[test]
    fn usable_budget_subtracts_only_external_usage_and_margin() {
        let cfg = AutotierConfig {
            safety_margin_bytes: 10,
            ..Default::default()
        };
        assert_eq!(super::usable_budget(input(100, 40, 20), &cfg), 70);
        assert_eq!(super::usable_budget(input(10, 100, 0), &cfg), 0);
    }

    #[test]
    fn stale_sample_blocks_commit() {
        let cfg = AutotierConfig {
            max_sample_age: Duration::from_secs(1),
            ..Default::default()
        };
        let mut old = input(100, 0, 0);
        old.sampled_at = Instant::now() - Duration::from_secs(2);
        assert!(super::commit_allowed(old, 0, 1, &cfg).is_err());
    }

    #[test]
    fn next_chunk_over_budget_constrains_immediately() {
        let mut policy = AutotierPolicy::new(AutotierConfig::default());
        let decision = policy.observe(input(100, 0, 0), 80, 21);
        assert_eq!(decision.state, AutotierState::Constrained);
        assert!(!decision.allow_commit);
    }

    #[test]
    fn recovery_requires_hysteresis_and_is_idempotent() {
        let cfg = AutotierConfig {
            constrained_samples: 2,
            recovery_samples: 2,
            ..Default::default()
        };
        let mut policy = AutotierPolicy::new(cfg);
        let low = input(100, 100, 0);
        assert_eq!(policy.observe(low, 0, 1).state, AutotierState::Available);
        assert_eq!(policy.observe(low, 0, 1).state, AutotierState::Constrained);
        assert_eq!(policy.mark_demoting(), AutotierState::Demoting);
        assert_eq!(policy.mark_demoting(), AutotierState::Demoting);
        assert_eq!(policy.mark_parked(0), Ok(AutotierState::Parked));
        assert!(policy.mark_parked(1).is_err());
        let high = input(u64::MAX, 0, 0);
        assert_eq!(policy.observe(high, 0, 1).state, AutotierState::Parked);
        assert_eq!(policy.observe(high, 0, 1).state, AutotierState::Recovering);
        assert_eq!(policy.mark_available(true), Ok(AutotierState::Available));
        assert!(policy.mark_available(false).is_err());
    }

    #[test]
    fn backend_release_requires_zero_used_and_confirmed_swapoff() {
        assert!(super::backend_release_allowed(false, false, 0));
        assert!(super::backend_release_allowed(true, true, 0));
        assert!(!super::backend_release_allowed(true, false, 0));
        assert!(!super::backend_release_allowed(true, true, 1));
        assert!(!super::backend_release_allowed(false, false, 1));
    }

    #[test]
    fn all_policy_error_and_reset_branches_are_covered() {
        let cfg = AutotierConfig {
            constrained_samples: 1,
            recovery_samples: 2,
            safety_margin_bytes: 0,
            ..Default::default()
        };
        let mut policy = AutotierPolicy::new(cfg);
        let healthy = input(100, 0, 0);
        assert!(policy.observe(healthy, 0, 1).allow_commit);
        assert!(policy.mark_parked(0).is_err());
        assert!(policy.mark_available(true).is_ok());
        assert_eq!(
            policy.observe(input(0, 0, 0), 0, 1).state,
            AutotierState::Constrained
        );
        assert!(policy.mark_available(true).is_err());
        assert_eq!(policy.mark_demoting(), AutotierState::Demoting);
        assert_eq!(policy.mark_parked(0), Ok(AutotierState::Parked));
        assert_eq!(
            policy.observe(input(0, 0, 0), 0, 1).state,
            AutotierState::Parked
        );
        assert!(policy.mark_available(false).is_err());
        let error = match super::commit_allowed(input(0, 0, 0), 0, 1, &cfg) {
            Ok(_) => panic!("zero budget unexpectedly allowed a commit"),
            Err(error) => error,
        };
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn recovery_tracker_requires_empty_tier_and_three_good_samples() {
        let mut tracker = super::RecoveryTracker::new(3);
        assert!(!tracker.observe(true, false));
        assert!(!tracker.observe(false, true));
        assert!(!tracker.observe(true, true));
        assert!(!tracker.observe(true, true));
        assert!(tracker.observe(true, true));
        tracker.reset();
        assert!(!tracker.observe(true, true));
    }
}
