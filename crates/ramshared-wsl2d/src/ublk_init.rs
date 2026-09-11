//! ublk device initialization module
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::{
    BackendKind, ExactSwapState, UBLK_CONTROL, SHUTDOWN, UblkHandle,
    handle_shutdown, strict_exact_swap_state_from_text, command_stdout_with_timeout,
    lock_memory, signal, SIGINT, SIGTERM, SECTOR, BLOCK_SIZE,
};
use ramshared_wsl2d::{ublk, ublk_control, ublk_server};
use ramshared_wsl2d::backend::RamBackend;
use ramshared_wsl2d::residency::ResidencyConfig;


/// A created ublk device identity. Keeping only these stable values at the
/// daemon boundary prevents the lifecycle core from touching a kernel handle.
#[derive(Clone, Copy)]
struct UblkDevice {
    id: u32,
    queue_depth: u16,
}

trait UblkServer {
    fn join(self: Box<Self>) -> std::io::Result<()>;
}

struct ProductionUblkServer(UblkHandle);

impl UblkServer for ProductionUblkServer {
    fn join(self: Box<Self>) -> std::io::Result<()> {
        self.0.join()
    }
}

/// OS/device edge for the ublk lifecycle. The lifecycle core owns ordering and
/// rollback; the production adapter is the only implementation that opens
/// `/dev/ublk-control` or creates a ublk server.
trait UblkRuntime {
    fn guard_not_wsl2(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn lock_memory(
        &mut self,
        force: bool,
        lock_future: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn install_shutdown_handler(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn add_device(&mut self, queue_depth: u16) -> Result<UblkDevice, Box<dyn std::error::Error>>;
    fn set_params(
        &mut self,
        device: UblkDevice,
        sectors: u64,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn start_server(
        &mut self,
        backend: BackendKind,
        char_path: &str,
        block_path: &str,
        queue_depth: u16,
        size: u64,
    ) -> Result<Box<dyn UblkServer>, Box<dyn std::error::Error>>;
    fn start_device(&mut self, device: UblkDevice) -> Result<(), Box<dyn std::error::Error>>;
    fn wait_for_shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn swap_state(
        &mut self,
        block_path: &str,
    ) -> Result<ExactSwapState, Box<dyn std::error::Error>>;
    fn swapoff(&mut self, block_path: &str) -> Result<(), Box<dyn std::error::Error>>;
    fn stop_device(&mut self, device: UblkDevice) -> Result<(), Box<dyn std::error::Error>>;
    fn delete_device(&mut self, device: UblkDevice) -> Result<(), Box<dyn std::error::Error>>;
}

struct ProductionUblkRuntime;

impl UblkRuntime for ProductionUblkRuntime {
    fn guard_not_wsl2(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        guard_not_wsl2()
    }

    fn lock_memory(
        &mut self,
        force: bool,
        lock_future: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        lock_memory(force, lock_future)
    }

    fn install_shutdown_handler(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            signal(SIGINT, handle_shutdown);
            signal(SIGTERM, handle_shutdown);
        }
        Ok(())
    }

    fn add_device(&mut self, queue_depth: u16) -> Result<UblkDevice, Box<dyn std::error::Error>> {
        let mut spec = ublk_control::DeviceSpec::smoke_auto();
        spec.queue_depth = queue_depth;
        let report = ublk_control::add_device(UBLK_CONTROL, spec)?;
        Ok(UblkDevice {
            id: report.dev_id,
            queue_depth: report.queue_depth,
        })
    }

    fn set_params(
        &mut self,
        device: UblkDevice,
        sectors: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        ublk_control::set_params(
            UBLK_CONTROL,
            device.id,
            ublk::Params::basic_disk(sectors, 12, 12),
        )?;
        Ok(())
    }

    fn start_server(
        &mut self,
        backend: BackendKind,
        char_path: &str,
        block_path: &str,
        queue_depth: u16,
        size: u64,
    ) -> Result<Box<dyn UblkServer>, Box<dyn std::error::Error>> {
        let handle = match backend {
            BackendKind::Vram => {
                UblkHandle::Vram(ublk_server::spawn_server_dt3_vram_with_residency(
                    char_path,
                    queue_depth,
                    BLOCK_SIZE as usize,
                    size as usize,
                    BLOCK_SIZE,
                    block_path.to_string(),
                    ResidencyConfig::default(),
                )?)
            }
            BackendKind::Ram => UblkHandle::Ram(ublk_server::spawn_server_dt3(
                char_path,
                queue_depth,
                BLOCK_SIZE as usize,
                RamBackend::new(size as usize),
            )?),
            BackendKind::Vulkan => {
                return Err("ublk with --backend vulkan not supported (DT-11)".into());
            }
        };
        Ok(Box::new(ProductionUblkServer(handle)))
    }

    fn start_device(&mut self, device: UblkDevice) -> Result<(), Box<dyn std::error::Error>> {
        ublk_control::start_dev(UBLK_CONTROL, device.id, std::process::id())?;
        Ok(())
    }

    fn wait_for_shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        while !SHUTDOWN.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(())
    }

    fn swap_state(
        &mut self,
        block_path: &str,
    ) -> Result<ExactSwapState, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string("/proc/swaps")?;
        Ok(strict_exact_swap_state_from_text(
            &text,
            block_path,
            canonical_ublk_identity,
        )?)
    }

    fn swapoff(&mut self, block_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        command_stdout_with_timeout("swapoff", &["--", block_path], Duration::from_secs(30))
            .ok_or_else(|| format!("swapoff {block_path} failed or exceeded its deadline"))?;
        Ok(())
    }

    fn stop_device(&mut self, device: UblkDevice) -> Result<(), Box<dyn std::error::Error>> {
        ublk_control::stop_dev(UBLK_CONTROL, device.id)?;
        Ok(())
    }

    fn delete_device(&mut self, device: UblkDevice) -> Result<(), Box<dyn std::error::Error>> {
        ublk_control::delete_device(UBLK_CONTROL, device.id)?;
        Ok(())
    }
}

/// ublk path: serves `/dev/ublkbN` directly (io_uring), without socket. The DT-3 worker is the
/// owner of the VRAM/CUDA context and runs the residency (canary §9/§9.4); DEMOTE performs
/// swapoff of the served device itself. The lifecycle goes until SIGINT/SIGTERM.
/// SPEC: docs/ublk-daemon-integration/SPEC.md F2.
pub(crate) fn run_ublk(
    size: u64,
    force: bool,
    queue_depth: u16,
    backend: BackendKind,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = ProductionUblkRuntime;
    run_ublk_with_runtime(size, force, queue_depth, backend, &mut runtime)
}

/// Ordered ublk lifecycle with explicit rollback after every post-create
/// failure. The test runtime is pure in-memory; only ProductionUblkRuntime can
/// touch a kernel device.
fn run_ublk_with_runtime(
    size: u64,
    force: bool,
    queue_depth: u16,
    backend: BackendKind,
    runtime: &mut dyn UblkRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    // DT-11: refusal before guard, memory lock, or device creation.
    if let BackendKind::Vulkan = backend {
        return Err("ublk with --backend vulkan not supported (DT-11: ublk \
             residency server is CUDA-fixed). Use --backend vram (CUDA), or Vulkan \
             via --slices (broker) / --transport nbd."
            .into());
    }
    runtime.guard_not_wsl2()?;
    // MCL_CURRENT only: MCL_FUTURE races dxgkrnl mapping and can hang the host.
    runtime.lock_memory(force, false)?;
    runtime.install_shutdown_handler()?;

    let device = runtime.add_device(queue_depth)?;
    let block_path = format!("/dev/ublkb{}", device.id);
    let sectors = size / SECTOR;
    if let Err(error) = runtime.set_params(device, sectors) {
        prove_ublk_swap_absent(runtime, &block_path)?;
        runtime.delete_device(device)?;
        return Err(error);
    }
    let char_path = format!("/dev/ublkc{}", device.id);
    let server =
        match runtime.start_server(backend, &char_path, &block_path, device.queue_depth, size) {
            Ok(server) => server,
            Err(error) => {
                prove_ublk_swap_absent(runtime, &block_path)?;
                runtime.delete_device(device)?;
                return Err(error);
            }
        };
    if let Err(error) = runtime.start_device(device) {
        prove_ublk_swap_absent(runtime, &block_path)?;
        runtime.stop_device(device)?;
        let _ = server.join();
        prove_ublk_swap_absent(runtime, &block_path)?;
        runtime.delete_device(device)?;
        return Err(error);
    }

    eprintln!(
        "[ramsharedd] ublk device: {block_path} ({} MiB, qd={}, backend={})",
        size >> 20,
        device.queue_depth,
        backend.label()
    );
    eprintln!("[ramsharedd] swapon: sudo swapon {block_path}");
    eprintln!("[ramsharedd] Ctrl-C / SIGTERM to exit");

    runtime.wait_for_shutdown().map_err(|error| {
        format!(
            "recoverable NO-GO while waiting for ublk shutdown ({error}); device and backend preserved"
        )
    })?;
    deactivate_ublk_swap(runtime, &block_path)?;
    prove_ublk_swap_absent(runtime, &block_path)?;
    runtime.stop_device(device)?;
    server.join()?;
    prove_ublk_swap_absent(runtime, &block_path)?;
    runtime.delete_device(device)?;
    eprintln!("[ramsharedd] ublk device removed");
    Ok(())
}

fn prove_ublk_swap_absent(
    runtime: &mut dyn UblkRuntime,
    block_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match runtime.swap_state(block_path)? {
        ExactSwapState::Absent => Ok(()),
        ExactSwapState::Active { used_kb } => Err(format!(
            "refusing ublk stop/delete: {block_path} remains active swap (used_kb={used_kb}); device and backend preserved"
        )
        .into()),
    }
}

fn deactivate_ublk_swap(
    runtime: &mut dyn UblkRuntime,
    block_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(runtime.swap_state(block_path)?, ExactSwapState::Absent) {
        return Ok(());
    }

    let swapoff = runtime.swapoff(block_path);
    if prove_ublk_swap_absent(runtime, block_path).is_err() {
        return Err(format!(
            "ublk swapoff failed or remained active for {block_path}; device and backend preserved"
        )
        .into());
    }
    if let Err(error) = swapoff {
        eprintln!(
            "[ramsharedd] swapoff command for {block_path} reported {error}, but a fresh strict snapshot proves absence"
        );
    }
    Ok(())
}

/// Refuses to serve standalone ublk on WSL2. There is no environment override:
/// teardown of the standalone ublk daemon,
/// if it fails (late SIGTERM -> SIGKILL race, or bug in STOP_DEV/join), leaves
/// `/dev/ublkbN` WITHOUT a server with I/O in flight -> processes in D-state in the
/// writeback/memory path -> the kernel may stop making progress even with the
/// current-page-only memory-lock policy; no WSL override is accepted.
/// This can become a global WSL2 stall (incident 2026-06-09). Validate the complete
/// daemon only in VM/QEMU (`scripts/kernel/qemu-validate.sh`), where a stall is
/// recoverable without dropping the host.
fn guard_not_wsl2() -> Result<(), Box<dyn std::error::Error>> {
    let osrelease = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    ublk_osrelease_guard(&osrelease).map_err(Into::into)
}

/// Pure WSL2 safety policy. Keeping the proc read outside this
/// function makes the refusal matrix testable without inspecting host state.
fn ublk_osrelease_guard(osrelease: &str) -> Result<(), String> {
    let lower = osrelease.to_ascii_lowercase();
    if lower.contains("microsoft") || lower.contains("wsl") {
        return Err(format!(
            "refused: --transport ublk on WSL2 ({}) can freeze the system if daemon teardown \
             fails (orphaned device -> D-state I/O). Validate the daemon in VM/QEMU.",
            osrelease.trim()
        ));
    }
    Ok(())
}

pub(crate) fn canonical_ublk_identity(path: &str) -> Option<String> {
    let trimmed = path.trim();
    let path = trimmed
        .strip_suffix("\\040(deleted)")
        .or_else(|| trimmed.strip_suffix(" (deleted)"))
        .unwrap_or(trimmed);
    let bare = if let Some(bare) = path.strip_prefix("/dev/") {
        bare
    } else if let Some(bare) = path.strip_prefix('/') {
        bare
    } else if !path.contains('/') {
        path
    } else {
        return None;
    };
    let suffix = bare.strip_prefix("ublkb")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!("/dev/{bare}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{MemoryLockStatus, memory_lock_status};

    #[test]
    fn daemon_ublk_runtime_orders_lifecycle_and_rolls_back_without_device() {
        #[derive(Default)]
        struct FakeServer {
            joined: bool,
        }

        impl UblkServer for FakeServer {
            fn join(mut self: Box<Self>) -> std::io::Result<()> {
                self.joined = true;
                Ok(())
            }
        }

        #[derive(Default)]
        struct FakeRuntime {
            calls: Vec<&'static str>,
        }

        impl UblkRuntime for FakeRuntime {
            fn guard_not_wsl2(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.calls.push("guard");
                Ok(())
            }

            fn lock_memory(
                &mut self,
                _force: bool,
                lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert!(!lock_future, "ublk keeps MCL_FUTURE disabled");
                self.calls.push("lock");
                Ok(())
            }

            fn install_shutdown_handler(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.calls.push("signal");
                Ok(())
            }

            fn add_device(
                &mut self,
                queue_depth: u16,
            ) -> Result<UblkDevice, Box<dyn std::error::Error>> {
                assert_eq!(queue_depth, 3);
                self.calls.push("add");
                Ok(UblkDevice {
                    id: 41,
                    queue_depth,
                })
            }

            fn set_params(
                &mut self,
                device: UblkDevice,
                sectors: u64,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert_eq!(device.id, 41);
                assert_eq!(sectors, 16);
                self.calls.push("params");
                Ok(())
            }

            fn start_server(
                &mut self,
                backend: BackendKind,
                char_path: &str,
                block_path: &str,
                queue_depth: u16,
                size: u64,
            ) -> Result<Box<dyn UblkServer>, Box<dyn std::error::Error>> {
                assert!(matches!(backend, BackendKind::Ram));
                assert_eq!(char_path, "/dev/ublkc41");
                assert_eq!(block_path, "/dev/ublkb41");
                assert_eq!(queue_depth, 3);
                assert_eq!(size, 8192);
                self.calls.push("server");
                Ok(Box::<FakeServer>::default())
            }

            fn start_device(
                &mut self,
                device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert_eq!(device.id, 41);
                self.calls.push("start");
                Ok(())
            }

            fn wait_for_shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.calls.push("wait");
                Ok(())
            }

            fn swap_state(
                &mut self,
                block_path: &str,
            ) -> Result<ExactSwapState, Box<dyn std::error::Error>> {
                assert_eq!(block_path, "/dev/ublkb41");
                self.calls.push("swap-state");
                Ok(ExactSwapState::Absent)
            }

            fn swapoff(&mut self, _block_path: &str) -> Result<(), Box<dyn std::error::Error>> {
                panic!("swapoff must not run when strict snapshots prove absence")
            }

            fn stop_device(
                &mut self,
                device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert_eq!(device.id, 41);
                self.calls.push("stop");
                Ok(())
            }

            fn delete_device(
                &mut self,
                device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert_eq!(device.id, 41);
                self.calls.push("delete");
                Ok(())
            }
        }

        let mut runtime = FakeRuntime::default();
        run_ublk_with_runtime(8192, false, 3, BackendKind::Ram, &mut runtime)
            .unwrap_or_else(|_| panic!("injected RAM ublk lifecycle"));
        assert_eq!(
            runtime.calls,
            vec![
                "guard",
                "lock",
                "signal",
                "add",
                "params",
                "server",
                "start",
                "wait",
                "swap-state",
                "swap-state",
                "stop",
                "swap-state",
                "delete",
            ]
        );

        let mut refusal_runtime = FakeRuntime::default();
        assert!(
            run_ublk_with_runtime(8192, false, 3, BackendKind::Vulkan, &mut refusal_runtime)
                .is_err()
        );
        assert!(
            refusal_runtime.calls.is_empty(),
            "Vulkan refusal must occur before a ublk runtime operation"
        );
    }

    #[test]
    fn daemon_ublk_runtime_failures_delete_only_after_fresh_absence_proof() {
        #[derive(Clone, Copy)]
        enum Failure {
            Params,
            Server,
            Start,
            Wait,
        }

        struct Server(std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>);

        impl UblkServer for Server {
            fn join(self: Box<Self>) -> std::io::Result<()> {
                self.0.lock().unwrap_or_else(|_| panic!("test call log")).push("join");
                Ok(())
            }
        }

        struct Runtime {
            failure: Failure,
            calls: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
        }

        impl Runtime {
            fn mark(&self, call: &'static str) {
                self.calls.lock().unwrap_or_else(|_| panic!("test call log")).push(call);
            }

            fn fail(&self, stage: Failure) -> bool {
                std::mem::discriminant(&self.failure) == std::mem::discriminant(&stage)
            }
        }

        impl UblkRuntime for Runtime {
            fn guard_not_wsl2(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("guard");
                Ok(())
            }

            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("lock");
                Ok(())
            }

            fn install_shutdown_handler(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("signal");
                Ok(())
            }

            fn add_device(
                &mut self,
                queue_depth: u16,
            ) -> Result<UblkDevice, Box<dyn std::error::Error>> {
                self.mark("add");
                Ok(UblkDevice { id: 9, queue_depth })
            }

            fn set_params(
                &mut self,
                _device: UblkDevice,
                _sectors: u64,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("params");
                if self.fail(Failure::Params) {
                    Err(std::io::Error::other("params failure").into())
                } else {
                    Ok(())
                }
            }

            fn start_server(
                &mut self,
                _backend: BackendKind,
                _char_path: &str,
                _block_path: &str,
                _queue_depth: u16,
                _size: u64,
            ) -> Result<Box<dyn UblkServer>, Box<dyn std::error::Error>> {
                self.mark("server");
                if self.fail(Failure::Server) {
                    Err(std::io::Error::other("server failure").into())
                } else {
                    Ok(Box::new(Server(std::sync::Arc::clone(&self.calls))))
                }
            }

            fn start_device(
                &mut self,
                _device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("start");
                if self.fail(Failure::Start) {
                    Err(std::io::Error::other("start failure").into())
                } else {
                    Ok(())
                }
            }

            fn wait_for_shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("wait");
                if self.fail(Failure::Wait) {
                    Err(std::io::Error::other("wait failure").into())
                } else {
                    Ok(())
                }
            }

            fn swap_state(
                &mut self,
                _block_path: &str,
            ) -> Result<ExactSwapState, Box<dyn std::error::Error>> {
                self.mark("swap-state");
                Ok(ExactSwapState::Absent)
            }

            fn swapoff(&mut self, _block_path: &str) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("swapoff");
                Ok(())
            }

            fn stop_device(
                &mut self,
                _device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("stop");
                Ok(())
            }

            fn delete_device(
                &mut self,
                _device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("delete");
                Ok(())
            }
        }

        for (failure, expected) in [
            (
                Failure::Params,
                vec![
                    "guard",
                    "lock",
                    "signal",
                    "add",
                    "params",
                    "swap-state",
                    "delete",
                ],
            ),
            (
                Failure::Server,
                vec![
                    "guard",
                    "lock",
                    "signal",
                    "add",
                    "params",
                    "server",
                    "swap-state",
                    "delete",
                ],
            ),
            (
                Failure::Start,
                vec![
                    "guard",
                    "lock",
                    "signal",
                    "add",
                    "params",
                    "server",
                    "start",
                    "swap-state",
                    "stop",
                    "join",
                    "swap-state",
                    "delete",
                ],
            ),
            (
                Failure::Wait,
                vec![
                    "guard", "lock", "signal", "add", "params", "server", "start", "wait",
                ],
            ),
        ] {
            let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let mut runtime = Runtime {
                failure,
                calls: std::sync::Arc::clone(&calls),
            };
            assert!(run_ublk_with_runtime(4096, false, 1, BackendKind::Ram, &mut runtime).is_err());
            assert_eq!(*calls.lock().unwrap_or_else(|_| panic!("test call log")), expected);
        }
    }

    #[test]
    fn daemon_ublk_shutdown_is_swapoff_first_and_preserves_on_uncertainty() {
        struct Server(std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>);

        impl UblkServer for Server {
            fn join(self: Box<Self>) -> std::io::Result<()> {
                self.0.lock().unwrap_or_else(|_| panic!("test call log")).push("join");
                Ok(())
            }
        }

        struct Runtime {
            calls: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
            states: std::collections::VecDeque<Result<ExactSwapState, &'static str>>,
            swapoff_fails: bool,
        }

        impl Runtime {
            fn new(
                states: impl IntoIterator<Item = Result<ExactSwapState, &'static str>>,
                swapoff_fails: bool,
            ) -> Self {
                Self {
                    calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                    states: states.into_iter().collect(),
                    swapoff_fails,
                }
            }

            fn mark(&self, call: &'static str) {
                self.calls.lock().unwrap_or_else(|_| panic!("test call log")).push(call);
            }

            fn calls(&self) -> Vec<&'static str> {
                self.calls.lock().unwrap_or_else(|_| panic!("test call log")).clone()
            }
        }

        impl UblkRuntime for Runtime {
            fn guard_not_wsl2(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("guard");
                Ok(())
            }

            fn lock_memory(
                &mut self,
                _force: bool,
                lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert!(!lock_future);
                self.mark("lock");
                Ok(())
            }

            fn install_shutdown_handler(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("signal");
                Ok(())
            }

            fn add_device(
                &mut self,
                queue_depth: u16,
            ) -> Result<UblkDevice, Box<dyn std::error::Error>> {
                self.mark("add");
                Ok(UblkDevice { id: 7, queue_depth })
            }

            fn set_params(
                &mut self,
                _device: UblkDevice,
                _sectors: u64,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("params");
                Ok(())
            }

            fn start_server(
                &mut self,
                _backend: BackendKind,
                _char_path: &str,
                _block_path: &str,
                _queue_depth: u16,
                _size: u64,
            ) -> Result<Box<dyn UblkServer>, Box<dyn std::error::Error>> {
                self.mark("server");
                Ok(Box::new(Server(std::sync::Arc::clone(&self.calls))))
            }

            fn start_device(
                &mut self,
                _device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("start");
                Ok(())
            }

            fn wait_for_shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("wait");
                Ok(())
            }

            fn swap_state(
                &mut self,
                _block_path: &str,
            ) -> Result<ExactSwapState, Box<dyn std::error::Error>> {
                self.mark("swap-state");
                match self.states.pop_front().unwrap_or_else(|| panic!("planned strict snapshot")) {
                    Ok(state) => Ok(state),
                    Err(error) => Err(std::io::Error::other(error).into()),
                }
            }

            fn swapoff(&mut self, _block_path: &str) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("swapoff");
                if self.swapoff_fails {
                    Err(std::io::Error::other("injected swapoff failure").into())
                } else {
                    Ok(())
                }
            }

            fn stop_device(
                &mut self,
                _device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("stop");
                Ok(())
            }

            fn delete_device(
                &mut self,
                _device: UblkDevice,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.mark("delete");
                Ok(())
            }
        }

        let prefix = vec![
            "guard", "lock", "signal", "add", "params", "server", "start", "wait",
        ];

        let mut active_zero = Runtime::new(
            [
                Ok(ExactSwapState::Active { used_kb: 0 }),
                Ok(ExactSwapState::Active { used_kb: 0 }),
            ],
            true,
        );
        assert!(run_ublk_with_runtime(4096, false, 1, BackendKind::Ram, &mut active_zero).is_err());
        let mut expected = prefix.clone();
        expected.extend(["swap-state", "swapoff", "swap-state"]);
        assert_eq!(active_zero.calls(), expected);

        let mut active_used = Runtime::new(
            [
                Ok(ExactSwapState::Active { used_kb: 12 }),
                Ok(ExactSwapState::Absent),
                Ok(ExactSwapState::Absent),
                Ok(ExactSwapState::Absent),
            ],
            false,
        );
        run_ublk_with_runtime(4096, false, 1, BackendKind::Ram, &mut active_used)
            .unwrap_or_else(|_| panic!("swapoff-first shutdown with fresh absence proofs"));
        let mut expected = prefix.clone();
        expected.extend([
            "swap-state",
            "swapoff",
            "swap-state",
            "swap-state",
            "stop",
            "join",
            "swap-state",
            "delete",
        ]);
        assert_eq!(active_used.calls(), expected);

        let mut unreadable = Runtime::new([Err("unreadable /proc/swaps")], false);
        assert!(run_ublk_with_runtime(4096, false, 1, BackendKind::Ram, &mut unreadable).is_err());
        let mut expected = prefix;
        expected.push("swap-state");
        assert_eq!(unreadable.calls(), expected);
    }

    #[test]
    fn daemon_ublk_wsl_guard_and_memory_lock_policy_are_pure_and_fail_closed() {
        assert!(ublk_osrelease_guard("6.6.0-microsoft-standard-WSL2").is_err());
        assert!(ublk_osrelease_guard("6.6.0-wsl").is_err());
        assert!(ublk_osrelease_guard("6.8.0-generic").is_ok());
        assert!(
            ublk_osrelease_guard("6.6.0-microsoft-standard-WSL2").is_err(),
            "the dangerous WSL2 override must not exist"
        );

        assert!(matches!(
            memory_lock_status(true, true, false),
            Ok(MemoryLockStatus::Protected)
        ));
        assert!(memory_lock_status(false, true, false).is_err());
        assert!(memory_lock_status(true, false, false).is_err());
        assert!(matches!(
            memory_lock_status(false, false, true),
            Ok(MemoryLockStatus::ForcedDegraded {
                locked: false,
                oom_ok: false
            })
        ));
        assert!(matches!(
            memory_lock_status(true, false, true),
            Ok(MemoryLockStatus::ForcedDegraded {
                locked: true,
                oom_ok: false
            })
        ));
    }
}
