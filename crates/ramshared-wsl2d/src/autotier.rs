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
        let cfg = AutotierConfig { safety_margin_bytes: 10, ..Default::default() };
        assert_eq!(super::usable_budget(input(100, 40, 20), &cfg), 70);
        assert_eq!(super::usable_budget(input(10, 100, 0), &cfg), 0);
    }

    #[test]
    fn stale_sample_blocks_commit() {
        let cfg = AutotierConfig { max_sample_age: Duration::from_secs(1), ..Default::default() };
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
        let cfg = AutotierConfig { constrained_samples: 2, recovery_samples: 2, ..Default::default() };
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
}
