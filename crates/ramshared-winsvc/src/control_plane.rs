//! Host-side control-plane policy helpers for the Windows service.
//!
//! This module contains heartbeat deadline tracking and bounded VHDX command
//! helpers. The AF_HYPERV transport is in `ramshared-ipc::vsock`; neither the
//! transport nor these helpers are wired into the production service yet.
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
/// Operations serialized through a mutex (DT-4). The runner is injectable so
/// the success and idempotency paths are exercisable without a Windows host.
pub struct VhdxLifecycle {
    attached: Mutex<Vec<String>>,
    _serialize: Mutex<()>,
    runner: Box<dyn CommandRunner>,
}

impl Default for VhdxLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl VhdxLifecycle {
    pub fn new() -> Self {
        Self::with_runner(Box::new(WslRunner))
    }

    /// Build a lifecycle around an injected runner (tests; production uses `new`).
    pub fn with_runner(runner: Box<dyn CommandRunner>) -> Self {
        Self {
            attached: Mutex::new(Vec::new()),
            _serialize: Mutex::new(()),
            runner,
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

        match self.runner.run(
            "wsl.exe",
            &["--mount", "--vhd", path, "--bare"],
            Duration::from_secs(10),
        ) {
            Ok(_) => {
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

        match self
            .runner
            .run("wsl.exe", &["--unmount", path], Duration::from_secs(10))
        {
            Ok(_) => {
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

/// Production runner: spawns the program with a bounded wait and discards output.
pub struct WslRunner;

impl CommandRunner for WslRunner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
        run_command_bounded(program, args, timeout).map(|()| String::new())
    }
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// Scriptable runner: records invocations and returns a canned result.
    /// The call counter is shared so tests can observe it after the runner is
    /// moved into a `Box`.
    struct MockRunner {
        calls: Arc<AtomicUsize>,
        result: Result<String, String>,
    }

    /// Build a mock runner plus an external handle to its invocation count.
    fn mock_runner(result: Result<String, String>) -> (Box<dyn CommandRunner>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = MockRunner {
            calls: Arc::clone(&calls),
            result,
        };
        (Box::new(runner), calls)
    }

    fn mock_ok() -> (Box<dyn CommandRunner>, Arc<AtomicUsize>) {
        mock_runner(Ok(String::new()))
    }

    fn mock_err(message: &str) -> (Box<dyn CommandRunner>, Arc<AtomicUsize>) {
        mock_runner(Err(message.to_string()))
    }

    impl CommandRunner for MockRunner {
        fn run(
            &self,
            _program: &str,
            _args: &[&str],
            _timeout: Duration,
        ) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.clone()
        }
    }

    /// Panic while holding a mutex so the next caller must recover from poison.
    fn poison<T>(lock: &Mutex<T>) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock.lock().unwrap();
            panic!("intentional poison for recovery coverage");
        }));
    }

    #[test]
    fn heartbeat_deadline_revokes_lease() {
        let tracker = HeartbeatTracker::new(1); // 1s interval → 3s lease
        assert!(tracker.lease_expired(), "no heartbeat = expired");

        tracker.record_heartbeat();
        assert!(!tracker.lease_expired(), "fresh heartbeat = valid");

        let remaining = tracker.lease_remaining_ms();
        assert!(
            (1..=3000).contains(&remaining),
            "remaining must sit inside the 3s lease: {remaining}"
        );
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
    fn heartbeat_tracker_lease_remaining_zero_when_no_heartbeat() {
        let tracker = HeartbeatTracker::new(5);
        assert_eq!(tracker.lease_remaining_ms(), 0);
    }

    #[test]
    fn heartbeat_lease_remaining_is_zero_once_expired() {
        // Zero interval → zero lease timeout, so a recorded heartbeat is
        // immediately past deadline and must report 0, not underflow.
        let tracker = HeartbeatTracker::new(0);
        assert_eq!(
            tracker.record_heartbeat(),
            0,
            "zero timeout = zero remaining"
        );
        assert!(tracker.lease_expired(), "zero timeout expires immediately");
        assert_eq!(tracker.lease_remaining_ms(), 0);
    }

    #[test]
    fn heartbeat_recovers_from_a_poisoned_lock() {
        let tracker = HeartbeatTracker::new(5);
        poison(&tracker.last_heartbeat);
        assert!(
            tracker.lease_expired(),
            "poisoned lock must not propagate panic"
        );
        assert_eq!(tracker.lease_remaining_ms(), 0);
        assert!(tracker.record_heartbeat() > 0, "poisoned lock must recover");
    }

    #[test]
    fn vhdx_attach_records_partuuid_on_success() {
        let (runner, calls) = mock_ok();
        let lifecycle = VhdxLifecycle::with_runner(runner);
        lifecycle
            .attach("C:\\test.vhdx", "11111111-2222-3333-4444-555555555555")
            .expect("attach succeeds under a healthy runner");
        assert_eq!(
            lifecycle.attached_partuuids(),
            vec!["11111111-2222-3333-4444-555555555555".to_string()]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "exactly one spawn");
    }

    #[test]
    fn vhdx_attach_is_idempotent() {
        // Kahneman #17 evidence: a retry of the same PartUUID must not spawn a
        // second `wsl.exe --mount`.
        let (runner, calls) = mock_ok();
        let lifecycle = VhdxLifecycle::with_runner(runner);
        let partuuid = "11111111-2222-3333-4444-555555555555";

        lifecycle
            .attach("C:\\test.vhdx", partuuid)
            .expect("first attach succeeds");
        // Second attach of the same PartUUID must skip without a second spawn.
        lifecycle
            .attach("C:\\test.vhdx", partuuid)
            .expect("second attach is a no-op success");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second attach of the same PartUUID must not spawn again"
        );
        assert_eq!(lifecycle.attached_partuuids(), vec![partuuid.to_string()]);
    }

    #[test]
    fn vhdx_attach_failure_leaves_list_empty() {
        let (runner, _calls) = mock_err("mount denied");
        let lifecycle = VhdxLifecycle::with_runner(runner);
        let error = lifecycle
            .attach("C:\\test.vhdx", "partuuid-x")
            .expect_err("runner failure must surface");
        assert!(error.contains("mount denied"), "unexpected error: {error}");
        assert!(lifecycle.attached_partuuids().is_empty());
    }

    #[test]
    fn vhdx_detach_clears_attached_list() {
        let (runner, _calls) = mock_ok();
        let lifecycle = VhdxLifecycle::with_runner(runner);
        lifecycle
            .attach("C:\\test.vhdx", "partuuid-x")
            .expect("attach succeeds");
        lifecycle.detach("C:\\test.vhdx").expect("detach succeeds");
        assert!(
            lifecycle.attached_partuuids().is_empty(),
            "single-VHDX Day-0 state: detach clears the list"
        );
    }

    #[test]
    fn vhdx_recovers_from_a_poisoned_lock() {
        let (runner, _calls) = mock_ok();
        let lifecycle = VhdxLifecycle::with_runner(runner);
        poison(&lifecycle._serialize);
        poison(&lifecycle.attached);
        lifecycle
            .attach("C:\\test.vhdx", "partuuid-x")
            .expect("poisoned locks must not propagate panic");
        assert_eq!(
            lifecycle.attached_partuuids(),
            vec!["partuuid-x".to_string()]
        );
    }

    #[test]
    fn vhdx_attach_timeout_is_bounded() {
        // Production runner: on a host without wsl.exe this is a spawn failure,
        // which must return promptly rather than hang.
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
    fn bounded_command_accepts_success() {
        assert!(run_command_bounded("true", &[], Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn bounded_command_reports_spawn_failure() {
        let error = run_command_bounded(
            "ramshared-definitely-not-a-real-program",
            &[],
            Duration::from_millis(50),
        )
        .expect_err("a missing program must fail, not hang");
        assert!(!error.is_empty());
    }

    #[test]
    fn bounded_command_reports_nonzero_exit() {
        let error = run_command_bounded("false", &[], Duration::from_secs(1))
            .expect_err("a nonzero exit must surface as an error");
        assert!(error.contains("exit status"), "unexpected error: {error}");
    }

    #[cfg(unix)]
    #[test]
    fn vhdx_command_runner_reaps_a_timed_out_child() {
        let start = Instant::now();
        let result = run_command_bounded("sleep", &["2"], Duration::from_millis(20));
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn wsl_runner_adapts_bounded_command_output() {
        let runner = WslRunner;
        assert!(runner.run("true", &[], Duration::from_secs(1)).is_ok());
        assert!(
            runner
                .run(
                    "ramshared-definitely-not-a-real-program",
                    &[],
                    Duration::from_millis(50)
                )
                .is_err()
        );
    }
}
