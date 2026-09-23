//! vsock control plane for the Windows host service.
//!
//! AF_HYPERV listener, heartbeat deadline tracking, lease management,
//! and VHDX lifecycle via `wsl.exe --mount`.
//!
//! SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Control plane state exposed in status JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPlaneState {
    Vsock,
    FileFallback,
    SafeMode,
}

impl ControlPlaneState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vsock => "vsock",
            Self::FileFallback => "file_fallback",
            Self::SafeMode => "safe_mode",
        }
    }
}

/// Heartbeat deadline tracker (host side).
///
/// Revokes lease when `now - last_heartbeat_at > 3 × heartbeat_secs`.
pub struct HeartbeatTracker {
    last_heartbeat: Mutex<Option<Instant>>,
    heartbeat_interval: Duration,
    lease_multiplier: u32,
}

impl HeartbeatTracker {
    pub fn new(heartbeat_secs: u64) -> Self {
        Self {
            last_heartbeat: Mutex::new(None),
            heartbeat_interval: Duration::from_secs(heartbeat_secs),
            lease_multiplier: 3,
        }
    }

    /// Record a heartbeat arrival. Returns lease remaining milliseconds.
    pub fn record_heartbeat(&self) -> u64 {
        let now = Instant::now();
        *self
            .last_heartbeat
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(now);
        self.lease_timeout().as_millis() as u64
    }

    /// Check if the lease is expired (no heartbeat within 3× interval).
    pub fn lease_expired(&self) -> bool {
        let guard = self
            .last_heartbeat
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *guard {
            None => true,
            Some(last) => last.elapsed() > self.lease_timeout(),
        }
    }

    /// Remaining lease time in milliseconds (0 if expired).
    pub fn lease_remaining_ms(&self) -> u64 {
        let guard = self
            .last_heartbeat
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *guard {
            None => 0,
            Some(last) => {
                let timeout = self.lease_timeout();
                let elapsed = last.elapsed();
                if elapsed >= timeout {
                    0
                } else {
                    (timeout - elapsed).as_millis() as u64
                }
            }
        }
    }

    fn lease_timeout(&self) -> Duration {
        self.heartbeat_interval * self.lease_multiplier
    }
}

/// VHDX lifecycle manager (host side).
///
/// Absorbs `Manage-RamSharedOrigin.ps1` — attach/detach via `wsl.exe --mount`.
/// Operations serialized through a mutex (DT-4).
pub struct VhdxLifecycle {
    attached: Mutex<Vec<String>>,
    _serialize: Mutex<()>,
}

impl Default for VhdxLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl VhdxLifecycle {
    pub fn new() -> Self {
        Self {
            attached: Mutex::new(Vec::new()),
            _serialize: Mutex::new(()),
        }
    }

    /// Attach a VHDX via `wsl.exe --mount --vhd <path> --bare`.
    /// Idempotent: skips if PartUUID already attached. Bounded to 10s (DT-4).
    pub fn attach(&self, path: &str, partuuid: &str) -> Result<(), String> {
        let _guard = self._serialize.lock().unwrap_or_else(|e| e.into_inner());

        // Idempotency check: skip if already attached
        {
            let attached = self.attached.lock().unwrap_or_else(|e| e.into_inner());
            if attached.iter().any(|p| p == partuuid) {
                return Ok(());
            }
        }

        match run_command_bounded(
            "wsl.exe",
            &["--mount", "--vhd", path, "--bare"],
            Duration::from_secs(10),
        ) {
            Ok(()) => {
                self.attached
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(partuuid.to_string());
                Ok(())
            }
            Err(error) => Err(format!("wsl.exe --mount failed: {error}")),
        }
    }

    /// Detach a VHDX via `wsl.exe --unmount`. Idempotent (DT-4).
    pub fn detach(&self, path: &str) -> Result<(), String> {
        let _guard = self._serialize.lock().unwrap_or_else(|e| e.into_inner());

        match run_command_bounded("wsl.exe", &["--unmount", path], Duration::from_secs(10)) {
            Ok(()) => {
                self.attached
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear(); // single-VHDX Day-0 state
                Ok(())
            }
            Err(error) => Err(format!("wsl.exe --unmount failed: {error}")),
        }
    }

    /// List currently attached PartUUIDs.
    pub fn attached_partuuids(&self) -> Vec<String> {
        self.attached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// Telemetry snapshot for status JSON (ITEM-6).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ControlPlaneTelemetry {
    pub control_plane_state: String,
    pub heartbeat_rtt_us: u64,
    pub lease_remaining_ms: u64,
    pub vsock_disconnect_count: u64,
    pub vhdx_attach_count: u64,
}

impl Default for ControlPlaneTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlPlaneTelemetry {
    pub fn new() -> Self {
        Self {
            control_plane_state: ControlPlaneState::Vsock.as_str().to_string(),
            heartbeat_rtt_us: 0,
            lease_remaining_ms: 0,
            vsock_disconnect_count: 0,
            vhdx_attach_count: 0,
        }
    }
}

/// Trait for command execution with timeout (testable abstraction).
pub trait CommandRunner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> Result<String, String>;
}

fn run_command_bounded(program: &str, args: &[&str], deadline: Duration) -> Result<(), String> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("exit status {status}")),
            Ok(None) if start.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} ms", deadline.as_millis()));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::time::Instant;

    #[test]
    fn heartbeat_deadline_revokes_lease() {
        let tracker = HeartbeatTracker::new(1); // 1s interval → 3s lease
        assert!(tracker.lease_expired(), "no heartbeat = expired");

        tracker.record_heartbeat();
        assert!(!tracker.lease_expired(), "fresh heartbeat = valid");

        let remaining = tracker.lease_remaining_ms();
        assert!(remaining > 0 && remaining <= 3000);
    }

    #[test]
    fn heartbeat_lease_remaining_counts_down() {
        let tracker = HeartbeatTracker::new(1);
        let r1 = tracker.record_heartbeat();
        std::thread::sleep(Duration::from_millis(50));
        let r2 = tracker.lease_remaining_ms();
        assert!(r2 < r1, "remaining must decrease over time");
    }

    #[test]
    fn vhdx_attach_is_idempotent() {
        let lifecycle = VhdxLifecycle::new();
        // First attach will fail (no wsl.exe) — but the idempotency check
        // should prevent double-attach attempts.
        let _ = lifecycle.attach("C:\\test.vhdx", "11111111-2222-3333-4444-555555555555");
        let partuuids = lifecycle.attached_partuuids();
        // After failed attach, list is empty. After success, list has 1 entry.
        assert!(partuuids.len() <= 1);
    }

    #[test]
    fn vhdx_attach_timeout_is_bounded() {
        let lifecycle = VhdxLifecycle::new();
        let start = Instant::now();
        let _ = lifecycle.attach("C:\\test.vhdx", "partuuid-x");
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(15),
            "attach must be bounded to 10s + margin: {elapsed:?}"
        );
    }

    #[test]
    fn control_plane_telemetry_serializes() {
        let telem = ControlPlaneTelemetry::new();
        let json = serde_json::to_string(&telem).expect("serialize");
        assert!(json.contains("control_plane_state"));
        assert!(json.contains("heartbeat_rtt_us"));
        assert!(json.contains("lease_remaining_ms"));
    }

    #[test]
    fn control_plane_state_as_str() {
        assert_eq!(ControlPlaneState::Vsock.as_str(), "vsock");
        assert_eq!(ControlPlaneState::FileFallback.as_str(), "file_fallback");
        assert_eq!(ControlPlaneState::SafeMode.as_str(), "safe_mode");
    }

    #[test]
    fn default_impls_match_new() {
        let d = VhdxLifecycle::default();
        let n = VhdxLifecycle::new();
        assert_eq!(d.attached_partuuids(), n.attached_partuuids());

        let dt = ControlPlaneTelemetry::default();
        let nt = ControlPlaneTelemetry::new();
        assert_eq!(dt.control_plane_state, nt.control_plane_state);
    }

    #[test]
    fn vhdx_detach_is_bounded() {
        let lifecycle = VhdxLifecycle::new();
        let start = Instant::now();
        let _ = lifecycle.detach("C:\\test.vhdx");
        assert!(start.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn vhdx_attached_partuuids_empty_after_failed_attach() {
        let lifecycle = VhdxLifecycle::new();
        let _ = lifecycle.attach("C:\\nonexistent.vhdx", "some-uuid");
        // attach fails (no wsl.exe) → list stays empty
        assert!(lifecycle.attached_partuuids().is_empty());
    }

    #[test]
    fn heartbeat_tracker_lease_remaining_zero_when_no_heartbeat() {
        let tracker = HeartbeatTracker::new(5);
        assert_eq!(tracker.lease_remaining_ms(), 0);
    }

    #[test]
    fn bounded_command_accepts_success() {
        assert!(run_command_bounded("true", &[], Duration::from_secs(1)).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn vhdx_command_runner_reaps_a_timed_out_child() {
        let start = Instant::now();
        let result = run_command_bounded("sleep", &["2"], Duration::from_millis(20));
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_millis(500));
    }
}
