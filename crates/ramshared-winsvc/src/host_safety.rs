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

impl CampaignVerdict {
    pub fn is_pass(&self, teardown_budget: Duration) -> bool {
        self.online
            && self.binary_match
            && self.rounds_pass
            && self.console_exit == Some(0)
            && !self.force_killed
            && self.lease_released
            && self.cuda_restored
            && self.no_new_dump
            && self.terminal_safe
            && self
                .teardown_ms
                .is_some_and(|ms| u128::from(ms) <= teardown_budget.as_millis())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::time::Duration;

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
    fn wildcard_or_ambiguous_pagefile_path_is_unsafe() {
        assert!(pagefile_may_target_volume(r"?:\pagefile.sys", 'S').unwrap());
        assert!(pagefile_may_target_volume(r"S:\pagefile.sys", 'S').unwrap());
        assert!(!pagefile_may_target_volume(r"C:\pagefile.sys", 'S').unwrap());
        assert!(pagefile_may_target_volume(r"\??\S:\pagefile.sys", 'S').is_err());
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
