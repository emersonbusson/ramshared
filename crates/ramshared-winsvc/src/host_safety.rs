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


/// Abstract provider for system information.
pub trait SysInfoProvider {
    fn available_memory(&self) -> u64;
    fn available_disk_space(&self, path: &str) -> Result<u64, String>;
    fn cpu_load_percent(&self) -> f32;
}

/// Validates that available system memory meets the required threshold.
pub fn memory_threshold_guard(sysinfo: &impl SysInfoProvider, required_bytes: u64) -> Result<(), String> {
    let available = sysinfo.available_memory();
    if available < required_bytes {
        return Err(format!("insufficient memory: {} < {}", available, required_bytes));
    }
    Ok(())
}

/// Validates that available disk space on the specified path meets the required threshold.
pub fn disk_space_guard(sysinfo: &impl SysInfoProvider, path: &str, required_bytes: u64) -> Result<(), String> {
    let available = sysinfo.available_disk_space(path)?;
    if available < required_bytes {
        return Err(format!("insufficient disk space: {} < {}", available, required_bytes));
    }
    Ok(())
}

/// Validates that the current CPU load does not exceed the maximum allowed percentage.
pub fn cpu_load_guard(sysinfo: &impl SysInfoProvider, max_load_percent: f32) -> Result<(), String> {
    let current_load = sysinfo.cpu_load_percent();
    if current_load.is_nan() || current_load < 0.0 || current_load > 100.0 {
        return Err(format!("invalid cpu load: {}", current_load));
    }
    if max_load_percent.is_nan() || max_load_percent < 0.0 || max_load_percent > 100.0 {
        return Err(format!("invalid max cpu load: {}", max_load_percent));
    }
    if current_load > max_load_percent {
        return Err(format!("cpu load exceeds maximum: {} > {}", current_load, max_load_percent));
    }
    Ok(())
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

    struct MockSysInfo {
        mem: u64,
        disk: Result<u64, String>,
        cpu: f32,
    }

    impl MockSysInfo {
        fn new(mem: u64, disk: Result<u64, String>, cpu: f32) -> Self {
            Self { mem, disk, cpu }
        }
    }

    impl SysInfoProvider for MockSysInfo {
        fn available_memory(&self) -> u64 {
            self.mem
        }

        fn available_disk_space(&self, _path: &str) -> Result<u64, String> {
            self.disk.clone()
        }

        fn cpu_load_percent(&self) -> f32 {
            self.cpu
        }
    }

    #[test]
    fn test_host_safety_memory_threshold_guard_success() {
        let sysinfo = MockSysInfo::new(1024, Ok(0), 0.0);
        assert!(memory_threshold_guard(&sysinfo, 512).is_ok());
        assert!(memory_threshold_guard(&sysinfo, 1024).is_ok());
    }

    #[test]
    fn test_host_safety_memory_threshold_guard_failure() {
        let sysinfo = MockSysInfo::new(512, Ok(0), 0.0);
        assert!(memory_threshold_guard(&sysinfo, 1024).is_err());
    }

    #[test]
    fn test_host_safety_memory_threshold_guard_zero_and_max() {
        let sysinfo_zero = MockSysInfo::new(0, Ok(0), 0.0);
        assert!(memory_threshold_guard(&sysinfo_zero, 0).is_ok());
        assert!(memory_threshold_guard(&sysinfo_zero, 1).is_err());

        let sysinfo_max = MockSysInfo::new(u64::MAX, Ok(0), 0.0);
        assert!(memory_threshold_guard(&sysinfo_max, u64::MAX).is_ok());
    }

    #[test]
    fn test_host_safety_disk_space_guard_success() {
        let sysinfo = MockSysInfo::new(0, Ok(2048), 0.0);
        assert!(disk_space_guard(&sysinfo, "C:\\\\\\\\", 1024).is_ok());
        assert!(disk_space_guard(&sysinfo, "C:\\\\\\\\", 2048).is_ok());
    }

    #[test]
    fn test_host_safety_disk_space_guard_failure() {
        let sysinfo = MockSysInfo::new(0, Ok(1024), 0.0);
        assert!(disk_space_guard(&sysinfo, "C:\\\\\\\\", 2048).is_err());
    }

    #[test]
    fn test_host_safety_disk_space_guard_sysinfo_error() {
        let sysinfo = MockSysInfo::new(0, Err(String::from("access denied")), 0.0);
        assert!(disk_space_guard(&sysinfo, "C:\\\\\\\\", 1024).is_err());
    }

    #[test]
    fn test_host_safety_disk_space_guard_zero_and_max() {
        let sysinfo_zero = MockSysInfo::new(0, Ok(0), 0.0);
        assert!(disk_space_guard(&sysinfo_zero, "C:\\\\\\\\", 0).is_ok());
        assert!(disk_space_guard(&sysinfo_zero, "C:\\\\\\\\", 1).is_err());

        let sysinfo_max = MockSysInfo::new(0, Ok(u64::MAX), 0.0);
        assert!(disk_space_guard(&sysinfo_max, "C:\\\\\\\\", u64::MAX).is_ok());
    }

    #[test]
    fn test_host_safety_cpu_load_guard_success() {
        let sysinfo = MockSysInfo::new(0, Ok(0), 50.0);
        assert!(cpu_load_guard(&sysinfo, 80.0).is_ok());
        assert!(cpu_load_guard(&sysinfo, 50.0).is_ok());
    }

    #[test]
    fn test_host_safety_cpu_load_guard_failure() {
        let sysinfo = MockSysInfo::new(0, Ok(0), 90.0);
        assert!(cpu_load_guard(&sysinfo, 80.0).is_err());
    }

    #[test]
    fn test_host_safety_cpu_load_guard_invalid_current_load() {
        let sysinfo_neg = MockSysInfo::new(0, Ok(0), -1.0);
        assert!(cpu_load_guard(&sysinfo_neg, 80.0).is_err());

        let sysinfo_over = MockSysInfo::new(0, Ok(0), 101.0);
        assert!(cpu_load_guard(&sysinfo_over, 80.0).is_err());

        let sysinfo_nan = MockSysInfo::new(0, Ok(0), f32::NAN);
        assert!(cpu_load_guard(&sysinfo_nan, 80.0).is_err());
    }

    #[test]
    fn test_host_safety_cpu_load_guard_invalid_max_load() {
        let sysinfo = MockSysInfo::new(0, Ok(0), 50.0);
        assert!(cpu_load_guard(&sysinfo, -1.0).is_err());
        assert!(cpu_load_guard(&sysinfo, 101.0).is_err());
        assert!(cpu_load_guard(&sysinfo, f32::NAN).is_err());
    }
}
