//! ramsharedd (crate `ramshared-wsl2d`) — VRAM tier daemon + Memory Broker (SPEC §4, §8).
//!
//! Serves fixed-newstyle NBD on a Unix socket; `nbd-client -unix <sock> /dev/nbdX`
//! wires up the kernel (the ioctls). The daemon contains narrowly scoped direct FFI
//! for `mlockall` and SIGINT/SIGTERM registration; the signal handler only performs
//! an atomic store, while CUDA-specific unsafe remains isolated in `ramshared-cuda`.
//!
//! Allocates VRAM and serves **N NBD connections** (`nbd-client -C N`) via a dedicated
//! reader/writer per connection + a **single CUDA worker** (thread affinity, §9.4/H1), with
//! `mlockall`+`oom_score_adj` (Discipline 3) and the residency canary §9 (latency
//! per-request, **serve-only**) + §9.4 (content/free probe).
//! Backoff remains as future work.

use core::ffi::c_int;
use std::fs::File;
use std::io::{Read, Seek};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ramshared_block::protocol::{
    NBD_FLAG_CAN_MULTI_CONN, NBD_FLAG_HAS_FLAGS, NBD_FLAG_SEND_FLUSH, NBD_FLAG_SEND_FUA,
};
use ramshared_block::{
    AuthoritativeOriginBackend, BlockBackend, CacheState as OriginCacheState, Command,
    CommitBudgetGate, DisabledCache, FileOrigin, OriginState as DurableOriginState,
    SparseVramBackend, WriteOptions, chunk_bytes_from_env, commit_cap_bytes_from_env,
    idle_free_secs_from_env, reserve_floor_bytes_from_env, safe_commit_cap, serve,
};
#[cfg(test)]
use ramshared_block::{GpuSample, WriteThroughCacheBackend};
use ramshared_broker::arbiter::ArbiterConfig;
use ramshared_broker::slices::SliceMap;
use ramshared_cuda::Cuda;
use ramshared_dxg::{DxgBudgetProvider, GpuBudgetProvider};
use ramshared_vram::{VramMemory, VramProvider};
use ramshared_vulkan::VulkanProvider;
use ramshared_wsl2d::autotier::{
    AutotierConfig, BudgetInput, RecoveryTracker, backend_release_allowed, commit_allowed,
};
use ramshared_wsl2d::broker_srv::{BrokerConfig, EndpointCfg, spawn_broker};
use ramshared_wsl2d::swap::{spawn_activate_swap, spawn_swapoff};
use ramshared_wsl2d::{
    CANARY_BYTES, CANARY_EVERY, CHAN_CAP, Cadence, Canary, CanaryProbe, DemoteReason, LiveCount,
    RamBackend, Reply, ResidencyConfig, ResidencySampler, SliceIoCounters, SliceView, Verdict,
    VramBackend, VramGauge, WMsg, spawn_acceptor,
};
use ramshared_wsl2d::{ublk, ublk_control, ublk_server};

// Discipline 3 (anti-deadlock): the daemon serves swap, so it cannot be swapped out.
unsafe extern "C" {
    fn mlockall(flags: c_int) -> c_int;
    // Signal handler registration (sighandler_t is a function pointer; the previous
    // return is ignored). Used only for SIGINT/SIGTERM in ublk mode.
    fn signal(signum: c_int, handler: extern "C" fn(c_int)) -> usize;
    #[link_name = "kill"]
    fn kill_process_group_raw(pid: c_int, signal: c_int) -> c_int;
}
const MCL_CURRENT: c_int = 1;
const SIGINT: c_int = 2;
const SIGTERM: c_int = 15;
const SIGKILL: c_int = 9;
const COMMAND_FATAL_EXIT_CODE: i32 = 125;
const COMMAND_OUTPUT_LIMIT: usize = 256 * 1024;
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const COMMAND_REAP_GRACE: Duration = Duration::from_millis(500);
const COMMAND_CAPTURE_GRACE: Duration = Duration::from_millis(500);

const GIB: u64 = 1024 * 1024 * 1024;
const DEFAULT_SIZE: u64 = 256 * 1024 * 1024;
const DEFAULT_ORIGIN_SIZE: u64 = 4 * GIB;
const MIN_ORIGIN_LOGICAL_SIZE: u64 = GIB;
const MAX_ORIGIN_LOGICAL_SIZE: u64 = 24 * GIB;
const BLOCK_SIZE: u32 = 4096;
const UBLK_CONTROL: &str = "/dev/ublk-control";
const CACHE_TARGET_REQUEST_PATH: &str = "/run/ramshared/cache-target.json";
const RECLAIM_REQUEST_PATH: &str = "/run/ramshared/reclaim-request.json";
const CONTROL_REQUEST_MAX_AGE_MS: u64 = 15_000;
const ORIGIN_MANIFEST_PATH: &str = "/etc/ramshared/origin.conf";
const HOST_ORIGIN_MANIFEST_PATH: &str =
    "/mnt/c/ProgramData/RamShared/ramshared-origin-manifest.json";
const ORIGIN_MANIFEST_MAX_BYTES: u64 = 64 * 1024;

const SECTOR: u64 = 512;

/// VRAM tier transport: NBD (Unix socket) or ublk (direct block device).
#[derive(Clone, Copy)]
enum Transport {
    Nbd,
    Ublk,
}

/// VRAM/tier backend: `Vram` (CUDA, with residency §9/§9.4), `Vulkan` (any GPU via
/// `ramshared-vulkan`, RF-G2) or `Ram` (without GPU). `Ram` exists to validate the **lifecycle/teardown**
/// of the ublk daemon in **QEMU** (where there is no GPU); the teardown bug that hung
/// WSL2 is independent of the backend. `Vulkan` covers broker + NBD single (generic paths); ublk
/// with Vulkan is deferred (DT-11: the ublk residency server is CUDA-fixed).
#[derive(Clone, Copy)]
enum BackendKind {
    Vram,
    Vulkan,
    Ram,
}

struct UnavailableVramProvider;
struct UnavailableVramMemory;

impl VramMemory for UnavailableVramMemory {
    fn len(&self) -> usize {
        0
    }

    fn zero(&mut self) -> Result<(), ramshared_vram::VramError> {
        Err(ramshared_vram::VramError::Provider(
            "VRAM cache is unavailable".into(),
        ))
    }

    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), ramshared_vram::VramError> {
        Err(ramshared_vram::VramError::OutOfRange {
            off,
            len: dst.len() as u64,
            size: 0,
        })
    }

    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), ramshared_vram::VramError> {
        Err(ramshared_vram::VramError::OutOfRange {
            off,
            len: src.len() as u64,
            size: 0,
        })
    }
}

impl VramProvider for UnavailableVramProvider {
    type Mem<'a> = UnavailableVramMemory;

    fn alloc(&self, _bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
        Err(ramshared_vram::VramError::Provider(
            "VRAM cache is unavailable".into(),
        ))
    }

    fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
        Err(ramshared_vram::VramError::Provider(
            "GPU measurement is unavailable".into(),
        ))
    }
}

impl BackendKind {
    fn label(self) -> &'static str {
        match self {
            BackendKind::Vram => "vram",
            BackendKind::Vulkan => "vulkan",
            BackendKind::Ram => "ram",
        }
    }
}

/// Unifies the two types of DT-3 server handles (VRAM-with-residency or pure RAM) for
/// a unified teardown in `run_ublk`.
enum UblkHandle {
    Vram(ublk_server::ServerHandleDt3VramResidency),
    Ram(ublk_server::ServerHandleDt3<RamBackend>),
}

impl UblkHandle {
    fn join(self) -> std::io::Result<()> {
        match self {
            UblkHandle::Vram(h) => h.join(),
            UblkHandle::Ram(h) => h.join().map(|_| ()),
        }
    }
}

/// Shutdown request (SIGINT/SIGTERM). The handler only does an atomic store
/// (async-signal-safe); the ublk daemon loop polls this flag.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_shutdown(_sig: c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[ramsharedd] error: {e}");
            std::process::ExitCode::from(1)
        }
    }
}

fn documented_private_listener_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            a == 127
                || a == 10
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 100 && (64..=127).contains(&b))
        }
        std::net::IpAddr::V6(ip) => ip.is_loopback() || ip.octets()[0] & 0xfe == 0xfc,
    }
}

/// Parses `IP:PORT` (accepts `tcp://`) and permits only loopback, RFC1918
/// IPv4, IPv6 ULA, and the exact Tailscale CGNAT 100.64.0.0/10 range.
fn parse_private_listen(s: &str) -> Result<std::net::SocketAddr, String> {
    let raw = s.strip_prefix("tcp://").unwrap_or(s);
    let addr: std::net::SocketAddr = raw
        .parse()
        .map_err(|_| format!("invalid address '{s}' (use IP:PORT)"))?;
    if !documented_private_listener_ip(addr.ip()) {
        return Err(format!(
            "bind on {} refused — RNF-2 permits only loopback, RFC1918, IPv6 ULA, or Tailscale 100.64.0.0/10",
            addr.ip()
        ));
    }
    Ok(addr)
}

/// Validates the combo of slice flags (DT-3: ublk is single-device in WSL2; `--slice-mb` mandatory).
/// Slices ceiling: `StatusReply` embeds `Vec<Slice>+Vec<SliceIo>+Vec<TenantStatus>` in a single
/// JSON line; above ~430 slices it exceeds the protocol's `MAX_LINE_BYTES` (64 KiB) and the other
/// end rejects the line (ADR-0005). 256 gives margins (~38 KiB) and covers any real use case.
const MAX_SLICES: u16 = 256;

fn validate_slice_flags(slices: u16, slice_mb: u64, is_ublk: bool) -> Result<(), String> {
    if slices > 0 && is_ublk {
        return Err(
            "--slices does not combine with --transport ublk (DT-3: ublk single-device on WSL2)"
                .into(),
        );
    }
    if slices > 0 && slice_mb == 0 {
        return Err("--slices > 0 requires --slice-mb N".into());
    }
    if slices > MAX_SLICES {
        return Err(format!(
            "--slices {slices} > {MAX_SLICES}: StatusReply would exceed the protocol line ceiling \
             (MAX_LINE_BYTES 64 KiB, ADR-0005)"
        ));
    }
    Ok(())
}

/// Zeroes the `[base, base+len)` window of the backend in 1 MiB chunks (slice hygiene, DT-17).
/// Runs on the thread owning the backend (single CUDA worker) — `WMsg::ZeroExport`.
fn zero_window<B: BlockBackend>(
    backend: &mut B,
    base: u64,
    len: u64,
) -> Result<(), ramshared_block::IoError> {
    const CHUNK: usize = 1 << 20;
    let buf = vec![0u8; CHUNK.min(len as usize)];
    let mut off = 0u64;
    while off < len {
        let n = ((len - off) as usize).min(buf.len());
        backend.write_at(base + off, &buf[..n])?;
        off += n as u64;
    }
    Ok(())
}

/// Per-request residency shared by NBD workers (single and broker): arms the latency
/// canary (§9, baseline→Canary; serve-only, DT-16) and runs the §9.4 probe (content/free in
/// cadence, with hysteresis via streak). Returns `Some(reason)` if any signal requests DEMOTE;
/// the caller decides the ACTION (local swapoff in single, `DemoteAll` via broker in multi-slice).
struct ResidencyCheckState<'a, M: VramMemory> {
    canary: &'a mut Option<Canary>,
    baseline: &'a mut Vec<u64>,
    sampler: &'a mut ResidencySampler,
    cadence: &'a mut Cadence,
    probe: &'a mut CanaryProbe<M>,
    free_floor_bytes: u64,
}

fn residency_check<M: VramMemory, F: Fn() -> Option<u64>>(
    lat_us: u64,
    state: &mut ResidencyCheckState<'_, M>,
    mem_free: F,
) -> Option<DemoteReason> {
    // §9: per-request latency canary. content_ok=true/free=u64::MAX ON PURPOSE — the signal
    // here is latency; content and free-floor come from the probe §9.4 below.
    let mut latency_reason = None;
    match state.canary.as_mut() {
        None => {
            state.baseline.push(lat_us);
            if state.baseline.len() >= 16 {
                state.baseline.sort_unstable();
                let med = state.baseline[state.baseline.len() / 2].max(1);
                *state.canary = Some(Canary::new(ResidencyConfig::default(), med));
                eprintln!("[ramsharedd] canario armado (baseline={med} us)");
            }
        }
        Some(c) => {
            if let Verdict::Demote(reason) = c.sample(lat_us, true, u64::MAX) {
                latency_reason = Some(reason);
            }
        }
    }
    // §9.4: dedicated content/free probe in cadence (corrupted content demotes immediately;
    // free-floor/transient error require streak).
    let mut probe_reason = None;
    if state.cadence.tick() {
        let content = state.probe.check_content().ok();
        let free = mem_free();
        let verdict = state.sampler.sample(content, free);
        let streak = state.sampler.bad_streak();
        let trace_probe = std::env::var("RAMSHARED_TRACE_PROBE").ok().as_deref() == Some("1");
        if should_log_probe_sample(content, free, state.free_floor_bytes, streak, trace_probe) {
            eprintln!(
                "[ramsharedd] sonda §9.4 sample: content={content:?} free={free:?} \
                 floor={} streak={streak}",
                state.free_floor_bytes
            );
        }
        if let Verdict::Demote(reason) = verdict {
            eprintln!(
                "[ramsharedd] sonda §9.4: content={content:?} free={free:?} streak={}",
                streak
            );
            probe_reason = Some(reason);
        }
    }
    choose_residency_reason(latency_reason, probe_reason)
}

fn choose_residency_reason(
    latency: Option<DemoteReason>,
    probe: Option<DemoteReason>,
) -> Option<DemoteReason> {
    probe.or(latency)
}

fn should_log_probe_sample(
    content: Option<bool>,
    free: Option<u64>,
    free_floor_bytes: u64,
    streak: u32,
    trace_probe: bool,
) -> bool {
    trace_probe
        || content != Some(true)
        || free.is_none()
        || free.is_some_and(|f| f < free_floor_bytes.saturating_mul(2))
        || streak > 0
}

fn sparse_residency_config(reserve_floor_bytes: u64) -> ResidencyConfig {
    ResidencyConfig {
        free_floor_bytes: reserve_floor_bytes,
        ..ResidencyConfig::default()
    }
}

fn sparse_residency_requests_swapoff(reason: DemoteReason) -> bool {
    !matches!(reason, DemoteReason::Latency)
}

fn parse_nvidia_smi_free_bytes(output: &str) -> Option<u64> {
    let first = output.lines().find(|line| !line.trim().is_empty())?.trim();
    let token = first
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .find(|part| !part.is_empty())?;
    let mib = token.parse::<u64>().ok()?;
    mib.checked_mul(1024 * 1024)
}

trait CommandFatalContainment {
    fn contain(&self, detail: &str);
}

struct ExitDaemon;

impl CommandFatalContainment for ExitDaemon {
    fn contain(&self, detail: &str) {
        eprintln!("ramsharedd fatal subprocess containment: {detail}");
        std::process::exit(COMMAND_FATAL_EXIT_CODE);
    }
}

trait CommandReapTarget {
    fn id(&self) -> u32;
    fn kill_group(&mut self) -> std::io::Result<()>;
    fn kill_direct(&mut self) -> std::io::Result<()>;
    fn observe_exit(&mut self) -> std::io::Result<bool>;
    fn reap_observed(&mut self) -> std::io::Result<Option<ExitStatus>>;
}

impl CommandReapTarget for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }

    fn kill_group(&mut self) -> std::io::Result<()> {
        let pid = c_int::try_from(self.id()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "child PID overflow")
        })?;
        // SAFETY: `pid` is the positive PID of the direct child that this
        // controller spawned as a new process-group leader. Negating it asks
        // POSIX `kill(2)` to signal exactly that owned process group.
        if unsafe { kill_process_group_raw(-pid, SIGKILL) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    fn kill_direct(&mut self) -> std::io::Result<()> {
        self.kill()
    }

    fn observe_exit(&mut self) -> std::io::Result<bool> {
        let raw = i32::try_from(self.id()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "child PID overflow")
        })?;
        let pid = rustix::process::Pid::from_raw(raw).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "zero child PID")
        })?;
        rustix::process::waitid(
            rustix::process::WaitId::Pid(pid),
            rustix::process::WaitIdOptions::EXITED
                | rustix::process::WaitIdOptions::NOHANG
                | rustix::process::WaitIdOptions::NOWAIT,
        )
        .map(|status| status.is_some())
        .map_err(std::io::Error::from)
    }

    fn reap_observed(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.try_wait()
    }
}

fn wait_for_command_exit_observation(
    target: &mut dyn CommandReapTarget,
    label: &str,
    grace: Duration,
    fatal: &dyn CommandFatalContainment,
) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        match target.observe_exit() {
            Ok(true) => return true,
            Ok(false) if Instant::now() < deadline => {
                std::thread::sleep(COMMAND_POLL_INTERVAL);
            }
            Ok(false) => {
                fatal.contain(&format!(
                    "{label}: process-group SIGKILL did not produce an observable direct-child exit {} within {} ms",
                    target.id(),
                    grace.as_millis()
                ));
                return false;
            }
            Err(error) => {
                fatal.contain(&format!(
                    "{label}: direct-child exit observation failed after SIGKILL: {error}"
                ));
                return false;
            }
        }
    }
}

fn reap_observed_command(
    target: &mut dyn CommandReapTarget,
    label: &str,
    grace: Duration,
    fatal: &dyn CommandFatalContainment,
) -> Option<ExitStatus> {
    let deadline = Instant::now() + grace;
    loop {
        match target.reap_observed() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(COMMAND_POLL_INTERVAL);
            }
            Ok(None) => {
                fatal.contain(&format!(
                    "{label}: observed direct child {} was not reaped within {} ms",
                    target.id(),
                    grace.as_millis()
                ));
                return None;
            }
            Err(error) => {
                fatal.contain(&format!("{label}: final direct-child reap failed: {error}"));
                return None;
            }
        }
    }
}

fn force_command_exit_observed(
    target: &mut dyn CommandReapTarget,
    label: &str,
    grace: Duration,
    fatal: &dyn CommandFatalContainment,
) -> Option<Vec<String>> {
    let mut errors = Vec::new();
    if let Err(error) = target.kill_group() {
        if error.raw_os_error() != Some(3) {
            errors.push(format!(
                "{label}: exact process-group SIGKILL before exit observation failed: {error}"
            ));
        }
        let _ = target.kill_direct();
    }
    if !wait_for_command_exit_observation(target, label, grace, fatal) {
        return None;
    }
    if let Err(error) = target.kill_group()
        && error.raw_os_error() != Some(3)
    {
        errors.push(format!(
            "{label}: exact process-group SIGKILL while the zombie leader pinned the group failed: {error}"
        ));
    }
    Some(errors)
}

fn contain_command_group_errors(errors: Vec<String>, fatal: &dyn CommandFatalContainment) -> bool {
    if errors.is_empty() {
        true
    } else {
        fatal.contain(&errors.join("; "));
        false
    }
}

fn terminate_command_target_with(
    target: &mut dyn CommandReapTarget,
    label: &str,
    grace: Duration,
    fatal: &dyn CommandFatalContainment,
) -> bool {
    let Some(errors) = force_command_exit_observed(target, label, grace, fatal) else {
        return false;
    };
    if reap_observed_command(target, label, grace, fatal).is_none() {
        return false;
    }
    contain_command_group_errors(errors, fatal)
}

fn terminate_command_child(child: &mut Child, label: &str) -> bool {
    terminate_command_target_with(child, label, COMMAND_REAP_GRACE, &ExitDaemon)
}

struct CommandCapture {
    receiver: std::sync::mpsc::Receiver<Result<Vec<u8>, ()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

enum CommandCaptureState {
    Ready(Result<Vec<u8>, ()>),
    TimedOut,
    Disconnected,
}

impl CommandCapture {
    fn spawn(mut stdout: impl Read + Send + 'static) -> std::io::Result<Self> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("ramshared-command-capture".into())
            .spawn(move || {
                let mut output = Vec::new();
                let mut buffer = [0_u8; 4096];
                let result = loop {
                    match stdout.read(&mut buffer) {
                        Ok(0) => break Ok(output),
                        Ok(read) if output.len().saturating_add(read) <= COMMAND_OUTPUT_LIMIT => {
                            output.extend_from_slice(&buffer[..read]);
                        }
                        Ok(_) | Err(_) => break Err(()),
                    }
                };
                let _ = sender.send(result);
            })?;
        Ok(Self {
            receiver,
            worker: Some(worker),
        })
    }

    fn receive_until(&self, deadline: Instant) -> CommandCaptureState {
        match self
            .receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(output) => CommandCaptureState::Ready(output),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => CommandCaptureState::TimedOut,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                CommandCaptureState::Disconnected
            }
        }
    }

    fn join(&mut self) -> bool {
        self.worker
            .take()
            .is_some_and(|worker| worker.join().is_ok())
    }
}

fn signal_owned_command_group_before_reap(
    group_id: u32,
    label: &str,
    fatal: &dyn CommandFatalContainment,
) -> bool {
    let Ok(pid) = c_int::try_from(group_id) else {
        fatal.contain(&format!(
            "{label}: process-group ID overflow before direct-child reap"
        ));
        return false;
    };
    // SAFETY: the group ID is retained from the exact direct child created by
    // this command runner, and that child was configured as the group leader.
    if unsafe { kill_process_group_raw(-pid, SIGKILL) } == 0 {
        return true;
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(3) {
        true
    } else {
        fatal.contain(&format!(
            "{label}: inherited output stayed open and group SIGKILL failed: {error}"
        ));
        false
    }
}

fn finish_command_capture(
    group_id: u32,
    label: &str,
    capture: &mut CommandCapture,
) -> Option<String> {
    let state = capture.receive_until(Instant::now() + COMMAND_CAPTURE_GRACE);
    if matches!(&state, CommandCaptureState::TimedOut)
        && !signal_owned_command_group_before_reap(group_id, label, &ExitDaemon)
    {
        return None;
    }
    settle_command_capture_after_group_stop(label, capture, state, true)
}

fn settle_command_capture_after_group_stop(
    label: &str,
    capture: &mut CommandCapture,
    state: CommandCaptureState,
    accept_ready_output: bool,
) -> Option<String> {
    match state {
        CommandCaptureState::Ready(Ok(output)) => {
            if !capture.join() {
                return None;
            }
            accept_ready_output
                .then(|| String::from_utf8(output).ok())
                .flatten()
        }
        CommandCaptureState::Ready(Err(())) | CommandCaptureState::Disconnected => {
            let _ = capture.join();
            None
        }
        CommandCaptureState::TimedOut => {
            match capture.receive_until(Instant::now() + COMMAND_CAPTURE_GRACE) {
                CommandCaptureState::Ready(_) | CommandCaptureState::Disconnected => {
                    let _ = capture.join();
                    None
                }
                CommandCaptureState::TimedOut => {
                    ExitDaemon.contain(&format!(
                        "{label}: capture worker remained blocked after exact group SIGKILL"
                    ));
                    None
                }
            }
        }
    }
}

struct CommandChildGuard {
    child: Child,
    label: String,
    armed: bool,
}

impl CommandChildGuard {
    fn new(child: Child, label: &str) -> Self {
        Self {
            child,
            label: label.to_string(),
            armed: true,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CommandChildGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = terminate_command_child(&mut self.child, &self.label);
        }
    }
}

/// Runs a trusted, short-lived helper in its own process group. RamShared's
/// helpers do not intentionally call `setsid`, `setpgid`, or daemonize; a
/// malicious helper that deliberately escapes this private group is outside
/// the custody boundary.
fn command_stdout_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let label = format!("{program} command");
    let mut command = ProcessCommand::new(program);
    command
        .args(args)
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = command.spawn().ok()?;
    let mut child = CommandChildGuard::new(child, &label);
    let group_id = child.id();
    let stdout = match child.child_mut().stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = terminate_command_child(child.child_mut(), &label);
            child.disarm();
            return None;
        }
    };
    let mut capture = match CommandCapture::spawn(stdout) {
        Ok(capture) => capture,
        Err(_) => {
            let _ = terminate_command_child(child.child_mut(), &label);
            child.disarm();
            return None;
        }
    };
    let deadline = Instant::now() + timeout;
    loop {
        match CommandReapTarget::observe_exit(child.child_mut()) {
            Ok(true) => {
                // A naturally closed capture proves no descendant retained the
                // pipe. If it stays open for the bounded grace, stop the exact
                // residual group and discard the otherwise ambiguous output.
                let capture_state = capture.receive_until(Instant::now() + COMMAND_CAPTURE_GRACE);
                let mut errors = Vec::new();
                if let Err(error) = CommandReapTarget::kill_group(child.child_mut())
                    && error.raw_os_error() != Some(3)
                {
                    errors.push(format!(
                        "{label}: exact process-group SIGKILL after normal exit observation failed: {error}"
                    ));
                }
                let accept_output = !matches!(&capture_state, CommandCaptureState::TimedOut);
                let output = settle_command_capture_after_group_stop(
                    &label,
                    &mut capture,
                    capture_state,
                    accept_output,
                );
                let status = reap_observed_command(
                    child.child_mut(),
                    &label,
                    COMMAND_REAP_GRACE,
                    &ExitDaemon,
                );
                child.disarm();
                if !contain_command_group_errors(errors, &ExitDaemon) {
                    return None;
                }
                let status = status?;
                let output = output?;
                return status.success().then_some(output);
            }
            Ok(false) if Instant::now() < deadline => {
                std::thread::sleep(COMMAND_POLL_INTERVAL);
            }
            Ok(false) => {
                let errors = force_command_exit_observed(
                    child.child_mut(),
                    &label,
                    COMMAND_REAP_GRACE,
                    &ExitDaemon,
                )?;
                let _ = finish_command_capture(group_id, &label, &mut capture);
                let reaped = reap_observed_command(
                    child.child_mut(),
                    &label,
                    COMMAND_REAP_GRACE,
                    &ExitDaemon,
                )
                .is_some();
                child.disarm();
                if !reaped || !contain_command_group_errors(errors, &ExitDaemon) {
                    return None;
                }
                return None;
            }
            Err(error) => {
                ExitDaemon.contain(&format!(
                    "{label}: direct-child exit observation failed: {error}"
                ));
                return None;
            }
        }
    }
}

fn global_gpu_free_bytes_with<A, R>(
    programs: &[&str],
    timeout: Duration,
    mut available: A,
    mut run: R,
) -> Option<u64>
where
    A: FnMut(&str) -> bool,
    R: FnMut(&str, &[&str], Duration) -> Option<String>,
{
    const ARGS: &[&str] = &["--query-gpu=memory.free", "--format=csv,noheader,nounits"];
    for program in programs {
        if !available(program) {
            continue;
        }
        if let Some(output) = run(program, ARGS, timeout)
            && let Some(bytes) = parse_nvidia_smi_free_bytes(&output)
        {
            return Some(bytes);
        }
    }
    None
}

fn global_gpu_free_bytes_from_nvidia_smi(timeout: Duration) -> Option<u64> {
    global_gpu_free_bytes_with(
        &["/usr/lib/wsl/lib/nvidia-smi", "nvidia-smi"],
        timeout,
        |program| !program.starts_with('/') || Path::new(program).exists(),
        command_stdout_with_timeout,
    )
}

fn observe_global_free_floor(
    free_bytes: Option<u64>,
    floor_bytes: u64,
    committed_bytes: u64,
    streak: &mut u32,
    required: u32,
) -> bool {
    if committed_bytes == 0 {
        *streak = 0;
        return false;
    }
    if free_bytes.is_some_and(|free| free < floor_bytes) {
        *streak = streak.saturating_add(1);
        *streak >= required.max(1)
    } else {
        *streak = 0;
        false
    }
}

fn validate_partuuid_path(path: &str) -> Result<&str, String> {
    let partuuid = path
        .strip_prefix("/dev/disk/by-partuuid/")
        .ok_or_else(|| "origin must use /dev/disk/by-partuuid/<uuid>".to_string())?;
    let bytes = partuuid.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if !valid {
        return Err("origin PARTUUID must use canonical 8-4-4-4-12 hexadecimal syntax".into());
    }
    Ok(partuuid)
}

fn canonical_guid(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    valid.then(|| value.to_ascii_lowercase())
}

fn canonical_sha256(value: &str) -> Option<String> {
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn canonical_device_number(value: &str) -> Option<String> {
    let (major, minor) = value.split_once(':')?;
    let major = major.parse::<u64>().ok()?;
    let minor = minor.parse::<u64>().ok()?;
    Some(format!("{major}:{minor}"))
}

fn linux_device_number(dev: u64) -> String {
    let major = ((dev & 0x0000_0000_000f_ff00) >> 8) | ((dev & 0xffff_f000_0000_0000) >> 32);
    let minor = (dev & 0xff) | ((dev & 0x0000_0fff_fff0_0000) >> 12);
    format!("{major}:{minor}")
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SealedOriginManifest {
    host_manifest_sha256: String,
    configuration_sha256: String,
    origin_path: String,
    partuuid: String,
    ptuuid: String,
    partition_dev_t: String,
    parent_dev_t: String,
    expected_swap_uuid: String,
    logical_capacity_mib: u64,
    physical_cache_cap_mib: u64,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct HostOriginManifest {
    schema_version: u32,
    origin_vhdx: String,
    fixed_size_bytes: u64,
    logical_capacity_mib: u64,
    physical_cache_cap_mib: u64,
    chunk_mib: u64,
    gpu_reserve_min_mib: u64,
    gpu_reserve_percent: u64,
    partuuid: String,
    disk_guid: String,
    expected_swap_uuid: String,
    ownership_proof_schema: u32,
    existing_wsl_swap_vhdx: String,
    configuration_sha256: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn windows_absolute_drive_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 4
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn host_configuration_text(manifest: &HostOriginManifest) -> String {
    format!(
        "schema=3\n\
         origin_vhdx={}\n\
         fixed_size_bytes={}\n\
         logical_capacity_mib={}\n\
         physical_cache_cap_mib={}\n\
         chunk_mib={}\n\
         gpu_reserve_min_mib={}\n\
         gpu_reserve_percent={}\n\
         partuuid={}\n\
         disk_guid={}\n\
         expected_swap_uuid={}\n\
         ownership_proof_schema={}\n\
         existing_wsl_swap_vhdx={}\n",
        manifest.origin_vhdx,
        manifest.fixed_size_bytes,
        manifest.logical_capacity_mib,
        manifest.physical_cache_cap_mib,
        manifest.chunk_mib,
        manifest.gpu_reserve_min_mib,
        manifest.gpu_reserve_percent,
        manifest.partuuid,
        manifest.disk_guid,
        manifest.expected_swap_uuid,
        manifest.ownership_proof_schema,
        manifest.existing_wsl_swap_vhdx,
    )
}

fn validate_host_origin_manifest_bytes(
    sealed: &SealedOriginManifest,
    bytes: &[u8],
) -> Result<(), String> {
    if sha256_hex(bytes) != sealed.host_manifest_sha256 {
        return Err("host origin manifest SHA-256 differs from the sealed guest hash".into());
    }
    let json_bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let host: HostOriginManifest = serde_json::from_slice(json_bytes)
        .map_err(|error| format!("host origin manifest JSON is invalid: {error}"))?;
    let host_partuuid = canonical_guid(&host.partuuid)
        .ok_or_else(|| "host origin manifest PARTUUID is invalid".to_string())?;
    let host_disk_guid = canonical_guid(&host.disk_guid)
        .ok_or_else(|| "host origin manifest disk GUID is invalid".to_string())?;
    let host_swap_uuid = canonical_guid(&host.expected_swap_uuid)
        .ok_or_else(|| "host origin manifest swap UUID is invalid".to_string())?;
    let host_configuration_sha256 = canonical_sha256(&host.configuration_sha256)
        .ok_or_else(|| "host origin manifest configuration SHA-256 is invalid".to_string())?;
    if host.schema_version != 3
        || host.ownership_proof_schema != 1
        || host.fixed_size_bytes != 25 * GIB
        || host.chunk_mib != 128
        || host.gpu_reserve_min_mib != 2048
        || host.gpu_reserve_percent != 20
        || !windows_absolute_drive_path(&host.origin_vhdx)
        || !windows_absolute_drive_path(&host.existing_wsl_swap_vhdx)
        || host_partuuid != sealed.partuuid
        || host_disk_guid != sealed.ptuuid
        || host_swap_uuid != sealed.expected_swap_uuid
        || host.logical_capacity_mib != sealed.logical_capacity_mib
        || host.physical_cache_cap_mib != sealed.physical_cache_cap_mib
    {
        return Err("host and guest origin manifests disagree on sealed policy or identity".into());
    }
    let computed_configuration_sha256 = sha256_hex(host_configuration_text(&host).as_bytes());
    if computed_configuration_sha256 != host_configuration_sha256
        || computed_configuration_sha256 != sealed.configuration_sha256
    {
        return Err("host origin configuration SHA-256 is not enforced end to end".into());
    }
    Ok(())
}

#[cfg(test)]
type ManifestReadHook = Box<dyn Fn(&Path)>;

#[cfg(test)]
thread_local! {
    static MANIFEST_READ_HOOK: std::cell::RefCell<Option<ManifestReadHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn invoke_manifest_read_hook(path: &Path) {
    MANIFEST_READ_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook(path);
        }
    });
}

#[cfg(not(test))]
fn invoke_manifest_read_hook(_path: &Path) {}

fn read_bounded_manifest_file(path: &Path, require_root_seal: bool) -> Result<Vec<u8>, String> {
    let opened = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| format!("open manifest without following links: {error}"))?;
    let mut file = File::from(opened);
    let before = file
        .metadata()
        .map_err(|error| format!("inspect opened manifest: {error}"))?;
    let named_before = std::fs::symlink_metadata(path)
        .map_err(|error| format!("inspect named manifest: {error}"))?;
    let unsafe_metadata = !before.file_type().is_file()
        || before.nlink() != 1
        || before.len() == 0
        || before.len() > ORIGIN_MANIFEST_MAX_BYTES
        || !named_before.file_type().is_file()
        || named_before.file_type().is_symlink()
        || before.dev() != named_before.dev()
        || before.ino() != named_before.ino()
        || before.len() != named_before.len()
        || require_root_seal && (before.uid() != 0 || before.mode() & 0o022 != 0);
    if unsafe_metadata {
        return Err("manifest is not a bounded, singly linked sealed regular file".into());
    }

    invoke_manifest_read_hook(path);
    let mut bytes = Vec::new();
    (&mut file)
        .take(ORIGIN_MANIFEST_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read bounded manifest: {error}"))?;
    if bytes.len() as u64 > ORIGIN_MANIFEST_MAX_BYTES {
        return Err("manifest exceeds its bounded read limit".into());
    }

    let after = file
        .metadata()
        .map_err(|error| format!("reinspect opened manifest: {error}"))?;
    let named_after = std::fs::symlink_metadata(path)
        .map_err(|error| format!("reinspect named manifest: {error}"))?;
    if !after.file_type().is_file()
        || !named_after.file_type().is_file()
        || named_after.file_type().is_symlink()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.dev() != named_after.dev()
        || before.ino() != named_after.ino()
        || before.len() != named_after.len()
        || bytes.len() as u64 != before.len()
    {
        return Err("manifest identity, type, or size changed during bounded read".into());
    }
    Ok(bytes)
}

fn read_host_origin_manifest_bytes(path: &str) -> Result<Vec<u8>, String> {
    if path != HOST_ORIGIN_MANIFEST_PATH {
        return Err(format!(
            "host origin manifest must use {HOST_ORIGIN_MANIFEST_PATH}"
        ));
    }
    read_bounded_manifest_file(Path::new(path), false)
}

fn parse_sealed_origin_manifest(text: &str) -> Result<SealedOriginManifest, String> {
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| "origin manifest contains a malformed line".to_string())?;
        if key.trim() != key || value.trim() != value || key.is_empty() || value.is_empty() {
            return Err(
                "origin manifest contains non-canonical whitespace or an empty value".into(),
            );
        }
        if values.insert(key, value).is_some() {
            return Err(format!("origin manifest repeats key {key}"));
        }
    }
    const KEYS: &[&str] = &[
        "schema_version",
        "host_manifest_sha256",
        "configuration_sha256",
        "origin_path",
        "partuuid",
        "ptuuid",
        "partition_dev_t",
        "parent_dev_t",
        "expected_swap_uuid",
        "swap_type",
        "logical_capacity_mib",
        "physical_cache_cap_mib",
    ];
    if values.len() != KEYS.len() || KEYS.iter().any(|key| !values.contains_key(key)) {
        return Err("origin manifest schema is incomplete or contains unknown keys".into());
    }
    if values["schema_version"] != "3" || values["swap_type"] != "swap" {
        return Err("origin manifest schema or swap type is invalid".into());
    }
    let partuuid = canonical_guid(values["partuuid"])
        .ok_or_else(|| "origin manifest PARTUUID is invalid".to_string())?;
    let ptuuid = canonical_guid(values["ptuuid"])
        .ok_or_else(|| "origin manifest PTUUID is invalid".to_string())?;
    let expected_swap_uuid = canonical_guid(values["expected_swap_uuid"])
        .ok_or_else(|| "origin manifest swap UUID is invalid".to_string())?;
    let origin_path = values["origin_path"].to_string();
    if validate_partuuid_path(&origin_path)?.to_ascii_lowercase() != partuuid {
        return Err("origin manifest path and PARTUUID disagree".into());
    }
    let logical_capacity_mib = values["logical_capacity_mib"]
        .parse::<u64>()
        .map_err(|_| "origin manifest logical capacity is invalid")?;
    if !(1024..=24 * 1024).contains(&logical_capacity_mib) {
        return Err("origin manifest capacities are outside the product policy".into());
    }
    let physical_cache_cap_mib = parse_physical_cache_cap(
        Some(values["physical_cache_cap_mib"]),
        logical_capacity_mib.saturating_mul(1024 * 1024),
    )?
    .div_ceil(1024 * 1024);
    Ok(SealedOriginManifest {
        host_manifest_sha256: canonical_sha256(values["host_manifest_sha256"])
            .ok_or_else(|| "origin host manifest SHA-256 is invalid".to_string())?,
        configuration_sha256: canonical_sha256(values["configuration_sha256"])
            .ok_or_else(|| "origin configuration SHA-256 is invalid".to_string())?,
        origin_path,
        partuuid,
        ptuuid,
        partition_dev_t: canonical_device_number(values["partition_dev_t"])
            .ok_or_else(|| "origin partition dev_t is invalid".to_string())?,
        parent_dev_t: canonical_device_number(values["parent_dev_t"])
            .ok_or_else(|| "origin parent dev_t is invalid".to_string())?,
        expected_swap_uuid,
        logical_capacity_mib,
        physical_cache_cap_mib,
    })
}

fn read_sealed_origin_manifest(path: &str) -> Result<SealedOriginManifest, String> {
    if path != ORIGIN_MANIFEST_PATH {
        return Err(format!(
            "origin manifest must be the sealed {ORIGIN_MANIFEST_PATH} path"
        ));
    }
    let bytes = read_bounded_manifest_file(Path::new(path), true)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "origin manifest is not canonical UTF-8".to_string())?;
    let sealed = parse_sealed_origin_manifest(&text)?;
    let host_bytes = read_host_origin_manifest_bytes(HOST_ORIGIN_MANIFEST_PATH)?;
    validate_host_origin_manifest_bytes(&sealed, &host_bytes)?;
    Ok(sealed)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OriginIdentityObservation {
    partuuid: String,
    ptuuid: String,
    path_dev_t: String,
    fd_dev_t: String,
    parent_dev_t: String,
    swap_uuid: String,
    swap_type: String,
    origin_size: u64,
    critical_dev_ts: Vec<String>,
}

fn validate_origin_manifest_identity(
    manifest: &SealedOriginManifest,
    observation: &OriginIdentityObservation,
    logical_size: u64,
) -> Result<(), String> {
    if logical_size != manifest.logical_capacity_mib.saturating_mul(1024 * 1024) {
        return Err("daemon logical capacity differs from the sealed origin manifest".into());
    }
    if observation.origin_size < logical_size {
        return Err(format!(
            "origin is smaller than logical capacity: {} < {logical_size}",
            observation.origin_size
        ));
    }
    if observation.partuuid != manifest.partuuid
        || observation.ptuuid != manifest.ptuuid
        || observation.path_dev_t != manifest.partition_dev_t
        || observation.fd_dev_t != manifest.partition_dev_t
        || observation.parent_dev_t != manifest.parent_dev_t
        || observation.swap_uuid != manifest.expected_swap_uuid
        || observation.swap_type != "swap"
    {
        return Err("opened origin identity differs from the sealed manifest".into());
    }
    if observation
        .critical_dev_ts
        .iter()
        .any(|device| device == &observation.fd_dev_t || device == &observation.parent_dev_t)
    {
        return Err(
            "origin aliases the current root, active swap, or one of their parent devices".into(),
        );
    }
    Ok(())
}

fn parent_block_device(resolved: &Path) -> Result<(std::path::PathBuf, String), String> {
    let name = resolved
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
        .ok_or_else(|| "origin block-device name is invalid".to_string())?;
    let sysfs = std::fs::canonicalize(Path::new("/sys/class/block").join(name))
        .map_err(|error| error.to_string())?;
    if !sysfs.join("partition").is_file() {
        return Err("origin must be a partition with a distinct parent disk".into());
    }
    let parent_name = sysfs
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .ok_or_else(|| "origin parent block device is unavailable".to_string())?;
    let parent = Path::new("/dev").join(parent_name);
    let metadata = std::fs::metadata(&parent).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_block_device() {
        return Err("origin parent is not a block device".into());
    }
    Ok((parent, linux_device_number(metadata.rdev())))
}

fn process_fd_path(file: &File) -> std::path::PathBuf {
    Path::new("/proc")
        .join(std::process::id().to_string())
        .join("fd")
        .join(file.as_raw_fd().to_string())
}

fn open_parent_block_device_from_dev_t(
    partition_dev_t: &str,
) -> Result<(File, std::path::PathBuf, String), String> {
    let sysfs = std::fs::canonicalize(Path::new("/sys/dev/block").join(partition_dev_t))
        .map_err(|error| format!("resolve origin partition through dev_t: {error}"))?;
    if !sysfs.join("partition").is_file() {
        return Err("origin must be a partition with a distinct parent disk".into());
    }
    let parent_sysfs = sysfs
        .parent()
        .ok_or_else(|| "origin partition has no parent sysfs identity".to_string())?;
    let parent_name = parent_sysfs
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
        .ok_or_else(|| "origin parent block-device name is invalid".to_string())?;
    let parent_sysfs_dev_t = std::fs::read_to_string(parent_sysfs.join("dev"))
        .map_err(|error| format!("read origin parent sysfs dev_t: {error}"))?;
    let expected_dev_t = canonical_device_number(parent_sysfs_dev_t.trim())
        .ok_or_else(|| "origin parent sysfs dev_t is invalid".to_string())?;
    let path = Path::new("/dev").join(parent_name);
    let named = std::fs::metadata(&path)
        .map_err(|error| format!("stat origin parent {}: {error}", path.display()))?;
    if !named.file_type().is_block_device() || linux_device_number(named.rdev()) != expected_dev_t {
        return Err("origin parent path does not match its sysfs dev_t".into());
    }
    let file = File::options()
        .read(true)
        .open(&path)
        .map_err(|error| format!("open exact origin parent {}: {error}", path.display()))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("stat opened origin parent: {error}"))?;
    if !opened.file_type().is_block_device()
        || opened.rdev() != named.rdev()
        || linux_device_number(opened.rdev()) != expected_dev_t
    {
        return Err("origin parent identity changed while opening".into());
    }
    Ok((file, path, expected_dev_t))
}

fn validate_stable_critical_device_snapshot(
    before: &[String],
    after: &[String],
) -> Result<(), String> {
    let mut before = before.to_vec();
    let mut after = after.to_vec();
    before.sort();
    before.dedup();
    after.sort();
    after.dedup();
    if before != after {
        return Err("root/swap critical-device identities changed during origin admission".into());
    }
    Ok(())
}

fn block_identity_value(device: &Path, field: &str) -> Result<String, String> {
    let device = device
        .to_str()
        .ok_or_else(|| "block-device path is not UTF-8".to_string())?;
    command_stdout_with_timeout(
        "blkid",
        &["-s", field, "-o", "value", device],
        Duration::from_secs(5),
    )
    .map(|value| value.trim().to_ascii_lowercase())
    .filter(|value| !value.is_empty())
    .ok_or_else(|| format!("cannot read {field} from {device}"))
}

fn root_mount_source(mountinfo: &str) -> Option<&str> {
    mountinfo.lines().find_map(|line| {
        let (mount, filesystem) = line.split_once(" - ")?;
        (mount.split_whitespace().nth(4)? == "/")
            .then(|| filesystem.split_whitespace().nth(1))
            .flatten()
            .filter(|source| source.starts_with("/dev/"))
    })
}

fn collect_critical_device_numbers() -> Result<Vec<String>, String> {
    let mut devices = Vec::new();
    let root = std::fs::metadata("/").map_err(|error| error.to_string())?;
    let root_dev_t = linux_device_number(root.dev());
    devices.push(root_dev_t.clone());
    collect_parent_for_device_number(&root_dev_t, &mut devices)?;
    if let Ok(mountinfo) = std::fs::read_to_string("/proc/self/mountinfo")
        && let Some(source) = root_mount_source(&mountinfo)
    {
        collect_device_and_parent(Path::new(source), &mut devices)?;
    }
    let swaps = std::fs::read_to_string("/proc/swaps").map_err(|error| error.to_string())?;
    for entry in parse_strict_proc_swaps(&swaps)? {
        collect_device_and_parent(Path::new(&entry.filename), &mut devices)?;
    }
    devices.sort();
    devices.dedup();
    Ok(devices)
}

fn collect_device_and_parent(path: &Path, devices: &mut Vec<String>) -> Result<(), String> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        format!(
            "cannot identify critical device {}: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_block_device() {
        devices.push(linux_device_number(metadata.rdev()));
        let resolved = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
        if let Ok((_, parent_dev_t)) = parent_block_device(&resolved) {
            devices.push(parent_dev_t);
        }
    } else if metadata.file_type().is_file() {
        let dev_t = linux_device_number(metadata.dev());
        devices.push(dev_t.clone());
        collect_parent_for_device_number(&dev_t, devices)?;
    } else {
        return Err(format!(
            "critical device {} is neither a block device nor a regular swap file",
            path.display()
        ));
    }
    Ok(())
}

fn collect_parent_for_device_number(dev_t: &str, devices: &mut Vec<String>) -> Result<(), String> {
    let sysfs =
        std::fs::canonicalize(Path::new("/sys/dev/block").join(dev_t)).map_err(|error| {
            format!("cannot resolve critical device {dev_t} through sysfs: {error}")
        })?;
    if !sysfs.join("partition").is_file() {
        return Ok(());
    }
    let parent_name = sysfs
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("critical device {dev_t} has no parent identity"))?;
    let parent = Path::new("/dev").join(parent_name);
    let metadata = std::fs::metadata(&parent)
        .map_err(|error| format!("cannot identify parent of critical device {dev_t}: {error}"))?;
    if !metadata.file_type().is_block_device() {
        return Err(format!(
            "parent of critical device {dev_t} is not a block device"
        ));
    }
    devices.push(linux_device_number(metadata.rdev()));
    Ok(())
}

fn parse_physical_cache_cap(value_mib: Option<&str>, logical_bytes: u64) -> Result<u64, String> {
    let value = value_mib.unwrap_or("1024");
    let mib = value
        .parse::<u64>()
        .map_err(|_| "physical cache cap must be an integer MiB value")?;
    let bytes = mib
        .checked_mul(1024 * 1024)
        .ok_or("physical cache cap overflows bytes")?;
    if mib < 1024 || bytes > logical_bytes {
        return Err("physical cache cap must be between 1024 MiB and logical capacity".into());
    }
    Ok(bytes)
}

fn open_validated_origin(
    manifest_path: &str,
    logical_size: u64,
) -> Result<(FileOrigin, String), Box<dyn std::error::Error>> {
    let manifest = read_sealed_origin_manifest(manifest_path)?;
    let resolved = std::fs::canonicalize(&manifest.origin_path)?;
    let metadata = std::fs::metadata(&resolved)?;
    if !metadata.file_type().is_block_device() {
        return Err("origin PARTUUID must resolve to a block device".into());
    }
    let mut file = File::options().read(true).write(true).open(&resolved)?;
    let opened = file.metadata()?;
    if !opened.file_type().is_block_device() || opened.rdev() != metadata.rdev() {
        return Err("origin device identity changed while opening".into());
    }
    let origin_size = file.seek(std::io::SeekFrom::End(0))?;
    let partition_dev_t = linux_device_number(opened.rdev());
    let (parent_file, parent_path, parent_dev_t) =
        open_parent_block_device_from_dev_t(&partition_dev_t)?;
    let partition_fd_path = process_fd_path(&file);
    let parent_fd_path = process_fd_path(&parent_file);
    let critical_before = collect_critical_device_numbers()?;
    let observation = OriginIdentityObservation {
        partuuid: canonical_guid(&block_identity_value(&partition_fd_path, "PARTUUID")?)
            .ok_or("opened origin PARTUUID is invalid")?,
        ptuuid: canonical_guid(&block_identity_value(&parent_fd_path, "PTUUID")?)
            .ok_or("opened origin PTUUID is invalid")?,
        path_dev_t: linux_device_number(metadata.rdev()),
        fd_dev_t: partition_dev_t,
        parent_dev_t,
        swap_uuid: canonical_guid(&block_identity_value(&partition_fd_path, "UUID")?)
            .ok_or("opened origin swap UUID is invalid")?,
        swap_type: block_identity_value(&partition_fd_path, "TYPE")?,
        origin_size,
        critical_dev_ts: critical_before.clone(),
    };
    let resolved_after = std::fs::canonicalize(&manifest.origin_path)?;
    let metadata_after = std::fs::metadata(&resolved_after)?;
    let opened_after = file.metadata()?;
    let parent_named_after = std::fs::metadata(&parent_path)?;
    let parent_opened_after = parent_file.metadata()?;
    if resolved_after != resolved
        || !metadata_after.file_type().is_block_device()
        || metadata_after.rdev() != opened_after.rdev()
        || opened_after.rdev() != opened.rdev()
        || !parent_named_after.file_type().is_block_device()
        || parent_named_after.rdev() != parent_opened_after.rdev()
        || linux_device_number(parent_opened_after.rdev()) != observation.parent_dev_t
    {
        return Err(
            "origin or parent path changed across fd-bound external identity probes".into(),
        );
    }
    let critical_after = collect_critical_device_numbers()?;
    validate_stable_critical_device_snapshot(&critical_before, &critical_after)?;
    validate_origin_manifest_identity(&manifest, &observation, logical_size)?;
    file.seek(std::io::SeekFrom::Start(0))?;
    Ok((FileOrigin::from_file(file), manifest.partuuid))
}

fn origin_cache_runtime_ok(origin: DurableOriginState, cache: OriginCacheState) -> bool {
    origin == DurableOriginState::Ready && cache != OriginCacheState::Stuck
}

fn unix_time_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn process_start_ticks(stat: &str) -> Option<&str> {
    stat.rsplit_once(") ")?.1.split_whitespace().nth(19)
}

fn daemon_instance_id() -> Option<String> {
    let pid = std::process::id();
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let start_ticks = process_start_ticks(&stat)?;
    (pid > 0 && !start_ticks.is_empty() && start_ticks.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| format!("{pid}-{start_ticks}"))
}

fn valid_daemon_instance_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn require_origin_daemon_identity(
    origin_mode: bool,
    identity: impl FnOnce() -> Option<String>,
) -> Result<Option<String>, String> {
    if !origin_mode {
        return Ok(None);
    }
    let identity = identity()
        .filter(|value| valid_daemon_instance_id(value))
        .ok_or_else(|| {
            "origin-cache startup requires a valid daemon instance identity".to_string()
        })?;
    Ok(Some(identity))
}

fn control_request_matches(
    value: &serde_json::Value,
    daemon_instance_id: &str,
    now_unix_ms: u64,
) -> bool {
    value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        == Some(1)
        && value.get("reason").and_then(serde_json::Value::as_str) == Some("control_pressure")
        && value
            .get("daemon_instance_id")
            .and_then(serde_json::Value::as_str)
            == Some(daemon_instance_id)
        && value
            .get("issued_at_unix_ms")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|issued_at| {
                now_unix_ms >= issued_at
                    && now_unix_ms.saturating_sub(issued_at) <= CONTROL_REQUEST_MAX_AGE_MS
            })
}

fn consume_critical_cache_request(
    value: &serde_json::Value,
    daemon_instance_id: &str,
    now_unix_ms: u64,
) -> Option<u64> {
    control_request_matches(value, daemon_instance_id, now_unix_ms)
        .then(|| {
            value
                .get("target_bytes")
                .and_then(serde_json::Value::as_u64)
        })
        .flatten()
        .filter(|target_bytes| *target_bytes == 0)
}

fn current_control_request_at(
    path: &Path,
    daemon_instance_id: &str,
    now_unix_ms: u64,
) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str(&text).ok()?;
    control_request_matches(&value, daemon_instance_id, now_unix_ms).then_some(value)
}

fn critical_cache_reclaim_requested_at(
    cache_target_path: &Path,
    reclaim_request_path: &Path,
    daemon_instance_id: &str,
    now_unix_ms: u64,
) -> bool {
    current_control_request_at(cache_target_path, daemon_instance_id, now_unix_ms)
        .as_ref()
        .and_then(|value| consume_critical_cache_request(value, daemon_instance_id, now_unix_ms))
        == Some(0)
        || current_control_request_at(reclaim_request_path, daemon_instance_id, now_unix_ms)
            .is_some()
}

struct AppArgs {
    size: u64,
    origin: Option<String>,
    sock: String,
    force: bool,
    nbd_dev: String,
    transport: Transport,
    queue_depth: u16,
    backend: BackendKind,
    slices: u16,
    slice_bytes: u64,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    arbiter_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    telemetry_jsonl: Option<std::path::PathBuf>,
}

impl AppArgs {
    /// Parses an explicit argv vector before any backend selection or side effect.
    /// Keeping this boundary injectable makes all public refusals testable without
    /// loading CUDA/Vulkan or touching swap, NBD, or ublk state (memory-broker DT-46).
    fn parse_from(args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut size = DEFAULT_SIZE;
        let mut size_explicit = false;
        let mut origin = None;
        let mut sock = "/run/ramshared/wsl2d.sock".to_string();
        let mut force = false;
        let mut nbd_dev = "/dev/nbd0".to_string();
        let mut transport = Transport::Nbd;
        let mut queue_depth = 1u16;
        let mut backend = BackendKind::Vram;
        let mut slices = 0u16;
        let mut slice_mb = 0u64;
        let mut listen_nbd: Option<String> = None;
        let mut arbiter: Option<String> = None;
        let mut advertise_nbd: Option<String> = None;
        let mut telemetry_jsonl: Option<String> = None;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--size" => {
                    i += 1;
                    let mb: u64 = args
                        .get(i)
                        .ok_or("--size requires a value (MiB)")?
                        .parse()?;
                    size = mb
                        .checked_mul(1024 * 1024)
                        .ok_or("--size: MiB value overflow")?;
                    size_explicit = true;
                }
                "--sock" => {
                    i += 1;
                    sock = args.get(i).ok_or("--sock requires a path")?.clone();
                }
                "--origin-manifest" => {
                    i += 1;
                    origin = Some(
                        args.get(i)
                            .ok_or("--origin-manifest requires a path")?
                            .clone(),
                    );
                }
                "--origin" => {
                    return Err("--origin is unsafe; use the sealed --origin-manifest path".into());
                }
                "--force" => force = true,
                "--nbd" => {
                    i += 1;
                    nbd_dev = args.get(i).ok_or("--nbd requires a path")?.clone();
                }
                "--transport" => {
                    i += 1;
                    transport = match args.get(i).map(String::as_str) {
                        Some("nbd") => Transport::Nbd,
                        Some("ublk") => Transport::Ublk,
                        _ => return Err("--transport requires 'nbd' or 'ublk'".into()),
                    };
                }
                "--queue-depth" => {
                    i += 1;
                    queue_depth = args
                        .get(i)
                        .ok_or("--queue-depth requires a value")?
                        .parse()
                        .map_err(|_| "--queue-depth is invalid")?;
                }
                "--backend" => {
                    i += 1;
                    backend = match args.get(i).map(String::as_str) {
                        Some("vram") => BackendKind::Vram,
                        Some("vulkan") => BackendKind::Vulkan,
                        Some("ram") => BackendKind::Ram,
                        _ => return Err("--backend requires 'vram', 'vulkan', or 'ram'".into()),
                    };
                }
                "--slices" => {
                    i += 1;
                    slices = args
                        .get(i)
                        .ok_or("--slices requires a value")?
                        .parse()
                        .map_err(|_| "--slices is invalid")?;
                }
                "--slice-mb" => {
                    i += 1;
                    slice_mb = args
                        .get(i)
                        .ok_or("--slice-mb requires a value (MiB)")?
                        .parse()
                        .map_err(|_| "--slice-mb is invalid")?;
                }
                "--listen-nbd" => {
                    i += 1;
                    listen_nbd = Some(
                        args.get(i)
                            .ok_or("--listen-nbd requires tcp://IP:PORT")?
                            .clone(),
                    );
                }
                "--arbiter-listen" => {
                    i += 1;
                    arbiter = Some(
                        args.get(i)
                            .ok_or("--arbiter-listen requires IP:PORT")?
                            .clone(),
                    );
                }
                "--advertise-nbd" => {
                    i += 1;
                    advertise_nbd = Some(
                        args.get(i)
                            .ok_or("--advertise-nbd requires HOST:PORT")?
                            .clone(),
                    );
                }
                "--telemetry-jsonl" => {
                    i += 1;
                    telemetry_jsonl = Some(
                        args.get(i)
                            .ok_or("--telemetry-jsonl requires a path")?
                            .clone(),
                    );
                }
                other => return Err(format!("unknown argument: {other}").into()),
            }
            i += 1;
        }
        if origin.is_some() && !size_explicit {
            size = DEFAULT_ORIGIN_SIZE;
        }
        size -= size % BLOCK_SIZE as u64; // align to the block size
        if origin.is_some() && !(MIN_ORIGIN_LOGICAL_SIZE..=MAX_ORIGIN_LOGICAL_SIZE).contains(&size)
        {
            return Err("origin-cache logical size must be between 1024 and 24576 MiB".into());
        }
        if let Some(path) = origin.as_deref()
            && path != ORIGIN_MANIFEST_PATH
        {
            return Err(format!(
                "--origin-manifest must use the sealed {ORIGIN_MANIFEST_PATH} path"
            )
            .into());
        }

        if let Err(e) = validate_slice_flags(slices, slice_mb, matches!(transport, Transport::Ublk))
        {
            return Err(e.into());
        }

        let listen_nbd_addr = listen_nbd
            .as_deref()
            .map(parse_private_listen)
            .transpose()?;
        let arbiter_addr = arbiter.as_deref().map(parse_private_listen).transpose()?;
        let advertise_nbd_addr = advertise_nbd
            .as_deref()
            .map(parse_private_listen)
            .transpose()?;

        if advertise_nbd_addr.is_some() && listen_nbd_addr.is_none() {
            return Err(
                "--advertise-nbd requires --listen-nbd (cannot advertise an unserved endpoint)"
                    .into(),
            );
        }

        if slices > 0 && arbiter_addr.is_none() {
            return Err("--slices requires --arbiter-listen IP:PORT (broker control point)".into());
        }
        if slices == 0 && (arbiter_addr.is_some() || listen_nbd_addr.is_some()) {
            return Err("--arbiter-listen/--listen-nbd require --slices N (N > 0)".into());
        }

        let advertise_tcp = advertise_nbd_addr
            .or(listen_nbd_addr)
            .map(|a| (a.ip().to_string(), a.port()));
        let telemetry_jsonl = telemetry_jsonl.map(std::path::PathBuf::from);

        let slice_bytes = if slices > 0 {
            slice_mb
                .checked_mul(1024 * 1024)
                .ok_or("--slice-mb: MiB value overflow")?
        } else {
            0
        };

        Ok(Self {
            size,
            origin,
            sock,
            force,
            nbd_dev,
            transport,
            queue_depth,
            backend,
            slices,
            slice_bytes,
            listen_nbd_addr,
            arbiter_addr,
            advertise_tcp,
            telemetry_jsonl,
        })
    }
}

fn daemon_version_requested(args: &[String]) -> bool {
    matches!(args, [_, flag] if flag == "--version" || flag == "-V" || flag == "version")
}

/// A validated daemon action. The parser and selector decide this before any
/// driver, swap, NBD-client, or ublk side effect (memory-broker DT-46).
enum DaemonAction {
    Broker(AppArgs),
    Nbd(AppArgs),
    Ublk(AppArgs),
}

/// The production shell is deliberately behind this small interface so the
/// safety-critical argv/plan boundary can be tested with a recording runner.
trait DaemonActionRunner {
    fn execute(&mut self, action: DaemonAction) -> Result<(), Box<dyn std::error::Error>>;
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let raw_args = std::env::args().collect::<Vec<_>>();
    if daemon_version_requested(&raw_args) {
        println!("ramsharedd {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let args = AppArgs::parse_from(&raw_args)?;
    let mut runner = ProductionDaemonRunner;
    run_with(args, &mut runner)
}

fn run_with<R: DaemonActionRunner>(
    args: AppArgs,
    runner: &mut R,
) -> Result<(), Box<dyn std::error::Error>> {
    let action = select_daemon_action(args)?;
    runner.execute(action)
}

fn select_daemon_action(args: AppArgs) -> Result<DaemonAction, Box<dyn std::error::Error>> {
    if args.slices > 0 {
        if args.origin.is_some() {
            return Err("--origin-manifest is valid only for the single NBD product path".into());
        }
        if args.arbiter_addr.is_none() {
            return Err("--slices requires --arbiter-listen IP:PORT (broker control point)".into());
        }
        return Ok(DaemonAction::Broker(args));
    }
    if args.arbiter_addr.is_some() || args.listen_nbd_addr.is_some() {
        return Err("--arbiter-listen/--listen-nbd require --slices N (N > 0)".into());
    }
    match (args.transport, args.backend) {
        (Transport::Nbd, BackendKind::Ram) => Err(
            "--backend ram has no single NBD path; use --slices (broker) or ublk".into(),
        ),
        (Transport::Ublk, BackendKind::Vulkan) => Err(
            "ublk with --backend vulkan is not supported (DT-11); use --backend vram, or Vulkan via --slices / --transport nbd"
                .into(),
        ),
        (Transport::Nbd, _) if args.origin.is_some() => Ok(DaemonAction::Nbd(args)),
        (Transport::Nbd, _) => {
            Err(format!(
                "product NBD requires --origin-manifest {ORIGIN_MANIFEST_PATH}"
            )
            .into())
        }
        (Transport::Ublk, _) if args.origin.is_some() => {
            Err("--origin-manifest is valid only with --transport nbd".into())
        }
        (Transport::Ublk, _) => Ok(DaemonAction::Ublk(args)),
    }
}

struct ProductionDaemonRunner;

impl DaemonActionRunner for ProductionDaemonRunner {
    fn execute(&mut self, action: DaemonAction) -> Result<(), Box<dyn std::error::Error>> {
        match action {
            DaemonAction::Broker(args) => {
                let AppArgs {
                    force,
                    backend,
                    slices,
                    slice_bytes,
                    sock,
                    listen_nbd_addr,
                    arbiter_addr,
                    advertise_tcp,
                    telemetry_jsonl,
                    ..
                } = args;
                let arbiter_addr = arbiter_addr
                    .ok_or("--slices requires --arbiter-listen IP:PORT (broker control point)")?;
                match backend {
                    BackendKind::Vram => {
                        let cuda = Cuda::load()?;
                        let dev = cuda.device(0)?;
                        eprintln!("[ramsharedd] GPU: {}", dev.name());
                        let ctx = cuda.create_context(&dev)?;
                        run_broker(
                            ctx,
                            slice_bytes,
                            slices,
                            sock,
                            force,
                            listen_nbd_addr,
                            advertise_tcp,
                            arbiter_addr,
                            telemetry_jsonl,
                        )
                    }
                    BackendKind::Vulkan => {
                        let provider = VulkanProvider::open(0)?;
                        eprintln!("[ramsharedd] GPU (Vulkan): {}", provider.device_name());
                        run_broker(
                            provider,
                            slice_bytes,
                            slices,
                            sock,
                            force,
                            listen_nbd_addr,
                            advertise_tcp,
                            arbiter_addr,
                            telemetry_jsonl,
                        )
                    }
                    BackendKind::Ram => run_broker_ram(
                        slice_bytes,
                        slices,
                        sock,
                        listen_nbd_addr,
                        advertise_tcp,
                        arbiter_addr,
                        telemetry_jsonl,
                    ),
                }
            }
            DaemonAction::Nbd(args) => {
                let AppArgs {
                    backend,
                    size,
                    origin,
                    sock,
                    force,
                    nbd_dev,
                    ..
                } = args;
                let validated_origin = match origin {
                    Some(path) => {
                        let (origin, partuuid) = open_validated_origin(&path, size)?;
                        eprintln!(
                            "[ramsharedd] mode=origin-cache logical={} MiB partuuid={partuuid}",
                            size >> 20
                        );
                        Some(origin)
                    }
                    None => None,
                };
                if validated_origin.is_some() {
                    if matches!(backend, BackendKind::Ram) {
                        return Err(
                            "--backend ram has no single NBD path; use --slices (broker) or ublk"
                                .into(),
                        );
                    }
                    eprintln!(
                        "[ramsharedd] GPU cache worker is isolated and not enabled; \
                         serving the authoritative origin with cache=UNAVAILABLE"
                    );
                    return run_nbd(
                        UnavailableVramProvider,
                        validated_origin,
                        size,
                        sock,
                        force,
                        nbd_dev,
                        false,
                    );
                }
                match backend {
                    BackendKind::Vram => {
                        let cuda = match Cuda::load() {
                            Ok(cuda) => cuda,
                            Err(error) if validated_origin.is_some() => {
                                eprintln!(
                                    "[ramsharedd] GPU cache unavailable: {error}; serving origin"
                                );
                                return run_nbd(
                                    UnavailableVramProvider,
                                    validated_origin,
                                    size,
                                    sock,
                                    force,
                                    nbd_dev,
                                    false,
                                );
                            }
                            Err(error) => return Err(error.into()),
                        };
                        let dev = match cuda.device(0) {
                            Ok(device) => device,
                            Err(error) if validated_origin.is_some() => {
                                eprintln!(
                                    "[ramsharedd] GPU cache unavailable: {error}; serving origin"
                                );
                                return run_nbd(
                                    UnavailableVramProvider,
                                    validated_origin,
                                    size,
                                    sock,
                                    force,
                                    nbd_dev,
                                    false,
                                );
                            }
                            Err(error) => return Err(error.into()),
                        };
                        eprintln!("[ramsharedd] GPU: {}", dev.name());
                        let provider = match cuda.create_context(&dev) {
                            Ok(provider) => provider,
                            Err(error) if validated_origin.is_some() => {
                                eprintln!(
                                    "[ramsharedd] GPU cache unavailable: {error}; serving origin"
                                );
                                return run_nbd(
                                    UnavailableVramProvider,
                                    validated_origin,
                                    size,
                                    sock,
                                    force,
                                    nbd_dev,
                                    false,
                                );
                            }
                            Err(error) => return Err(error.into()),
                        };
                        run_nbd(provider, validated_origin, size, sock, force, nbd_dev, true)
                    }
                    BackendKind::Vulkan => match VulkanProvider::open(0) {
                        Ok(provider) => {
                            eprintln!("[ramsharedd] GPU (Vulkan): {}", provider.device_name());
                            run_nbd(
                                provider,
                                validated_origin,
                                size,
                                sock,
                                force,
                                nbd_dev,
                                false,
                            )
                        }
                        Err(error) if validated_origin.is_some() => {
                            eprintln!(
                                "[ramsharedd] GPU cache unavailable: {error}; serving origin"
                            );
                            run_nbd(
                                UnavailableVramProvider,
                                validated_origin,
                                size,
                                sock,
                                force,
                                nbd_dev,
                                false,
                            )
                        }
                        Err(error) => Err(error.into()),
                    },
                    BackendKind::Ram => Err(
                        "--backend ram has no single NBD path; use --slices (broker) or ublk"
                            .into(),
                    ),
                }
            }
            DaemonAction::Ublk(args) => {
                run_ublk(args.size, args.force, args.queue_depth, args.backend)
            }
        }
    }
}

/// Minimal WDDM-budget view used by the NBD policy. The daemon core needs only
/// a fresh budget/current-usage sample; `/dev/dxg` stays in the production
/// adapter so deterministic tests can inject a safe snapshot.
struct NbdBudgetSnapshot {
    budget: u64,
    current_usage: u64,
    sampled_at: Instant,
}

trait NbdBudgetProvider {
    fn snapshot(&self) -> Result<NbdBudgetSnapshot, String>;
}

struct ProductionNbdBudgetProvider(DxgBudgetProvider);

impl NbdBudgetProvider for ProductionNbdBudgetProvider {
    fn snapshot(&self) -> Result<NbdBudgetSnapshot, String> {
        let snapshot = self.0.snapshot().map_err(|error| error.to_string())?;
        Ok(NbdBudgetSnapshot {
            budget: snapshot.budget,
            current_usage: snapshot.current_usage,
            sampled_at: snapshot.sampled_at,
        })
    }
}

/// OS-facing edges for the NBD worker. The production implementation owns the
/// process memory lock, protocol acceptor, `/proc/swaps` observation, and
/// demote-status file. Tests inject a deterministic in-memory implementation,
/// so exercising daemon logic never requires a CUDA context, NBD client, swap
/// device, or host status path.
trait NbdRuntimeStarter {
    fn lock_memory(
        &mut self,
        force: bool,
        lock_future: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn start_acceptor(
        &mut self,
        listener: UnixListener,
        exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
        tx_flags: u16,
        jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn start_shutdown_bridge(
        &mut self,
        _jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
    ) -> Result<Option<NbdShutdownBridge>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    fn nbd_used_kb(&mut self, nbd_dev: &str) -> u64;

    /// Absence must be proved by the production `/proc/swaps` identity
    /// observation or by an explicit test adapter. An ambiguous zero-used
    /// value is never absence proof.
    fn nbd_swap_is_explicitly_absent(&mut self, _nbd_dev: &str) -> bool {
        false
    }

    fn publish_demote(&mut self, total: u64, reason: &Option<String>, in_progress: bool);

    fn publish_origin_cache(&mut self, _status: &OriginCacheStatus) {}

    fn elapsed_us(&mut self, started: Instant) -> u64;

    fn spawn_swapoff(&mut self, nbd_dev: &str) -> std::sync::mpsc::Receiver<bool>;

    /// Starts recovery activation outside the only NBD serving thread. The
    /// caller owns and polls the returned receiver until it observes a terminal
    /// result, including during shutdown.
    fn spawn_recovery_activation(
        &mut self,
        nbd_dev: &str,
        priority: i16,
    ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>>;

    fn startup_budget(
        &mut self,
        _requested: bool,
    ) -> Result<Option<Box<dyn NbdBudgetProvider>>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    fn global_free_bytes(&mut self, _timeout: Duration) -> Option<u64> {
        None
    }

    /// Production waits between fail-closed teardown observations. Tests inject
    /// zero only after supplying deterministic replacement observations.
    fn teardown_retry_delay(&mut self) -> Duration {
        Duration::from_secs(5)
    }
}

#[derive(Clone, serde::Serialize)]
struct OriginCacheStatus {
    schema_version: u8,
    daemon_instance_id: String,
    written_at_unix_ms: u64,
    ok: bool,
    origin_state: &'static str,
    cache_state: &'static str,
    logical_capacity_kib: u64,
    vram_cached_kib: u64,
    gpu_headroom_kib: Option<u64>,
    ssd_origin_written_kib: u64,
    cache_fallback_reads: u64,
    cache_invalidations: u64,
    cache_releases: u64,
    cache_target_kib: u64,
}

fn write_origin_cache_status(path: &Path, status: &OriginCacheStatus) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "cache-status path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let encoded = serde_json::to_vec(status).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".cache-status.{}.tmp", std::process::id()));
    if let Err(error) =
        std::fs::write(&temporary, encoded).and_then(|()| std::fs::rename(&temporary, path))
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

struct ProductionNbdRuntimeStarter;

struct NbdShutdownBridge {
    stop: std::sync::Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

const RECOVERY_ACTIVATION_OBSERVATION_DEADLINE: Duration = Duration::from_secs(30);
const RECOVERY_ACTIVATION_POLL_TICK: Duration = Duration::from_millis(100);

/// Terminal observation for a recovery `swapon` child. `Pending` intentionally
/// remains non-terminal after the observation deadline: the child may still be
/// reading the NBD export, so freeing the backend would recreate the deadlock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryActivationPoll {
    Idle,
    Pending,
    Succeeded,
    Failed,
}

/// Owns at most one recovery activation child receiver. A failed healthy epoch
/// remains parked until an unhealthy or nonempty observation begins a new
/// epoch, preventing retry storms against the same NBD export.
#[derive(Default)]
struct RecoveryActivation {
    result_rx: Option<std::sync::mpsc::Receiver<bool>>,
    failed_epoch: bool,
    shutdown_requested: bool,
    started_at: Option<Instant>,
    deadline_reported: bool,
}

impl RecoveryActivation {
    fn start(&mut self, result_rx: std::sync::mpsc::Receiver<bool>) -> Result<(), &'static str> {
        if self.result_rx.is_some() {
            return Err("recovery activation is already pending");
        }
        if self.failed_epoch {
            return Err("recovery activation is parked for this healthy epoch");
        }
        self.result_rx = Some(result_rx);
        self.started_at = Some(Instant::now());
        self.deadline_reported = false;
        Ok(())
    }

    fn poll(&mut self) -> RecoveryActivationPoll {
        let Some(rx) = self.result_rx.take() else {
            return RecoveryActivationPoll::Idle;
        };
        match rx.try_recv() {
            Ok(true) => {
                self.started_at = None;
                self.deadline_reported = false;
                self.failed_epoch = false;
                RecoveryActivationPoll::Succeeded
            }
            Ok(false) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.started_at = None;
                self.deadline_reported = false;
                self.failed_epoch = true;
                RecoveryActivationPoll::Failed
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.result_rx = Some(rx);
                RecoveryActivationPoll::Pending
            }
        }
    }

    fn launch_allowed(&mut self, healthy: bool, tier_empty: bool) -> bool {
        if !healthy || !tier_empty {
            self.failed_epoch = false;
            return false;
        }
        self.result_rx.is_none() && !self.failed_epoch
    }

    fn is_pending(&self) -> bool {
        self.result_rx.is_some()
    }

    fn mark_dispatch_failure(&mut self) {
        self.failed_epoch = true;
        self.started_at = None;
        self.deadline_reported = false;
    }

    fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
    }

    fn backend_release_allowed(&self) -> bool {
        !self.shutdown_requested || self.result_rx.is_none()
    }

    fn take_observation_deadline_exceeded(&mut self) -> bool {
        let overdue = self.result_rx.is_some()
            && self.started_at.is_some_and(|started| {
                started.elapsed() >= RECOVERY_ACTIVATION_OBSERVATION_DEADLINE
            });
        if overdue && !self.deadline_reported {
            self.deadline_reported = true;
            return true;
        }
        false
    }
}

impl Drop for NbdShutdownBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn spawn_nbd_shutdown_bridge(
    shutdown: &'static AtomicBool,
    jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
    poll_interval: Duration,
) -> NbdShutdownBridge {
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let worker_stop = std::sync::Arc::clone(&stop);
    let worker = std::thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) && !worker_stop.load(Ordering::SeqCst) {
            std::thread::sleep(poll_interval);
        }
        if shutdown.load(Ordering::SeqCst) {
            let _ = jobs_tx.try_send(WMsg::Shutdown);
        }
    });
    NbdShutdownBridge {
        stop,
        worker: Some(worker),
    }
}

impl NbdRuntimeStarter for ProductionNbdRuntimeStarter {
    fn lock_memory(
        &mut self,
        force: bool,
        lock_future: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        lock_memory(force, lock_future)
    }

    fn start_acceptor(
        &mut self,
        listener: UnixListener,
        exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
        tx_flags: u16,
        jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _ = spawn_acceptor(listener, exports, tx_flags, jobs_tx);
        Ok(())
    }

    fn start_shutdown_bridge(
        &mut self,
        jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
    ) -> Result<Option<NbdShutdownBridge>, Box<dyn std::error::Error>> {
        unsafe {
            signal(SIGINT, handle_shutdown);
            signal(SIGTERM, handle_shutdown);
        }
        Ok(Some(spawn_nbd_shutdown_bridge(
            &SHUTDOWN,
            jobs_tx,
            Duration::from_millis(100),
        )))
    }

    fn nbd_used_kb(&mut self, nbd_dev: &str) -> u64 {
        nbd_used_kb_from_proc(nbd_dev).unwrap_or_else(|error| {
            eprintln!(
                "[ramsharedd] strict /proc/swaps observation failed for {nbd_dev}: {error}; keeping backend allocated"
            );
            u64::MAX
        })
    }

    fn nbd_swap_is_explicitly_absent(&mut self, nbd_dev: &str) -> bool {
        nbd_swap_is_explicitly_absent_from_proc(nbd_dev).unwrap_or_else(|error| {
            eprintln!(
                "[ramsharedd] strict /proc/swaps absence proof failed for {nbd_dev}: {error}; keeping backend allocated"
            );
            false
        })
    }

    fn publish_demote(&mut self, total: u64, reason: &Option<String>, in_progress: bool) {
        let st = ramshared_wsl2d::DemoteStatusFile {
            total,
            last_reason: reason.clone(),
            in_progress,
        };
        if let Err(error) = ramshared_wsl2d::write_demote_status(
            std::path::Path::new(ramshared_wsl2d::DEMOTE_STATUS_PATH),
            &st,
        ) {
            eprintln!("[ramsharedd] demote-status write: {error}");
        }
    }

    fn publish_origin_cache(&mut self, status: &OriginCacheStatus) {
        const STATUS_PATH: &str = "/run/ramshared/cache-status.json";
        if let Err(error) = write_origin_cache_status(Path::new(STATUS_PATH), status) {
            eprintln!("[ramsharedd] cache-status write: {error}");
        }
    }

    fn elapsed_us(&mut self, started: Instant) -> u64 {
        started.elapsed().as_micros() as u64
    }

    fn spawn_swapoff(&mut self, nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
        spawn_swapoff(nbd_dev)
    }

    fn spawn_recovery_activation(
        &mut self,
        nbd_dev: &str,
        priority: i16,
    ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
        Ok(spawn_activate_swap(nbd_dev, priority)?)
    }

    fn startup_budget(
        &mut self,
        requested: bool,
    ) -> Result<Option<Box<dyn NbdBudgetProvider>>, Box<dyn std::error::Error>> {
        if !requested {
            return Ok(None);
        }
        match DxgBudgetProvider::open(None) {
            Ok(provider) => {
                eprintln!(
                    "[ramsharedd] budget_source=dxg adapter={} (WDDM authority)",
                    provider.adapter_luid()
                );
                Ok(Some(Box::new(ProductionNbdBudgetProvider(provider))))
            }
            Err(error) if error.permits_startup_fallback() => {
                eprintln!(
                    "[ramsharedd] budget_source=cuda-fallback reason={error}; \
                     CUDA free-floor is secondary compatibility mode"
                );
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn global_free_bytes(&mut self, timeout: Duration) -> Option<u64> {
        global_gpu_free_bytes_from_nvidia_smi(timeout)
    }
}

/// NBD path (fixed-newstyle in Unix socket). Product startup supplies an
/// authoritative origin and has no full-capacity fallback.
#[allow(clippy::too_many_arguments)] // explicit storage authority and platform seams
fn run_nbd<P: VramProvider>(
    provider: P,
    origin: Option<FileOrigin>,
    size: u64,
    sock: String,
    force: bool,
    nbd_dev: String,
    use_dxg_budget: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut starter = ProductionNbdRuntimeStarter;
    run_nbd_with_startup(
        provider,
        origin,
        size,
        sock,
        force,
        nbd_dev,
        use_dxg_budget,
        &mut starter,
    )
}

/// Injectable NBD composition. Product selection reaches this function only
/// with an authoritative origin; the sparse branch is a non-product test seam.
#[allow(clippy::too_many_arguments)] // explicit daemon boundary keeps OS-facing test seams injectable
fn run_nbd_with_startup<P: VramProvider, S: NbdRuntimeStarter>(
    provider: P,
    origin: Option<FileOrigin>,
    size: u64,
    sock: String,
    force: bool,
    nbd_dev: String,
    use_dxg_budget: bool,
    starter: &mut S,
) -> Result<(), Box<dyn std::error::Error>> {
    let origin_mode = origin.is_some();
    let origin_daemon_instance_id =
        require_origin_daemon_identity(origin_mode, daemon_instance_id)?;
    let (free, total) = if origin_mode {
        (0, 0)
    } else {
        provider.mem_info()?
    };
    if !origin_mode {
        eprintln!(
            "[ramsharedd] VRAM free={} MiB total={} MiB",
            free >> 20,
            total >> 20
        );
    }
    let dxg = if origin_mode {
        None
    } else {
        starter.startup_budget(use_dxg_budget)?
    };
    struct NbdBudgetGate<'a> {
        provider: &'a dyn NbdBudgetProvider,
        config: AutotierConfig,
    }
    impl CommitBudgetGate for NbdBudgetGate<'_> {
        fn allow_commit(&self, committed: u64, next_chunk: u64) -> Result<(), String> {
            let snapshot = self.provider.snapshot()?;
            commit_allowed(
                BudgetInput {
                    budget: snapshot.budget,
                    current_usage: snapshot.current_usage,
                    cuda_committed: committed,
                    sampled_at: snapshot.sampled_at,
                },
                committed,
                next_chunk,
                &self.config,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        }
    }
    let dxg_gate = dxg.as_deref().map(|provider| NbdBudgetGate {
        provider,
        config: AutotierConfig::default(),
    });
    let budget_gate = dxg_gate.as_ref().map(|gate| gate as &dyn CommitBudgetGate);
    let autotier_config = AutotierConfig::default();
    // Discipline 3: mlock host pages; for sparse, CUDA commit is on-demand (SPEC).
    starter.lock_memory(force, false)?;

    // The origin-cache starts with zero VRAM. The non-product sparse test seam
    // retains the dedicated canary for its isolated lifecycle coverage.
    let mut probe = if origin_mode {
        None
    } else {
        Some(CanaryProbe::new(provider.alloc(CANARY_BYTES)?))
    };
    let mut cadence = Cadence::new(CANARY_EVERY);
    let reserve_floor = reserve_floor_bytes_from_env();
    let residency_cfg = sparse_residency_config(reserve_floor);
    let mut sampler = ResidencySampler::new(residency_cfg);
    let free_floor = residency_cfg.free_floor_bytes;
    let idle_free = Duration::from_secs(idle_free_secs_from_env());

    enum Be<'a, Pr: VramProvider + 'a> {
        Sparse(SparseVramBackend<'a, Pr>),
        Origin(AuthoritativeOriginBackend<FileOrigin, DisabledCache>),
    }
    impl<'a, Pr: VramProvider + 'a> BlockBackend for Be<'a, Pr> {
        fn size_bytes(&self) -> u64 {
            match self {
                Be::Sparse(b) => b.size_bytes(),
                Be::Origin(b) => b.size_bytes(),
            }
        }
        fn block_size(&self) -> u32 {
            match self {
                Be::Sparse(b) => b.block_size(),
                Be::Origin(b) => b.block_size(),
            }
        }
        fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), ramshared_block::IoError> {
            match self {
                Be::Sparse(b) => b.read_at(off, buf),
                Be::Origin(b) => b.read_at(off, buf),
            }
        }
        fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), ramshared_block::IoError> {
            match self {
                Be::Sparse(b) => b.write_at(off, data),
                Be::Origin(b) => b.write_at(off, data),
            }
        }
        fn write_at_with_options(
            &mut self,
            off: u64,
            data: &[u8],
            options: WriteOptions,
        ) -> Result<(), ramshared_block::IoError> {
            match self {
                Be::Sparse(b) => b.write_at_with_options(off, data, options),
                Be::Origin(b) => b.write_at_with_options(off, data, options),
            }
        }
        fn flush(&mut self) -> Result<(), ramshared_block::IoError> {
            match self {
                Be::Sparse(b) => b.flush(),
                Be::Origin(b) => b.flush(),
            }
        }
    }

    let mut backend: Be<'_, P> = if let Some(origin) = origin {
        let cache = AuthoritativeOriginBackend::new(origin, DisabledCache, size, BLOCK_SIZE)
            .map_err(|error| error.0)?;
        eprintln!(
            "[ramsharedd] mode=authoritative-origin logical={} MiB cache=UNAVAILABLE \
             isolation=bounded-worker-required",
            size >> 20
        );
        Be::Origin(cache)
    } else {
        let chunk = chunk_bytes_from_env();
        let reserve = reserve_floor;
        let env_cap = commit_cap_bytes_from_env();
        let auto_cap = safe_commit_cap(size, total, reserve);
        let commit_cap = env_cap.min(auto_cap);
        let sparse = SparseVramBackend::new_with_config(
            &provider,
            ramshared_block::sparse_vram::SparseVramConfig {
                capacity: size,
                chunk_bytes: chunk,
                block_size: BLOCK_SIZE,
                reserve_floor_bytes: reserve,
                commit_cap_bytes: Some(commit_cap),
                budget_gate,
            },
        )
        .map_err(|e| e.0)?;
        eprintln!(
            "[ramsharedd] VRAM mode=sparse capacity={} MiB chunk={} MiB \
             commit_cap={} MiB reserve_floor={} MiB committed=0 (ondemand+safety)",
            size >> 20,
            chunk >> 20,
            commit_cap >> 20,
            reserve >> 20
        );
        Be::Sparse(sparse)
    };

    // --- Unix socket ---
    let path = Path::new(&sock);
    let (listener, socket_guard) = bind_owned_unix_listener(path)?;
    eprintln!("[ramsharedd] listening on {sock}");
    eprintln!("[ramsharedd] conecte: sudo nbd-client -C <N> -unix {sock} {nbd_dev}");

    let tx_flags =
        NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_SEND_FUA | NBD_FLAG_CAN_MULTI_CONN;
    let device_size = backend.size_bytes();
    let exports = std::sync::Arc::new(vec![ramshared_block::handshake::Export {
        name: "default".to_string(),
        size: device_size,
        block_size: 4096,
    }]);
    let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel::<WMsg>(CHAN_CAP);
    starter.start_acceptor(listener, exports, tx_flags, jobs_tx.clone())?;
    let _shutdown_bridge = starter.start_shutdown_bridge(jobs_tx)?;
    eprintln!("[ramsharedd] transmitting (single CUDA worker; multi-connection)");

    let mut canary: Option<Canary> = None;
    let mut baseline: Vec<u64> = Vec::new();
    let mut demoted = false;
    let mut demote_rx: Option<std::sync::mpsc::Receiver<bool>> = None;
    let mut swapoff_attempted = false;
    let mut swapoff_confirmed = false;
    let mut observed_budget_refuses = 0;
    let mut recovery = RecoveryTracker::new(3);
    let mut recovery_activation = RecoveryActivation::default();
    let mut shutdown_requested = false;
    let mut live = LiveCount::new();
    let trace_probe = std::env::var("RAMSHARED_TRACE_PROBE").ok().as_deref() == Some("1");
    let global_probe_interval = Duration::from_secs(1);
    let global_probe_timeout = Duration::from_secs(2);
    let mut next_global_probe_at = Instant::now();
    let mut last_global_free: Option<u64> = None;
    let mut global_free_streak = 0u32;
    // CLI status --json demote fields (cascade-lifecycle-observability ITEM-3)
    let mut demotes_total: u64 = 0;
    let mut last_demote_reason: Option<String> = None;
    starter.publish_demote(0, &None, false);

    const RECV_TICK: Duration = Duration::from_secs(5);

    'serve: loop {
        if SHUTDOWN.load(Ordering::SeqCst) && !shutdown_requested {
            shutdown_requested = true;
            recovery_activation.request_shutdown();
        }

        match recovery_activation.poll() {
            RecoveryActivationPoll::Succeeded => {
                demoted = false;
                swapoff_attempted = false;
                swapoff_confirmed = false;
                recovery.reset();
                eprintln!("[ramsharedd] RECOVERING -> available: swapon {nbd_dev} prio=100");
                if shutdown_requested {
                    last_demote_reason = Some("RecoveryShutdown".into());
                    demote_rx = Some(starter.spawn_swapoff(&nbd_dev));
                    swapoff_attempted = true;
                    starter.publish_demote(demotes_total, &last_demote_reason, true);
                }
            }
            RecoveryActivationPoll::Failed => {
                recovery.reset();
                eprintln!("[ramsharedd] RECOVERING: swapon {nbd_dev} failed; parked");
            }
            RecoveryActivationPoll::Idle | RecoveryActivationPoll::Pending => {}
        }
        if let Some(rx) = demote_rx.take() {
            match rx.try_recv() {
                Ok(true) => {
                    demoted = true;
                    swapoff_confirmed = true;
                    demotes_total = demotes_total.saturating_add(1);
                    starter.publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: swapoff {nbd_dev} OK (canario desarmado)");
                }
                Ok(false) => {
                    starter.publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: swapoff {nbd_dev} FALHOU; canario re-armado");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    demote_rx = Some(rx);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    starter.publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: thread de swapoff sumiu; canario re-armado");
                }
            }
        }
        if recovery_activation.take_observation_deadline_exceeded() {
            eprintln!(
                "[ramsharedd] RECOVERING: swapon {nbd_dev} observation exceeded {}s; \
                 keeping NBD backend alive",
                RECOVERY_ACTIVATION_OBSERVATION_DEADLINE.as_secs()
            );
        }
        if shutdown_requested
            && recovery_activation.backend_release_allowed()
            && demote_rx.is_none()
        {
            break;
        }
        let recv_tick = if recovery_activation.is_pending() {
            RECOVERY_ACTIVATION_POLL_TICK
        } else {
            RECV_TICK
        };
        let msg = match jobs_rx.recv_timeout(recv_tick) {
            Ok(m) => Some(m),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                shutdown_requested = true;
                recovery_activation.request_shutdown();
                if recovery_activation.backend_release_allowed() {
                    break 'serve;
                }
                // A disconnected producer cannot deliver further NBD jobs. Keep
                // the backend alive for an owned recovery child, but yield so a
                // closed channel cannot turn the terminal-observation loop into
                // a busy spin.
                std::thread::sleep(recv_tick);
                None
            }
        };

        // While the activation child is pending, avoid turning its bounded
        // observation tick into repeated WDDM/reclaim probes. The next loop
        // iteration polls the child again, while any real NBD job still wakes
        // and is served immediately.
        if msg.is_none() && recovery_activation.is_pending() {
            continue;
        }

        if let Some(msg) = msg {
            let job = match msg {
                WMsg::Opened => {
                    live.on_open();
                    // fall through to reclaim tick
                    None
                }
                WMsg::Closed => {
                    if live.on_close() {
                        // No client can observe residency while the runtime is
                        // quiescent. Wait for the next generation or explicit
                        // shutdown instead of running a synthetic reclaim tick.
                        continue;
                    }
                    None
                }
                WMsg::Shutdown => {
                    shutdown_requested = true;
                    recovery_activation.request_shutdown();
                    // Preserve the normal immediate teardown path, but do not
                    // release an NBD backend while an owned `swapon` child can
                    // still be reading it. The pending path returns to the
                    // top of the serving loop to observe that child.
                    if recovery_activation.backend_release_allowed() {
                        break 'serve;
                    }
                    continue 'serve;
                }
                WMsg::Job(job) => Some(job),
                WMsg::ZeroExport { base, len, done } => {
                    let ok = zero_window(&mut backend, base, len).is_ok();
                    let _ = done.send(ok);
                    None
                }
            };

            if let Some(job) = job {
                let touches_vram = matches!(job.req.cmd, Command::Read | Command::Write);
                let t0 = std::time::Instant::now();
                let out = serve(&job.req, &job.payload, &mut backend);
                let lat_us = starter.elapsed_us(t0);
                let _ = job.reply.send(Reply {
                    reply: out.reply,
                    data: out.read_data,
                    disconnect: out.disconnect,
                });

                if touches_vram
                    && !demoted
                    && demote_rx.is_none()
                    && !matches!(backend, Be::Origin(_))
                {
                    let probe = probe
                        .as_mut()
                        .ok_or("legacy residency probe is unavailable")?;
                    let mut residency_state = ResidencyCheckState {
                        canary: &mut canary,
                        baseline: &mut baseline,
                        sampler: &mut sampler,
                        cadence: &mut cadence,
                        probe,
                        free_floor_bytes: free_floor,
                    };
                    if let Some(reason) = residency_check(lat_us, &mut residency_state, || {
                        let cuda_free = provider.mem_info().ok().map(|(f, _)| f);
                        match (cuda_free, last_global_free) {
                            (Some(cuda), Some(global)) => Some(cuda.min(global)),
                            (Some(cuda), None) => Some(cuda),
                            (None, Some(global)) => Some(global),
                            (None, None) => None,
                        }
                    }) {
                        let sparse = matches!(backend, Be::Sparse(_));
                        let skip = sparse && !sparse_residency_requests_swapoff(reason);
                        if skip {
                            eprintln!(
                                "[ramsharedd] sparse skip swapoff for {reason:?} lat={lat_us}us"
                            );
                        }
                        if !skip {
                            eprintln!(
                                "[ramsharedd] DEMOTE ({reason:?}) lat={lat_us}us -> swapoff {nbd_dev}"
                            );
                            last_demote_reason = Some(format!("{reason:?}"));
                            demote_rx = Some(starter.spawn_swapoff(&nbd_dev));
                            swapoff_attempted = true;
                            starter.publish_demote(demotes_total, &last_demote_reason, true);
                        }
                    }
                }
            }

            let budget_refuses = match &backend {
                Be::Sparse(sparse) => sparse.budget_refuses,
                Be::Origin(_) => 0,
            };
            if budget_refuses > observed_budget_refuses {
                observed_budget_refuses = budget_refuses;
                if !demoted && demote_rx.is_none() {
                    eprintln!("[ramsharedd] WDDM constrained -> bounded swapoff {nbd_dev}");
                    last_demote_reason = Some("WddmBudget".into());
                    demote_rx = Some(starter.spawn_swapoff(&nbd_dev));
                    swapoff_attempted = true;
                    starter.publish_demote(demotes_total, &last_demote_reason, true);
                }
            }
        }

        // Poll DEMOTE even when no further NBD request arrives after swapoff.
        if let Some(rx) = demote_rx.take() {
            match rx.try_recv() {
                Ok(true) => {
                    demoted = true;
                    swapoff_confirmed = true;
                    recovery.reset();
                    demotes_total = demotes_total.saturating_add(1);
                    starter.publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: swapoff {nbd_dev} OK (parked)");
                }
                Ok(false) => {
                    recovery.reset();
                    starter.publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: swapoff {nbd_dev} FALHOU");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => demote_rx = Some(rx),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    recovery.reset();
                    starter.publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: thread de swapoff sumiu");
                }
            }
        }

        // SPEC ITEM-2: reclaim on worker thread (I/O or idle tick).
        if let Be::Sparse(ref mut sp) = backend {
            let used_kb = starter.nbd_used_kb(&nbd_dev);
            let free_b = provider.mem_info().ok().map(|(f, _)| f);
            match sp.try_reclaim(used_kb, free_b, free_floor, idle_free) {
                Ok(0) => {}
                Ok(n) => eprintln!(
                    "[ramsharedd] sparse reclaim freed {} MiB (used_kb={used_kb} live={})",
                    n >> 20,
                    sp.chunks_live()
                ),
                Err(e) => eprintln!("[ramsharedd] sparse reclaim err: {}", e.0),
            }
        }

        if let Be::Origin(ref mut cache) = backend {
            if matches!(
                cache.origin_state(),
                DurableOriginState::Failed | DurableOriginState::Degraded
            ) {
                let _ = cache.probe_origin();
            }
            let critical_reclaim = origin_daemon_instance_id
                .as_deref()
                .zip(unix_time_ms())
                .is_some_and(|(daemon_instance_id, now_unix_ms)| {
                    critical_cache_reclaim_requested_at(
                        Path::new(CACHE_TARGET_REQUEST_PATH),
                        Path::new(RECLAIM_REQUEST_PATH),
                        daemon_instance_id,
                        now_unix_ms,
                    )
                });
            let control_release = critical_reclaim.then(|| cache.release_cache());
            match control_release {
                Some(Ok(released_bytes)) => eprintln!(
                    "[ramsharedd] control pressure reclaimed {} MiB of clean origin cache",
                    released_bytes >> 20
                ),
                Some(Err(error)) => {
                    eprintln!(
                        "[ramsharedd] control cache release was not acknowledged: {}",
                        error.0
                    );
                }
                None => {}
            }
            let telemetry = cache.telemetry();
            starter.publish_origin_cache(&OriginCacheStatus {
                schema_version: 1,
                daemon_instance_id: origin_daemon_instance_id.clone().unwrap_or_default(),
                written_at_unix_ms: unix_time_ms().unwrap_or_default(),
                ok: !critical_reclaim
                    && origin_cache_runtime_ok(cache.origin_state(), cache.cache_state()),
                origin_state: cache.origin_state().as_str(),
                cache_state: cache.cache_state().as_str(),
                logical_capacity_kib: cache.size_bytes() >> 10,
                vram_cached_kib: cache.cached_bytes() >> 10,
                gpu_headroom_kib: None,
                ssd_origin_written_kib: telemetry.origin_written_bytes >> 10,
                cache_fallback_reads: telemetry.fallback_reads,
                cache_invalidations: telemetry.invalidations,
                cache_releases: telemetry.releases,
                cache_target_kib: if critical_reclaim {
                    0
                } else {
                    cache.target_bytes() >> 10
                },
            });
        }

        if let (Some(dxg_provider), Be::Sparse(sparse)) = (&dxg, &backend) {
            let now = Instant::now();
            if now >= next_global_probe_at {
                next_global_probe_at = now + global_probe_interval;
                last_global_free = starter.global_free_bytes(global_probe_timeout);
                if trace_probe || last_global_free.is_some_and(|free| free < free_floor * 2) {
                    eprintln!(
                        "[ramsharedd] global GPU sample: free={last_global_free:?} \
                         floor={free_floor} streak={global_free_streak}"
                    );
                }
            }

            let committed = sparse.committed_bytes();
            let chunk = sparse.chunk_bytes();
            let budget_healthy = match dxg_provider.snapshot() {
                Ok(snapshot) => {
                    let decision = commit_allowed(
                        BudgetInput {
                            budget: snapshot.budget,
                            current_usage: snapshot.current_usage,
                            cuda_committed: committed,
                            sampled_at: snapshot.sampled_at,
                        },
                        committed,
                        chunk,
                        &autotier_config,
                    );
                    if trace_probe {
                        eprintln!(
                            "[ramsharedd] WDDM poll sample: budget={} current_usage={} \
                             cuda_committed={} chunk={} allow={}",
                            snapshot.budget,
                            snapshot.current_usage,
                            committed,
                            chunk,
                            decision.is_ok()
                        );
                    }
                    decision.is_ok()
                }
                Err(error) => {
                    if trace_probe {
                        eprintln!("[ramsharedd] WDDM poll sample error: {error}");
                    }
                    false
                }
            };
            let global_constrained = observe_global_free_floor(
                last_global_free,
                free_floor,
                committed,
                &mut global_free_streak,
                autotier_config.constrained_samples,
            );
            let global_healthy = last_global_free.is_none_or(|free| free >= free_floor);

            if !demoted && demote_rx.is_none() && (!budget_healthy || global_constrained) {
                if global_constrained {
                    eprintln!(
                        "[ramsharedd] global GPU free-floor constrained -> swapoff {nbd_dev} \
                         free={last_global_free:?} floor={free_floor}"
                    );
                    last_demote_reason = Some("GlobalGpuFreeFloor".into());
                } else {
                    eprintln!("[ramsharedd] WDDM poll constrained -> swapoff {nbd_dev}");
                    last_demote_reason = Some("WddmBudgetPoll".into());
                }
                demote_rx = Some(starter.spawn_swapoff(&nbd_dev));
                swapoff_attempted = true;
                starter.publish_demote(demotes_total, &last_demote_reason, true);
            } else if demoted && demote_rx.is_none() {
                let recovery_healthy = budget_healthy && global_healthy;
                let tier_empty = sparse.chunks_live() == 0;
                let recovery_ready = recovery.observe(recovery_healthy, tier_empty);
                let activation_allowed =
                    recovery_activation.launch_allowed(recovery_healthy, tier_empty);
                if !shutdown_requested && recovery_ready && activation_allowed {
                    match starter.spawn_recovery_activation(&nbd_dev, 100) {
                        Ok(rx) => match recovery_activation.start(rx) {
                            Ok(()) => eprintln!(
                                "[ramsharedd] RECOVERING: swapon {nbd_dev} pending on owned worker"
                            ),
                            Err(error) => {
                                recovery_activation.mark_dispatch_failure();
                                recovery.reset();
                                eprintln!(
                                    "[ramsharedd] RECOVERING: refusing duplicate swapon {nbd_dev}: {error}"
                                );
                            }
                        },
                        Err(error) => {
                            recovery_activation.mark_dispatch_failure();
                            recovery.reset();
                            eprintln!(
                                "[ramsharedd] RECOVERING: could not start swapon {nbd_dev}: {error}; parked"
                            );
                        }
                    }
                }
            }
        }
    }

    recovery_activation.request_shutdown();
    if let Some(rx) = demote_rx.take() {
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(true) => {
                swapoff_confirmed = true;
                eprintln!("[ramsharedd] teardown: swapoff {nbd_dev} confirmed (clean DEMOTE)")
            }
            Ok(false) => eprintln!(
                "[ramsharedd] teardown: WARNING swapoff {nbd_dev} did not confirm (swap may be inconsistent)"
            ),
            Err(_) => eprintln!(
                "[ramsharedd] teardown: WARNING no swapoff confirmation for {nbd_dev} in 5s (timeout/thread disappeared)"
            ),
        }
    }
    let mut used_kb = starter.nbd_used_kb(&nbd_dev);
    let mut explicitly_absent = starter.nbd_swap_is_explicitly_absent(&nbd_dev);
    while !backend_release_allowed(
        swapoff_attempted,
        swapoff_confirmed,
        used_kb,
        explicitly_absent,
    ) {
        eprintln!(
            "[ramsharedd] REFUSE teardown: swapoff_confirmed={swapoff_confirmed} \
             used_kb={used_kb}; keeping CUDA backend alive"
        );
        if !swapoff_attempted {
            swapoff_attempted = true;
        }
        if swapoff_attempted
            && !swapoff_confirmed
            && let Ok(true) = starter
                .spawn_swapoff(&nbd_dev)
                .recv_timeout(Duration::from_secs(30))
        {
            swapoff_confirmed = true;
        }
        std::thread::sleep(starter.teardown_retry_delay());
        used_kb = starter.nbd_used_kb(&nbd_dev);
        explicitly_absent = starter.nbd_swap_is_explicitly_absent(&nbd_dev);
    }
    match &mut backend {
        Be::Sparse(b) => {
            let n = b.free_all_live();
            eprintln!(
                "[ramsharedd] stopped (freed {} MiB sparse VRAM plus canary)",
                n >> 20
            );
        }
        Be::Origin(b) => {
            let released = b.release_cache().map_err(|error| {
                format!("origin cache release was not acknowledged: {}", error.0)
            })?;
            eprintln!(
                "[ramsharedd] stopped (released {} MiB clean origin cache)",
                released >> 20
            );
        }
    }
    if let Some(probe) = probe.as_mut() {
        let _ = probe.zero();
    }
    drop(socket_guard);
    Ok(())
}

/// `used_kb` for the given NBD device path from `/proc/swaps`.
///
/// A read error is unsafe to interpret as an absent swap device, so return the
/// maximum value and keep the backend allocated.
fn nbd_used_kb_from_proc(nbd_dev: &str) -> Result<u64, String> {
    let text = std::fs::read_to_string("/proc/swaps")
        .map_err(|error| format!("read /proc/swaps: {error}"))?;
    nbd_used_kb_from_text(&text, nbd_dev)
}

fn nbd_swap_is_explicitly_absent_from_proc(nbd_dev: &str) -> Result<bool, String> {
    let text = std::fs::read_to_string("/proc/swaps")
        .map_err(|error| format!("read /proc/swaps: {error}"))?;
    nbd_swap_is_explicitly_absent_from_text(&text, nbd_dev)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExactSwapState {
    Absent,
    Active { used_kb: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StrictSwapEntry {
    filename: String,
    used_kb: u64,
}

fn parse_strict_proc_swaps(text: &str) -> Result<Vec<StrictSwapEntry>, String> {
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| "/proc/swaps is empty".to_string())?;
    if header.split_whitespace().collect::<Vec<_>>()
        != ["Filename", "Type", "Size", "Used", "Priority"]
    {
        return Err("/proc/swaps header is malformed or unsupported".into());
    }
    let mut entries = Vec::new();
    let mut identities = std::collections::BTreeSet::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            return Err(format!("/proc/swaps row {} is empty", index + 2));
        }
        let cols = line.split_whitespace().collect::<Vec<_>>();
        if cols.len() != 5 {
            return Err(format!("/proc/swaps row {} is malformed", index + 2));
        }
        if !matches!(cols[1], "file" | "partition") {
            return Err(format!("/proc/swaps row {} has an invalid type", index + 2));
        }
        let _size_kb = cols[2]
            .parse::<u64>()
            .map_err(|_| format!("/proc/swaps row {} has an invalid size", index + 2))?;
        let used_kb = cols[3]
            .parse::<u64>()
            .map_err(|_| format!("/proc/swaps row {} has invalid usage", index + 2))?;
        let _priority = cols[4]
            .parse::<i32>()
            .map_err(|_| format!("/proc/swaps row {} has invalid priority", index + 2))?;
        let filename = cols[0];
        if !filename.starts_with('/') || filename.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(format!("/proc/swaps row {} has an invalid path", index + 2));
        }
        let identity = filename.strip_suffix("\\040(deleted)").unwrap_or(filename);
        if !identities.insert(identity.to_string()) {
            return Err(format!("/proc/swaps row {} repeats a device", index + 2));
        }
        entries.push(StrictSwapEntry {
            filename: filename.to_string(),
            used_kb,
        });
    }
    Ok(entries)
}

fn strict_exact_swap_state_from_text(
    text: &str,
    device: &str,
    canonicalize: fn(&str) -> Option<String>,
) -> Result<ExactSwapState, String> {
    let key = canonicalize(device).ok_or_else(|| "managed swap identity is invalid".to_string())?;
    let mut matched = None;
    for entry in parse_strict_proc_swaps(text)? {
        if canonicalize(&entry.filename).as_deref() == Some(key.as_str())
            && matched.replace(entry.used_kb).is_some()
        {
            return Err("managed swap identity appears more than once".into());
        }
    }
    Ok(
        matched.map_or(ExactSwapState::Absent, |used_kb| ExactSwapState::Active {
            used_kb,
        }),
    )
}

fn nbd_swap_is_explicitly_absent_from_text(text: &str, nbd_dev: &str) -> Result<bool, String> {
    strict_exact_swap_state_from_text(text, nbd_dev, canonical_nbd_identity)
        .map(|state| state == ExactSwapState::Absent)
}

fn canonical_nbd_identity(path: &str) -> Option<String> {
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
    let suffix = bare.strip_prefix("nbd")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!("/dev/{bare}"))
}

fn canonical_ublk_identity(path: &str) -> Option<String> {
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

fn nbd_used_kb_from_text(text: &str, nbd_dev: &str) -> Result<u64, String> {
    strict_exact_swap_state_from_text(text, nbd_dev, canonical_nbd_identity).map(
        |state| match state {
            ExactSwapState::Absent => 0,
            ExactSwapState::Active { used_kb } => used_kb,
        },
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SocketFileIdentity {
    device: u64,
    inode: u64,
    owner: u32,
}

fn socket_identity(stat: &rustix::fs::Stat) -> SocketFileIdentity {
    SocketFileIdentity {
        device: stat.st_dev,
        inode: stat.st_ino,
        owner: stat.st_uid,
    }
}

struct SocketBindTarget {
    parent: File,
    parent_path: PathBuf,
    parent_identity: SocketFileIdentity,
    name: std::ffi::OsString,
}

fn open_socket_parent(path: &Path) -> std::io::Result<SocketBindTarget> {
    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "socket path has no filename",
            )
        })?
        .to_os_string();
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let anchor = if parent_path.is_absolute() { "/" } else { "." };
    let mut parent = File::from(
        rustix::fs::open(
            anchor,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(std::io::Error::from)?,
    );
    for component in parent_path.components() {
        let component = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(component) => component,
            Component::ParentDir | Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "socket path contains an unsafe parent component",
                ));
            }
        };
        parent = File::from(
            rustix::fs::openat(
                &parent,
                component,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
    }
    let stat = rustix::fs::fstat(&parent).map_err(std::io::Error::from)?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Directory {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "socket parent is not a directory",
        ));
    }
    Ok(SocketBindTarget {
        parent,
        parent_path: parent_path.to_path_buf(),
        parent_identity: socket_identity(&stat),
        name,
    })
}

/// Validate a socket target without removing anything already present. A stale
/// pathname is still someone else's namespace entry and requires an explicit
/// operator cleanup; startup never guesses whether it is safe to unlink.
fn prepare_unix_socket_path(path: &Path) -> std::io::Result<SocketBindTarget> {
    let target = open_socket_parent(path)?;
    match rustix::fs::statat(
        &target.parent,
        &target.name,
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    ) {
        Err(rustix::io::Errno::NOENT) => Ok(target),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "refusing to replace existing socket path {}",
                path.display()
            ),
        )),
        Err(error) => Err(std::io::Error::from(error)),
    }
}

/// Exact ownership token for a pathname created by this daemon. Cleanup is
/// FD-relative to the original parent and only removes the captured socket
/// device/inode/owner tuple. An ABA replacement therefore survives old cleanup.
struct OwnedUnixSocketPath {
    parent: File,
    /// `O_PATH` pins the original filesystem inode so an unlink/rebind cannot
    /// recycle the same dev+ino tuple while this cleanup token is alive.
    pinned: File,
    name: std::ffi::OsString,
    identity: SocketFileIdentity,
    armed: bool,
}

impl OwnedUnixSocketPath {
    fn remove_if_owned(&mut self) {
        if !self.armed {
            return;
        }
        let pinned_is_original = rustix::fs::fstat(&self.pinned).is_ok_and(|stat| {
            rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Socket
                && socket_identity(&stat) == self.identity
        });
        if !pinned_is_original {
            self.armed = false;
            return;
        }
        let current = rustix::fs::statat(
            &self.parent,
            &self.name,
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        );
        if let Ok(stat) = current
            && rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Socket
            && socket_identity(&stat) == self.identity
        {
            let _ = rustix::fs::unlinkat(&self.parent, &self.name, rustix::fs::AtFlags::empty());
            let _ = rustix::fs::fsync(&self.parent);
        }
        self.armed = false;
    }
}

impl Drop for OwnedUnixSocketPath {
    fn drop(&mut self) {
        self.remove_if_owned();
    }
}

fn bind_owned_unix_listener(path: &Path) -> std::io::Result<(UnixListener, OwnedUnixSocketPath)> {
    let target = prepare_unix_socket_path(path)?;
    let listener = UnixListener::bind(path)?;
    let stat = rustix::fs::statat(
        &target.parent,
        &target.name,
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    )
    .map_err(std::io::Error::from)?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Socket
        || stat.st_uid != rustix::process::geteuid().as_raw()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "bound socket has an unsafe type or owner",
        ));
    }
    let pinned = File::from(
        rustix::fs::openat(
            &target.parent,
            &target.name,
            rustix::fs::OFlags::PATH | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(std::io::Error::from)?,
    );
    let pinned_stat = rustix::fs::fstat(&pinned).map_err(std::io::Error::from)?;
    if rustix::fs::FileType::from_raw_mode(pinned_stat.st_mode) != rustix::fs::FileType::Socket
        || socket_identity(&pinned_stat) != socket_identity(&stat)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "bound socket identity changed before it could be pinned",
        ));
    }
    let guard = OwnedUnixSocketPath {
        parent: target.parent,
        pinned,
        name: target.name,
        identity: socket_identity(&stat),
        armed: true,
    };
    let named_parent = open_socket_parent(path)?;
    if named_parent.parent_path != target.parent_path
        || named_parent.parent_identity != target.parent_identity
    {
        drop(listener);
        drop(guard);
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "socket parent identity changed during bind",
        ));
    }
    Ok((listener, guard))
}

/// The worker-owned half of broker startup. Keeping the `Receiver` owned (not
/// borrowed) makes the data-plane runnable in a bounded test thread without
/// pretending that `Receiver` is `Sync`.
struct BrokerWorkerRuntime {
    geom: Vec<(u64, u64)>,
    jobs_rx: std::sync::mpsc::Receiver<WMsg>,
    demote_tx: std::sync::mpsc::Sender<DemoteReason>,
    shutdown: std::sync::Arc<AtomicBool>,
    /// A full worker queue cannot be awakened by a detached blocking sender:
    /// the worker clears this pending bit when its top-of-iteration terminal
    /// check preempts the queued work.
    shutdown_wake_pending: std::sync::Arc<AtomicBool>,
    shutdown_wake_tx: std::sync::mpsc::SyncSender<WMsg>,
    /// IO counters per slice (telemetry RF-1): worker increments, broker reads in `Status`.
    pub slice_io: std::sync::Arc<Vec<SliceIoCounters>>,
    /// VRAM Gauge (RF-3): the residency closure publishes free/total; broker reads on tick.
    pub vram: std::sync::Arc<VramGauge>,
}

/// Result of the nonblocking broker-worker shutdown wake (DT-50). A full queue
/// is safe because it proves the receiver has pending work; the worker checks
/// the terminal flag before receiving the next message. A disconnected queue
/// is already terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrokerShutdownWake {
    Queued,
    QueueFull,
    Disconnected,
}

/// Paired terminal flag and explicit worker-channel wake. `request` stores the
/// flag first and never blocks, so signal mirroring and rollback paths cannot
/// deadlock behind a full bounded queue.
#[derive(Clone)]
struct BrokerShutdown {
    flag: std::sync::Arc<AtomicBool>,
    wake_tx: std::sync::mpsc::SyncSender<WMsg>,
    wake_pending: std::sync::Arc<AtomicBool>,
}

impl BrokerShutdown {
    fn new(flag: std::sync::Arc<AtomicBool>, wake_tx: std::sync::mpsc::SyncSender<WMsg>) -> Self {
        Self {
            flag,
            wake_tx,
            wake_pending: std::sync::Arc::new(AtomicBool::new(false)),
        }
    }

    fn request(&self) -> BrokerShutdownWake {
        self.flag.store(true, Ordering::SeqCst);
        match self.wake_tx.try_send(WMsg::Shutdown) {
            Ok(()) => BrokerShutdownWake::Queued,
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                // Never detach a blocking `send`: if no receiver remains it
                // would leak a test/daemon thread indefinitely. A full queue
                // already guarantees the worker is runnable; its independent
                // top-of-iteration flag check preempts all queued work.
                self.wake_pending.store(true, Ordering::SeqCst);
                BrokerShutdownWake::QueueFull
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => BrokerShutdownWake::Disconnected,
        }
    }
}

/// Broker control-plane parts retained by the daemon shell until the worker has
/// stopped. Splitting this value transfers the non-`Sync` receiver exactly once
/// to the worker and keeps the broker join handle for orderly cleanup.
struct BrokerRuntime {
    worker: BrokerWorkerRuntime,
    broker: std::thread::JoinHandle<()>,
    shutdown: BrokerShutdown,
    socket: Option<OwnedUnixSocketPath>,
}

impl BrokerRuntime {
    fn into_parts(
        self,
    ) -> (
        BrokerWorkerRuntime,
        std::thread::JoinHandle<()>,
        BrokerShutdown,
        Option<OwnedUnixSocketPath>,
    ) {
        (self.worker, self.broker, self.shutdown, self.socket)
    }
}

/// Joins the broker on a dedicated observer so an early broker panic can wake
/// and stop the worker before the lifecycle attempts cleanup. The observer is
/// itself owned and joined; no detached cleanup thread survives a return.
struct BrokerJoinMonitor {
    result_rx: std::sync::mpsc::Receiver<std::thread::Result<()>>,
    observer: Option<std::thread::JoinHandle<()>>,
    shutdown: BrokerShutdown,
    complete: bool,
}

impl BrokerJoinMonitor {
    fn start(broker: std::thread::JoinHandle<()>, shutdown: BrokerShutdown) -> Self {
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let observer_shutdown = shutdown.clone();
        let observer = std::thread::spawn(move || {
            let result = broker.join();
            let _ = observer_shutdown.request();
            let _ = result_tx.send(result);
        });
        Self {
            result_rx,
            observer: Some(observer),
            shutdown,
            complete: false,
        }
    }

    fn finish(mut self) -> Result<(), Box<dyn std::error::Error>> {
        let result = self
            .result_rx
            .recv()
            .map_err(|_| "broker result observer disconnected")?;
        let observer = self
            .observer
            .take()
            .ok_or("broker result observer is missing")?;
        if observer.join().is_err() {
            return Err("broker result observer panicked".into());
        }
        self.complete = true;
        match result {
            Ok(()) => Ok(()),
            Err(payload) => {
                let detail = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic payload");
                Err(format!("broker control-plane thread panicked: {detail}").into())
            }
        }
    }
}

impl Drop for BrokerJoinMonitor {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        let _ = self.shutdown.request();
        if let Some(observer) = self.observer.take() {
            let _ = observer.join();
        }
    }
}

/// Reconciliation tolerance (DT-7, provisional — calibrate at P0).
const RECON_TOL_FRAC: f64 = 0.10;
/// Consecutive ticks to confirm a reconciliation flag (hysteresis DT-12).
const RECON_STREAK: u32 = 3;

/// Builds the broker control-plane configuration before binding any listener.
/// This preserves the exact telemetry and endpoint wiring while keeping the
/// decision boundary testable without a GPU, swap device, or NBD client.
#[cfg(test)]
fn build_broker_config(
    slices: u16,
    sock: &str,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
    telemetry_jsonl: Option<std::path::PathBuf>,
) -> BrokerConfig {
    build_broker_config_with_tick(
        slices,
        sock,
        advertise_tcp,
        arbiter_addr,
        telemetry_jsonl,
        Duration::from_secs(2),
    )
}

/// Builds the exact broker control-plane configuration with an explicit core
/// poll interval. Production retains the two-second contract; a bounded local
/// harness can use a short interval solely to prove shutdown/cleanup without
/// starting a GPU, swap device, or NBD client.
fn build_broker_config_with_tick(
    slices: u16,
    sock: &str,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
    telemetry_jsonl: Option<std::path::PathBuf>,
    tick: Duration,
) -> BrokerConfig {
    let slice_io = std::sync::Arc::new(
        (0..slices)
            .map(|_| SliceIoCounters::default())
            .collect::<Vec<_>>(),
    );
    let vram = std::sync::Arc::new(VramGauge::default());
    BrokerConfig {
        listen: arbiter_addr,
        endpoints: EndpointCfg {
            nbd_unix: Some(sock.to_string()),
            nbd_tcp: advertise_tcp,
        },
        swap_prio: None,
        arbiter: ArbiterConfig::default(),
        tick,
        slice_io,
        vram,
        tol_frac: RECON_TOL_FRAC,
        recon_streak: RECON_STREAK,
        telemetry_jsonl,
    }
}

/// The listeners owned by one broker-start attempt. The Unix pathname is cleaned
/// only when it was successfully bound by this attempt; an existing regular
/// file or socket is never replaced (DT-17 safe cleanup).
struct BrokerListeners {
    unix: UnixListener,
    tcp: Option<std::net::TcpListener>,
    socket: OwnedUnixSocketPath,
}

impl BrokerListeners {
    fn cleanup(self) {
        drop(self);
    }

    fn into_parts(
        self,
    ) -> (
        UnixListener,
        Option<std::net::TcpListener>,
        OwnedUnixSocketPath,
    ) {
        (self.unix, self.tcp, self.socket)
    }
}

/// Binds the safe socket portion of broker startup. If the optional TCP bind
/// fails after Unix binding succeeded, it closes and removes only that newly
/// created Unix socket before returning the refusal.
fn bind_broker_listeners(
    path: &Path,
    listen_nbd_addr: Option<std::net::SocketAddr>,
) -> std::io::Result<BrokerListeners> {
    let (unix, socket) = bind_owned_unix_listener(path)?;
    let tcp = match listen_nbd_addr {
        Some(addr) => Some(std::net::TcpListener::bind(addr)?),
        None => None,
    };
    Ok(BrokerListeners { unix, tcp, socket })
}

/// Starts the data-plane acceptors after the broker control plane has bound.
/// Keeping this side-effect behind a narrow interface lets the daemon prove its
/// listener ownership and bounded shutdown with a recording harness, while the
/// production implementation still uses the real protocol acceptors.
trait BrokerAcceptorStarter {
    fn start(
        &mut self,
        unix: UnixListener,
        tcp: Option<std::net::TcpListener>,
        exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
        tx_flags: u16,
        jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

struct ProductionBrokerAcceptorStarter;

impl BrokerAcceptorStarter for ProductionBrokerAcceptorStarter {
    fn start(
        &mut self,
        unix: UnixListener,
        tcp: Option<std::net::TcpListener>,
        exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
        tx_flags: u16,
        jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _ = spawn_acceptor(
            unix,
            std::sync::Arc::clone(&exports),
            tx_flags,
            jobs_tx.clone(),
        );
        if let Some(tcp) = tcp {
            let addr = tcp.local_addr()?;
            eprintln!("[ramsharedd] NBD TCP listener at {addr}");
            let _ = ramshared_wsl2d::conn::spawn_acceptor_tcp(tcp, exports, tx_flags, jobs_tx);
        }
        Ok(())
    }
}

/// Installs the production signal handlers before listener setup. The handler
/// remains async-signal-safe and only stores the process-global atomic flag.
fn install_broker_signal_handlers() {
    unsafe {
        signal(SIGINT, handle_shutdown);
        signal(SIGTERM, handle_shutdown);
    }
}

/// Mirrors the process signal into both the broker terminal flag and its
/// explicit channel wake. Tests call `BrokerShutdown::request` directly and do
/// not modify process signal state.
fn install_broker_shutdown_bridge(shutdown: BrokerShutdown) {
    std::thread::spawn(move || {
        while !SHUTDOWN.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = shutdown.request();
    });
}

/// Starts the broker control-plane (independent of backend): slices map + geometry +
/// NBD exports ("s0".."sN"), acceptors (always Unix; TCP if `--listen-nbd`) feeding the
/// SAME `jobs` worker channel, the arbiter (`spawn_broker`, sharing `jobs` for the
/// hygiene `ZeroExport` DT-17 and consuming the DEMOTE channel) and the `SHUTDOWN` bridge
/// (signal handler only touches the async-signal-safe static variable → mirrored in `Arc`).
#[allow(clippy::too_many_arguments)] // control-plane setup: geometry, network, and telemetry
fn broker_setup(
    slices: u16,
    slice_bytes: u64,
    sock: &str,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
    telemetry_jsonl: Option<std::path::PathBuf>,
) -> Result<BrokerRuntime, Box<dyn std::error::Error>> {
    install_broker_signal_handlers();
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    let mut acceptors = ProductionBrokerAcceptorStarter;
    let runtime = broker_setup_with_acceptors(
        slices,
        slice_bytes,
        sock,
        listen_nbd_addr,
        advertise_tcp,
        arbiter_addr,
        telemetry_jsonl,
        shutdown,
        Duration::from_secs(2),
        &mut acceptors,
    )?;
    install_broker_shutdown_bridge(runtime.shutdown.clone());
    Ok(runtime)
}

/// The bounded, injectable broker startup core. Production passes the signal
/// bridge and real protocol acceptors; local tests pass an explicit shutdown
/// flag and recording starter, while still binding only temporary Unix and
/// loopback TCP sockets.
#[allow(clippy::too_many_arguments)]
fn broker_setup_with_acceptors(
    slices: u16,
    slice_bytes: u64,
    sock: &str,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
    telemetry_jsonl: Option<std::path::PathBuf>,
    shutdown: std::sync::Arc<AtomicBool>,
    broker_tick: Duration,
    acceptors: &mut dyn BrokerAcceptorStarter,
) -> Result<BrokerRuntime, Box<dyn std::error::Error>> {
    // Slices map: export index (resolved by handshake) == geometry index == exports index
    // ("s{id}" names identical to those emitted by the broker in SwapOn).
    let slice_map = SliceMap::new(slices, slice_bytes);
    let geom: Vec<(u64, u64)> = slice_map
        .slices()
        .iter()
        .map(|s| (s.offset, s.len))
        .collect();
    let exports = std::sync::Arc::new(
        slice_map
            .exports()
            .into_iter()
            .map(|(name, size)| ramshared_block::handshake::Export {
                name,
                size,
                block_size: 4096,
            })
            .collect::<Vec<_>>(),
    );

    let tx_flags =
        NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_SEND_FUA | NBD_FLAG_CAN_MULTI_CONN;
    let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel::<WMsg>(CHAN_CAP);
    let broker_shutdown = BrokerShutdown::new(std::sync::Arc::clone(&shutdown), jobs_tx.clone());
    let (demote_tx, demote_rx) = std::sync::mpsc::channel::<DemoteReason>();

    let listeners = bind_broker_listeners(Path::new(sock), listen_nbd_addr)?;

    let bcfg = build_broker_config_with_tick(
        slices,
        sock,
        advertise_tcp,
        arbiter_addr,
        telemetry_jsonl,
        broker_tick,
    );
    let slice_io = std::sync::Arc::clone(&bcfg.slice_io);
    let vram = std::sync::Arc::clone(&bcfg.vram);
    let (broker, broker_addr) = match spawn_broker(
        bcfg,
        slice_map,
        demote_rx,
        jobs_tx.clone(),
        std::sync::Arc::clone(&shutdown),
    ) {
        Ok(result) => result,
        Err(error) => {
            listeners.cleanup();
            return Err(error.into());
        }
    };
    let (unix, tcp, socket) = listeners.into_parts();
    eprintln!("[ramsharedd] NBD Unix listener at {sock}");
    if let Err(error) = acceptors.start(unix, tcp, exports, tx_flags, jobs_tx.clone()) {
        let _ = broker_shutdown.request();
        let _ = broker.join();
        drop(socket);
        return Err(error);
    }
    eprintln!("[ramsharedd] broker arbiter at {broker_addr}");
    drop(jobs_tx); // clones (acceptors + broker) keep the channel; worker owns the rx

    Ok(BrokerRuntime {
        worker: BrokerWorkerRuntime {
            geom,
            jobs_rx,
            demote_tx,
            shutdown,
            shutdown_wake_pending: std::sync::Arc::clone(&broker_shutdown.wake_pending),
            shutdown_wake_tx: broker_shutdown.wake_tx.clone(),
            slice_io,
            vram,
        },
        broker,
        shutdown: broker_shutdown,
        socket: Some(socket),
    })
}

/// Bounded broker worker loop. Production uses a 500 ms shutdown poll; tests
/// inject a shorter positive interval to prove that an idle worker cannot wait
/// indefinitely after its explicit stop signal.
fn serve_broker_jobs_with_poll<B: BlockBackend>(
    backend: B,
    rt: BrokerWorkerRuntime,
    residency: impl FnMut(u64) -> Option<DemoteReason>,
    poll_interval: Duration,
) -> B {
    serve_broker_jobs_with_poll_and_reply_hook(backend, rt, residency, poll_interval, || {})
}

/// Worker core with an injected post-publication hook. The hook lets tests
/// freeze the worker immediately after a reply becomes observable and prove
/// that all completion state was published first.
fn serve_broker_jobs_with_poll_and_reply_hook<B: BlockBackend>(
    mut backend: B,
    rt: BrokerWorkerRuntime,
    mut residency: impl FnMut(u64) -> Option<DemoteReason>,
    poll_interval: Duration,
    mut reply_published: impl FnMut(),
) -> B {
    let poll_interval = if poll_interval.is_zero() {
        Duration::from_millis(1)
    } else {
        poll_interval
    };
    let mut demoted = false;
    eprintln!("[ramsharedd] serving (single worker; multi-slice broker)");
    loop {
        if rt.shutdown.load(Ordering::SeqCst) {
            rt.shutdown_wake_pending.store(false, Ordering::SeqCst);
            break;
        }
        let msg = match rt.jobs_rx.recv_timeout(poll_interval) {
            Ok(m) => m,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if rt.shutdown.load(Ordering::SeqCst) {
                    break; // DT-28: stop only after SIGINT or SIGTERM.
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let job = match msg {
            // DT-28: NBD connections coming and going do NOT terminate the daemon (the broker persists).
            WMsg::Opened | WMsg::Closed => None,
            WMsg::Shutdown => break,
            WMsg::Job(job) => Some(job),
            WMsg::ZeroExport { base, len, done } => {
                let ok = zero_window(&mut backend, base, len).is_ok();
                let _ = done.send(ok);
                None
            }
        };

        if let Some(job) = job {
            let touches = matches!(job.req.cmd, Command::Read | Command::Write);
            // Export geometry (handshake already resolved name→index). Defensive fallback: entire
            // backend (should not happen — every Job carries a valid export).
            let (base, len) = rt
                .geom
                .get(job.export)
                .copied()
                .unwrap_or((0, backend.size_bytes()));
            let t0 = std::time::Instant::now();
            let out = {
                let mut view = SliceView::new(&mut backend, base, len);
                serve(&job.req, &job.payload, &mut view)
            };
            let lat_us = t0.elapsed().as_micros() as u64;

            // Telemetry RF-1: a reply is the completion barrier. Publish the
            // counters first so every observer that receives the reply also
            // observes the completed IO accounting.
            if touches && let Some(c) = rt.slice_io.get(job.export) {
                c.bytes_served
                    .fetch_add(u64::from(job.req.len), Ordering::Relaxed);
                c.io_count.fetch_add(1, Ordering::Relaxed);
            }
            if job
                .reply
                .send(Reply {
                    reply: out.reply,
                    data: out.read_data,
                    disconnect: out.disconnect,
                })
                .is_ok()
            {
                reply_published();
            }

            if touches
                && !demoted
                && let Some(reason) = residency(lat_us)
            {
                eprintln!("[ramsharedd] DEMOTE ({reason:?}) lat={lat_us}us -> broker DemoteAll");
                let _ = rt.demote_tx.send(reason);
                demoted = true;
            }
        }

        if rt.shutdown_wake_pending.swap(false, Ordering::SeqCst) {
            match rt.shutdown_wake_tx.try_send(WMsg::Shutdown) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    rt.shutdown_wake_pending.store(true, Ordering::SeqCst);
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
            }
        }
    }
    backend
}

/// VRAM broker path (ITEM-8): slices VRAM into `slices` NBD exports served by Unix +
/// (optional) TCP, with the arbiter deciding who uses each slice. The single worker owns the
/// VRAM/CUDA context and runs residency §9/§9.4. Live execution is the QEMU gate (`--backend
/// ram`, ITEM-11) / civm (ITEM-12) — real VRAM does not run in QEMU (no GPU).
#[allow(clippy::too_many_arguments)] // entry-point do daemon: config de geometria + rede + provider
fn run_broker<P: VramProvider>(
    provider: P,
    slice_bytes: u64,
    slices: u16,
    sock: String,
    force: bool,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
    telemetry_jsonl: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let setup_sock = sock.clone();
    run_broker_with_setup(
        provider,
        slice_bytes,
        slices,
        sock,
        force,
        lock_memory,
        move || {
            broker_setup(
                slices,
                slice_bytes,
                &setup_sock,
                listen_nbd_addr,
                advertise_tcp,
                arbiter_addr,
                telemetry_jsonl,
            )
        },
        Duration::from_millis(500),
    )
}

/// Generic VRAM broker lifecycle after driver construction. The production
/// wrapper supplies the real memory lock and broker setup; tests supply a
/// heap-backed provider and pre-stopped bounded runtime. This preserves the
/// same zero-before-release ordering while making it observable without CUDA.
#[allow(clippy::too_many_arguments)] // explicit daemon boundary keeps lock/setup test seams injectable
fn run_broker_with_setup<P, L, S>(
    provider: P,
    slice_bytes: u64,
    slices: u16,
    _sock: String,
    force: bool,
    lock: L,
    setup: S,
    worker_poll: Duration,
) -> Result<(), Box<dyn std::error::Error>>
where
    P: VramProvider,
    L: FnOnce(bool, bool) -> Result<(), Box<dyn std::error::Error>>,
    S: FnOnce() -> Result<BrokerRuntime, Box<dyn std::error::Error>>,
{
    let total = (slices as u64)
        .checked_mul(slice_bytes)
        .ok_or("--slices * --slice-mb: overflow")?;

    // The provider has already been initialized by the production shell. The
    // lifecycle below remains generic over VramProvider/VramMemory (RF-G1).
    let (free, total_vram) = provider.mem_info()?;
    eprintln!(
        "[ramsharedd] VRAM free={} MiB total={} MiB",
        free >> 20,
        total_vram >> 20
    );
    let mut mem = provider.alloc(total as usize)?;
    mem.zero()?;
    // Lock only mappings that already exist. The canary and any later GPU/DXG
    // mappings must never inherit a process-wide MCL_FUTURE obligation.
    if let Err(error) = lock(force, false) {
        let _ = mem.zero();
        return Err(error);
    }
    let mut backend = VramBackend::new(mem, BLOCK_SIZE);
    eprintln!(
        "[ramsharedd] broker VRAM: {slices} slices x {} MiB = {} MiB, block_size={BLOCK_SIZE}",
        slice_bytes >> 20,
        total >> 20
    );

    // Residency canary (§9.4): separated region, not addressable by NBD.
    let canary_region = provider.alloc(CANARY_BYTES)?;
    let mut probe = CanaryProbe::new(canary_region);
    let mut cadence = Cadence::new(CANARY_EVERY);
    let mut sampler = ResidencySampler::new(ResidencyConfig::default());
    let mut canary: Option<Canary> = None;
    let mut baseline: Vec<u64> = Vec::new();

    let rt = match setup() {
        Ok(runtime) => runtime,
        Err(error) => {
            let backend_zeroed = backend.zero();
            let probe_zeroed = probe.zero();
            backend_zeroed?;
            probe_zeroed?;
            return Err(error);
        }
    };
    let (worker, broker, shutdown, socket) = rt.into_parts();
    let broker_monitor = BrokerJoinMonitor::start(broker, shutdown);
    let vram = std::sync::Arc::clone(&worker.vram);
    backend = serve_broker_jobs_with_poll(
        backend,
        worker,
        |lat_us| {
            let mut residency_state = ResidencyCheckState {
                canary: &mut canary,
                baseline: &mut baseline,
                sampler: &mut sampler,
                cadence: &mut cadence,
                probe: &mut probe,
                free_floor_bytes: ResidencyConfig::default().free_floor_bytes,
            };
            residency_check(lat_us, &mut residency_state, || {
                let (f, t) = provider.mem_info().ok()?;
                // RF-3/DT-5: publishes the gauge for reconciliation (free/total in bytes).
                vram.free.store(f, Ordering::Relaxed);
                vram.total.store(t, Ordering::Relaxed);
                Some(f)
            })
        },
        worker_poll,
    );

    let broker_result = broker_monitor.finish();
    let zeroed = backend.zero();
    let _ = probe.zero(); // DT-12/DT-17: zeroes the canary-region as well
    drop(socket);
    zeroed?;
    broker_result?;
    eprintln!("[ramsharedd] broker VRAM stopped (VRAM zeroed)");
    Ok(())
}

/// RAM broker path (without GPU): same control-plane, backend in heap. Exists to validate the
/// arbitration + swap lifecycle in **QEMU** (ITEM-11), where there is no GPU. Without residency
/// (RAM does not suffer eviction). `Cuda::load()` is never called → runs without libcuda.
fn run_broker_ram(
    slice_bytes: u64,
    slices: u16,
    sock: String,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
    telemetry_jsonl: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let setup_sock = sock.clone();
    run_broker_ram_with_setup(
        slice_bytes,
        slices,
        sock,
        move || {
            broker_setup(
                slices,
                slice_bytes,
                &setup_sock,
                listen_nbd_addr,
                advertise_tcp,
                arbiter_addr,
                telemetry_jsonl,
            )
        },
        Duration::from_millis(500),
    )
}

/// Heap-RAM broker lifecycle using the same bounded worker and cleanup order as
/// the VRAM path. The setup boundary is injectable for a no-driver local proof.
fn run_broker_ram_with_setup<S>(
    slice_bytes: u64,
    slices: u16,
    _sock: String,
    setup: S,
    worker_poll: Duration,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: FnOnce() -> Result<BrokerRuntime, Box<dyn std::error::Error>>,
{
    let total = (slices as u64)
        .checked_mul(slice_bytes)
        .ok_or("--slices * --slice-mb: overflow")?;
    let backend = RamBackend::new(total as usize);
    eprintln!(
        "[ramsharedd] broker RAM (without GPU): {slices} slices x {} MiB = {} MiB, block_size={BLOCK_SIZE}",
        slice_bytes >> 20,
        total >> 20
    );

    let rt = setup()?;
    let (worker, broker, shutdown, socket) = rt.into_parts();
    let broker_monitor = BrokerJoinMonitor::start(broker, shutdown);
    let _ = serve_broker_jobs_with_poll(backend, worker, |_| None, worker_poll); // RAM: no residency

    let broker_result = broker_monitor.finish();
    drop(socket);
    broker_result?;
    eprintln!("[ramsharedd] broker RAM stopped");
    Ok(())
}

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
fn run_ublk(
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MemoryLockStatus {
    Protected,
    ForcedDegraded { locked: bool, oom_ok: bool },
}

/// Pure fail-closed decision after the two process-local protection attempts.
/// `--force` is the sole explicit opt-in to a degraded anti-deadlock posture.
fn memory_lock_status(
    locked: bool,
    oom_ok: bool,
    force: bool,
) -> Result<MemoryLockStatus, &'static str> {
    if !locked && !force {
        return Err("mlockall failed; run as root or use --force");
    }
    if !oom_ok && !force {
        return Err("could not set oom_score_adj=-1000; run as root or use --force");
    }
    if locked && oom_ok {
        Ok(MemoryLockStatus::Protected)
    } else {
        Ok(MemoryLockStatus::ForcedDegraded { locked, oom_ok })
    }
}

/// Locks memory (mlockall) + protects from OOM killer (oom_score_adj=-1000) BEFORE
/// serving swap (Discipline 3, anti-deadlock). `--force` continues without protection, warning.
///
/// `MCL_FUTURE` is deliberately rejected. CUDA, DXG, cache growth, thread
/// stacks, and runtime libraries can all create mappings after this frontier;
/// inheriting the lock into those mappings can turn memory pressure into a
/// process or host stall.
fn lock_memory(force: bool, lock_future: bool) -> Result<(), Box<dyn std::error::Error>> {
    if lock_future {
        return Err("MCL_FUTURE is forbidden before runtime GPU/DXG mappings".into());
    }
    // SAFETY: mlockall is a syscall with no unsafe memory side effects.
    let locked = unsafe { mlockall(MCL_CURRENT) } == 0;
    let oom_ok = std::fs::write("/proc/self/oom_score_adj", "-1000").is_ok();
    match memory_lock_status(locked, oom_ok, force)? {
        MemoryLockStatus::Protected => {
            eprintln!("[ramsharedd] memory locked (mlockall) + oom_score_adj=-1000");
        }
        MemoryLockStatus::ForcedDegraded { locked, oom_ok } => {
            eprintln!(
                "[ramsharedd] WARNING --force: mlockall={} oom_score_adj={} (anti-deadlock NOT guaranteed)",
                if locked { "ok" } else { "FAILED" },
                if oom_ok { "ok" } else { "FAILED" }
            );
        }
    }
    Ok(())
}

// NOTE: `arm_future_lock` (arm future lock post-init) was REMOVED — had a race with the
// asynchronous CUDA init of the worker (spawn_server_dt3_vram_with_residency), which would have
// re-triggered the dxgkrnl kernel BUG. The ublk+vram path remains in MCL_CURRENT only.
// See the "dxgkrnl ANTI-BUG" comment in run_ublk.

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;

    struct TestMemory {
        bytes: RefCell<Vec<u8>>,
    }

    impl TestMemory {
        fn new(bytes: usize) -> Self {
            Self {
                bytes: RefCell::new(vec![0; bytes]),
            }
        }

        fn range(&self, off: u64, len: usize) -> Result<(usize, usize), ramshared_vram::VramError> {
            let start =
                usize::try_from(off).map_err(|_| ramshared_vram::VramError::OutOfRange {
                    off,
                    len: len as u64,
                    size: self.bytes.borrow().len() as u64,
                })?;
            let end =
                start
                    .checked_add(len)
                    .ok_or_else(|| ramshared_vram::VramError::OutOfRange {
                        off,
                        len: len as u64,
                        size: self.bytes.borrow().len() as u64,
                    })?;
            if end > self.bytes.borrow().len() {
                return Err(ramshared_vram::VramError::OutOfRange {
                    off,
                    len: len as u64,
                    size: self.bytes.borrow().len() as u64,
                });
            }
            Ok((start, end))
        }
    }

    impl VramMemory for TestMemory {
        fn len(&self) -> usize {
            self.bytes.borrow().len()
        }

        fn zero(&mut self) -> Result<(), ramshared_vram::VramError> {
            self.bytes.borrow_mut().fill(0);
            Ok(())
        }

        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), ramshared_vram::VramError> {
            let (start, end) = self.range(off, dst.len())?;
            dst.copy_from_slice(&self.bytes.borrow()[start..end]);
            Ok(())
        }

        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), ramshared_vram::VramError> {
            let (start, end) = self.range(off, src.len())?;
            self.bytes.borrow_mut()[start..end].copy_from_slice(src);
            Ok(())
        }
    }

    /// In-memory VRAM that records each secure-wipe request. It lets the
    /// lifecycle refusal tests prove wipe ordering without a GPU context.
    struct ZeroRecordingMemory {
        bytes: usize,
        zeroed: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
    }

    impl VramMemory for ZeroRecordingMemory {
        fn len(&self) -> usize {
            self.bytes
        }

        fn zero(&mut self) -> Result<(), ramshared_vram::VramError> {
            self.zeroed
                .lock()
                .expect("test zero record lock")
                .push(self.bytes);
            Ok(())
        }

        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), ramshared_vram::VramError> {
            Err(ramshared_vram::VramError::OutOfRange {
                off,
                len: dst.len() as u64,
                size: self.bytes as u64,
            })
        }

        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), ramshared_vram::VramError> {
            Err(ramshared_vram::VramError::OutOfRange {
                off,
                len: src.len() as u64,
                size: self.bytes as u64,
            })
        }
    }

    struct ZeroRecordingProvider {
        zeroed: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
    }

    impl VramProvider for ZeroRecordingProvider {
        type Mem<'a>
            = ZeroRecordingMemory
        where
            Self: 'a;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
            Ok(ZeroRecordingMemory {
                bytes,
                zeroed: std::sync::Arc::clone(&self.zeroed),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
            Ok((8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024))
        }
    }

    fn daemon_argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    struct TestOrigin {
        bytes: Vec<u8>,
        fail: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl ramshared_block::OriginStorage for TestOrigin {
        fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, ramshared_block::IoError> {
            if self.fail.get() {
                return Err(ramshared_block::IoError("injected origin failure".into()));
            }
            let start = off as usize;
            let count = buf.len().min(self.bytes.len().saturating_sub(start));
            buf[..count].copy_from_slice(&self.bytes[start..start + count]);
            Ok(count)
        }

        fn write_at(&mut self, off: u64, data: &[u8]) -> Result<usize, ramshared_block::IoError> {
            if self.fail.get() {
                return Err(ramshared_block::IoError("injected origin failure".into()));
            }
            let start = off as usize;
            let count = data.len().min(self.bytes.len().saturating_sub(start));
            self.bytes[start..start + count].copy_from_slice(&data[..count]);
            Ok(count)
        }

        fn sync_data(&mut self) -> Result<(), ramshared_block::IoError> {
            if self.fail.get() {
                Err(ramshared_block::IoError("injected origin failure".into()))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn product_origin_mode_does_not_preallocate_logical_capacity() {
        struct CountingProvider(std::cell::Cell<u64>);
        impl VramProvider for CountingProvider {
            type Mem<'a> = TestMemory;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                self.0.set(self.0.get().saturating_add(bytes as u64));
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                panic!("origin composition must not query the GPU provider")
            }
        }

        let provider = CountingProvider(std::cell::Cell::new(0));
        let fail = std::rc::Rc::new(std::cell::Cell::new(false));
        let origin = TestOrigin {
            bytes: vec![0; BLOCK_SIZE as usize],
            fail,
        };
        let cache = AuthoritativeOriginBackend::new(origin, DisabledCache, GIB, BLOCK_SIZE)
            .expect("valid 1 GiB authoritative origin");

        assert_eq!(cache.size_bytes(), GIB);
        assert_eq!(cache.cached_bytes(), 0);
        assert_eq!(provider.0.get(), 0, "constructor must allocate no VRAM");
        drop(cache);

        struct OriginStarter;
        impl NbdRuntimeStarter for OriginStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert!(
                    !lock_future,
                    "origin mode must never arm MCL_FUTURE before cache mappings"
                );
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                _listener: UnixListener,
                exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                assert_eq!(exports[0].size, GIB);
                jobs_tx.send(WMsg::Shutdown)?;
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                0
            }

            fn nbd_swap_is_explicitly_absent(&mut self, _nbd_dev: &str) -> bool {
                true
            }

            fn publish_demote(
                &mut self,
                _total: u64,
                _reason: &Option<String>,
                _in_progress: bool,
            ) {
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                0
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                panic!("origin-cache clean shutdown must not swapoff an inactive fixture")
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                _priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                panic!("origin-cache fixture must not start recovery activation")
            }

            fn startup_budget(
                &mut self,
                _requested: bool,
            ) -> Result<Option<Box<dyn NbdBudgetProvider>>, Box<dyn std::error::Error>>
            {
                panic!("origin composition must not initialize DXG")
            }
        }

        let root =
            std::env::temp_dir().join(format!("ramshared-origin-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let origin_path = root.join("origin.img");
        let origin_file = File::options()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&origin_path)
            .unwrap();
        origin_file.set_len(GIB).unwrap();
        let socket = root.join("daemon.sock");
        run_nbd_with_startup(
            provider,
            Some(FileOrigin::from_file(origin_file)),
            GIB,
            socket.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            true,
            &mut OriginStarter,
        )
        .expect("tempfile origin mode must compose without a GPU or NBD device");
        assert!(!socket.exists());
        std::fs::remove_dir_all(root).unwrap();

        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--origin-manifest",
            ORIGIN_MANIFEST_PATH,
        ]))
        .expect("sealed origin manifest args");
        assert!(matches!(
            select_daemon_action(args),
            Ok(DaemonAction::Nbd(_))
        ));

        let missing_origin = AppArgs::parse_from(&daemon_argv(&["ramsharedd", "--size", "1024"]))
            .expect("syntactically valid originless size args");
        assert!(select_daemon_action(missing_origin).is_err());

        let unavailable = UnavailableVramProvider;
        assert!(
            unavailable
                .alloc(ramshared_block::ORIGIN_CACHE_CHUNK_BYTES as usize)
                .is_err()
        );
        assert!(unavailable.mem_info().is_err());
        let mut memory = UnavailableVramMemory;
        assert_eq!(memory.len(), 0);
        assert!(memory.zero().is_err());
        assert!(memory.read_at(0, &mut [0; 1]).is_err());
        assert!(memory.write_at(0, &[0; 1]).is_err());
    }

    #[test]
    fn missing_gpu_measurement_sets_zero_cache_target() {
        assert_eq!(ramshared_block::physical_target_bytes(4 * GIB, None), 0);
    }

    #[test]
    fn critical_supervisor_request_is_consumed_and_reclaims_daemon_cache_to_zero() {
        struct AllocatingProvider;
        impl VramProvider for AllocatingProvider {
            type Mem<'a> = TestMemory;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((16 * GIB, 16 * GIB))
            }
        }

        let provider = AllocatingProvider;
        let origin = TestOrigin {
            bytes: vec![0; 32],
            fail: std::rc::Rc::new(std::cell::Cell::new(false)),
        };
        let mut cache =
            WriteThroughCacheBackend::with_chunk_bytes(&provider, origin, 32, 4, 8).unwrap();
        cache.set_physical_cap_bytes(8);
        let sample = Some(GpuSample {
            budget_bytes: 16 * GIB,
            external_usage_bytes: 0,
            total_vram_bytes: 16 * GIB,
        });
        for seconds in 0..3 {
            cache.observe_gpu(sample, Duration::from_secs(seconds));
        }
        assert_eq!(cache.cached_bytes(), 8);

        let request = serde_json::json!({
            "schema_version": 1,
            "target_bytes": 0,
            "reason": "control_pressure",
            "daemon_instance_id": "fixture-daemon",
            "issued_at_unix_ms": 1_000,
        });

        let root = std::env::temp_dir().join(format!(
            "ramshared-critical-cache-request-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let cache_target = root.join("cache-target.json");
        let reclaim_request = root.join("reclaim-request.json");
        std::fs::write(&cache_target, request.to_string()).unwrap();
        assert!(critical_cache_reclaim_requested_at(
            &cache_target,
            &reclaim_request,
            "fixture-daemon",
            1_001,
        ));
        assert_eq!(
            consume_critical_cache_request(&request, "fixture-daemon", 1_001),
            Some(0)
        );
        assert_eq!(cache.release_cache(), 8);
        assert_eq!(cache.cached_bytes(), 0);
        assert_eq!(
            consume_critical_cache_request(&request, "foreign-daemon", 1_001),
            None
        );
        assert_eq!(
            consume_critical_cache_request(&request, "fixture-daemon", 16_001),
            None
        );
        assert_eq!(
            consume_critical_cache_request(
                &serde_json::json!({"target_bytes": 0}),
                "fixture-daemon",
                1_001
            ),
            None
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn origin_and_cache_failures_are_sticky_until_exact_recovery() {
        struct RefusingProvider;
        impl VramProvider for RefusingProvider {
            type Mem<'a> = TestMemory;

            fn alloc(&self, _bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                Err(ramshared_vram::VramError::Provider(
                    "injected cache allocation refusal".into(),
                ))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Err(ramshared_vram::VramError::Provider(
                    "injected missing measurement".into(),
                ))
            }
        }

        let fail = std::rc::Rc::new(std::cell::Cell::new(true));
        let origin = TestOrigin {
            bytes: vec![0; BLOCK_SIZE as usize],
            fail: std::rc::Rc::clone(&fail),
        };
        let provider = RefusingProvider;
        let mut backend = WriteThroughCacheBackend::new(&provider, origin, GIB, BLOCK_SIZE)
            .expect("valid origin-cache fixture");
        assert!(backend.write_at(0, &[0; BLOCK_SIZE as usize]).is_err());
        assert_eq!(backend.origin_state(), DurableOriginState::Failed);
        assert_eq!(
            backend
                .observe_gpu(None, Duration::from_secs(1))
                .target_bytes,
            0
        );
        assert_eq!(backend.cache_state(), OriginCacheState::Unavailable);
        assert!(!origin_cache_runtime_ok(
            backend.origin_state(),
            backend.cache_state()
        ));

        let status = OriginCacheStatus {
            schema_version: 1,
            daemon_instance_id: "fixture-daemon".into(),
            written_at_unix_ms: 1_000,
            ok: origin_cache_runtime_ok(backend.origin_state(), backend.cache_state()),
            origin_state: backend.origin_state().as_str(),
            cache_state: backend.cache_state().as_str(),
            logical_capacity_kib: backend.size_bytes() >> 10,
            vram_cached_kib: backend.cached_bytes() >> 10,
            gpu_headroom_kib: None,
            ssd_origin_written_kib: backend.telemetry().origin_written_bytes >> 10,
            cache_fallback_reads: backend.telemetry().fallback_reads,
            cache_invalidations: backend.telemetry().invalidations,
            cache_releases: backend.telemetry().releases,
            cache_target_kib: backend.target_bytes() >> 10,
        };
        let serialized = serde_json::to_value(&status).unwrap();
        assert_eq!(serialized["origin_state"], "FAILED");
        assert_eq!(serialized["cache_state"], "UNAVAILABLE");
        assert_eq!(serialized["ok"], false);

        let root =
            std::env::temp_dir().join(format!("ramshared-cache-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join("nested/cache-status.json");
        write_origin_cache_status(&path, &status).unwrap();
        write_origin_cache_status(&path, &status).unwrap();
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted["origin_state"], "FAILED");
        let blocker = root.join("not-a-directory");
        std::fs::write(&blocker, b"block").unwrap();
        assert!(write_origin_cache_status(&blocker.join("status.json"), &status).is_err());
        assert!(write_origin_cache_status(Path::new("/"), &status).is_err());
        std::fs::remove_dir_all(root).unwrap();

        fail.set(false);
        assert_eq!(
            backend.probe_origin().unwrap(),
            DurableOriginState::Degraded
        );
        assert_eq!(
            backend.probe_origin().unwrap(),
            DurableOriginState::Degraded
        );
        assert_eq!(backend.probe_origin().unwrap(), DurableOriginState::Ready);
        assert!(origin_cache_runtime_ok(
            backend.origin_state(),
            backend.cache_state()
        ));
    }

    #[test]
    fn origin_manifest_fd_identity_pairs_refusal_with_legitimate_fixture() {
        let text = format!(
            "schema_version=3\n\
             host_manifest_sha256={}\n\
             configuration_sha256={}\n\
             origin_path=/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555\n\
             partuuid=11111111-2222-4333-8444-555555555555\n\
             ptuuid=aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee\n\
             partition_dev_t=43:1\n\
             parent_dev_t=43:0\n\
             expected_swap_uuid=99999999-8888-4777-8666-555555555555\n\
             swap_type=swap\n\
             logical_capacity_mib=4096\n\
             physical_cache_cap_mib=1024\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let manifest = parse_sealed_origin_manifest(&text).unwrap();
        let legitimate = OriginIdentityObservation {
            partuuid: manifest.partuuid.clone(),
            ptuuid: manifest.ptuuid.clone(),
            path_dev_t: manifest.partition_dev_t.clone(),
            fd_dev_t: manifest.partition_dev_t.clone(),
            parent_dev_t: manifest.parent_dev_t.clone(),
            swap_uuid: manifest.expected_swap_uuid.clone(),
            swap_type: "swap".into(),
            origin_size: 25 * GIB,
            critical_dev_ts: vec!["8:0".into(), "8:1".into()],
        };
        validate_origin_manifest_identity(&manifest, &legitimate, 4 * GIB).unwrap();
        validate_stable_critical_device_snapshot(
            &["8:1".into(), "8:0".into(), "8:1".into()],
            &["8:0".into(), "8:1".into()],
        )
        .expect("equivalent critical-device snapshots");
        assert!(
            validate_stable_critical_device_snapshot(
                &["8:0".into(), "8:1".into()],
                &["8:0".into(), "8:2".into()],
            )
            .is_err()
        );
        let null = File::open("/dev/null").expect("open fd-path fixture");
        let fd_path = process_fd_path(&null);
        let named = null.metadata().expect("stat fd-path fixture");
        let through_proc = std::fs::metadata(&fd_path).expect("stat process fd path");
        assert_eq!(named.dev(), through_proc.dev());
        assert_eq!(named.ino(), through_proc.ino());

        for mutated in [
            OriginIdentityObservation {
                fd_dev_t: "43:2".into(),
                ..legitimate.clone()
            },
            OriginIdentityObservation {
                ptuuid: "aaaaaaaa-bbbb-4ccc-8ddd-cccccccccccc".into(),
                ..legitimate.clone()
            },
            OriginIdentityObservation {
                swap_uuid: "99999999-8888-4777-8666-444444444444".into(),
                ..legitimate.clone()
            },
            OriginIdentityObservation {
                critical_dev_ts: vec!["43:1".into()],
                ..legitimate.clone()
            },
            OriginIdentityObservation {
                critical_dev_ts: vec!["43:0".into()],
                ..legitimate.clone()
            },
            OriginIdentityObservation {
                origin_size: GIB,
                ..legitimate.clone()
            },
        ] {
            assert!(validate_origin_manifest_identity(&manifest, &mutated, 4 * GIB).is_err());
        }
        assert!(validate_origin_manifest_identity(&manifest, &legitimate, 8 * GIB).is_err());
        assert_eq!(
            root_mount_source(
                "36 25 8:2 / / rw,relatime - ext4 /dev/sdb2 rw,discard,errors=remount-ro\n"
            ),
            Some("/dev/sdb2")
        );
        assert!(parse_sealed_origin_manifest(&(text + "unknown=value\n")).is_err());
    }

    #[test]
    // TestName: manifest_fd_reader_accepts_one_bounded_regular_file
    fn manifest_fd_reader_accepts_one_bounded_regular_file() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-manifest-valid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("manifest.json");
        std::fs::write(&path, b"{\"schema\":3}\n").unwrap();
        assert_eq!(
            read_bounded_manifest_file(&path, false).unwrap(),
            b"{\"schema\":3}\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    // TestName: manifest_fd_reader_refuses_oversize_symlink_and_nonregular_inputs
    fn manifest_fd_reader_refuses_oversize_symlink_and_nonregular_inputs() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-manifest-types-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let oversized = dir.join("oversized");
        std::fs::write(
            &oversized,
            vec![b'x'; ORIGIN_MANIFEST_MAX_BYTES as usize + 1],
        )
        .unwrap();
        assert!(read_bounded_manifest_file(&oversized, false).is_err());

        let legitimate = dir.join("legitimate");
        std::fs::write(&legitimate, b"bounded").unwrap();
        let symlink = dir.join("symlink");
        std::os::unix::fs::symlink(&legitimate, &symlink).unwrap();
        assert!(read_bounded_manifest_file(&symlink, false).is_err());
        assert!(read_bounded_manifest_file(&dir, false).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    // TestName: manifest_fd_reader_bounds_concurrent_append_at_max_plus_one
    fn manifest_fd_reader_bounds_concurrent_append_at_max_plus_one() {
        use std::io::Write as _;

        let dir =
            std::env::temp_dir().join(format!("ramshared-manifest-append-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("manifest");
        std::fs::write(&path, vec![b'a'; ORIGIN_MANIFEST_MAX_BYTES as usize]).unwrap();
        MANIFEST_READ_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|opened_path| {
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(opened_path)
                    .unwrap();
                file.write_all(&vec![b'b'; 1024 * 1024]).unwrap();
            }));
        });
        let error = read_bounded_manifest_file(&path, false)
            .expect_err("an append after open must be refused");
        assert!(error.contains("bounded read limit"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    // TestName: manifest_fd_reader_refuses_path_replacement_after_open
    fn manifest_fd_reader_refuses_path_replacement_after_open() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-manifest-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("manifest");
        std::fs::write(&path, b"original").unwrap();
        MANIFEST_READ_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|opened_path| {
                std::fs::remove_file(opened_path).unwrap();
                std::fs::write(opened_path, b"replaced").unwrap();
            }));
        });
        assert!(read_bounded_manifest_file(&path, false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"replaced");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn host_manifest_hash_fields_are_enforced_end_to_end() {
        let mut host = HostOriginManifest {
            schema_version: 3,
            origin_vhdx: "I:\\RamShared\\ramshared-origin.vhdx".into(),
            fixed_size_bytes: 25 * GIB,
            logical_capacity_mib: 4096,
            physical_cache_cap_mib: 1024,
            chunk_mib: 128,
            gpu_reserve_min_mib: 2048,
            gpu_reserve_percent: 20,
            partuuid: "11111111-2222-4333-8444-555555555555".into(),
            disk_guid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
            expected_swap_uuid: "99999999-8888-4777-8666-555555555555".into(),
            ownership_proof_schema: 1,
            existing_wsl_swap_vhdx: "R:\\wsl_swap\\swap.vhdx".into(),
            configuration_sha256: String::new(),
        };
        host.configuration_sha256 = sha256_hex(host_configuration_text(&host).as_bytes());
        let bytes = serde_json::to_vec(&host).expect("serialize host origin manifest fixture");
        let sealed = SealedOriginManifest {
            host_manifest_sha256: sha256_hex(&bytes),
            configuration_sha256: host.configuration_sha256.clone(),
            origin_path: "/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555".into(),
            partuuid: host.partuuid.clone(),
            ptuuid: host.disk_guid.clone(),
            partition_dev_t: "43:1".into(),
            parent_dev_t: "43:0".into(),
            expected_swap_uuid: host.expected_swap_uuid.clone(),
            logical_capacity_mib: host.logical_capacity_mib,
            physical_cache_cap_mib: host.physical_cache_cap_mib,
        };
        validate_host_origin_manifest_bytes(&sealed, &bytes)
            .expect("matching host and guest manifests");

        let mut one_byte_tamper = bytes.clone();
        let drive = one_byte_tamper
            .windows(3)
            .position(|window| window == b"I:\\")
            .expect("locate origin drive fixture");
        one_byte_tamper[drive] = b'J';
        assert!(validate_host_origin_manifest_bytes(&sealed, &one_byte_tamper).is_err());

        let mut resealed_raw_only = sealed.clone();
        resealed_raw_only.host_manifest_sha256 = sha256_hex(&one_byte_tamper);
        let error = validate_host_origin_manifest_bytes(&resealed_raw_only, &one_byte_tamper)
            .expect_err("raw hash alone cannot bypass the canonical configuration hash");
        assert!(error.contains("configuration SHA-256"), "{error}");

        host.configuration_sha256 = "f".repeat(64);
        let wrong_configuration =
            serde_json::to_vec(&host).expect("serialize wrong configuration fixture");
        let mut resealed_wrong_configuration = sealed;
        resealed_wrong_configuration.host_manifest_sha256 = sha256_hex(&wrong_configuration);
        assert!(
            validate_host_origin_manifest_bytes(
                &resealed_wrong_configuration,
                &wrong_configuration
            )
            .is_err()
        );
    }

    #[test]
    fn origin_args_default_to_four_gib_and_enforce_one_to_twenty_four_gib() {
        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--origin-manifest",
            ORIGIN_MANIFEST_PATH,
        ]))
        .expect("sealed product origin argv");
        assert_eq!(args.size, 4 * GIB);
        assert_eq!(args.origin.as_deref(), Some(ORIGIN_MANIFEST_PATH));

        for size in ["1023", "24577"] {
            assert!(
                AppArgs::parse_from(&daemon_argv(&[
                    "ramsharedd",
                    "--origin-manifest",
                    ORIGIN_MANIFEST_PATH,
                    "--size",
                    size,
                ]))
                .is_err()
            );
        }

        let ublk = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--origin-manifest",
            ORIGIN_MANIFEST_PATH,
            "--transport",
            "ublk",
        ]))
        .unwrap();
        assert!(select_daemon_action(ublk).is_err());

        let broker = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--origin-manifest",
            ORIGIN_MANIFEST_PATH,
            "--slices",
            "1",
            "--slice-mb",
            "1024",
            "--arbiter-listen",
            "127.0.0.1:7777",
        ]))
        .unwrap();
        assert!(select_daemon_action(broker).is_err());

        assert!(
            AppArgs::parse_from(&daemon_argv(&[
                "ramsharedd",
                "--origin-manifest",
                "/tmp/caller-controlled.conf",
            ]))
            .is_err()
        );
        assert!(
            AppArgs::parse_from(&daemon_argv(&[
                "ramsharedd",
                "--origin",
                "/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555",
            ]))
            .is_err()
        );
    }

    #[test]
    fn physical_cache_cap_is_explicit_bounded_and_defaults_to_one_gib() {
        assert_eq!(parse_physical_cache_cap(None, 4 * GIB).unwrap(), GIB);
        assert_eq!(parse_physical_cache_cap(None, GIB).unwrap(), GIB);
        assert!(parse_physical_cache_cap(None, GIB - 1).is_err());
        assert_eq!(
            parse_physical_cache_cap(Some("1024"), 4 * GIB).unwrap(),
            GIB
        );
        assert!(parse_physical_cache_cap(Some("0"), 4 * GIB).is_err());
        assert!(parse_physical_cache_cap(Some("4097"), 4 * GIB).is_err());
        assert!(parse_physical_cache_cap(Some("invalid"), 4 * GIB).is_err());
    }

    #[test]
    fn origin_mode_refuses_to_start_without_a_valid_daemon_identity() {
        assert!(require_origin_daemon_identity(true, || None).is_err());
        assert_eq!(
            require_origin_daemon_identity(true, || Some("fixture-daemon".into())).unwrap(),
            Some("fixture-daemon".into())
        );
        assert_eq!(
            require_origin_daemon_identity(false, || None).unwrap(),
            None
        );
    }

    #[test]
    fn daemon_residency_probe_and_latency_paths_are_fake_backed() {
        let mut canary = None;
        let mut baseline = Vec::new();
        let mut sampler = ResidencySampler::new(ResidencyConfig {
            latency_mult: 2,
            consecutive: 1,
            free_floor_bytes: 100,
        });
        let mut cadence = Cadence::new(1);
        let mut probe = CanaryProbe::new(TestMemory::new(CANARY_BYTES));
        let mut state = ResidencyCheckState {
            canary: &mut canary,
            baseline: &mut baseline,
            sampler: &mut sampler,
            cadence: &mut cadence,
            probe: &mut probe,
            free_floor_bytes: 100,
        };
        assert_eq!(
            residency_check(10, &mut state, || Some(99)),
            Some(DemoteReason::FreeFloor)
        );

        let mut canary = None;
        let mut baseline = Vec::new();
        let mut sampler = ResidencySampler::new(ResidencyConfig::default());
        let mut cadence = Cadence::new(u32::MAX);
        let mut probe = CanaryProbe::new(TestMemory::new(CANARY_BYTES));
        let mut state = ResidencyCheckState {
            canary: &mut canary,
            baseline: &mut baseline,
            sampler: &mut sampler,
            cadence: &mut cadence,
            probe: &mut probe,
            free_floor_bytes: ResidencyConfig::default().free_floor_bytes,
        };
        for _ in 0..16 {
            assert_eq!(residency_check(10, &mut state, || Some(u64::MAX)), None);
        }
        assert_eq!(residency_check(1_000, &mut state, || Some(u64::MAX)), None);
        assert_eq!(residency_check(1_000, &mut state, || Some(u64::MAX)), None);
        assert_eq!(
            residency_check(1_000, &mut state, || Some(u64::MAX)),
            Some(DemoteReason::Latency)
        );
    }

    #[test]
    fn daemon_zero_window_and_backend_labels_are_deterministic() {
        let mut backend = RamBackend::new(2 * 1024 * 1024);
        backend
            .write_at(0, &vec![0xA5; 2 * 1024 * 1024])
            .expect("fill heap backend");
        zero_window(&mut backend, 512, (1 << 20) + 512).expect("zero selected window");
        let mut zeroed = vec![0xFF; (1 << 20) + 512];
        backend
            .read_at(512, &mut zeroed)
            .expect("read zeroed window");
        assert!(zeroed.iter().all(|byte| *byte == 0));
        let mut untouched = [0u8; 512];
        backend.read_at(0, &mut untouched).expect("read prefix");
        assert!(untouched.iter().all(|byte| *byte == 0xA5));
        assert_eq!(BackendKind::Vram.label(), "vram");
        assert_eq!(BackendKind::Vulkan.label(), "vulkan");
        assert_eq!(BackendKind::Ram.label(), "ram");
    }

    #[test]
    fn daemon_args_accept_broker_wiring_and_normalize_addresses() {
        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--size",
            "257",
            "--sock",
            "/tmp/ramshared-daemon-test.sock",
            "--backend",
            "ram",
            "--slices",
            "2",
            "--slice-mb",
            "64",
            "--listen-nbd",
            "tcp://127.0.0.1:10809",
            "--advertise-nbd",
            "127.0.0.1:10810",
            "--arbiter-listen",
            "127.0.0.1:7777",
            "--telemetry-jsonl",
            "/tmp/ramshared-daemon-test.jsonl",
        ]))
        .expect("safe broker-RAM argv must parse");

        assert_eq!(args.size, 257 * 1024 * 1024);
        assert_eq!(args.slice_bytes, 64 * 1024 * 1024);
        assert!(matches!(args.backend, BackendKind::Ram));
        assert!(matches!(args.transport, Transport::Nbd));
        assert_eq!(args.listen_nbd_addr.unwrap().to_string(), "127.0.0.1:10809");
        assert_eq!(args.arbiter_addr.unwrap().to_string(), "127.0.0.1:7777");
        assert_eq!(args.advertise_tcp, Some(("127.0.0.1".into(), 10810)));
        assert_eq!(
            args.telemetry_jsonl.unwrap(),
            std::path::PathBuf::from("/tmp/ramshared-daemon-test.jsonl")
        );
    }

    #[test]
    fn daemon_args_refuse_invalid_or_unsafe_combinations_before_backend() {
        for argv in [
            daemon_argv(&["ramsharedd", "--unknown"]),
            daemon_argv(&["ramsharedd", "--slices", "1"]),
            daemon_argv(&[
                "ramsharedd",
                "--slices",
                "1",
                "--slice-mb",
                "1",
                "--arbiter-listen",
                "0.0.0.0:7777",
            ]),
            daemon_argv(&["ramsharedd", "--slices", "257", "--slice-mb", "1"]),
            daemon_argv(&["ramsharedd", "--advertise-nbd", "127.0.0.1:10809"]),
            daemon_argv(&[
                "ramsharedd",
                "--transport",
                "ublk",
                "--slices",
                "1",
                "--slice-mb",
                "1",
            ]),
        ] {
            assert!(
                AppArgs::parse_from(&argv).is_err(),
                "unsafe argv unexpectedly parsed: {argv:?}"
            );
        }
    }

    #[test]
    fn daemon_args_cover_flag_boundaries_before_backend() {
        let parsed = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--size",
            "3",
            "--sock",
            "/tmp/ramsharedd-boundary.sock",
            "--force",
            "--nbd",
            "/dev/nbd77",
            "--transport",
            "nbd",
            "--queue-depth",
            "2",
            "--backend",
            "vram",
        ]))
        .expect("all single-daemon scalar flags must parse before backend selection");
        assert_eq!(parsed.size, 3 * 1024 * 1024);
        assert_eq!(parsed.sock, "/tmp/ramsharedd-boundary.sock");
        assert!(parsed.force);
        assert_eq!(parsed.nbd_dev, "/dev/nbd77");
        assert_eq!(parsed.queue_depth, 2);
        assert!(matches!(parsed.transport, Transport::Nbd));
        assert!(matches!(parsed.backend, BackendKind::Vram));

        for argv in [
            daemon_argv(&["ramsharedd", "--size"]),
            daemon_argv(&["ramsharedd", "--size", "not-a-number"]),
            daemon_argv(&["ramsharedd", "--size", "18446744073709551615"]),
            daemon_argv(&["ramsharedd", "--sock"]),
            daemon_argv(&["ramsharedd", "--nbd"]),
            daemon_argv(&["ramsharedd", "--transport"]),
            daemon_argv(&["ramsharedd", "--transport", "tcp"]),
            daemon_argv(&["ramsharedd", "--queue-depth"]),
            daemon_argv(&["ramsharedd", "--queue-depth", "zero"]),
            daemon_argv(&["ramsharedd", "--backend"]),
            daemon_argv(&["ramsharedd", "--backend", "cpu"]),
            daemon_argv(&["ramsharedd", "--slices"]),
            daemon_argv(&["ramsharedd", "--slices", "many"]),
            daemon_argv(&["ramsharedd", "--slice-mb"]),
            daemon_argv(&["ramsharedd", "--slice-mb", "many"]),
            daemon_argv(&["ramsharedd", "--listen-nbd"]),
            daemon_argv(&["ramsharedd", "--listen-nbd", "not-an-address"]),
            daemon_argv(&["ramsharedd", "--arbiter-listen"]),
            daemon_argv(&["ramsharedd", "--arbiter-listen", "not-an-address"]),
            daemon_argv(&["ramsharedd", "--advertise-nbd"]),
            daemon_argv(&["ramsharedd", "--advertise-nbd", "not-an-address"]),
            daemon_argv(&["ramsharedd", "--telemetry-jsonl"]),
        ] {
            assert!(
                AppArgs::parse_from(&argv).is_err(),
                "invalid argv unexpectedly reached a backend plan: {argv:?}"
            );
        }

        let overflow = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--slices",
            "1",
            "--slice-mb",
            "18446744073709551615",
            "--arbiter-listen",
            "127.0.0.1:7777",
        ]));
        assert!(
            overflow.is_err(),
            "slice MiB overflow must refuse before a plan"
        );
    }

    #[test]
    fn daemon_broker_config_preserves_telemetry_and_exact_endpoints() {
        let telemetry = std::path::PathBuf::from("/tmp/ramshared-daemon-telemetry.jsonl");
        let cfg = build_broker_config(
            3,
            "/tmp/ramshared-daemon.sock",
            Some(("127.0.0.1".into(), 10810)),
            "127.0.0.1:7777".parse().expect("loopback address"),
            Some(telemetry.clone()),
        );

        assert_eq!(cfg.listen.to_string(), "127.0.0.1:7777");
        assert_eq!(
            cfg.endpoints.nbd_unix.as_deref(),
            Some("/tmp/ramshared-daemon.sock")
        );
        assert_eq!(cfg.endpoints.nbd_tcp, Some(("127.0.0.1".into(), 10810)));
        assert_eq!(cfg.telemetry_jsonl, Some(telemetry));
        assert_eq!(cfg.slice_io.len(), 3);
        assert_eq!(cfg.vram.free.load(Ordering::Relaxed), 0);
        assert_eq!(cfg.vram.total.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn daemon_broker_ram_binds_loopback_and_cleans_owned_socket() {
        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-broker-{}-{}.sock",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_file(&path);
        let listeners = bind_broker_listeners(
            &path,
            Some("127.0.0.1:0".parse().expect("loopback address")),
        )
        .expect("temporary loopback listeners must bind");

        assert!(
            std::fs::symlink_metadata(&path)
                .expect("owned socket exists")
                .file_type()
                .is_socket()
        );
        assert!(
            listeners
                .tcp
                .as_ref()
                .expect("TCP listener requested")
                .local_addr()
                .expect("TCP listener address")
                .ip()
                .is_loopback()
        );

        listeners.cleanup();
        assert!(
            !path.exists(),
            "cleanup must remove only the Unix socket it bound"
        );
    }

    #[test]
    fn daemon_broker_bind_conflict_refuses_and_preserves_existing_listener() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("occupy loopback");
        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-conflict-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        assert!(
            bind_broker_listeners(
                &path,
                Some(occupied.local_addr().expect("occupied address"))
            )
            .is_err(),
            "an occupied broker port must fail before a worker starts"
        );
        assert!(
            !path.exists(),
            "a failed TCP bind must clean up the Unix socket created for this attempt"
        );
        let probe = std::net::TcpStream::connect(occupied.local_addr().expect("occupied address"));
        assert!(
            probe.is_ok(),
            "the pre-existing listener must remain usable"
        );
    }

    #[test]
    fn daemon_broker_setup_stops_bounded_without_backend() {
        struct RecordingAcceptors {
            unix_started: bool,
            tcp_started: bool,
            exports: usize,
        }

        impl BrokerAcceptorStarter for RecordingAcceptors {
            fn start(
                &mut self,
                unix: UnixListener,
                tcp: Option<std::net::TcpListener>,
                exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                _jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.unix_started = unix.local_addr().is_ok();
                self.tcp_started = tcp
                    .as_ref()
                    .and_then(|listener| listener.local_addr().ok())
                    .is_some_and(|addr| addr.ip().is_loopback());
                self.exports = exports.len();
                Ok(())
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-bounded-broker-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let mut acceptors = RecordingAcceptors {
            unix_started: false,
            tcp_started: false,
            exports: 0,
        };
        let runtime = broker_setup_with_acceptors(
            1,
            4096,
            path.to_str().expect("temporary path is UTF-8"),
            Some("127.0.0.1:0".parse().expect("loopback NBD listener")),
            Some(("127.0.0.1".into(), 10809)),
            "127.0.0.1:0".parse().expect("loopback arbiter listener"),
            None,
            std::sync::Arc::clone(&shutdown),
            Duration::from_millis(1),
            &mut acceptors,
        )
        .expect("safe temporary broker control plane must start");
        assert!(
            acceptors.unix_started,
            "temporary Unix listener was handed off"
        );
        assert!(
            acceptors.tcp_started,
            "loopback TCP listener was handed off"
        );
        assert_eq!(acceptors.exports, 1, "slice geometry becomes one export");

        assert!(matches!(
            runtime.shutdown.request(),
            BrokerShutdownWake::Queued | BrokerShutdownWake::QueueFull
        ));
        let (worker, broker, _shutdown, socket) = runtime.into_parts();
        let _backend = serve_broker_jobs_with_poll(
            RamBackend::new(4096),
            worker,
            |_| None,
            Duration::from_millis(1),
        );
        broker
            .join()
            .expect("broker exits after its injected shutdown");
        drop(socket);
        assert!(
            !path.exists(),
            "owned temporary socket is cleaned after stop"
        );
    }

    #[test]
    fn daemon_broker_acceptor_failure_rolls_back_owned_socket() {
        struct FailingAcceptors;

        impl BrokerAcceptorStarter for FailingAcceptors {
            fn start(
                &mut self,
                _unix: UnixListener,
                _tcp: Option<std::net::TcpListener>,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                _jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Err(std::io::Error::other("injected acceptor failure").into())
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-rollback-broker-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let result = broker_setup_with_acceptors(
            1,
            4096,
            path.to_str().expect("temporary path is UTF-8"),
            None,
            None,
            "127.0.0.1:0".parse().expect("loopback arbiter listener"),
            None,
            std::sync::Arc::clone(&shutdown),
            Duration::from_millis(1),
            &mut FailingAcceptors,
        );
        let error = match result {
            Ok(_) => panic!("injected acceptor failure must refuse startup"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("injected acceptor failure"));
        assert!(
            shutdown.load(Ordering::SeqCst),
            "failure signals broker shutdown"
        );
        assert!(
            !path.exists(),
            "failure removes only the temporary Unix socket created by this attempt"
        );
    }

    #[test]
    fn daemon_nbd_serves_two_connection_generations_before_explicit_shutdown() {
        struct TestProvider;

        impl VramProvider for TestProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024))
            }
        }

        struct SafeStarter {
            lock_calls: usize,
            saw_socket: bool,
            zero_done: Option<std::sync::mpsc::Receiver<bool>>,
            reply: Option<std::sync::mpsc::Receiver<Reply>>,
            status_updates: Vec<(u64, Option<String>, bool)>,
        }

        impl NbdRuntimeStarter for SafeStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.lock_calls += 1;
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                listener: UnixListener,
                exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.saw_socket = listener.local_addr().is_ok()
                    && exports.len() == 1
                    && exports[0].name == "default"
                    && exports[0].size == 4096;
                let (zero_tx, zero_rx) = std::sync::mpsc::channel();
                let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                jobs_tx.send(WMsg::Opened)?;
                jobs_tx.send(WMsg::ZeroExport {
                    base: 0,
                    len: 512,
                    done: zero_tx,
                })?;
                jobs_tx.send(WMsg::Job(ramshared_wsl2d::conn::Job {
                    export: 0,
                    req: ramshared_block::Request {
                        flags: 0,
                        cmd: Command::Write,
                        handle: 99,
                        offset: 0,
                        len: 512,
                    },
                    payload: vec![0xC3; 512],
                    reply: reply_tx.clone(),
                }))?;
                jobs_tx.send(WMsg::Closed)?;
                jobs_tx.send(WMsg::Opened)?;
                jobs_tx.send(WMsg::Job(ramshared_wsl2d::conn::Job {
                    export: 0,
                    req: ramshared_block::Request {
                        flags: 0,
                        cmd: Command::Write,
                        handle: 100,
                        offset: 512,
                        len: 512,
                    },
                    payload: vec![0x5A; 512],
                    reply: reply_tx,
                }))?;
                jobs_tx.send(WMsg::Closed)?;
                jobs_tx.send(WMsg::Shutdown)?;
                self.zero_done = Some(zero_rx);
                self.reply = Some(reply_rx);
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                0
            }

            fn nbd_swap_is_explicitly_absent(&mut self, _nbd_dev: &str) -> bool {
                true
            }

            fn publish_demote(&mut self, total: u64, reason: &Option<String>, in_progress: bool) {
                self.status_updates
                    .push((total, reason.clone(), in_progress));
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                10
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                panic!("the safe sparse fixture must not request swapoff")
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                _priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                panic!("the safe sparse fixture must not activate swap")
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-safe-nbd-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut starter = SafeStarter {
            lock_calls: 0,
            saw_socket: false,
            zero_done: None,
            reply: None,
            status_updates: Vec::new(),
        };
        run_nbd_with_startup(
            TestProvider,
            None,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
            &mut starter,
        )
        .expect("fake sparse worker must complete without a CUDA, swap, or NBD device");

        assert_eq!(starter.lock_calls, 1);
        assert!(
            starter.saw_socket,
            "temporary Unix listener reached injected starter"
        );
        assert_eq!(
            starter
                .zero_done
                .take()
                .expect("zero completion receiver")
                .recv_timeout(Duration::from_secs(1)),
            Ok(true)
        );
        let replies = starter
            .reply
            .take()
            .expect("worker reply receiver")
            .try_iter()
            .collect::<Vec<_>>();
        assert_eq!(replies.len(), 2, "both connection generations are served");
        assert!(replies.iter().all(|reply| !reply.disconnect));
        assert_eq!(starter.status_updates, vec![(0, None, false)]);
        assert!(
            !path.exists(),
            "temporary socket is removed after fake worker teardown"
        );
    }

    #[test]
    fn daemon_nbd_explicit_shutdown_wakes_idle_runtime_without_timer_dependence() {
        static TEST_SHUTDOWN: AtomicBool = AtomicBool::new(false);
        TEST_SHUTDOWN.store(false, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let bridge = spawn_nbd_shutdown_bridge(&TEST_SHUTDOWN, tx, Duration::from_millis(5));
        assert!(
            rx.try_recv().is_err(),
            "shutdown is not emitted before approval"
        );

        TEST_SHUTDOWN.store(true, Ordering::SeqCst);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(1)),
            Ok(WMsg::Shutdown)
        ));
        drop(bridge);
        TEST_SHUTDOWN.store(false, Ordering::SeqCst);
    }

    #[test]
    fn daemon_nbd_shutdown_bridge_full_or_disconnected_queue_is_nonblocking() {
        static FULL_SHUTDOWN: AtomicBool = AtomicBool::new(false);
        FULL_SHUTDOWN.store(false, Ordering::SeqCst);
        let (full_tx, full_rx) = std::sync::mpsc::sync_channel(1);
        full_tx.send(WMsg::Opened).expect("fill bounded queue");
        let full_bridge =
            spawn_nbd_shutdown_bridge(&FULL_SHUTDOWN, full_tx, Duration::from_millis(5));
        FULL_SHUTDOWN.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));
        let started = Instant::now();
        drop(full_bridge);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(matches!(full_rx.recv(), Ok(WMsg::Opened)));
        assert!(
            full_rx.try_recv().is_err(),
            "a full queue is never blocked on"
        );
        FULL_SHUTDOWN.store(false, Ordering::SeqCst);

        static DISCONNECTED_SHUTDOWN: AtomicBool = AtomicBool::new(false);
        DISCONNECTED_SHUTDOWN.store(false, Ordering::SeqCst);
        let (disconnected_tx, disconnected_rx) = std::sync::mpsc::sync_channel(1);
        drop(disconnected_rx);
        let disconnected_bridge = spawn_nbd_shutdown_bridge(
            &DISCONNECTED_SHUTDOWN,
            disconnected_tx,
            Duration::from_millis(5),
        );
        DISCONNECTED_SHUTDOWN.store(true, Ordering::SeqCst);
        drop(disconnected_bridge);
        DISCONNECTED_SHUTDOWN.store(false, Ordering::SeqCst);
    }

    #[test]
    fn daemon_nbd_sparse_floor_refusal_reclaims_without_provider_allocation() {
        struct FloorProvider;

        impl VramProvider for FloorProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                assert_eq!(
                    bytes, CANARY_BYTES,
                    "sparse write must refuse before chunk allocation"
                );
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((0, 8 * 1024 * 1024 * 1024))
            }
        }

        struct SparseStarter {
            replies: Option<std::sync::mpsc::Receiver<Reply>>,
            used_kb_calls: usize,
        }

        impl NbdRuntimeStarter for SparseStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                _listener: UnixListener,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                jobs_tx.send(WMsg::Opened)?;
                jobs_tx.send(WMsg::Job(ramshared_wsl2d::conn::Job {
                    export: 0,
                    req: ramshared_block::Request {
                        flags: 0,
                        cmd: Command::Write,
                        handle: 1,
                        offset: 0,
                        len: 512,
                    },
                    payload: vec![0x9A; 512],
                    reply: reply_tx.clone(),
                }))?;
                jobs_tx.send(WMsg::Job(ramshared_wsl2d::conn::Job {
                    export: 0,
                    req: ramshared_block::Request {
                        flags: 0,
                        cmd: Command::Flush,
                        handle: 2,
                        offset: 0,
                        len: 0,
                    },
                    payload: Vec::new(),
                    reply: reply_tx,
                }))?;
                jobs_tx.send(WMsg::Closed)?;
                jobs_tx.send(WMsg::Shutdown)?;
                self.replies = Some(reply_rx);
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                self.used_kb_calls += 1;
                0
            }

            fn nbd_swap_is_explicitly_absent(&mut self, _nbd_dev: &str) -> bool {
                true
            }

            fn publish_demote(
                &mut self,
                _total: u64,
                _reason: &Option<String>,
                _in_progress: bool,
            ) {
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                10
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                panic!("free-floor refusal must not request swapoff")
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                _priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                panic!("free-floor refusal must not activate swap")
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-sparse-floor-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut starter = SparseStarter {
            replies: None,
            used_kb_calls: 0,
        };
        run_nbd_with_startup(
            FloorProvider,
            None,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
            &mut starter,
        )
        .expect("sparse floor refusal remains a safe daemon outcome");
        assert!(
            starter.used_kb_calls >= 2,
            "worker and teardown use injected usage evidence"
        );
        assert_eq!(
            starter
                .replies
                .take()
                .expect("reply receiver")
                .try_iter()
                .count(),
            2
        );
        assert!(
            !path.exists(),
            "sparse safe teardown removes its temporary socket"
        );
    }

    #[test]
    fn daemon_nbd_budget_poll_uses_injected_wddm_snapshot_and_global_probe() {
        struct BudgetProvider;

        impl VramProvider for BudgetProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                assert_eq!(
                    bytes, CANARY_BYTES,
                    "poll fixture must not allocate sparse chunks"
                );
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024))
            }
        }

        struct FreshBudget(std::rc::Rc<std::cell::Cell<usize>>);

        impl NbdBudgetProvider for FreshBudget {
            fn snapshot(&self) -> Result<NbdBudgetSnapshot, String> {
                self.0.set(self.0.get() + 1);
                Ok(NbdBudgetSnapshot {
                    budget: 4 * 1024 * 1024 * 1024,
                    current_usage: 0,
                    sampled_at: Instant::now(),
                })
            }
        }

        struct BudgetStarter {
            budget: Option<Box<dyn NbdBudgetProvider>>,
            budget_calls: std::rc::Rc<std::cell::Cell<usize>>,
            global_calls: usize,
        }

        impl NbdRuntimeStarter for BudgetStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                _listener: UnixListener,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                let (reply_tx, _reply_rx) = std::sync::mpsc::channel();
                jobs_tx.send(WMsg::Opened)?;
                jobs_tx.send(WMsg::Job(ramshared_wsl2d::conn::Job {
                    export: 0,
                    req: ramshared_block::Request {
                        flags: 0,
                        cmd: Command::Flush,
                        handle: 7,
                        offset: 0,
                        len: 0,
                    },
                    payload: Vec::new(),
                    reply: reply_tx,
                }))?;
                jobs_tx.send(WMsg::Closed)?;
                jobs_tx.send(WMsg::Shutdown)?;
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                0
            }

            fn nbd_swap_is_explicitly_absent(&mut self, _nbd_dev: &str) -> bool {
                true
            }

            fn publish_demote(
                &mut self,
                _total: u64,
                _reason: &Option<String>,
                _in_progress: bool,
            ) {
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                10
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                panic!("healthy fresh budget must not request swapoff")
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                _priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                panic!("healthy fresh budget must not activate swap")
            }

            fn startup_budget(
                &mut self,
                requested: bool,
            ) -> Result<Option<Box<dyn NbdBudgetProvider>>, Box<dyn std::error::Error>>
            {
                assert!(requested, "test requests the WDDM budget path");
                Ok(self.budget.take())
            }

            fn global_free_bytes(&mut self, _timeout: Duration) -> Option<u64> {
                self.global_calls += 1;
                Some(8 * 1024 * 1024 * 1024)
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-budget-poll-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut starter = BudgetStarter {
            budget: Some(Box::new(FreshBudget(std::rc::Rc::clone(&calls)))),
            budget_calls: std::rc::Rc::clone(&calls),
            global_calls: 0,
        };
        run_nbd_with_startup(
            BudgetProvider,
            None,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            true,
            &mut starter,
        )
        .expect("fresh injected budget keeps sparse daemon available");
        assert!(
            starter.budget_calls.get() >= 1,
            "budget snapshot was polled"
        );
        assert!(
            starter.global_calls >= 1,
            "global free probe was bounded and polled"
        );
        assert!(
            !path.exists(),
            "budget poll fixture cleans its temporary socket"
        );
    }

    #[test]
    fn daemon_nbd_budget_constraint_demotes_then_recovers_with_fake_swap() {
        struct BudgetProvider;

        impl VramProvider for BudgetProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                assert_eq!(
                    bytes, CANARY_BYTES,
                    "recovery fixture has no sparse allocation"
                );
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024))
            }
        }

        struct SequenceBudget(
            std::cell::RefCell<std::collections::VecDeque<Result<NbdBudgetSnapshot, String>>>,
        );

        impl NbdBudgetProvider for SequenceBudget {
            fn snapshot(&self) -> Result<NbdBudgetSnapshot, String> {
                self.0
                    .borrow_mut()
                    .pop_front()
                    .expect("one budget sample per injected worker tick")
            }
        }

        struct RecoveryStarter {
            budget: Option<Box<dyn NbdBudgetProvider>>,
            swapoff_calls: usize,
            activate_calls: usize,
            statuses: Vec<(u64, Option<String>, bool)>,
        }

        impl NbdRuntimeStarter for RecoveryStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                _listener: UnixListener,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                let (reply_tx, _reply_rx) = std::sync::mpsc::channel();
                jobs_tx.send(WMsg::Opened)?;
                for handle in 0..3 {
                    jobs_tx.send(WMsg::Job(ramshared_wsl2d::conn::Job {
                        export: 0,
                        req: ramshared_block::Request {
                            flags: 0,
                            cmd: Command::Flush,
                            handle,
                            offset: 0,
                            len: 0,
                        },
                        payload: Vec::new(),
                        reply: reply_tx.clone(),
                    }))?;
                }
                jobs_tx.send(WMsg::Closed)?;
                jobs_tx.send(WMsg::Shutdown)?;
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                0
            }

            fn publish_demote(&mut self, total: u64, reason: &Option<String>, in_progress: bool) {
                self.statuses.push((total, reason.clone(), in_progress));
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                10
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                self.swapoff_calls += 1;
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send(true).expect("in-memory swapoff completion");
                rx
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                assert_eq!(priority, 100);
                self.activate_calls += 1;
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send(true).expect("in-memory recovery completion");
                Ok(rx)
            }

            fn startup_budget(
                &mut self,
                requested: bool,
            ) -> Result<Option<Box<dyn NbdBudgetProvider>>, Box<dyn std::error::Error>>
            {
                assert!(requested);
                Ok(self.budget.take())
            }

            fn global_free_bytes(&mut self, _timeout: Duration) -> Option<u64> {
                Some(8 * 1024 * 1024 * 1024)
            }
        }

        let healthy = || NbdBudgetSnapshot {
            budget: 4 * 1024 * 1024 * 1024,
            current_usage: 0,
            sampled_at: Instant::now(),
        };
        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-budget-recovery-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut starter = RecoveryStarter {
            budget: Some(Box::new(SequenceBudget(std::cell::RefCell::new(
                [
                    Err("injected stale WDDM sample".into()),
                    Ok(healthy()),
                    Ok(healthy()),
                    Ok(healthy()),
                ]
                .into(),
            )))),
            swapoff_calls: 0,
            activate_calls: 0,
            statuses: Vec::new(),
        };
        run_nbd_with_startup(
            BudgetProvider,
            None,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            true,
            &mut starter,
        )
        .expect("healthy hysteresis recovers the fake sparse daemon");
        assert_eq!(
            starter.swapoff_calls, 2,
            "the recovered fake swap must receive a second, confirmed teardown swapoff"
        );
        assert_eq!(starter.activate_calls, 1);
        assert_eq!(
            starter.statuses,
            vec![
                (0, None, false),
                (0, Some("WddmBudgetPoll".into()), true),
                (1, Some("WddmBudgetPoll".into()), false),
            ]
        );
        assert!(
            !path.exists(),
            "recovery fixture cleans its temporary socket"
        );
    }

    #[test]
    fn daemon_nbd_recovery_activation_does_not_block_nbd_jobs() {
        fn assert_nbd_ok(reply: Reply, context: &str) {
            assert!(
                !reply.disconnect,
                "{context} must keep the NBD connection open"
            );
            assert_eq!(
                [
                    reply.reply[4],
                    reply.reply[5],
                    reply.reply[6],
                    reply.reply[7]
                ],
                [0; 4],
                "{context} must return NBD_OK"
            );
        }

        struct TestProvider;

        impl VramProvider for TestProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024))
            }
        }

        struct RecoveryBudget {
            calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }

        impl NbdBudgetProvider for RecoveryBudget {
            fn snapshot(&self) -> Result<NbdBudgetSnapshot, String> {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err("injected initial constrained budget".into());
                }
                Ok(NbdBudgetSnapshot {
                    budget: 4 * 1024 * 1024 * 1024,
                    current_usage: 0,
                    sampled_at: Instant::now(),
                })
            }
        }

        struct PendingRecoveryStarter {
            budget: Option<Box<dyn NbdBudgetProvider>>,
            jobs_tx: std::sync::mpsc::Sender<std::sync::mpsc::SyncSender<WMsg>>,
            activation_started: std::sync::mpsc::Sender<()>,
            activation_rx: Option<std::sync::mpsc::Receiver<bool>>,
            activation_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
            swapoff_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }

        struct ActivationOutcomeGuard(Option<std::sync::mpsc::Sender<bool>>);

        impl ActivationOutcomeGuard {
            fn succeed(&mut self) {
                self.0
                    .take()
                    .expect("the test still owns the activation outcome")
                    .send(true)
                    .expect("observe terminal activation success");
            }
        }

        impl Drop for ActivationOutcomeGuard {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(false);
                }
            }
        }

        impl NbdRuntimeStarter for PendingRecoveryStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                _listener: UnixListener,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                self.jobs_tx.send(jobs_tx)?;
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                0
            }

            fn publish_demote(
                &mut self,
                _total: u64,
                _reason: &Option<String>,
                _in_progress: bool,
            ) {
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                10
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                self.swapoff_calls.fetch_add(1, Ordering::SeqCst);
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send(true).expect("in-memory demote completion");
                rx
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                assert_eq!(priority, 100);
                self.activation_calls.fetch_add(1, Ordering::SeqCst);
                self.activation_started.send(())?;
                self.activation_rx
                    .take()
                    .ok_or_else(|| std::io::Error::other("duplicate recovery activation"))
                    .map_err(Into::into)
            }

            fn startup_budget(
                &mut self,
                requested: bool,
            ) -> Result<Option<Box<dyn NbdBudgetProvider>>, Box<dyn std::error::Error>>
            {
                assert!(requested, "the test drives the bounded recovery path");
                Ok(self.budget.take())
            }

            fn global_free_bytes(&mut self, _timeout: Duration) -> Option<u64> {
                Some(8 * 1024 * 1024 * 1024)
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-pending-recovery-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let (jobs_tx, jobs_rx) = std::sync::mpsc::channel();
        let (activation_started_tx, activation_started_rx) = std::sync::mpsc::channel();
        let (activation_result_tx, activation_result_rx) = std::sync::mpsc::channel();
        let mut activation_outcome = ActivationOutcomeGuard(Some(activation_result_tx));
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let activation_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let budget_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread_path = path.clone();
        let thread_activation_calls = std::sync::Arc::clone(&activation_calls);
        let swapoff_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread_swapoff_calls = std::sync::Arc::clone(&swapoff_calls);
        let runner = std::thread::spawn(move || {
            let mut starter = PendingRecoveryStarter {
                budget: Some(Box::new(RecoveryBudget {
                    calls: budget_calls,
                })),
                jobs_tx,
                activation_started: activation_started_tx,
                activation_rx: Some(activation_result_rx),
                activation_calls: thread_activation_calls,
                swapoff_calls: thread_swapoff_calls,
            };
            let result = run_nbd_with_startup(
                TestProvider,
                None,
                4096,
                thread_path.to_string_lossy().into_owned(),
                false,
                "/dev/ramshared-test-nbd".into(),
                true,
                &mut starter,
            )
            .map_err(|error| error.to_string());
            let _ = done_tx.send(result);
        });

        let worker_tx = jobs_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("injected acceptor exposes the live NBD worker");
        worker_tx.send(WMsg::Opened).expect("open NBD generation");
        for handle in 0..3 {
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            worker_tx
                .send(WMsg::Job(ramshared_wsl2d::conn::Job {
                    export: 0,
                    req: ramshared_block::Request {
                        flags: 0,
                        cmd: Command::Flush,
                        handle,
                        offset: 0,
                        len: 0,
                    },
                    payload: Vec::new(),
                    reply: reply_tx,
                }))
                .expect("drive one bounded recovery sample");
            assert_nbd_ok(
                reply_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("pre-recovery NBD job reply"),
                "pre-recovery NBD job",
            );
        }
        activation_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the third healthy sample starts one pending activation");

        worker_tx
            .send(WMsg::Shutdown)
            .expect("request shutdown while activation is pending");
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        worker_tx
            .send(WMsg::Job(ramshared_wsl2d::conn::Job {
                export: 0,
                req: ramshared_block::Request {
                    flags: 0,
                    cmd: Command::Flush,
                    handle: 99,
                    offset: 0,
                    len: 0,
                },
                payload: Vec::new(),
                reply: reply_tx,
            }))
            .expect("queue NBD work after pending shutdown");
        assert_nbd_ok(
            reply_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("pending recovery must not block the NBD serve loop"),
            "post-shutdown pending-recovery NBD job",
        );
        assert_eq!(
            activation_calls.load(Ordering::SeqCst),
            1,
            "a pending activation suppresses duplicate recovery launches"
        );
        assert!(
            path.exists(),
            "shutdown cannot clean the owned socket while swapon remains unobserved"
        );

        activation_outcome.succeed();
        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("daemon exits after the terminal activation outcome"),
            Ok(())
        );
        runner
            .join()
            .expect("pending-recovery test daemon thread joins");
        assert_eq!(
            swapoff_calls.load(Ordering::SeqCst),
            2,
            "recovery success during shutdown must start and observe final swapoff"
        );
        assert!(
            !path.exists(),
            "the owned socket is removed only after terminal recovery observation"
        );

        let (success_tx, success_rx) = std::sync::mpsc::channel();
        let mut success = RecoveryActivation::default();
        success
            .start(success_rx)
            .expect("the successful activation receiver is owned");
        success_tx
            .send(true)
            .expect("inject terminal swapon success");
        assert_eq!(success.poll(), RecoveryActivationPoll::Succeeded);
        assert!(
            success.launch_allowed(true, true),
            "a successful terminal result returns the tier to the available state"
        );
    }

    #[test]
    fn daemon_nbd_recovery_failure_parks_without_relaunch() {
        let mut dispatch_failure = RecoveryActivation::default();
        dispatch_failure.mark_dispatch_failure();
        assert!(
            !dispatch_failure.launch_allowed(true, true),
            "a failed activation dispatch must park the current healthy epoch"
        );
        assert!(!dispatch_failure.launch_allowed(false, true));
        assert!(dispatch_failure.launch_allowed(true, true));

        let (activation_tx, activation_rx) = std::sync::mpsc::channel();
        let mut activation = RecoveryActivation::default();
        activation
            .start(activation_rx)
            .expect("the first recovery activation is owned");
        activation_tx
            .send(false)
            .expect("inject terminal swapon failure");

        assert_eq!(activation.poll(), RecoveryActivationPoll::Failed);
        assert!(
            !activation.launch_allowed(true, true),
            "one failed healthy epoch must not relaunch swapon"
        );
        assert!(!activation.launch_allowed(false, true));
        assert!(
            activation.launch_allowed(true, true),
            "an unhealthy observation starts a later recovery epoch"
        );

        let (disconnected_tx, disconnected_rx) = std::sync::mpsc::channel();
        drop(disconnected_tx);
        let mut disconnected = RecoveryActivation::default();
        disconnected
            .start(disconnected_rx)
            .expect("the disconnected activation receiver is owned");
        assert_eq!(disconnected.poll(), RecoveryActivationPoll::Failed);
        assert!(
            !disconnected.launch_allowed(true, true),
            "a disconnected activation receiver must also park the healthy epoch"
        );
    }

    #[test]
    fn daemon_nbd_shutdown_with_pending_recovery_fails_closed() {
        let (activation_tx, activation_rx) = std::sync::mpsc::channel();
        let mut activation = RecoveryActivation::default();
        activation
            .start(activation_rx)
            .expect("the recovery activation is owned");
        activation.request_shutdown();

        assert_eq!(activation.poll(), RecoveryActivationPoll::Pending);
        assert!(
            !activation.backend_release_allowed(),
            "an unobserved swapon child must retain the backend"
        );
        activation_tx
            .send(false)
            .expect("inject terminal swapon failure");
        assert_eq!(activation.poll(), RecoveryActivationPoll::Failed);
        assert!(activation.backend_release_allowed());
    }

    #[test]
    fn daemon_nbd_teardown_refuses_until_fake_usage_and_swapoff_confirm() {
        struct TestProvider;

        impl VramProvider for TestProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024))
            }
        }

        struct TeardownStarter {
            usage: std::collections::VecDeque<u64>,
            swapoff_calls: usize,
        }

        impl NbdRuntimeStarter for TeardownStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                _listener: UnixListener,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                jobs_tx.send(WMsg::Opened)?;
                jobs_tx.send(WMsg::Closed)?;
                jobs_tx.send(WMsg::Shutdown)?;
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                self.usage
                    .pop_front()
                    .expect("one injected usage observation per teardown step")
            }

            fn publish_demote(
                &mut self,
                _total: u64,
                _reason: &Option<String>,
                _in_progress: bool,
            ) {
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                10
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                self.swapoff_calls += 1;
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send(true).expect("fake swapoff completion");
                rx
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                _priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                panic!("teardown refusal must not activate swap")
            }

            fn teardown_retry_delay(&mut self) -> Duration {
                Duration::ZERO
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-teardown-refusal-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut starter = TeardownStarter {
            // Opened/reclaim observes 64 KiB. Teardown then observes zero,
            // performs the injected swapoff, and revalidates zero before
            // releasing the backend.
            usage: [64, 0, 0].into(),
            swapoff_calls: 0,
        };
        run_nbd_with_startup(
            TestProvider,
            None,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
            &mut starter,
        )
        .expect("confirmed fake swapoff and zero usage release the fake backend");
        assert_eq!(starter.swapoff_calls, 1);
        assert!(
            starter.usage.is_empty(),
            "all teardown observations were consumed"
        );
        assert!(
            !path.exists(),
            "teardown fixture cleans its temporary socket"
        );
    }

    #[test]
    // TestName: daemon_nbd_shutdown_attempts_swapoff_when_usage_is_zero_or_absent
    fn daemon_nbd_shutdown_attempts_swapoff_when_usage_is_zero_or_absent() {
        struct TestProvider;

        impl VramProvider for TestProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((8 * GIB, 8 * GIB))
            }
        }

        struct AbsentSwapStarter {
            swapoff_calls: usize,
        }

        impl NbdRuntimeStarter for AbsentSwapStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                _listener: UnixListener,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                jobs_tx.send(WMsg::Shutdown)?;
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                0
            }

            fn nbd_swap_is_explicitly_absent(&mut self, _nbd_dev: &str) -> bool {
                false
            }

            fn publish_demote(
                &mut self,
                _total: u64,
                _reason: &Option<String>,
                _in_progress: bool,
            ) {
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                0
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                self.swapoff_calls += 1;
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send(true).expect("fake absent swapoff confirmation");
                rx
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                _priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                panic!("direct shutdown must not activate swap")
            }

            fn teardown_retry_delay(&mut self) -> Duration {
                Duration::ZERO
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-absent-swap-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut starter = AbsentSwapStarter { swapoff_calls: 0 };
        run_nbd_with_startup(
            TestProvider,
            None,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
            &mut starter,
        )
        .expect("explicit absent-state swapoff confirmation releases the backend");
        assert_eq!(starter.swapoff_calls, 1, "zero usage bypassed swapoff");
        assert!(!path.exists(), "terminal cleanup leaked the socket");
    }

    #[test]
    fn daemon_nbd_residency_demote_uses_injected_clock_and_swapoff() {
        struct TestProvider;

        impl VramProvider for TestProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                // Three cadence samples below the configured floor produce a
                // FreeFloor demotion, which remains a valid sparse swapoff
                // reason. Latency alone is intentionally non-destructive.
                Ok((1, 8 * 1024 * 1024 * 1024))
            }
        }

        struct DemoteStarter {
            latencies: std::collections::VecDeque<u64>,
            swapoff_calls: usize,
            replies: Option<std::sync::mpsc::Receiver<Reply>>,
            producer: Option<std::thread::JoinHandle<()>>,
            status_updates: Vec<(u64, Option<String>, bool)>,
        }

        impl NbdRuntimeStarter for DemoteStarter {
            fn lock_memory(
                &mut self,
                _force: bool,
                _lock_future: bool,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }

            fn start_acceptor(
                &mut self,
                _listener: UnixListener,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                self.replies = Some(reply_rx);
                self.producer = Some(std::thread::spawn(move || {
                    jobs_tx.send(WMsg::Opened).expect("open fake NBD client");
                    for handle in 0..192 {
                        jobs_tx
                            .send(WMsg::Job(ramshared_wsl2d::conn::Job {
                                export: 0,
                                req: ramshared_block::Request {
                                    flags: 0,
                                    cmd: Command::Write,
                                    handle,
                                    offset: 0,
                                    len: 512,
                                },
                                payload: vec![0x3C; 512],
                                reply: reply_tx.clone(),
                            }))
                            .expect("queue fake NBD write");
                    }
                    jobs_tx.send(WMsg::Closed).expect("close fake NBD client");
                    jobs_tx
                        .send(WMsg::Shutdown)
                        .expect("request fake NBD shutdown");
                }));
                Ok(())
            }

            fn nbd_used_kb(&mut self, _nbd_dev: &str) -> u64 {
                0
            }

            fn publish_demote(&mut self, total: u64, reason: &Option<String>, in_progress: bool) {
                self.status_updates
                    .push((total, reason.clone(), in_progress));
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                self.latencies
                    .pop_front()
                    .expect("one injected latency per worker job")
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                self.swapoff_calls += 1;
                let (tx, rx) = std::sync::mpsc::channel();
                tx.send(true).expect("in-memory swapoff completion");
                rx
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                _priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                panic!("a successful terminal DEMOTE must not attempt recovery")
            }
        }

        let path = std::env::temp_dir().join(format!(
            "ramshared-daemon-fake-demote-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut starter = DemoteStarter {
            latencies: std::iter::repeat_n(10, 192).collect(),
            swapoff_calls: 0,
            replies: None,
            producer: None,
            status_updates: Vec::new(),
        };
        run_nbd_with_startup(
            TestProvider,
            None,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
            &mut starter,
        )
        .expect("injected terminal DEMOTE must tear down the fake sparse backend");
        starter
            .producer
            .take()
            .expect("injected producer exists")
            .join()
            .expect("injected producer completes");

        assert_eq!(
            starter.swapoff_calls, 1,
            "one DEMOTE starts one bounded swapoff"
        );
        assert_eq!(
            starter.status_updates,
            vec![
                (0, None, false),
                (0, Some("FreeFloor".into()), true),
                (1, Some("FreeFloor".into()), false),
            ]
        );
        let replies = starter
            .replies
            .take()
            .expect("injected reply receiver")
            .try_iter()
            .count();
        assert_eq!(
            replies, 192,
            "every queued write gets a reply before shutdown"
        );
        assert!(
            !path.exists(),
            "fake DEMOTE cleanup removes the temporary socket"
        );
    }

    #[test]
    fn daemon_broker_vram_and_ram_lifecycles_use_injected_runtime() {
        struct TestProvider;

        impl VramProvider for TestProvider {
            type Mem<'a>
                = TestMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                Ok(TestMemory::new(bytes))
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024))
            }
        }

        struct NoopAcceptors;

        impl BrokerAcceptorStarter for NoopAcceptors {
            fn start(
                &mut self,
                _unix: UnixListener,
                _tcp: Option<std::net::TcpListener>,
                _exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
                _tx_flags: u16,
                _jobs_tx: std::sync::mpsc::SyncSender<WMsg>,
            ) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }
        }

        fn runtime(path: &Path, shutdown: std::sync::Arc<AtomicBool>) -> BrokerRuntime {
            broker_setup_with_acceptors(
                1,
                4096,
                path.to_str().expect("temporary path is UTF-8"),
                None,
                None,
                "127.0.0.1:0".parse().expect("loopback arbiter listener"),
                None,
                shutdown,
                Duration::from_millis(1),
                &mut NoopAcceptors,
            )
            .expect("temporary injected broker runtime")
        }

        let vram_path = std::env::temp_dir().join(format!(
            "ramshared-daemon-vram-lifecycle-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&vram_path);
        let vram_shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let vram_runtime = runtime(&vram_path, std::sync::Arc::clone(&vram_shutdown));
        let _ = vram_runtime.shutdown.request();
        run_broker_with_setup(
            TestProvider,
            4096,
            1,
            vram_path.to_string_lossy().into_owned(),
            false,
            |_force, _future| Ok(()),
            || Ok(vram_runtime),
            Duration::from_millis(1),
        )
        .expect("fake VRAM broker lifecycle");
        assert!(
            !vram_path.exists(),
            "VRAM lifecycle cleans its owned socket"
        );

        let ram_path = std::env::temp_dir().join(format!(
            "ramshared-daemon-ram-lifecycle-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&ram_path);
        let ram_shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let ram_runtime = runtime(&ram_path, std::sync::Arc::clone(&ram_shutdown));
        let _ = ram_runtime.shutdown.request();
        run_broker_ram_with_setup(
            4096,
            1,
            ram_path.to_string_lossy().into_owned(),
            || Ok(ram_runtime),
            Duration::from_millis(1),
        )
        .expect("heap RAM broker lifecycle");
        assert!(!ram_path.exists(), "RAM lifecycle cleans its owned socket");
    }

    #[test]
    // TestName: daemon_broker_panic_propagates_after_bounded_worker_cleanup
    fn daemon_broker_panic_propagates_after_bounded_worker_cleanup() {
        let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel(CHAN_CAP);
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let stop = BrokerShutdown::new(std::sync::Arc::clone(&shutdown), jobs_tx);
        let (demote_tx, _demote_rx) = std::sync::mpsc::channel();
        let broker = std::thread::spawn(|| panic!("injected broker panic"));
        let runtime = BrokerRuntime {
            worker: BrokerWorkerRuntime {
                geom: vec![(0, 4096)],
                jobs_rx,
                demote_tx,
                shutdown,
                shutdown_wake_pending: std::sync::Arc::clone(&stop.wake_pending),
                shutdown_wake_tx: stop.wake_tx.clone(),
                slice_io: std::sync::Arc::new(vec![SliceIoCounters::default()]),
                vram: std::sync::Arc::new(VramGauge::default()),
            },
            broker,
            shutdown: stop.clone(),
            socket: None,
        };
        let socket = std::env::temp_dir().join(format!(
            "ramshared-broker-panic-{}.sock",
            std::process::id()
        ));
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let controller = std::thread::spawn(move || {
            let result = run_broker_ram_with_setup(
                4096,
                1,
                socket.to_string_lossy().into_owned(),
                || Ok(runtime),
                Duration::from_secs(30),
            )
            .map_err(|error| error.to_string());
            let _ = result_tx.send(result);
        });

        let (needed_rescue, result) = match result_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(result) => (false, result),
            Err(_) => {
                let _ = stop.request();
                (
                    true,
                    result_rx
                        .recv_timeout(Duration::from_secs(1))
                        .expect("fixture rescue must bound cleanup of the broken implementation"),
                )
            }
        };
        controller
            .join()
            .expect("broker lifecycle controller joins");

        assert!(
            !needed_rescue,
            "broker panic did not independently stop its worker"
        );
        assert!(
            result
                .expect_err("broker panic must not become clean daemon success")
                .contains("broker"),
            "broker panic error lost its typed context"
        );
    }

    #[test]
    fn daemon_version_flag_is_side_effect_free() {
        assert!(daemon_version_requested(&[
            "ramsharedd".to_string(),
            "--version".to_string()
        ]));
        assert!(!daemon_version_requested(&[
            "ramsharedd".to_string(),
            "--size".to_string(),
            "1024".to_string()
        ]));
    }

    #[test]
    fn daemon_broker_setup_failure_zeroes_allocated_vram_before_return() {
        let zeroed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let result = run_broker_with_setup(
            ZeroRecordingProvider {
                zeroed: std::sync::Arc::clone(&zeroed),
            },
            4096,
            1,
            std::env::temp_dir()
                .join(format!(
                    "ramshared-daemon-setup-refusal-{}.sock",
                    std::process::id()
                ))
                .to_string_lossy()
                .into_owned(),
            false,
            |_force, _future| Ok(()),
            || Err("injected broker setup refusal".into()),
            Duration::from_millis(1),
        );

        assert!(
            result.is_err(),
            "injected broker setup refusal must propagate"
        );
        assert_eq!(
            *zeroed.lock().expect("test zero record lock"),
            vec![4096, 4096, CANARY_BYTES],
            "initialization, backend cleanup, then canary cleanup must all zero before return"
        );
    }

    #[test]
    fn daemon_broker_lock_refusal_zeroes_allocated_vram_before_return() {
        let zeroed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let setup_called = std::sync::Arc::new(AtomicBool::new(false));
        let setup_called_by_closure = std::sync::Arc::clone(&setup_called);
        let result = run_broker_with_setup(
            ZeroRecordingProvider {
                zeroed: std::sync::Arc::clone(&zeroed),
            },
            4096,
            1,
            std::env::temp_dir()
                .join(format!(
                    "ramshared-daemon-lock-refusal-{}.sock",
                    std::process::id()
                ))
                .to_string_lossy()
                .into_owned(),
            false,
            |_force, _future| Err("injected memory-lock refusal".into()),
            move || {
                setup_called_by_closure.store(true, Ordering::SeqCst);
                Err("broker setup must not run after memory-lock refusal".into())
            },
            Duration::from_millis(1),
        );

        assert!(
            result.is_err(),
            "injected memory-lock refusal must propagate"
        );
        assert!(
            !setup_called.load(Ordering::SeqCst),
            "memory-lock refusal must stop before canary allocation or broker setup"
        );
        assert_eq!(
            *zeroed.lock().expect("test zero record lock"),
            vec![4096, 4096],
            "initialization and refusal cleanup must both wipe the allocated backend"
        );
    }

    #[test]
    fn gpu_base_mapping_precedes_current_only_lock_and_future_lock_is_refused() {
        struct OrderProvider {
            calls: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
            zeroed: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
        }

        impl VramProvider for OrderProvider {
            type Mem<'a>
                = ZeroRecordingMemory
            where
                Self: 'a;

            fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, ramshared_vram::VramError> {
                self.calls.lock().expect("order log").push("gpu-map");
                Ok(ZeroRecordingMemory {
                    bytes,
                    zeroed: std::sync::Arc::clone(&self.zeroed),
                })
            }

            fn mem_info(&self) -> Result<(u64, u64), ramshared_vram::VramError> {
                Ok((8 * GIB, 8 * GIB))
            }
        }

        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let lock_calls = std::sync::Arc::clone(&calls);
        let result = run_broker_with_setup(
            OrderProvider {
                calls: std::sync::Arc::clone(&calls),
                zeroed: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            },
            4096,
            1,
            std::env::temp_dir()
                .join(format!(
                    "ramshared-daemon-lock-order-{}.sock",
                    std::process::id()
                ))
                .to_string_lossy()
                .into_owned(),
            false,
            move |_force, lock_future| {
                assert!(!lock_future, "MCL_FUTURE must never be requested");
                lock_calls.lock().expect("order log").push("mlock-current");
                Err("stop after the lock-order frontier".into())
            },
            || panic!("setup must not run after the injected lock refusal"),
            Duration::from_millis(1),
        );

        assert!(result.is_err());
        assert_eq!(
            *calls.lock().expect("order log"),
            vec!["gpu-map", "mlock-current"]
        );
    }

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
            .expect("injected RAM ublk lifecycle");
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
                self.0.lock().expect("test call log").push("join");
                Ok(())
            }
        }

        struct Runtime {
            failure: Failure,
            calls: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
        }

        impl Runtime {
            fn mark(&self, call: &'static str) {
                self.calls.lock().expect("test call log").push(call);
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
            assert_eq!(*calls.lock().expect("test call log"), expected);
        }
    }

    #[test]
    fn daemon_ublk_shutdown_is_swapoff_first_and_preserves_on_uncertainty() {
        struct Server(std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>);

        impl UblkServer for Server {
            fn join(self: Box<Self>) -> std::io::Result<()> {
                self.0.lock().expect("test call log").push("join");
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
                self.calls.lock().expect("test call log").push(call);
            }

            fn calls(&self) -> Vec<&'static str> {
                self.calls.lock().expect("test call log").clone()
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
                match self.states.pop_front().expect("planned strict snapshot") {
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
            .expect("swapoff-first shutdown with fresh absence proofs");
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

    #[test]
    fn origin_mode_never_arms_mcl_future_before_cache_mappings() {
        let error = lock_memory(false, true).unwrap_err();
        assert!(error.to_string().contains("MCL_FUTURE is forbidden"));
    }

    struct BrokerWorkerShutdownGuard<'a>(&'a BrokerShutdown);

    impl Drop for BrokerWorkerShutdownGuard<'_> {
        fn drop(&mut self) {
            let _ = self.0.request();
        }
    }

    #[test]
    fn daemon_worker_reply_is_io_accounting_barrier_and_shutdown_is_bounded() {
        const REPLIES: u64 = 512;
        let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel(CHAN_CAP);
        let (demote_tx, _demote_rx) = std::sync::mpsc::channel();
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let stop = BrokerShutdown::new(std::sync::Arc::clone(&shutdown), jobs_tx.clone());
        let slice_io = std::sync::Arc::new(vec![SliceIoCounters::default()]);
        let worker_rt = BrokerWorkerRuntime {
            geom: vec![(0, 4096)],
            jobs_rx,
            demote_tx,
            shutdown: std::sync::Arc::clone(&shutdown),
            shutdown_wake_pending: std::sync::Arc::clone(&stop.wake_pending),
            shutdown_wake_tx: stop.wake_tx.clone(),
            slice_io: std::sync::Arc::clone(&slice_io),
            vram: std::sync::Arc::new(VramGauge::default()),
        };
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let (published_tx, published_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let hook_timed_out = std::sync::Arc::new(AtomicBool::new(false));
        let hook_timeout_observed = std::sync::Arc::clone(&hook_timed_out);
        let hook_shutdown = std::sync::Arc::clone(&shutdown);

        let (observations, failure, wake, joined, elapsed) = std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                serve_broker_jobs_with_poll_and_reply_hook(
                    RamBackend::new(4096),
                    worker_rt,
                    |_| None,
                    Duration::from_millis(10),
                    move || {
                        if published_tx.send(()).is_ok()
                            && release_rx.recv_timeout(Duration::from_secs(2)).is_err()
                        {
                            hook_timeout_observed.store(true, Ordering::SeqCst);
                            hook_shutdown.store(true, Ordering::SeqCst);
                        }
                    },
                )
            });
            let started = Instant::now();
            let _shutdown_guard = BrokerWorkerShutdownGuard(&stop);
            let mut observations = Vec::with_capacity(REPLIES as usize);
            let mut failure = None;

            for sequence in 1..=REPLIES {
                if jobs_tx
                    .send(WMsg::Job(ramshared_wsl2d::conn::Job {
                        export: 0,
                        req: ramshared_block::Request {
                            flags: 0,
                            cmd: Command::Write,
                            handle: sequence,
                            offset: 0,
                            len: 512,
                        },
                        payload: vec![0x5A; 512],
                        reply: reply_tx.clone(),
                    }))
                    .is_err()
                {
                    failure = Some(format!("worker queue disconnected at reply {sequence}"));
                    break;
                }

                let reply = match reply_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(reply) => reply,
                    Err(error) => {
                        failure = Some(format!("reply {sequence} missed its deadline: {error}"));
                        break;
                    }
                };
                if reply.disconnect {
                    failure = Some(format!("reply {sequence} unexpectedly disconnected"));
                    let _ = release_tx.send(());
                    break;
                }
                if let Err(error) = published_rx.recv_timeout(Duration::from_secs(2)) {
                    failure = Some(format!(
                        "reply {sequence} publication hook missed its deadline: {error}"
                    ));
                    let _ = release_tx.send(());
                    break;
                }
                observations.push((
                    sequence,
                    slice_io[0].bytes_served.load(Ordering::Acquire),
                    slice_io[0].io_count.load(Ordering::Acquire),
                ));
                if release_tx.send(()).is_err() {
                    failure = Some(format!(
                        "worker publication hook disconnected at reply {sequence}"
                    ));
                    break;
                }
            }

            let wake = stop.request();
            let joined = worker.join();
            (observations, failure, wake, joined, started.elapsed())
        });

        assert_eq!(failure, None);
        assert!(!hook_timed_out.load(Ordering::SeqCst));
        assert_eq!(wake, BrokerShutdownWake::Queued);
        assert!(joined.is_ok(), "worker joins after shutdown");
        assert_eq!(observations.len(), REPLIES as usize);
        for (sequence, bytes, io_count) in observations {
            assert_eq!(
                bytes,
                sequence * 512,
                "reply {sequence} exposed stale byte accounting"
            );
            assert_eq!(
                io_count, sequence,
                "reply {sequence} exposed stale IO accounting"
            );
        }
        assert!(
            elapsed < Duration::from_secs(5),
            "worker completion and shutdown exceeded the bounded deadline"
        );
    }

    #[test]
    fn daemon_worker_shutdown_wake_is_not_timer_dependent() {
        let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel(CHAN_CAP);
        let (demote_tx, _demote_rx) = std::sync::mpsc::channel();
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let stop = BrokerShutdown::new(std::sync::Arc::clone(&shutdown), jobs_tx.clone());
        let worker_rt = BrokerWorkerRuntime {
            geom: vec![(0, 4096)],
            jobs_rx,
            demote_tx,
            shutdown: std::sync::Arc::clone(&shutdown),
            shutdown_wake_pending: std::sync::Arc::clone(&stop.wake_pending),
            shutdown_wake_tx: stop.wake_tx.clone(),
            slice_io: std::sync::Arc::new(vec![SliceIoCounters::default()]),
            vram: std::sync::Arc::new(VramGauge::default()),
        };

        std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                serve_broker_jobs_with_poll(
                    RamBackend::new(4096),
                    worker_rt,
                    |_| None,
                    Duration::from_secs(30),
                )
            });
            std::thread::sleep(Duration::from_millis(20));
            assert_eq!(stop.request(), BrokerShutdownWake::Queued);
            let started = Instant::now();
            let _backend = worker.join().expect("shutdown wake joins worker");
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "explicit shutdown wake must not wait for the receive timer"
            );
        });
    }

    fn run_full_queue_shutdown_fixture() {
        let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel(CHAN_CAP);
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        jobs_tx
            .try_send(WMsg::Job(ramshared_wsl2d::conn::Job {
                export: 0,
                req: ramshared_block::Request {
                    flags: 0,
                    cmd: Command::Write,
                    handle: 17,
                    offset: 0,
                    len: 512,
                },
                payload: vec![0xA5; 512],
                reply: reply_tx,
            }))
            .expect("manufactured queue accepts one admitted IO before shutdown");
        for _ in 1..CHAN_CAP {
            jobs_tx
                .try_send(WMsg::Opened)
                .expect("manufactured queue has exact capacity");
        }
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let stop = BrokerShutdown::new(std::sync::Arc::clone(&shutdown), jobs_tx.clone());
        let (demote_tx, _demote_rx) = std::sync::mpsc::channel();
        let slice_io = std::sync::Arc::new(vec![SliceIoCounters::default()]);
        let worker_rt = BrokerWorkerRuntime {
            geom: vec![(0, 4096)],
            jobs_rx,
            demote_tx,
            shutdown: std::sync::Arc::clone(&shutdown),
            shutdown_wake_pending: std::sync::Arc::clone(&stop.wake_pending),
            shutdown_wake_tx: stop.wake_tx.clone(),
            slice_io: std::sync::Arc::clone(&slice_io),
            vram: std::sync::Arc::new(VramGauge::default()),
        };

        assert_eq!(stop.request(), BrokerShutdownWake::QueueFull);
        assert!(shutdown.load(Ordering::SeqCst));
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let backend = serve_broker_jobs_with_poll(
                RamBackend::new(4096),
                worker_rt,
                |_| None,
                Duration::from_millis(10),
            );
            let _ = done_tx.send(backend);
        });

        let _backend = done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker observes the terminal flag before a full queue");
        assert!(
            reply_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "worker executed queued IO after terminal state"
        );
        assert_eq!(slice_io[0].bytes_served.load(Ordering::Relaxed), 0);
        assert_eq!(slice_io[0].io_count.load(Ordering::Relaxed), 0);
        worker
            .join()
            .expect("full-queue worker thread must join after bounded shutdown");
        assert!(
            !stop.wake_pending.load(Ordering::SeqCst),
            "the worker retained a shutdown notifier after it stopped"
        );
    }

    #[test]
    // TestName: daemon_worker_shutdown_full_queue_is_nonblocking
    fn daemon_worker_shutdown_full_queue_is_nonblocking() {
        run_full_queue_shutdown_fixture();
    }

    #[test]
    // TestName: daemon_worker_parallel_full_queue_shutdowns_reap_without_notifier_threads
    fn daemon_worker_parallel_full_queue_shutdowns_reap_without_notifier_threads() {
        std::thread::scope(|scope| {
            let workers = (0..8)
                .map(|_| scope.spawn(run_full_queue_shutdown_fixture))
                .collect::<Vec<_>>();
            for worker in workers {
                worker
                    .join()
                    .expect("parallel full-queue shutdown fixture must join");
            }
        });
    }

    #[test]
    // TestName: daemon_worker_shutdown_preempts_queued_io_at_iteration_boundary
    fn daemon_worker_shutdown_preempts_queued_io_at_iteration_boundary() {
        let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel(CHAN_CAP);
        let (demote_tx, _demote_rx) = std::sync::mpsc::channel();
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let stop = BrokerShutdown::new(std::sync::Arc::clone(&shutdown), jobs_tx.clone());
        let slice_io = std::sync::Arc::new(vec![SliceIoCounters::default()]);
        let worker_rt = BrokerWorkerRuntime {
            geom: vec![(0, 4096)],
            jobs_rx,
            demote_tx,
            shutdown,
            shutdown_wake_pending: std::sync::Arc::clone(&stop.wake_pending),
            shutdown_wake_tx: stop.wake_tx.clone(),
            slice_io: std::sync::Arc::clone(&slice_io),
            vram: std::sync::Arc::new(VramGauge::default()),
        };
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        jobs_tx
            .send(WMsg::Job(ramshared_wsl2d::conn::Job {
                export: 0,
                req: ramshared_block::Request {
                    flags: 0,
                    cmd: Command::Write,
                    handle: 11,
                    offset: 0,
                    len: 512,
                },
                payload: vec![0xA5; 512],
                reply: reply_tx,
            }))
            .expect("queue accepts write before shutdown");
        assert_eq!(stop.request(), BrokerShutdownWake::Queued);

        let _backend = serve_broker_jobs_with_poll(
            RamBackend::new(4096),
            worker_rt,
            |_| None,
            Duration::from_secs(30),
        );
        assert!(
            reply_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "terminal worker executed queued IO after observing shutdown"
        );
        assert_eq!(slice_io[0].bytes_served.load(Ordering::Relaxed), 0);
        assert_eq!(slice_io[0].io_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    // TestName: daemon_worker_terminal_flag_wins_over_512_continuous_queue_refills
    fn daemon_worker_terminal_flag_wins_over_512_continuous_queue_refills() {
        const REFILLS: u64 = 512;
        let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel(CHAN_CAP);
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let make_job = |handle| {
            WMsg::Job(ramshared_wsl2d::conn::Job {
                export: 0,
                req: ramshared_block::Request {
                    flags: 0,
                    cmd: Command::Write,
                    handle,
                    offset: 0,
                    len: 512,
                },
                payload: vec![0x5A; 512],
                reply: reply_tx.clone(),
            })
        };
        for handle in 0..CHAN_CAP as u64 {
            jobs_tx
                .try_send(make_job(handle))
                .expect("fixture starts with an exactly full worker queue");
        }
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let stop = BrokerShutdown::new(std::sync::Arc::clone(&shutdown), jobs_tx.clone());
        let (demote_tx, _demote_rx) = std::sync::mpsc::channel();
        let worker_rt = BrokerWorkerRuntime {
            geom: vec![(0, 4096)],
            jobs_rx,
            demote_tx,
            shutdown,
            shutdown_wake_pending: std::sync::Arc::clone(&stop.wake_pending),
            shutdown_wake_tx: stop.wake_tx.clone(),
            slice_io: std::sync::Arc::new(vec![SliceIoCounters::default()]),
            vram: std::sync::Arc::new(VramGauge::default()),
        };
        let (refill_tx, refill_rx) = std::sync::mpsc::sync_channel::<u64>(0);
        let (refilled_tx, refilled_rx) = std::sync::mpsc::sync_channel::<()>(0);
        let processed = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let hook_processed = std::sync::Arc::clone(&processed);
        let hook_stop = stop.clone();

        std::thread::scope(|scope| {
            let producer = scope.spawn(move || {
                for sequence in 0..REFILLS {
                    let handle = refill_rx
                        .recv_timeout(Duration::from_secs(1))
                        .expect("worker requests each deterministic refill");
                    jobs_tx
                        .send(make_job(handle.saturating_add(CHAN_CAP as u64)))
                        .expect("one freed queue slot accepts one refill");
                    refilled_tx
                        .send(())
                        .expect("worker observes queue-full restoration");
                    assert_eq!(handle, sequence + 1);
                }
            });
            let worker = scope.spawn(move || {
                serve_broker_jobs_with_poll_and_reply_hook(
                    RamBackend::new(4096),
                    worker_rt,
                    |_| None,
                    Duration::from_millis(10),
                    move || {
                        let sequence = hook_processed.fetch_add(1, Ordering::SeqCst) + 1;
                        if sequence <= REFILLS {
                            refill_tx
                                .send(sequence)
                                .expect("refill producer remains present");
                            refilled_rx
                                .recv_timeout(Duration::from_secs(1))
                                .expect("producer restores a full queue without sleeps");
                        }
                        if sequence == REFILLS {
                            assert_eq!(hook_stop.request(), BrokerShutdownWake::QueueFull);
                        }
                    },
                )
            });
            producer.join().expect("bounded refill producer joins");
            worker.join().expect("terminal worker joins");
        });
        let reply_count = reply_rx.try_iter().count() as u64;

        assert_eq!(
            processed.load(Ordering::SeqCst),
            REFILLS,
            "a continuously full queue starved the terminal flag"
        );
        assert_eq!(reply_count, REFILLS);
    }

    #[test]
    fn daemon_command_timeout_terminates_child_without_hang() {
        let output = command_stdout_with_timeout(
            "head",
            &["-c", "131072", "/dev/zero"],
            Duration::from_secs(5),
        )
        .expect("a finite child that fills stdout must be drained before its deadline");
        assert_eq!(output.len(), 131072);

        assert_eq!(
            command_stdout_with_timeout("false", &[], Duration::from_secs(1)),
            None
        );
        assert_eq!(
            command_stdout_with_timeout(
                "definitely-not-a-ramshared-command",
                &[],
                Duration::from_secs(1)
            ),
            None
        );
        let started = Instant::now();
        assert_eq!(
            command_stdout_with_timeout("sleep", &["1"], Duration::from_millis(25)),
            None
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a timed-out child must be reaped, not left running"
        );
    }

    #[test]
    // TestName: daemon_command_success_reaps_all_stdio_redirected_descendant
    fn daemon_command_success_reaps_all_stdio_redirected_descendant() {
        let output = command_stdout_with_timeout(
            "/bin/sh",
            &[
                "-c",
                "sleep 10 </dev/null >/dev/null 2>&1 & printf '%s\\n' \"$!\"",
            ],
            Duration::from_secs(1),
        )
        .expect("successful helper leader remains legitimate");
        let descendant = output
            .trim()
            .parse::<u32>()
            .expect("fixture prints its exact descendant PID");
        let descendant_path = std::path::PathBuf::from(format!("/proc/{descendant}"));
        let deadline = Instant::now() + Duration::from_secs(1);
        while descendant_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let gone = !descendant_path.exists();
        if !gone && let Ok(pid) = c_int::try_from(descendant) {
            // SAFETY: this is the positive PID printed by the exact descendant
            // created by this fixture, so cleanup cannot address another group.
            let _ = unsafe { kill_process_group_raw(pid, SIGKILL) };
        }

        assert!(
            gone,
            "daemon helper left an all-stdio-redirected owned descendant alive"
        );
    }

    #[test]
    // TestName: daemon_command_timeout_reaps_term_ignoring_fixture
    fn daemon_command_timeout_reaps_term_ignoring_fixture() {
        let started = Instant::now();
        assert_eq!(
            command_stdout_with_timeout(
                "/bin/sh",
                &["-c", "trap '' TERM; while :; do :; done"],
                Duration::from_millis(25),
            ),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    struct NeverReapedCommand {
        id: u32,
        group_kills: Cell<usize>,
        direct_kills: Cell<usize>,
    }

    struct ScriptedCommandTarget {
        id: u32,
        observations: VecDeque<std::io::Result<bool>>,
        reap_error: Option<std::io::Error>,
        group_errno: Option<i32>,
        group_kills: usize,
        direct_kills: usize,
    }

    impl ScriptedCommandTarget {
        fn new(observations: Vec<std::io::Result<bool>>, group_errno: Option<i32>) -> Self {
            Self {
                id: 43,
                observations: observations.into(),
                reap_error: None,
                group_errno,
                group_kills: 0,
                direct_kills: 0,
            }
        }

        fn with_reap_error(mut self, error: std::io::Error) -> Self {
            self.reap_error = Some(error);
            self
        }
    }

    impl CommandReapTarget for ScriptedCommandTarget {
        fn id(&self) -> u32 {
            self.id
        }

        fn kill_group(&mut self) -> std::io::Result<()> {
            self.group_kills += 1;
            match self.group_errno {
                Some(errno) => Err(std::io::Error::from_raw_os_error(errno)),
                None => Ok(()),
            }
        }

        fn kill_direct(&mut self) -> std::io::Result<()> {
            self.direct_kills += 1;
            Ok(())
        }

        fn observe_exit(&mut self) -> std::io::Result<bool> {
            self.observations.pop_front().unwrap_or(Ok(true))
        }

        fn reap_observed(&mut self) -> std::io::Result<Option<ExitStatus>> {
            match self.reap_error.take() {
                Some(error) => Err(error),
                None => Ok(Some(ExitStatus::from_raw(0))),
            }
        }
    }

    struct PanickingCommandReader;

    impl Read for PanickingCommandReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            panic!("fixture command capture panic")
        }
    }

    impl CommandReapTarget for NeverReapedCommand {
        fn id(&self) -> u32 {
            self.id
        }

        fn kill_group(&mut self) -> std::io::Result<()> {
            self.group_kills.set(self.group_kills.get() + 1);
            Ok(())
        }

        fn kill_direct(&mut self) -> std::io::Result<()> {
            self.direct_kills.set(self.direct_kills.get() + 1);
            Ok(())
        }

        fn observe_exit(&mut self) -> std::io::Result<bool> {
            Ok(false)
        }

        fn reap_observed(&mut self) -> std::io::Result<Option<ExitStatus>> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct RecordingCommandFatal(RefCell<Vec<String>>);

    impl CommandFatalContainment for RecordingCommandFatal {
        fn contain(&self, detail: &str) {
            self.0.borrow_mut().push(detail.to_string());
        }
    }

    #[test]
    // TestName: daemon_unreaped_group_selects_fatal_containment
    fn daemon_unreaped_group_selects_fatal_containment() {
        let mut target = NeverReapedCommand {
            id: 42,
            group_kills: Cell::new(0),
            direct_kills: Cell::new(0),
        };
        let fatal = RecordingCommandFatal::default();

        assert!(!terminate_command_target_with(
            &mut target,
            "fixture command",
            Duration::ZERO,
            &fatal,
        ));
        assert_eq!(target.group_kills.get(), 1);
        assert_eq!(target.direct_kills.get(), 0);
        assert_eq!(fatal.0.borrow().len(), 1);
        assert!(fatal.0.borrow()[0].contains("observable direct-child exit"));
    }

    #[test]
    fn daemon_reap_policy_covers_inspection_races_and_signal_failures() {
        let fatal = RecordingCommandFatal::default();
        let mut already_observed = ScriptedCommandTarget::new(vec![Ok(true)], None);
        assert!(terminate_command_target_with(
            &mut already_observed,
            "already observed",
            Duration::ZERO,
            &fatal,
        ));
        assert_eq!(already_observed.group_kills, 2);

        let mut inspection_failed = ScriptedCommandTarget::new(
            vec![Err(std::io::Error::other("fixture observation failure"))],
            None,
        );
        assert!(!terminate_command_target_with(
            &mut inspection_failed,
            "observation failure",
            Duration::ZERO,
            &fatal,
        ));

        let mut esrch_race = ScriptedCommandTarget::new(vec![Ok(true)], Some(3));
        assert!(terminate_command_target_with(
            &mut esrch_race,
            "ESRCH race",
            Duration::ZERO,
            &fatal,
        ));
        assert_eq!(esrch_race.direct_kills, 1);

        let mut group_failed = ScriptedCommandTarget::new(vec![Ok(true)], Some(5));
        assert!(!terminate_command_target_with(
            &mut group_failed,
            "group failure",
            Duration::ZERO,
            &fatal,
        ));
        assert_eq!(group_failed.direct_kills, 1);

        let mut reap_failed = ScriptedCommandTarget::new(vec![Ok(true)], None)
            .with_reap_error(std::io::Error::other("fixture reap failure"));
        assert!(!terminate_command_target_with(
            &mut reap_failed,
            "reap failure",
            Duration::ZERO,
            &fatal,
        ));
        assert_eq!(fatal.0.borrow().len(), 3);
        assert!(fatal.0.borrow()[0].contains("exit observation failed"));
        assert!(fatal.0.borrow()[1].contains("process-group SIGKILL"));
        assert!(fatal.0.borrow()[2].contains("final direct-child reap failed"));
    }

    #[test]
    fn daemon_capture_failures_and_direct_fallback_remain_accounted() {
        let mut panicked = CommandCapture::spawn(PanickingCommandReader)
            .expect("the panic fixture capture worker must start");
        assert!(matches!(
            panicked.receive_until(Instant::now() + Duration::from_secs(1)),
            CommandCaptureState::Disconnected
        ));
        assert!(!panicked.join(), "capture panic must be visible at join");
        assert!(!panicked.join(), "a capture worker must not join twice");

        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender
            .send(Ok(b"ready-without-worker".to_vec()))
            .expect("queue synthetic capture result");
        let mut missing_worker = CommandCapture {
            receiver,
            worker: None,
        };
        assert_eq!(
            finish_command_capture(u32::MAX, "missing worker", &mut missing_worker),
            None,
            "capture success is not accepted without accounting its worker"
        );

        let fatal = RecordingCommandFatal::default();
        assert!(!signal_owned_command_group_before_reap(
            u32::MAX,
            "overflow fixture",
            &fatal,
        ));
        assert!(fatal.0.borrow()[0].contains("ID overflow"));

        let mut command = ProcessCommand::new("/bin/sh");
        command.args(["-c", "exec sleep 10"]);
        command.process_group(0);
        let mut child = command.spawn().expect("the exact fixture child must start");
        assert_eq!(CommandReapTarget::id(&child), child.id());
        CommandReapTarget::kill_direct(&mut child)
            .expect("the fallback must signal only the exact direct child");
        assert!(
            !child
                .wait()
                .expect("the exact child must be reaped")
                .success()
        );
    }

    #[test]
    fn daemon_probe_and_origin_helpers_refuse_without_live_device_access() {
        let calls = RefCell::new(Vec::new());
        let free = global_gpu_free_bytes_with(
            &["/missing/nvidia-smi", "fixture-nvidia-smi"],
            Duration::from_millis(10),
            |program| !program.starts_with("/missing/"),
            |program, args, timeout| {
                calls
                    .borrow_mut()
                    .push((program.to_string(), args.len(), timeout));
                Some("321\n".into())
            },
        );
        assert_eq!(free, Some(321 * 1024 * 1024));
        assert_eq!(
            calls.borrow().as_slice(),
            &[("fixture-nvidia-smi".into(), 2, Duration::from_millis(10))]
        );
        assert_eq!(
            global_gpu_free_bytes_with(
                &["fixture-nvidia-smi"],
                Duration::from_millis(10),
                |_| true,
                |_, _, _| Some("N/A\n".into()),
            ),
            None
        );

        assert_eq!(linux_device_number(0x0801), "8:1");
        assert!(unix_time_ms().is_some());
        assert!(read_host_origin_manifest_bytes("/tmp/not-the-host-manifest").is_err());
        assert!(read_sealed_origin_manifest("/tmp/not-the-origin-manifest").is_err());
        assert!(parent_block_device(Path::new("/")).is_err());
        let non_utf8 = std::path::PathBuf::from(OsString::from_vec(vec![0xff]));
        assert!(block_identity_value(&non_utf8, "UUID").is_err());

        let mut broker_without_arbiter = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--backend",
            "ram",
            "--slices",
            "1",
            "--slice-mb",
            "1",
            "--arbiter-listen",
            "127.0.0.1:7777",
        ]))
        .expect("valid broker fixture");
        broker_without_arbiter.arbiter_addr = None;
        assert!(select_daemon_action(broker_without_arbiter).is_err());

        let mut no_slices_with_listener =
            AppArgs::parse_from(&daemon_argv(&["ramsharedd", "--transport", "ublk"]))
                .expect("valid ublk fixture");
        no_slices_with_listener.listen_nbd_addr = Some("127.0.0.1:10809".parse().unwrap());
        assert!(select_daemon_action(no_slices_with_listener).is_err());
        let ublk = AppArgs::parse_from(&daemon_argv(&["ramsharedd", "--transport", "ublk"]))
            .expect("valid ublk fixture");
        assert!(matches!(
            select_daemon_action(ublk),
            Ok(DaemonAction::Ublk(_))
        ));

        let root =
            std::env::temp_dir().join(format!("ramshared-reclaim-fallback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create reclaim fallback fixture");
        let cache_target = root.join("cache-target.json");
        let reclaim = root.join("reclaim.json");
        std::fs::write(
            &reclaim,
            serde_json::json!({
                "schema_version": 1,
                "reason": "control_pressure",
                "daemon_instance_id": "fixture-daemon",
                "issued_at_unix_ms": 1_000,
            })
            .to_string(),
        )
        .expect("write reclaim fallback fixture");
        assert!(critical_cache_reclaim_requested_at(
            &cache_target,
            &reclaim,
            "fixture-daemon",
            1_001,
        ));
        std::fs::remove_dir_all(root).expect("remove exact reclaim fallback fixture");
    }

    #[test]
    // TestName: daemon_command_contains_inherited_output_and_bounds_capture
    fn daemon_command_contains_inherited_output_and_bounds_capture() {
        let root = std::env::temp_dir().join(format!(
            "ramshared-daemon-command-child-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let program = root.join("inherited-output");
        std::fs::write(
            &program,
            "#!/bin/sh\n(sleep 1) &\nprintf '4096\\n'\nexit 0\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&program).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&program, permissions).unwrap();

        let started = Instant::now();
        assert_eq!(
            command_stdout_with_timeout(program.to_str().unwrap(), &[], Duration::from_millis(100),),
            None,
            "a descendant-held output pipe must fail closed",
        );
        assert!(
            started.elapsed() < Duration::from_millis(750),
            "descendant-held output outlived the bounded command deadline"
        );
        assert_eq!(
            command_stdout_with_timeout(
                "head",
                &["-c", "1048576", "/dev/zero"],
                Duration::from_secs(2),
            ),
            None,
            "command output storage must have a finite upper bound",
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn daemon_plan_routes_validated_actions_without_starting_a_backend() {
        struct CapturingRunner(Option<DaemonAction>);

        impl DaemonActionRunner for CapturingRunner {
            fn execute(&mut self, action: DaemonAction) -> Result<(), Box<dyn std::error::Error>> {
                self.0 = Some(action);
                Ok(())
            }
        }

        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--backend",
            "ram",
            "--slices",
            "1",
            "--slice-mb",
            "1",
            "--arbiter-listen",
            "127.0.0.1:7777",
        ]))
        .expect("validated broker argv");
        let mut runner = CapturingRunner(None);
        run_with(args, &mut runner).expect("capturing runner must receive a plan");
        assert!(matches!(
            runner.0,
            Some(DaemonAction::Broker(AppArgs {
                slices: 1,
                backend: BackendKind::Ram,
                ..
            }))
        ));

        let single_ram = AppArgs::parse_from(&daemon_argv(&["ramsharedd", "--backend", "ram"]))
            .expect("argv itself is syntactically valid");
        assert!(
            select_daemon_action(single_ram).is_err(),
            "single NBD RAM must refuse before a backend is selected"
        );
    }

    #[test]
    fn daemon_ublk_vulkan_refuses_before_device_mutation() {
        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--transport",
            "ublk",
            "--backend",
            "vulkan",
        ]))
        .expect("the syntax is valid so the planner owns this refusal");
        let error = match select_daemon_action(args) {
            Ok(_) => panic!("ublk Vulkan must refuse before selecting any device backend"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("ublk with --backend vulkan"),
            "refusal must identify the unsupported transport/backend pair"
        );
    }

    #[test]
    fn daemon_production_runner_refuses_safe_terminal_actions_before_platform_load() {
        fn terminal_args(transport: Transport, backend: BackendKind) -> AppArgs {
            AppArgs {
                size: DEFAULT_SIZE,
                origin: None,
                sock: "/tmp/ramsharedd-terminal.sock".into(),
                force: false,
                nbd_dev: "/dev/ramshared-test-nbd".into(),
                transport,
                queue_depth: 1,
                backend,
                slices: 0,
                slice_bytes: 0,
                listen_nbd_addr: None,
                arbiter_addr: None,
                advertise_tcp: None,
                telemetry_jsonl: None,
            }
        }

        let mut runner = ProductionDaemonRunner;
        let nbd = runner.execute(DaemonAction::Nbd(terminal_args(
            Transport::Nbd,
            BackendKind::Ram,
        )));
        assert!(
            nbd.expect_err("RAM single NBD is terminally refused")
                .to_string()
                .contains("no single NBD path")
        );

        let ublk = runner.execute(DaemonAction::Ublk(terminal_args(
            Transport::Ublk,
            BackendKind::Vulkan,
        )));
        assert!(
            ublk.expect_err("Vulkan ublk is terminally refused")
                .to_string()
                .contains("ublk with --backend vulkan")
        );

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ramshared-production-runner-refusal-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).expect("create isolated temporary refusal directory");
        let sock = dir.join("owned-by-test-regular-file");
        std::fs::write(&sock, b"do-not-replace").expect("create regular preflight file");
        let broker = runner.execute(DaemonAction::Broker(AppArgs {
            size: DEFAULT_SIZE,
            origin: None,
            sock: sock.to_string_lossy().into_owned(),
            force: false,
            nbd_dev: "/dev/ramshared-test-nbd".into(),
            transport: Transport::Nbd,
            queue_depth: 1,
            backend: BackendKind::Ram,
            slices: 1,
            slice_bytes: BLOCK_SIZE as u64,
            listen_nbd_addr: None,
            arbiter_addr: Some("127.0.0.1:7777".parse().expect("loopback arbiter address")),
            advertise_tcp: None,
            telemetry_jsonl: None,
        }));
        assert!(
            broker
                .expect_err("broker-RAM must refuse a regular socket path")
                .to_string()
                .contains("refusing to replace existing socket path"),
            "the production runner must preserve a non-socket path before acceptor startup"
        );
        assert_eq!(
            std::fs::read(&sock).expect("regular preflight file remains readable"),
            b"do-not-replace"
        );
        std::fs::remove_dir_all(&dir).expect("remove isolated temporary refusal directory");
    }

    #[test]
    // TestName: private_listener_accepts_only_documented_untrusted_network_ranges
    fn private_listener_accepts_only_documented_untrusted_network_ranges() {
        for accepted in [
            "127.0.0.1:7777",
            "tcp://10.1.2.3:10809",
            "172.16.0.1:10809",
            "172.31.255.254:10809",
            "192.168.0.50:10809",
            "100.64.0.1:10809",
            "100.127.255.254:10809",
            "[::1]:10809",
            "[fd12:3456::1]:10809",
        ] {
            assert!(
                parse_private_listen(accepted).is_ok(),
                "documented private listener was refused: {accepted}"
            );
        }
        for refused in [
            "0.0.0.0:10809",
            "8.8.8.8:10809",
            "192.0.2.1:10809",
            "198.51.100.1:10809",
            "203.0.113.1:10809",
            "224.0.0.1:10809",
            "169.254.1.1:10809",
            "100.63.255.255:10809",
            "100.128.0.0:10809",
            "[::]:10809",
            "[2001:4860:4860::8888]:10809",
            "[2001:db8::1]:10809",
            "[ff02::1]:10809",
            "[fe80::1]:10809",
            "[::ffff:127.0.0.1]:10809",
        ] {
            assert!(
                parse_private_listen(refused).is_err(),
                "undocumented listener range was accepted: {refused}"
            );
        }
    }

    #[test]
    fn private_listen_rejects_garbage() {
        assert!(parse_private_listen("nao-eh-addr").is_err());
        assert!(parse_private_listen("127.0.0.1").is_err()); // sem porta
    }

    #[test]
    fn slice_flags_reject_ublk_with_slices() {
        assert!(validate_slice_flags(2, 64, true).is_err()); // DT-3
        assert!(validate_slice_flags(0, 0, true).is_ok()); // ublk single ok
    }

    #[test]
    fn slice_flags_require_slice_mb() {
        assert!(validate_slice_flags(2, 0, false).is_err());
        assert!(validate_slice_flags(2, 64, false).is_ok());
        assert!(validate_slice_flags(0, 0, false).is_ok()); // single-mode ok
    }

    #[test]
    fn slice_flags_cap_protects_status_line() {
        // MED-1: --slices above MAX_SLICES would blow the StatusReply (MAX_LINE_BYTES 64 KiB).
        assert!(validate_slice_flags(MAX_SLICES, 64, false).is_ok());
        assert!(validate_slice_flags(MAX_SLICES + 1, 64, false).is_err());
    }

    #[test]
    fn sparse_free_floor_requests_swapoff_but_latency_does_not() {
        assert!(sparse_residency_requests_swapoff(DemoteReason::FreeFloor));
        assert!(sparse_residency_requests_swapoff(DemoteReason::Corruption));
        assert!(!sparse_residency_requests_swapoff(DemoteReason::Latency));
    }

    #[test]
    fn sparse_residency_uses_configured_reserve_floor() {
        let cfg = sparse_residency_config(512 * 1024 * 1024);
        assert_eq!(cfg.free_floor_bytes, 512 * 1024 * 1024);
        assert_eq!(cfg.latency_mult, ResidencyConfig::default().latency_mult);
        assert_eq!(cfg.consecutive, ResidencyConfig::default().consecutive);
    }

    #[test]
    fn probe_residency_reason_has_priority_over_latency() {
        assert_eq!(
            choose_residency_reason(Some(DemoteReason::Latency), Some(DemoteReason::FreeFloor)),
            Some(DemoteReason::FreeFloor)
        );
        assert_eq!(
            choose_residency_reason(Some(DemoteReason::Latency), Some(DemoteReason::Corruption)),
            Some(DemoteReason::Corruption)
        );
    }

    #[test]
    fn probe_sample_logs_low_free_or_degraded_state() {
        assert!(should_log_probe_sample(
            Some(true),
            Some(128),
            512,
            1,
            false
        ));
        assert!(should_log_probe_sample(None, Some(1024), 512, 0, false));
        assert!(should_log_probe_sample(Some(true), None, 512, 0, false));
        assert!(!should_log_probe_sample(
            Some(true),
            Some(2048),
            512,
            0,
            false
        ));
        assert!(should_log_probe_sample(
            Some(true),
            Some(2048),
            512,
            0,
            true
        ));
    }

    #[test]
    fn nvidia_smi_free_parser_accepts_plain_csv_mib() {
        assert_eq!(
            parse_nvidia_smi_free_bytes("4731\n"),
            Some(4731 * 1024 * 1024)
        );
        assert_eq!(
            parse_nvidia_smi_free_bytes(" 222 MiB \n"),
            Some(222 * 1024 * 1024)
        );
        assert_eq!(
            parse_nvidia_smi_free_bytes("222, 5733\n"),
            Some(222 * 1024 * 1024)
        );
    }

    #[test]
    fn nvidia_smi_free_parser_rejects_empty_or_bad_output() {
        assert_eq!(parse_nvidia_smi_free_bytes(""), None);
        assert_eq!(parse_nvidia_smi_free_bytes("N/A\n"), None);
    }

    #[test]
    fn global_free_floor_demote_requires_committed_tier_and_streak() {
        let mut streak = 0;
        assert!(!observe_global_free_floor(
            Some(128),
            512,
            0,
            &mut streak,
            3
        ));
        assert_eq!(streak, 0);

        assert!(!observe_global_free_floor(
            Some(128),
            512,
            1024,
            &mut streak,
            3
        ));
        assert_eq!(streak, 1);
        assert!(!observe_global_free_floor(
            Some(128),
            512,
            1024,
            &mut streak,
            3
        ));
        assert!(observe_global_free_floor(
            Some(128),
            512,
            1024,
            &mut streak,
            3
        ));
    }

    #[test]
    fn global_free_floor_resets_on_healthy_or_missing_sample() {
        let mut streak = 2;
        assert!(!observe_global_free_floor(
            Some(2048),
            512,
            1024,
            &mut streak,
            3
        ));
        assert_eq!(streak, 0);
        streak = 2;
        assert!(!observe_global_free_floor(None, 512, 1024, &mut streak, 3));
        assert_eq!(streak, 0);
    }

    #[test]
    fn nbd_used_parser_requires_exact_device_identity() {
        let swaps = "\
Filename\t\t\tType\t\tSize\t\tUsed\t\tPriority
/dev/nbd01                              partition\t1024\t\t0\t\t-2
/tmp/nbd0-backup                       file\t\t1024\t\t0\t\t-3
/dev/nbd0                              partition\t1024\t\t37\t\t-4
";
        assert_eq!(nbd_used_kb_from_text(swaps, "/dev/nbd0"), Ok(37));
        assert_eq!(nbd_used_kb_from_text(swaps, "/dev/nbd01"), Ok(0));
        assert_eq!(nbd_used_kb_from_text(swaps, "/dev/nbd9"), Ok(0));
    }

    #[test]
    fn nbd_used_parser_is_fail_safe_for_duplicate_and_deleted_rows() {
        let swaps = "\
Filename\t\t\tType\t\tSize\t\tUsed\t\tPriority
/dev/nbd0                              partition\t1024\t\t0\t\t-2
/dev/nbd0\\040(deleted)                 partition\t1024\t\t55\t\t-3
";
        assert!(nbd_used_kb_from_text(swaps, "nbd0").is_err());
    }

    #[test]
    fn nbd_swap_absence_is_not_a_zero_used_active_entry() {
        let header = "Filename Type Size Used Priority\n";
        let active_zero = "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n";
        let deleted_zero =
            "Filename Type Size Used Priority\n/dev/nbd0\\040(deleted) partition 1024 0 100\n";
        assert_eq!(
            nbd_swap_is_explicitly_absent_from_text(header, "/dev/nbd0"),
            Ok(true)
        );
        assert_eq!(
            nbd_swap_is_explicitly_absent_from_text(active_zero, "/dev/nbd0"),
            Ok(false)
        );
        assert_eq!(
            nbd_swap_is_explicitly_absent_from_text(deleted_zero, "/dev/nbd0"),
            Ok(false)
        );
        assert!(nbd_swap_is_explicitly_absent_from_text(header, "/dev/not-nbd").is_err());
    }

    #[test]
    fn strict_swap_parser_rejects_malformed_or_ambiguous_snapshots() {
        let bad_header = "Filename Type Size Used\n";
        let bad_numeric = "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 unknown -2\n";
        let bad_type = "Filename Type Size Used Priority\n/dev/ublkb7 mystery 1024 0 -2\n";
        let duplicate = "\
Filename Type Size Used Priority
/dev/ublkb7 partition 1024 0 -2
/dev/ublkb7 partition 1024 0 -3
";

        for snapshot in [bad_header, bad_numeric, bad_type, duplicate] {
            assert!(
                strict_exact_swap_state_from_text(snapshot, "/dev/ublkb7", canonical_ublk_identity)
                    .is_err(),
                "malformed or ambiguous /proc/swaps data must fail closed"
            );
        }
    }

    #[test]
    fn unix_socket_setup_refuses_regular_file_and_symlink() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-socket-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).expect("create socket test directory");
        let regular = dir.join("regular");
        std::fs::write(&regular, b"keep").expect("create regular file");
        assert!(prepare_unix_socket_path(&regular).is_err());
        assert_eq!(
            std::fs::read(&regular).expect("regular file preserved"),
            b"keep"
        );

        let link = dir.join("link");
        std::os::unix::fs::symlink(&regular, &link).expect("create symlink");
        assert!(prepare_unix_socket_path(&link).is_err());
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("symlink preserved")
                .file_type()
                .is_symlink()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    // TestName: second_daemon_refuses_existing_socket_and_preserves_original_listener
    fn second_daemon_refuses_existing_socket_and_preserves_original_listener() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-existing-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("daemon.sock");
        let original = UnixListener::bind(&path).unwrap();

        let refused = prepare_unix_socket_path(&path).is_err();
        let connectable = std::os::unix::net::UnixStream::connect(&path).is_ok();
        drop(original);
        let _ = std::fs::remove_file(&path);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(refused, "a second daemon removed the live socket pathname");
        assert!(
            connectable,
            "the original listener stopped being connectable after second-daemon refusal"
        );
    }

    #[test]
    // TestName: stale_unix_socket_requires_explicit_cleanup_and_is_never_unlinked_on_startup
    fn stale_unix_socket_requires_explicit_cleanup_and_is_never_unlinked_on_startup() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-stale-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("daemon.sock");
        drop(UnixListener::bind(&path).unwrap());

        assert!(bind_owned_unix_listener(&path).is_err());
        assert!(
            std::fs::symlink_metadata(&path)
                .expect("stale socket pathname remains")
                .file_type()
                .is_socket()
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    // TestName: old_socket_cleanup_preserves_aba_replacement_identity
    fn old_socket_cleanup_preserves_aba_replacement_identity() {
        let dir = std::env::temp_dir().join(format!("ramshared-socket-aba-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("daemon.sock");
        let (original, old_guard) = bind_owned_unix_listener(&path).unwrap();
        drop(original);
        std::fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();

        drop(old_guard);
        let replacement_survived = path.exists();
        let connectable = std::os::unix::net::UnixStream::connect(&path).is_ok();
        drop(replacement);
        let _ = std::fs::remove_file(&path);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(
            replacement_survived && connectable,
            "cleanup for an old socket identity removed its ABA replacement"
        );
    }

    #[test]
    // TestName: unix_socket_parent_symlink_is_refused_before_bind
    fn unix_socket_parent_symlink_is_refused_before_bind() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-socket-parent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let real = dir.join("real");
        let alias = dir.join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let refused = prepare_unix_socket_path(&alias.join("daemon.sock")).is_err();
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(refused, "socket parent symlink was accepted");
    }

    #[test]
    // TestName: owned_unix_socket_cleanup_removes_only_the_exact_bound_identity
    fn owned_unix_socket_cleanup_removes_only_the_exact_bound_identity() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-owned-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("daemon.sock");
        let (listener, guard) = bind_owned_unix_listener(&path).unwrap();
        assert!(std::os::unix::net::UnixStream::connect(&path).is_ok());
        drop(listener);
        drop(guard);
        assert!(!path.exists(), "exact owned socket was not cleaned");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
