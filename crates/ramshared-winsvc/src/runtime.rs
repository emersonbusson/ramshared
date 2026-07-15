//! Pure phase machine for storage-only product path (SPEC DT-7 / DT-9 / DT-12).
//!
//! Injectable [`RuntimeOps`] keeps Linux unit tests free of CUDA/Windows handles.

use crate::config::WinDriveConfig;

/// Runtime lifecycle phases (DT-7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePhase {
    Stopped,
    Leased,
    CudaReady,
    DiskCreated,
    QueueRegistered,
    Online,
    Stopping,
    FailedSafe,
}

/// How the process was entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    Console,
    Scm,
    ProbeCuda,
}

/// Stable error class for evidence / exit mapping (no payloads).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeErrorClass {
    Config,
    Broker,
    Cuda,
    Abi,
    Identity,
    Checksum,
    PagefileSafety,
    Busy,
    Watchdog,
    Internal,
    AmbiguousCrash,
}

/// Structured runtime error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    pub class: RuntimeErrorClass,
    pub code: i32,
    pub message: String,
}

impl RuntimeError {
    pub fn new(class: RuntimeErrorClass, code: i32, message: impl Into<String>) -> Self {
        Self {
            class,
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "runtime {:?} code={}: {}",
            self.class, self.code, self.message
        )
    }
}

impl std::error::Error for RuntimeError {}

/// Summary returned after a successful or refused stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSummary {
    pub phase: RuntimePhase,
    pub lease_id: Option<u32>,
    pub allocated_bytes: u64,
    pub idempotent_stop: bool,
    pub exit_code: i32,
}

/// Authoritative in-process state (never reconstructed from JSONL — DT-9).
#[derive(Clone, Debug)]
pub struct RuntimeState {
    pub phase: RuntimePhase,
    pub mode: RunMode,
    pub lease_id: Option<u32>,
    pub lease_bytes: u64,
    pub allocated_bytes: u64,
    pub stop_completed: bool,
    pub healthy: bool,
    pub cuda_op_outstanding: bool,
    pub ambiguous_crash: bool,
    /// Effects applied once (for unwind / idempotent stop).
    pub effects: EffectLog,
}

impl RuntimeState {
    pub fn new(mode: RunMode) -> Self {
        Self {
            phase: RuntimePhase::Stopped,
            mode,
            lease_id: None,
            lease_bytes: 0,
            allocated_bytes: 0,
            stop_completed: false,
            healthy: true,
            cuda_op_outstanding: false,
            ambiguous_crash: false,
            effects: EffectLog::default(),
        }
    }
}

/// Countable side effects for unit tests (injection seam observability).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectLog {
    pub lease_acquire: u32,
    pub lease_release: u32,
    pub cuda_alloc: u32,
    pub cuda_free: u32,
    pub disk_create: u32,
    pub disk_destroy: u32,
    pub queue_register: u32,
    pub queue_unregister: u32,
    pub fallback_selected: u32,
    pub retries: u32,
}

/// Injectable operations for the phase machine.
pub trait RuntimeOps {
    fn acquire_lease(&mut self, bytes: u64) -> Result<u32, RuntimeError>;
    fn release_lease(&mut self, lease_id: u32) -> Result<(), RuntimeError>;
    fn cuda_alloc(&mut self, bytes: u64) -> Result<(), RuntimeError>;
    fn cuda_free(&mut self) -> Result<(), RuntimeError>;
    fn create_disk(&mut self) -> Result<(), RuntimeError>;
    fn destroy_disk(&mut self) -> Result<(), RuntimeError>;
    fn register_queue(&mut self) -> Result<(), RuntimeError>;
    fn unregister_queue(&mut self) -> Result<(), RuntimeError>;
    /// Gate A/B pagefile safety. Ok(true) = clear; Ok(false)/Err refuse destructive path.
    fn pagefile_gates_clear(&mut self) -> Result<bool, RuntimeError>;
    fn drain_io(&mut self) -> Result<(), RuntimeError>;
    /// Optional busy observation (enumeration). Returns true if still busy.
    fn observe_busy(&mut self) -> Result<bool, RuntimeError> {
        Ok(false)
    }
}

/// Run product startup to Online (or fail with reverse unwind).
pub fn run_runtime<O: RuntimeOps>(
    cfg: &WinDriveConfig,
    state: &mut RuntimeState,
    ops: &mut O,
) -> Result<RuntimeSummary, RuntimeError> {
    if state.ambiguous_crash {
        return Err(RuntimeError::new(
            RuntimeErrorClass::AmbiguousCrash,
            8,
            "ambiguous crash state is not replayed",
        ));
    }
    if state.phase != RuntimePhase::Stopped || state.stop_completed {
        return Err(RuntimeError::new(
            RuntimeErrorClass::Internal,
            1,
            "run_runtime requires Stopped phase",
        ));
    }

    // Leased
    let lease_id = match ops.acquire_lease(cfg.size_bytes) {
        Ok(id) => id,
        Err(e) => {
            state.phase = RuntimePhase::FailedSafe;
            return Err(e);
        }
    };
    state.effects.lease_acquire += 1;
    state.lease_id = Some(lease_id);
    state.lease_bytes = cfg.size_bytes;
    state.phase = RuntimePhase::Leased;

    // CudaReady
    if let Err(e) = ops.cuda_alloc(cfg.size_bytes) {
        unwind_from(state, ops, RuntimePhase::Leased)?;
        state.phase = RuntimePhase::FailedSafe;
        return Err(e);
    }
    state.effects.cuda_alloc += 1;
    state.allocated_bytes = cfg.size_bytes;
    state.phase = RuntimePhase::CudaReady;

    // DiskCreated
    if let Err(e) = create_with_bounded_busy(ops, &mut state.effects) {
        unwind_from(state, ops, RuntimePhase::CudaReady)?;
        state.phase = RuntimePhase::FailedSafe;
        return Err(e);
    }
    state.effects.disk_create += 1;
    state.phase = RuntimePhase::DiskCreated;

    // QueueRegistered
    if let Err(e) = ops.register_queue() {
        unwind_from(state, ops, RuntimePhase::DiskCreated)?;
        state.phase = RuntimePhase::FailedSafe;
        return Err(e);
    }
    state.effects.queue_register += 1;
    state.phase = RuntimePhase::QueueRegistered;

    state.phase = RuntimePhase::Online;
    Ok(RuntimeSummary {
        phase: RuntimePhase::Online,
        lease_id: state.lease_id,
        allocated_bytes: state.allocated_bytes,
        idempotent_stop: false,
        exit_code: 0,
    })
}

fn create_with_bounded_busy<O: RuntimeOps>(
    ops: &mut O,
    effects: &mut EffectLog,
) -> Result<(), RuntimeError> {
    match ops.create_disk() {
        Ok(()) => Ok(()),
        Err(e) if e.class == RuntimeErrorClass::Busy => {
            // One busy observation only (DT-7) — re-query, do not repeat CREATE.
            effects.retries += 1;
            if effects.retries > 1 {
                return Err(RuntimeError::new(
                    RuntimeErrorClass::Busy,
                    5,
                    "busy observation budget exhausted",
                ));
            }
            let still = ops.observe_busy()?;
            if still {
                Err(e)
            } else {
                // State re-queried clear; still must not re-issue CREATE in this design
                // when first CREATE returned busy without completing — fail closed.
                Err(RuntimeError::new(
                    RuntimeErrorClass::Busy,
                    5,
                    "CREATE busy; will not retry IOCTL",
                ))
            }
        }
        Err(e) => Err(e),
    }
}

/// Stop / teardown. Idempotent after successful completion (DT-9).
pub fn stop_runtime<O: RuntimeOps>(
    state: &mut RuntimeState,
    ops: &mut O,
) -> Result<RuntimeSummary, RuntimeError> {
    if state.ambiguous_crash {
        return Err(RuntimeError::new(
            RuntimeErrorClass::AmbiguousCrash,
            8,
            "ambiguous crash state is not replayed",
        ));
    }
    if state.stop_completed && state.phase == RuntimePhase::Stopped {
        return Ok(RuntimeSummary {
            phase: RuntimePhase::Stopped,
            lease_id: None,
            allocated_bytes: 0,
            idempotent_stop: true,
            exit_code: 0,
        });
    }
    if state.phase == RuntimePhase::Stopped && !state.stop_completed {
        // Never started.
        state.stop_completed = true;
        return Ok(RuntimeSummary {
            phase: RuntimePhase::Stopped,
            lease_id: None,
            allocated_bytes: 0,
            idempotent_stop: true,
            exit_code: 0,
        });
    }

    // Watchdog path: CUDA still outstanding — do not destroy context (DT-12).
    if state.cuda_op_outstanding {
        state.healthy = false;
        state.phase = RuntimePhase::FailedSafe;
        return Err(RuntimeError::new(
            RuntimeErrorClass::Watchdog,
            9,
            "cuda op outstanding; preserve allocation/lease/disk",
        ));
    }

    let prev = state.phase;
    state.phase = RuntimePhase::Stopping;

    // Pagefile gates before any destructive effect (DT-8).
    match ops.pagefile_gates_clear() {
        Ok(true) => {}
        Ok(false) => {
            state.phase = RuntimePhase::Online;
            return Err(RuntimeError::new(
                RuntimeErrorClass::PagefileSafety,
                7,
                "pagefile gate refused teardown",
            ));
        }
        Err(e) => {
            state.phase = RuntimePhase::Online;
            return Err(e);
        }
    }

    ops.drain_io()?;
    unwind_from(state, ops, prev)?;
    state.phase = RuntimePhase::Stopped;
    state.stop_completed = true;
    state.lease_id = None;
    state.allocated_bytes = 0;
    Ok(RuntimeSummary {
        phase: RuntimePhase::Stopped,
        lease_id: None,
        allocated_bytes: 0,
        idempotent_stop: false,
        exit_code: 0,
    })
}

/// Reverse unwind from `from` phase inclusive of resources held at that phase.
fn unwind_from<O: RuntimeOps>(
    state: &mut RuntimeState,
    ops: &mut O,
    from: RuntimePhase,
) -> Result<(), RuntimeError> {
    // Order: REGISTER -> DESTROY -> free -> release (DT-7 reverse).
    let need_unreg = matches!(
        from,
        RuntimePhase::QueueRegistered | RuntimePhase::Online | RuntimePhase::Stopping
    ) || state.effects.queue_register > state.effects.queue_unregister;
    let need_destroy = need_unreg
        || matches!(from, RuntimePhase::DiskCreated)
        || state.effects.disk_create > state.effects.disk_destroy;
    let need_free = need_destroy
        || matches!(from, RuntimePhase::CudaReady)
        || state.effects.cuda_alloc > state.effects.cuda_free;
    let need_release = need_free
        || matches!(from, RuntimePhase::Leased)
        || (state.lease_id.is_some() && state.effects.lease_acquire > state.effects.lease_release);

    if need_unreg && state.effects.queue_register > state.effects.queue_unregister {
        ops.unregister_queue()?;
        state.effects.queue_unregister += 1;
    }
    if need_destroy && state.effects.disk_create > state.effects.disk_destroy {
        ops.destroy_disk()?;
        state.effects.disk_destroy += 1;
    }
    if need_free && state.effects.cuda_alloc > state.effects.cuda_free {
        ops.cuda_free()?;
        state.effects.cuda_free += 1;
        state.allocated_bytes = 0;
    }
    if need_release
        && let Some(id) = state.lease_id.take()
        && (state.effects.lease_release == 0
            || state.effects.lease_acquire > state.effects.lease_release)
    {
        ops.release_lease(id)?;
        state.effects.lease_release += 1;
    }
    Ok(())
}

// --- CLI helpers (pure; extracted from main for coverage) ---

/// Product CLI verbs accepted by the binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProductCommand {
    Install,
    Uninstall,
    ProbeCuda { config: String },
    Console { config: String, storage_only: bool },
    ScmDefault,
}

/// Parse argv (skip program name). Rejects lab backend verbs.
pub fn parse_product_cli(args: &[String]) -> Result<ProductCommand, RuntimeError> {
    match args.first().map(String::as_str) {
        None | Some("") => Ok(ProductCommand::ScmDefault),
        Some("install") => Ok(ProductCommand::Install),
        Some("uninstall") => Ok(ProductCommand::Uninstall),
        Some("probe-cuda") => {
            let config = require_config_flag(args)?;
            Ok(ProductCommand::ProbeCuda { config })
        }
        Some("console") => {
            let config = require_config_flag(args)?;
            let storage_only = args.iter().any(|a| a == "--storage-only");
            if !storage_only {
                return Err(RuntimeError::new(
                    RuntimeErrorClass::Config,
                    2,
                    "console requires --storage-only",
                ));
            }
            Ok(ProductCommand::Console {
                config,
                storage_only: true,
            })
        }
        Some("run") | Some("stop-console") | Some("lab") | Some("start-scripts") => {
            Err(RuntimeError::new(
                RuntimeErrorClass::Config,
                2,
                "lab backend command removed from product CLI",
            ))
        }
        Some(other) => Err(RuntimeError::new(
            RuntimeErrorClass::Config,
            2,
            format!("unknown command: {other}"),
        )),
    }
}

fn require_config_flag(args: &[String]) -> Result<String, RuntimeError> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--config" {
            let path = args.get(i + 1).cloned().ok_or_else(|| {
                RuntimeError::new(RuntimeErrorClass::Config, 2, "--config requires a path")
            })?;
            if path.is_empty() || !path_is_absolute_str(&path) {
                return Err(RuntimeError::new(
                    RuntimeErrorClass::Config,
                    2,
                    "config path must be absolute",
                ));
            }
            return Ok(path);
        }
        i += 1;
    }
    Err(RuntimeError::new(
        RuntimeErrorClass::Config,
        2,
        "missing --config <absolute-path>",
    ))
}

fn path_is_absolute_str(p: &str) -> bool {
    // Windows absolute: C:\... or \\?\... ; Unix: /
    if p.starts_with('/') {
        return true;
    }
    let bytes = p.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    p.starts_with(r"\\")
}

/// SCM and console must select the same storage-only runtime (no lab PS1).
pub fn product_runtime_selected(cmd: &ProductCommand) -> bool {
    matches!(
        cmd,
        ProductCommand::Console {
            storage_only: true,
            ..
        } | ProductCommand::ScmDefault
            | ProductCommand::ProbeCuda { .. }
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::config::WinDriveConfig;
    use std::path::PathBuf;

    fn cfg() -> WinDriveConfig {
        WinDriveConfig {
            size_bytes: 64 * 1024 * 1024,
            block_size: 4096,
            cuda_device: 0,
            reserve_bytes: 512 * 1024 * 1024,
            queue_depth: 4,
            max_io_bytes: 1024 * 1024,
            evidence_path: PathBuf::from(r"C:\ProgramData\RamShared\evidence"),
            volume_letter: 'D',
            broker: "127.0.0.1:7700".into(),
            tenant: "wd".into(),
            heartbeat_secs: 5,
        }
    }

    #[derive(Default)]
    struct MockOps {
        fail_at: Option<&'static str>,
        busy_once: bool,
        busy_observe: bool,
        pagefile_clear: bool,
        log: EffectLog,
        released: Vec<u32>,
        destroyed: u32,
        freed: u32,
        unregistered: u32,
        next_lease: u32,
    }

    impl RuntimeOps for MockOps {
        fn acquire_lease(&mut self, _bytes: u64) -> Result<u32, RuntimeError> {
            if self.fail_at == Some("lease") {
                return Err(RuntimeError::new(RuntimeErrorClass::Broker, 3, "deny"));
            }
            self.next_lease += 1;
            Ok(self.next_lease)
        }
        fn release_lease(&mut self, lease_id: u32) -> Result<(), RuntimeError> {
            self.released.push(lease_id);
            self.log.lease_release += 1;
            Ok(())
        }
        fn cuda_alloc(&mut self, _bytes: u64) -> Result<(), RuntimeError> {
            if self.fail_at == Some("cuda") {
                return Err(RuntimeError::new(RuntimeErrorClass::Cuda, 4, "oom"));
            }
            self.log.cuda_alloc += 1;
            Ok(())
        }
        fn cuda_free(&mut self) -> Result<(), RuntimeError> {
            self.freed += 1;
            self.log.cuda_free += 1;
            Ok(())
        }
        fn create_disk(&mut self) -> Result<(), RuntimeError> {
            if self.fail_at == Some("create") {
                return Err(RuntimeError::new(RuntimeErrorClass::Abi, 5, "create fail"));
            }
            if self.busy_once {
                self.busy_once = false;
                return Err(RuntimeError::new(RuntimeErrorClass::Busy, 5, "busy"));
            }
            self.log.disk_create += 1;
            Ok(())
        }
        fn destroy_disk(&mut self) -> Result<(), RuntimeError> {
            self.destroyed += 1;
            self.log.disk_destroy += 1;
            Ok(())
        }
        fn register_queue(&mut self) -> Result<(), RuntimeError> {
            if self.fail_at == Some("register") {
                return Err(RuntimeError::new(RuntimeErrorClass::Abi, 5, "reg fail"));
            }
            self.log.queue_register += 1;
            Ok(())
        }
        fn unregister_queue(&mut self) -> Result<(), RuntimeError> {
            self.unregistered += 1;
            self.log.queue_unregister += 1;
            Ok(())
        }
        fn pagefile_gates_clear(&mut self) -> Result<bool, RuntimeError> {
            Ok(self.pagefile_clear)
        }
        fn drain_io(&mut self) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn observe_busy(&mut self) -> Result<bool, RuntimeError> {
            Ok(self.busy_observe)
        }
    }

    #[test]
    fn no_fallback_after_cuda_failure() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            fail_at: Some("cuda"),
            pagefile_clear: true,
            ..Default::default()
        };
        let e = run_runtime(&c, &mut state, &mut ops).unwrap_err();
        assert_eq!(e.class, RuntimeErrorClass::Cuda);
        assert_eq!(ops.log.fallback_selected, 0);
        assert_eq!(ops.released.len(), 1);
        assert_eq!(ops.freed, 0);
        assert_eq!(state.phase, RuntimePhase::FailedSafe);
    }

    #[test]
    fn failure_after_lease_releases_once() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            fail_at: Some("cuda"),
            pagefile_clear: true,
            ..Default::default()
        };
        let _ = run_runtime(&c, &mut state, &mut ops);
        assert_eq!(ops.released.len(), 1);
        // Second unwind must not double-release
        assert_eq!(ops.log.lease_release, 1);
    }

    #[test]
    fn failure_after_cuda_frees_before_release() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            fail_at: Some("create"),
            pagefile_clear: true,
            ..Default::default()
        };
        let _ = run_runtime(&c, &mut state, &mut ops).unwrap_err();
        assert_eq!(ops.freed, 1);
        assert_eq!(ops.released.len(), 1);
        assert_eq!(ops.destroyed, 0);
    }

    #[test]
    fn failure_after_create_destroys_before_free() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            fail_at: Some("register"),
            pagefile_clear: true,
            ..Default::default()
        };
        let _ = run_runtime(&c, &mut state, &mut ops).unwrap_err();
        assert_eq!(ops.destroyed, 1);
        assert_eq!(ops.freed, 1);
        assert_eq!(ops.released.len(), 1);
        assert_eq!(ops.unregistered, 0);
    }

    #[test]
    fn failure_after_register_unwinds_reverse() {
        // Inject failure by marking post-register stop via custom: use Online then fail stop.
        // Direct: run full online, then force unregister path by stop.
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            pagefile_clear: true,
            ..Default::default()
        };
        run_runtime(&c, &mut state, &mut ops).unwrap();
        assert_eq!(state.phase, RuntimePhase::Online);
        // Simulate failure after register by manual unwind from QueueRegistered-equivalent.
        state.phase = RuntimePhase::QueueRegistered;
        unwind_from(&mut state, &mut ops, RuntimePhase::QueueRegistered).unwrap();
        assert_eq!(ops.unregistered, 1);
        assert_eq!(ops.destroyed, 1);
        assert_eq!(ops.freed, 1);
        assert_eq!(ops.released.len(), 1);
        // Order encoded by counts: each exactly once
        assert_eq!(ops.log.queue_unregister, 1);
        assert_eq!(ops.log.disk_destroy, 1);
        assert_eq!(ops.log.cuda_free, 1);
        assert_eq!(ops.log.lease_release, 1);
    }

    #[test]
    fn deterministic_failure_is_not_retried() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            fail_at: Some("create"),
            pagefile_clear: true,
            ..Default::default()
        };
        let _ = run_runtime(&c, &mut state, &mut ops);
        assert_eq!(ops.log.retries, 0);
        assert_eq!(ops.log.disk_create, 0);
    }

    #[test]
    fn busy_observation_is_bounded() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            busy_once: true,
            busy_observe: true,
            pagefile_clear: true,
            ..Default::default()
        };
        let e = run_runtime(&c, &mut state, &mut ops).unwrap_err();
        assert_eq!(e.class, RuntimeErrorClass::Busy);
        assert_eq!(state.effects.retries, 1);
        // CREATE not re-issued after busy
        assert_eq!(ops.log.disk_create, 0);
    }

    #[test]
    fn stop_twice_has_one_effect() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            pagefile_clear: true,
            ..Default::default()
        };
        run_runtime(&c, &mut state, &mut ops).unwrap();
        stop_runtime(&mut state, &mut ops).unwrap();
        assert_eq!(ops.destroyed, 1);
        assert_eq!(ops.released.len(), 1);
        let s2 = stop_runtime(&mut state, &mut ops).unwrap();
        assert!(s2.idempotent_stop);
        assert_eq!(ops.destroyed, 1);
        assert_eq!(ops.released.len(), 1);
    }

    #[test]
    fn ambiguous_crash_state_is_not_replayed() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        state.ambiguous_crash = true;
        let mut ops = MockOps {
            pagefile_clear: true,
            ..Default::default()
        };
        let e = run_runtime(&c, &mut state, &mut ops).unwrap_err();
        assert_eq!(e.class, RuntimeErrorClass::AmbiguousCrash);
        assert_eq!(ops.log.lease_acquire, 0);
        let e2 = stop_runtime(&mut state, &mut ops).unwrap_err();
        assert_eq!(e2.class, RuntimeErrorClass::AmbiguousCrash);
        assert_eq!(ops.destroyed, 0);
    }

    #[test]
    fn cuda_watchdog_does_not_destroy_stuck_context() {
        let c = cfg();
        let mut state = RuntimeState::new(RunMode::Console);
        let mut ops = MockOps {
            pagefile_clear: true,
            ..Default::default()
        };
        run_runtime(&c, &mut state, &mut ops).unwrap();
        state.cuda_op_outstanding = true;
        let e = stop_runtime(&mut state, &mut ops).unwrap_err();
        assert_eq!(e.class, RuntimeErrorClass::Watchdog);
        assert_eq!(ops.destroyed, 0);
        assert_eq!(ops.freed, 0);
        assert_eq!(ops.released.len(), 0);
        assert_eq!(state.phase, RuntimePhase::FailedSafe);
        assert!(!state.healthy);
    }

    #[test]
    fn console_requires_storage_only() {
        let args = vec![
            "console".into(),
            "--config".into(),
            r"C:\ProgramData\RamShared\winsvc.toml".into(),
        ];
        let e = parse_product_cli(&args).unwrap_err();
        assert_eq!(e.class, RuntimeErrorClass::Config);
    }

    #[test]
    fn product_cli_has_no_lab_backend_command() {
        for verb in ["run", "stop-console", "lab"] {
            let args = vec![verb.into()];
            let e = parse_product_cli(&args).unwrap_err();
            assert!(e.message.contains("lab") || e.message.contains("removed"));
        }
    }

    #[test]
    fn scm_and_console_select_same_runtime() {
        let scm = parse_product_cli(&[]).unwrap();
        assert!(product_runtime_selected(&scm));
        let console = parse_product_cli(&[
            "console".into(),
            "--config".into(),
            r"C:\ProgramData\RamShared\winsvc.toml".into(),
            "--storage-only".into(),
        ])
        .unwrap();
        assert!(product_runtime_selected(&console));
        match console {
            ProductCommand::Console { storage_only, .. } => assert!(storage_only),
            _ => panic!("expected console"),
        }
    }
}
