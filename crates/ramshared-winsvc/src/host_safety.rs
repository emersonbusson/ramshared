//! Pure host-safety decisions for Windows product teardown (SPEC DT-8 / DT-13).

use std::collections::BTreeMap;
use std::time::Duration;

/// Trait representing a mockable interface for reading host system information.
pub trait SysInfo {
    fn available_memory_bytes(&self) -> Result<u64, String>;
    fn available_disk_space_bytes(&self) -> Result<u64, String>;
    fn cpu_load_percentage(&self) -> Result<u8, String>;
}

/// Validates that available memory meets the required minimum threshold.
pub fn memory_threshold_guard<S: SysInfo>(sys: &S, min_required_bytes: u64) -> Result<(), String> {
    let avail = sys.available_memory_bytes()?;
    if avail < min_required_bytes {
        return Err(format!(
            "insufficient memory: {} bytes available, {} required",
            avail, min_required_bytes
        ));
    }
    Ok(())
}

/// Validates that available disk space meets the required minimum threshold.
pub fn disk_space_guard<S: SysInfo>(sys: &S, min_required_bytes: u64) -> Result<(), String> {
    let avail = sys.available_disk_space_bytes()?;
    if avail < min_required_bytes {
        return Err(format!(
            "insufficient disk space: {} bytes available, {} required",
            avail, min_required_bytes
        ));
    }
    Ok(())
}

/// Validates that CPU load does not exceed the maximum allowed percentage.
pub fn cpu_load_guard<S: SysInfo>(sys: &S, max_allowed_percent: u8) -> Result<(), String> {
    if max_allowed_percent > 100 {
        return Err("max_allowed_percent cannot exceed 100".to_string());
    }
    let load = sys.cpu_load_percentage()?;
    if load > 100 {
        return Err("system reported cpu load > 100".to_string());
    }
    if load > max_allowed_percent {
        return Err(format!(
            "cpu load too high: {}%, max allowed {}%",
            load, max_allowed_percent
        ));
    }
    Ok(())
}

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

    struct MockSysInfo {
        mem: Result<u64, String>,
        disk: Result<u64, String>,
        cpu: Result<u8, String>,
    }

    impl MockSysInfo {
        fn new(
            mem: Result<u64, String>,
            disk: Result<u64, String>,
            cpu: Result<u8, String>,
        ) -> Self {
            Self { mem, disk, cpu }
        }
    }

    impl SysInfo for MockSysInfo {
        fn available_memory_bytes(&self) -> Result<u64, String> {
            self.mem.clone()
        }

        fn available_disk_space_bytes(&self) -> Result<u64, String> {
            self.disk.clone()
        }

        fn cpu_load_percentage(&self) -> Result<u8, String> {
            self.cpu.clone()
        }
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

    #[test]
    fn test_host_safety_memory_guard_pass() {
        let sys = MockSysInfo::new(Ok(1000), Ok(1000), Ok(50));
        assert!(memory_threshold_guard(&sys, 500).is_ok());
        assert!(memory_threshold_guard(&sys, 1000).is_ok()); // boundary
    }

    #[test]
    fn test_host_safety_memory_guard_fail_insufficient() {
        let sys = MockSysInfo::new(Ok(499), Ok(1000), Ok(50));
        let err = memory_threshold_guard(&sys, 500).unwrap_err();
        assert_eq!(
            err,
            "insufficient memory: 499 bytes available, 500 required"
        );
    }

    #[test]
    fn test_host_safety_memory_guard_fail_sys_error() {
        let sys = MockSysInfo::new(Err("WMI timeout".to_string()), Ok(1000), Ok(50));
        let err = memory_threshold_guard(&sys, 500).unwrap_err();
        assert_eq!(err, "WMI timeout");
    }

    #[test]
    fn test_host_safety_disk_guard_pass() {
        let sys = MockSysInfo::new(Ok(1000), Ok(2000), Ok(50));
        assert!(disk_space_guard(&sys, 1000).is_ok());
        assert!(disk_space_guard(&sys, 2000).is_ok()); // boundary
    }

    #[test]
    fn test_host_safety_disk_guard_fail_insufficient() {
        let sys = MockSysInfo::new(Ok(1000), Ok(999), Ok(50));
        let err = disk_space_guard(&sys, 1000).unwrap_err();
        assert_eq!(
            err,
            "insufficient disk space: 999 bytes available, 1000 required"
        );
    }

    #[test]
    fn test_host_safety_disk_guard_fail_sys_error() {
        let sys = MockSysInfo::new(Ok(1000), Err("permission denied".to_string()), Ok(50));
        let err = disk_space_guard(&sys, 1000).unwrap_err();
        assert_eq!(err, "permission denied");
    }

    #[test]
    fn test_host_safety_cpu_guard_pass() {
        let sys = MockSysInfo::new(Ok(1000), Ok(1000), Ok(80));
        assert!(cpu_load_guard(&sys, 90).is_ok());
        assert!(cpu_load_guard(&sys, 80).is_ok()); // boundary
    }

    #[test]
    fn test_host_safety_cpu_guard_fail_too_high() {
        let sys = MockSysInfo::new(Ok(1000), Ok(1000), Ok(95));
        let err = cpu_load_guard(&sys, 90).unwrap_err();
        assert_eq!(err, "cpu load too high: 95%, max allowed 90%");
    }

    #[test]
    fn test_host_safety_cpu_guard_fail_sys_error() {
        let sys = MockSysInfo::new(Ok(1000), Ok(1000), Err("missing deps".to_string()));
        let err = cpu_load_guard(&sys, 90).unwrap_err();
        assert_eq!(err, "missing deps");
    }

    #[test]
    fn test_host_safety_cpu_guard_invalid_max() {
        let sys = MockSysInfo::new(Ok(1000), Ok(1000), Ok(50));
        let err = cpu_load_guard(&sys, 101).unwrap_err();
        assert_eq!(err, "max_allowed_percent cannot exceed 100");
    }

    #[test]
    fn test_host_safety_cpu_guard_invalid_sys_load() {
        let sys = MockSysInfo::new(Ok(1000), Ok(1000), Ok(105));
        let err = cpu_load_guard(&sys, 90).unwrap_err();
        assert_eq!(err, "system reported cpu load > 100");
    }

    #[test]
    fn test_host_safety_memory_guard_zero_required() {
        let sys = MockSysInfo::new(Ok(0), Ok(0), Ok(0));
        assert!(memory_threshold_guard(&sys, 0).is_ok());
    }

    #[test]
    fn test_host_safety_disk_guard_zero_required() {
        let sys = MockSysInfo::new(Ok(0), Ok(0), Ok(0));
        assert!(disk_space_guard(&sys, 0).is_ok());
    }
}
