use std::time::{SystemTime, UNIX_EPOCH};
use crate::cascade::Tier;

/// Represents a validated cascade tier transition event for security auditing.
#[derive(Debug, Clone)]
pub struct TierTransitionAuditLog {
    /// UTC timestamp of the transition authorization.
    pub timestamp_sec: u64,
    /// The external trigger or caller (e.g. "demote-daemon", "manual-cli").
    pub trigger_source: String,
    /// The state of the cascade prior to the transition.
    pub before_tier: Tier,
    /// The state of the cascade after the transition.
    pub after_tier: Tier,
}

impl TierTransitionAuditLog {
    /// Authorizes a transition and constructs an audit log entry.
    /// Uses zero-trust validation for the trigger source (cannot be empty).
    pub fn authorize(
        trigger_source: &str,
        before_tier: Tier,
        after_tier: Tier,
    ) -> Result<Self, &'static str> {
        let trigger = trigger_source.trim();
        if trigger.is_empty() {
            return Err("untrusted transition trigger: source cannot be empty");
        }

        let timestamp_sec = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        log::info!("Transition Authorized: {} from {:?} to {:?} at {}", trigger, before_tier, after_tier, timestamp_sec);
        Ok(Self {
            timestamp_sec,
            trigger_source: trigger.to_string(),
            before_tier,
            after_tier,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authorize_valid_transition() {
        let log = TierTransitionAuditLog::authorize("demote-daemon", Tier::Zram, Tier::Vram).unwrap();
        assert_eq!(log.trigger_source, "demote-daemon");
        assert_eq!(log.before_tier, Tier::Zram);
        assert_eq!(log.after_tier, Tier::Vram);
        assert!(log.timestamp_sec > 0);
    }

    #[test]
    fn test_authorize_rejects_empty_source() {
        let result = TierTransitionAuditLog::authorize("   ", Tier::Zram, Tier::Vram);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "untrusted transition trigger: source cannot be empty");
    }
}
