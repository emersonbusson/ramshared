//! Pure host-safety decisions for Windows product teardown (SPEC DT-8 / DT-13).

use std::collections::BTreeMap;
use std::time::Duration;

/// Combine configured and currently active pagefiles.
///
/// Both observations are mandatory. Paths are deduplicated case-insensitively
/// because Windows drive paths are case-insensitive.
pub fn merge_pagefile_sources(
    configured: Result<Vec<String>, String>,
    active: Result<Vec<String>, String>,
) -> Result<Vec<String>, String> {
    let configured = configured.map_err(|e| format!("configured pagefiles: {e}"))?;
    let active = active.map_err(|e| format!("active pagefiles: {e}"))?;
    let mut unique = BTreeMap::new();
    for path in configured.into_iter().chain(active) {
        let path = path.trim().to_string();
        if path.is_empty() {
            return Err("pagefile source returned an empty path".into());
        }
        unique.insert(path.to_ascii_uppercase(), path);
    }
    Ok(unique.into_values().collect())
}

/// Classify a DOS pagefile path against one volume. `?:\` is the Windows
/// system-managed wildcard and is unsafe for every candidate volume. Unknown
/// path forms are ambiguous and therefore fail closed.
pub fn pagefile_may_target_volume(path: &str, volume_letter: char) -> Result<bool, String> {
    let letter = volume_letter.to_ascii_uppercase();
    if !('D'..='Z').contains(&letter) {
        return Err("product volume letter must be D..=Z".into());
    }
    let path = path.trim().to_ascii_uppercase();
    let bytes = path.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || bytes[2] != b'\\' {
        return Err(format!("ambiguous pagefile path: {path}"));
    }
    if bytes[0] == b'?' {
        return Ok(true);
    }
    if !bytes[0].is_ascii_alphabetic() {
        return Err(format!("invalid pagefile drive: {path}"));
    }
    Ok(bytes[0] == letter as u8)
}

/// Pure lock-wait decision. A deadline breach is never an Online refusal:
/// the mutating worker may still own an in-flight lock operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockWaitDecision {
    Completed,
    KeepPumping,
    EnterFailedSafe,
    /// Test sentinel documenting the forbidden transition.
    ResumeOnline,
}

pub fn lock_wait_decision(
    elapsed: Duration,
    deadline: Duration,
    result_ready: bool,
) -> LockWaitDecision {
    if result_ready {
        LockWaitDecision::Completed
    } else if elapsed >= deadline {
        LockWaitDecision::EnterFailedSafe
    } else {
        LockWaitDecision::KeepPumping
    }
}

/// Complete isolated-campaign promotion conjunction (SPEC DT-13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignVerdict {
    pub online: bool,
    pub binary_match: bool,
    pub rounds_pass: bool,
    pub console_exit: Option<i32>,
    pub force_killed: bool,
    pub lease_released: bool,
    pub cuda_restored: bool,
    pub no_new_dump: bool,
    pub terminal_safe: bool,
    pub teardown_ms: Option<u64>,
}

/// Structured error for a campaign safety failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafetyFailure {
    pub check_name: String,
    pub threshold: String,
    pub actual_value: String,
}

impl std::fmt::Display for SafetyFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "safety check '{}' failed: expected {}, got {}",
            self.check_name, self.threshold, self.actual_value
        )
    }
}

impl std::error::Error for SafetyFailure {}

impl CampaignVerdict {
    pub fn is_pass(&self, teardown_budget: Duration) -> bool {
        self.check_safety(teardown_budget).is_ok()
    }

    pub fn check_safety(&self, teardown_budget: Duration) -> Result<(), SafetyFailure> {
        if !self.online {
            return Err(SafetyFailure {
                check_name: "online".into(),
                threshold: "true".into(),
                actual_value: "false".into(),
            });
        }
        if !self.binary_match {
            return Err(SafetyFailure {
                check_name: "binary_match".into(),
                threshold: "true".into(),
                actual_value: "false".into(),
            });
        }
        if !self.rounds_pass {
            return Err(SafetyFailure {
                check_name: "rounds_pass".into(),
                threshold: "true".into(),
                actual_value: "false".into(),
            });
        }
        if self.console_exit != Some(0) {
            return Err(SafetyFailure {
                check_name: "console_exit".into(),
                threshold: "Some(0)".into(),
                actual_value: match self.console_exit {
                    Some(code) => code.to_string(),
                    None => "None".into(),
                },
            });
        }
        if self.force_killed {
            return Err(SafetyFailure {
                check_name: "force_killed".into(),
                threshold: "false".into(),
                actual_value: "true".into(),
            });
        }
        if !self.lease_released {
            return Err(SafetyFailure {
                check_name: "lease_released".into(),
                threshold: "true".into(),
                actual_value: "false".into(),
            });
        }
        if !self.cuda_restored {
            return Err(SafetyFailure {
                check_name: "cuda_restored".into(),
                threshold: "true".into(),
                actual_value: "false".into(),
            });
        }
        if !self.no_new_dump {
            return Err(SafetyFailure {
                check_name: "no_new_dump".into(),
                threshold: "true".into(),
                actual_value: "false".into(),
            });
        }
        if !self.terminal_safe {
            return Err(SafetyFailure {
                check_name: "terminal_safe".into(),
                threshold: "true".into(),
                actual_value: "false".into(),
            });
        }
        match self.teardown_ms {
            Some(ms) if u128::from(ms) <= teardown_budget.as_millis() => Ok(()),
            Some(ms) => Err(SafetyFailure {
                check_name: "teardown_ms".into(),
                threshold: format!("<= {}", teardown_budget.as_millis()),
                actual_value: ms.to_string(),
            }),
            None => Err(SafetyFailure {
                check_name: "teardown_ms".into(),
                threshold: format!("<= {}", teardown_budget.as_millis()),
                actual_value: "None".into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::time::Duration;

    fn base_pass() -> CampaignVerdict {
        CampaignVerdict {
            online: true,
            binary_match: true,
            rounds_pass: true,
            console_exit: Some(0),
            force_killed: false,
            lease_released: true,
            cuda_restored: true,
            no_new_dump: true,
            terminal_safe: true,
            teardown_ms: Some(2_000),
        }
    }

    #[test]
    fn test_host_safety_online_false_returns_error() {
        let mut v = base_pass();
        v.online = false;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "online");
        assert_eq!(err.actual_value, "false");
    }

    #[test]
    fn test_host_safety_binary_match_false_returns_error() {
        let mut v = base_pass();
        v.binary_match = false;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "binary_match");
        assert_eq!(err.actual_value, "false");
    }

    #[test]
    fn test_host_safety_rounds_pass_false_returns_error() {
        let mut v = base_pass();
        v.rounds_pass = false;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "rounds_pass");
        assert_eq!(err.actual_value, "false");
    }

    #[test]
    fn test_host_safety_console_exit_nonzero_returns_error() {
        let mut v = base_pass();
        v.console_exit = Some(7);
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "console_exit");
        assert_eq!(err.threshold, "Some(0)");
        assert_eq!(err.actual_value, "7");

        v.console_exit = None;
        let err2 = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err2.check_name, "console_exit");
        assert_eq!(err2.actual_value, "None");
    }

    #[test]
    fn test_host_safety_force_killed_true_returns_error() {
        let mut v = base_pass();
        v.force_killed = true;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "force_killed");
        assert_eq!(err.threshold, "false");
        assert_eq!(err.actual_value, "true");
    }

    #[test]
    fn test_host_safety_lease_released_false_returns_error() {
        let mut v = base_pass();
        v.lease_released = false;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "lease_released");
    }

    #[test]
    fn test_host_safety_cuda_restored_false_returns_error() {
        let mut v = base_pass();
        v.cuda_restored = false;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "cuda_restored");
    }

    #[test]
    fn test_host_safety_no_new_dump_false_returns_error() {
        let mut v = base_pass();
        v.no_new_dump = false;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "no_new_dump");
    }

    #[test]
    fn test_host_safety_terminal_safe_false_returns_error() {
        let mut v = base_pass();
        v.terminal_safe = false;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "terminal_safe");
    }

    #[test]
    fn test_host_safety_teardown_ms_exceeds_budget_returns_error() {
        let mut v = base_pass();
        v.teardown_ms = Some(30_001);
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "teardown_ms");
        assert_eq!(err.threshold, "<= 30000");
        assert_eq!(err.actual_value, "30001");
    }

    #[test]
    fn test_host_safety_teardown_ms_none_returns_error() {
        let mut v = base_pass();
        v.teardown_ms = None;
        let err = v.check_safety(Duration::from_secs(30)).unwrap_err();
        assert_eq!(err.check_name, "teardown_ms");
        assert_eq!(err.threshold, "<= 30000");
        assert_eq!(err.actual_value, "None");
    }

    #[test]
    fn pagefile_sources_are_unioned() {
        let rows = merge_pagefile_sources(
            Ok(vec![r"C:\pagefile.sys".into(), r"S:\configured.sys".into()]),
            Ok(vec![r"C:\pagefile.sys".into(), r"S:\active.sys".into()]),
        )
        .unwrap();
        assert_eq!(
            rows,
            vec![
                r"C:\pagefile.sys".to_string(),
                r"S:\active.sys".to_string(),
                r"S:\configured.sys".to_string(),
            ]
        );
    }

    #[test]
    fn either_pagefile_source_error_fails_closed() {
        assert!(merge_pagefile_sources(Err("registry denied".into()), Ok(vec![])).is_err());
        assert!(merge_pagefile_sources(Ok(vec![]), Err("WMI timeout".into())).is_err());
    }

    #[test]
    fn empty_pagefile_path_fails_closed() {
        assert!(merge_pagefile_sources(Ok(vec![String::new()]), Ok(vec![])).is_err());
        assert!(merge_pagefile_sources(Ok(vec![]), Ok(vec!["   ".into()])).is_err());
    }

    #[test]
    fn wildcard_or_ambiguous_pagefile_path_is_unsafe() {
        assert!(pagefile_may_target_volume(r"?:\pagefile.sys", 'S').unwrap());
        assert!(pagefile_may_target_volume(r"S:\pagefile.sys", 'S').unwrap());
        assert!(!pagefile_may_target_volume(r"C:\pagefile.sys", 'S').unwrap());
        assert!(pagefile_may_target_volume(r"\??\S:\pagefile.sys", 'S').is_err());
    }

    #[test]
    fn pagefile_may_target_volume_edge_cases() {
        // Invalid volume letters
        assert!(pagefile_may_target_volume(r"S:\pagefile.sys", 'C').is_err());
        assert!(pagefile_may_target_volume(r"S:\pagefile.sys", '1').is_err());

        // Ambiguous paths (length < 3, missing slash, wrong slash)
        assert!(pagefile_may_target_volume(r"S:", 'S').is_err());
        assert!(pagefile_may_target_volume(r"S:pagefile", 'S').is_err());
        assert!(pagefile_may_target_volume(r"S:/pagefile.sys", 'S').is_err());

        // Invalid drive letters
        assert!(pagefile_may_target_volume(r"1:\pagefile.sys", 'S').is_err());

        // Whitespace trimming and case insensitivity
        assert!(pagefile_may_target_volume("  s:\\pagefile.sys ", 's').unwrap());
    }

    #[test]
    fn lock_deadline_never_resumes_online() {
        assert_eq!(
            lock_wait_decision(Duration::from_secs(29), Duration::from_secs(30), false),
            LockWaitDecision::KeepPumping
        );
        assert_eq!(
            lock_wait_decision(Duration::from_secs(30), Duration::from_secs(30), false),
            LockWaitDecision::EnterFailedSafe
        );
        assert_ne!(
            lock_wait_decision(Duration::from_secs(31), Duration::from_secs(30), false),
            LockWaitDecision::ResumeOnline
        );
    }

    #[test]
    fn complete_campaign_verdict_requires_every_safety_term() {
        let pass = CampaignVerdict {
            online: true,
            binary_match: true,
            rounds_pass: true,
            console_exit: Some(0),
            force_killed: false,
            lease_released: true,
            cuda_restored: true,
            no_new_dump: true,
            terminal_safe: true,
            teardown_ms: Some(2_000),
        };
        assert!(pass.is_pass(Duration::from_secs(30)));

        let mut crash = pass.clone();
        crash.console_exit = Some(7);
        assert!(!crash.is_pass(Duration::from_secs(30)));

        let mut forced = pass.clone();
        forced.force_killed = true;
        assert!(!forced.is_pass(Duration::from_secs(30)));

        let mut slow = pass;
        slow.teardown_ms = Some(30_001);
        assert!(!slow.is_pass(Duration::from_secs(30)));
    }
}
