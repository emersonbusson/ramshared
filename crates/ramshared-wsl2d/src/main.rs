//! ramsharedd (crate `ramshared-wsl2d`) — VRAM tier daemon + Memory Broker (SPEC §4, §8).
//!
//! Serves fixed-newstyle NBD on a Unix socket; `nbd-client -unix <sock> /dev/nbdX`
//! wires up the kernel (the ioctls). This keeps the daemon **without `unsafe`** — the
//! only `unsafe` in the project lives isolated in `ramshared-cuda`.
//!
//! Allocates VRAM and serves **N NBD connections** (`nbd-client -C N`) via a dedicated
//! reader/writer per connection + a **single CUDA worker** (thread affinity, §9.4/H1), with
//! `mlockall`+`oom_score_adj` (Discipline 3) and the residency canary §9 (latency
//! per-request, **serve-only**) + §9.4 (content/free probe).
//! Backoff remains as future work.

use core::ffi::c_int;
use std::io::Read;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ramshared_block::protocol::{NBD_FLAG_CAN_MULTI_CONN, NBD_FLAG_HAS_FLAGS, NBD_FLAG_SEND_FLUSH};
use ramshared_block::{
    BlockBackend, Command, CommitBudgetGate, SparseVramBackend, chunk_bytes_from_env,
    commit_cap_bytes_from_env, idle_free_secs_from_env, prealloc_enabled,
    reserve_floor_bytes_from_env, safe_commit_cap, serve,
};
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
}
const MCL_CURRENT: c_int = 1;
const MCL_FUTURE: c_int = 2;
const SIGINT: c_int = 2;
const SIGTERM: c_int = 15;

const DEFAULT_SIZE: u64 = 256 * 1024 * 1024;
const BLOCK_SIZE: u32 = 4096;
const UBLK_CONTROL: &str = "/dev/ublk-control";
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

/// Parses `IP:PORT` (accepts `tcp://` prefix) and **rejects unspecified addresses** (0.0.0.0/::)
/// — RNF-2: bind only on private network/loopback, never public. Fails BEFORE any `bind()`.
fn parse_private_listen(s: &str) -> Result<std::net::SocketAddr, String> {
    let raw = s.strip_prefix("tcp://").unwrap_or(s);
    let addr: std::net::SocketAddr = raw
        .parse()
        .map_err(|_| format!("invalid address '{s}' (use IP:PORT)"))?;
    if addr.ip().is_unspecified() {
        return Err(format!(
            "bind on {} refused — RNF-2: private network or loopback only, never 0.0.0.0/::",
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

fn command_stdout_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = ProcessCommand::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Drain while the child is still running. Waiting for exit before reading
    // stdout lets a finite command deadlock once its pipe fills; on every
    // terminal path below the child is reaped before this reader is joined.
    let stdout = child.stdout.take()?;
    let drain = std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut output = String::new();
        stdout.read_to_string(&mut output).map(|_| output)
    });
    let started = Instant::now();
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = drain.join();
                return None;
            }
        };
        if let Some(status) = status {
            let output = drain.join().ok()?.ok()?;
            if !status.success() {
                return None;
            }
            return Some(output);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = drain.join();
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn global_gpu_free_bytes_from_nvidia_smi(timeout: Duration) -> Option<u64> {
    const ARGS: &[&str] = &["--query-gpu=memory.free", "--format=csv,noheader,nounits"];
    for program in ["/usr/lib/wsl/lib/nvidia-smi", "nvidia-smi"] {
        if program.starts_with('/') && !Path::new(program).exists() {
            continue;
        }
        if let Some(output) = command_stdout_with_timeout(program, ARGS, timeout)
            && let Some(bytes) = parse_nvidia_smi_free_bytes(&output)
        {
            return Some(bytes);
        }
    }
    None
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

struct AppArgs {
    size: u64,
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
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().collect();
        Self::parse_from(&args)
    }

    /// Parses an explicit argv vector before any backend selection or side effect.
    /// Keeping this boundary injectable makes all public refusals testable without
    /// loading CUDA/Vulkan or touching swap, NBD, or ublk state (memory-broker DT-46).
    fn parse_from(args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut size = DEFAULT_SIZE;
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
                }
                "--sock" => {
                    i += 1;
                    sock = args.get(i).ok_or("--sock requires a path")?.clone();
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
        size -= size % BLOCK_SIZE as u64; // align to the block size

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
    let args = AppArgs::parse()?;
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
        (Transport::Nbd, _) => Ok(DaemonAction::Nbd(args)),
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
                    sock,
                    force,
                    nbd_dev,
                    ..
                } = args;
                match backend {
                    BackendKind::Vram => {
                        let cuda = Cuda::load()?;
                        let dev = cuda.device(0)?;
                        eprintln!("[ramsharedd] GPU: {}", dev.name());
                        let ctx = cuda.create_context(&dev)?;
                        run_nbd(ctx, size, sock, force, nbd_dev, true)
                    }
                    BackendKind::Vulkan => {
                        let provider = VulkanProvider::open(0)?;
                        eprintln!("[ramsharedd] GPU (Vulkan): {}", provider.device_name());
                        run_nbd(provider, size, sock, force, nbd_dev, false)
                    }
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

    fn publish_demote(&mut self, total: u64, reason: &Option<String>, in_progress: bool);

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
        nbd_used_kb_from_proc(nbd_dev)
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

/// NBD path (fixed-newstyle in Unix socket). Single worker on current thread, generic over the
/// VRAM provider (RF-G1). SPEC cascade-vram-ondemand: sparse default; full prealloc via env.
fn run_nbd<P: VramProvider>(
    provider: P,
    size: u64,
    sock: String,
    force: bool,
    nbd_dev: String,
    use_dxg_budget: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut starter = ProductionNbdRuntimeStarter;
    run_nbd_with_startup(
        provider,
        size,
        sock,
        force,
        nbd_dev,
        use_dxg_budget,
        prealloc_enabled(),
        &mut starter,
    )
}

/// Injectable NBD composition. `use_prealloc` is captured by the production
/// environment wrapper before this function begins; tests choose the small
/// preallocated path explicitly and never mutate process environment.
#[allow(clippy::too_many_arguments)] // explicit daemon boundary keeps OS-facing test seams injectable
fn run_nbd_with_startup<P: VramProvider, S: NbdRuntimeStarter>(
    provider: P,
    size: u64,
    sock: String,
    force: bool,
    nbd_dev: String,
    use_dxg_budget: bool,
    use_prealloc: bool,
    starter: &mut S,
) -> Result<(), Box<dyn std::error::Error>> {
    let (free, total) = provider.mem_info()?;
    eprintln!(
        "[ramsharedd] VRAM livre={} MiB total={} MiB",
        free >> 20,
        total >> 20
    );

    let dxg = starter.startup_budget(use_dxg_budget)?;
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
    starter.lock_memory(force, true)?;

    // --- dedicated residency canary (§9.4) — always a small separate alloc ---
    let canary_region = provider.alloc(CANARY_BYTES)?;
    let mut probe = CanaryProbe::new(canary_region);
    let mut cadence = Cadence::new(CANARY_EVERY);
    let reserve_floor = reserve_floor_bytes_from_env();
    let residency_cfg = sparse_residency_config(reserve_floor);
    let mut sampler = ResidencySampler::new(residency_cfg);
    let free_floor = residency_cfg.free_floor_bytes;
    let idle_free = Duration::from_secs(idle_free_secs_from_env());

    enum Be<'a, Pr: VramProvider + 'a> {
        Pre(VramBackend<Pr::Mem<'a>>),
        Sparse(SparseVramBackend<'a, Pr>),
    }
    impl<'a, Pr: VramProvider + 'a> BlockBackend for Be<'a, Pr> {
        fn size_bytes(&self) -> u64 {
            match self {
                Be::Pre(b) => b.size_bytes(),
                Be::Sparse(b) => b.size_bytes(),
            }
        }
        fn block_size(&self) -> u32 {
            match self {
                Be::Pre(b) => b.block_size(),
                Be::Sparse(b) => b.block_size(),
            }
        }
        fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), ramshared_block::IoError> {
            match self {
                Be::Pre(b) => b.read_at(off, buf),
                Be::Sparse(b) => b.read_at(off, buf),
            }
        }
        fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), ramshared_block::IoError> {
            match self {
                Be::Pre(b) => b.write_at(off, data),
                Be::Sparse(b) => b.write_at(off, data),
            }
        }
        fn flush(&mut self) -> Result<(), ramshared_block::IoError> {
            match self {
                Be::Pre(b) => b.flush(),
                Be::Sparse(b) => b.flush(),
            }
        }
    }

    let mut backend: Be<'_, P> = if use_prealloc {
        if let Some(gate) = budget_gate {
            gate.allow_commit(0, size)
                .map_err(|message| format!("WDDM prealloc refused: {message}"))?;
        }
        let mut mem = provider.alloc(size as usize)?;
        mem.zero()?;
        eprintln!(
            "[ramsharedd] VRAM mode=prealloc capacity={} MiB (RAMSHARED_VRAM_PREALLOC)",
            size >> 20
        );
        Be::Pre(VramBackend::new(mem, BLOCK_SIZE))
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
    prepare_unix_socket_path(path)?;
    let listener = UnixListener::bind(path)?;
    eprintln!("[ramsharedd] escutando em {sock}");
    eprintln!("[ramsharedd] conecte: sudo nbd-client -C <N> -unix {sock} {nbd_dev}");

    let tx_flags = NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_CAN_MULTI_CONN;
    let device_size = backend.size_bytes();
    let exports = std::sync::Arc::new(vec![ramshared_block::handshake::Export {
        name: "default".to_string(),
        size: device_size,
    }]);
    let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel::<WMsg>(CHAN_CAP);
    if let Err(error) = starter.start_acceptor(listener, exports, tx_flags, jobs_tx.clone()) {
        cleanup_unix_socket_path(path);
        return Err(error);
    }
    let _shutdown_bridge = match starter.start_shutdown_bridge(jobs_tx) {
        Ok(bridge) => bridge,
        Err(error) => {
            cleanup_unix_socket_path(path);
            return Err(error);
        }
    };
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

                if touches_vram && !demoted && demote_rx.is_none() {
                    let mut residency_state = ResidencyCheckState {
                        canary: &mut canary,
                        baseline: &mut baseline,
                        sampler: &mut sampler,
                        cadence: &mut cadence,
                        probe: &mut probe,
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
                Be::Pre(_) => 0,
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
    while !backend_release_allowed(swapoff_attempted, swapoff_confirmed, used_kb) {
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
    }
    match &mut backend {
        Be::Pre(b) => {
            b.zero()?;
            eprintln!("[ramsharedd] encerrado (VRAM zerada prealloc)");
        }
        Be::Sparse(b) => {
            let n = b.free_all_live();
            eprintln!(
                "[ramsharedd] encerrado (sparse free {} MiB + canary)",
                n >> 20
            );
        }
    }
    let _ = probe.zero();
    cleanup_unix_socket_path(path);
    Ok(())
}

/// `used_kb` for the given NBD device path from `/proc/swaps`.
///
/// A read error is unsafe to interpret as an absent swap device, so return the
/// maximum value and keep the backend allocated.
fn nbd_used_kb_from_proc(nbd_dev: &str) -> u64 {
    match std::fs::read_to_string("/proc/swaps") {
        Ok(text) => nbd_used_kb_from_text(&text, nbd_dev),
        Err(error) => {
            eprintln!(
                "[ramsharedd] cannot read /proc/swaps for {nbd_dev}: {error}; \
                 keeping backend allocated"
            );
            u64::MAX
        }
    }
}

fn canonical_nbd_identity(path: &str) -> Option<String> {
    let path = path
        .trim()
        .strip_suffix("\\040(deleted)")
        .unwrap_or(path.trim());
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

fn nbd_used_kb_from_text(text: &str, nbd_dev: &str) -> u64 {
    let Some(key) = canonical_nbd_identity(nbd_dev) else {
        return u64::MAX;
    };
    let mut used_kb = 0;
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 {
            continue;
        }
        if canonical_nbd_identity(cols[0]).as_deref() == Some(key.as_str())
            && let Ok(observed) = cols[3].parse::<u64>()
        {
            used_kb = used_kb.max(observed);
        }
    }
    used_kb
}

fn prepare_unix_socket_path(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(path),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("refusing to replace non-socket path {}", path.display()),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn cleanup_unix_socket_path(path: &Path) {
    if matches!(
        std::fs::symlink_metadata(path),
        Ok(metadata) if metadata.file_type().is_socket()
    ) {
        let _ = std::fs::remove_file(path);
    }
}

/// The worker-owned half of broker startup. Keeping the `Receiver` owned (not
/// borrowed) makes the data-plane runnable in a bounded test thread without
/// pretending that `Receiver` is `Sync`.
struct BrokerWorkerRuntime {
    geom: Vec<(u64, u64)>,
    jobs_rx: std::sync::mpsc::Receiver<WMsg>,
    demote_tx: std::sync::mpsc::Sender<DemoteReason>,
    shutdown: std::sync::Arc<AtomicBool>,
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
    NotifierUnavailable,
}

/// Paired terminal flag and explicit worker-channel wake. `request` stores the
/// flag first and never blocks, so signal mirroring and rollback paths cannot
/// deadlock behind a full bounded queue.
#[derive(Clone)]
struct BrokerShutdown {
    flag: std::sync::Arc<AtomicBool>,
    wake_tx: std::sync::mpsc::SyncSender<WMsg>,
}

impl BrokerShutdown {
    fn new(flag: std::sync::Arc<AtomicBool>, wake_tx: std::sync::mpsc::SyncSender<WMsg>) -> Self {
        Self { flag, wake_tx }
    }

    fn request(&self) -> BrokerShutdownWake {
        self.flag.store(true, Ordering::SeqCst);
        match self.wake_tx.try_send(WMsg::Shutdown) {
            Ok(()) => BrokerShutdownWake::Queued,
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                let wake_tx = self.wake_tx.clone();
                match std::thread::Builder::new()
                    .name("ramshared-broker-shutdown".into())
                    .spawn(move || {
                        let _ = wake_tx.send(WMsg::Shutdown);
                    }) {
                    Ok(_) => BrokerShutdownWake::QueueFull,
                    Err(_) => BrokerShutdownWake::NotifierUnavailable,
                }
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
}

impl BrokerRuntime {
    fn into_parts(self) -> (BrokerWorkerRuntime, std::thread::JoinHandle<()>) {
        (self.worker, self.broker)
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
    path: std::path::PathBuf,
    unix: UnixListener,
    tcp: Option<std::net::TcpListener>,
}

impl BrokerListeners {
    fn cleanup(self) {
        let path = self.path.clone();
        drop(self);
        cleanup_unix_socket_path(&path);
    }

    fn into_parts(self) -> (UnixListener, Option<std::net::TcpListener>) {
        (self.unix, self.tcp)
    }
}

/// Binds the safe socket portion of broker startup. If the optional TCP bind
/// fails after Unix binding succeeded, it closes and removes only that newly
/// created Unix socket before returning the refusal.
fn bind_broker_listeners(
    path: &Path,
    listen_nbd_addr: Option<std::net::SocketAddr>,
) -> std::io::Result<BrokerListeners> {
    prepare_unix_socket_path(path)?;
    let unix = UnixListener::bind(path)?;
    let tcp = match listen_nbd_addr {
        Some(addr) => match std::net::TcpListener::bind(addr) {
            Ok(listener) => Some(listener),
            Err(error) => {
                drop(unix);
                cleanup_unix_socket_path(path);
                return Err(error);
            }
        },
        None => None,
    };
    Ok(BrokerListeners {
        path: path.to_path_buf(),
        unix,
        tcp,
    })
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
            .map(|(name, size)| ramshared_block::handshake::Export { name, size })
            .collect::<Vec<_>>(),
    );

    let tx_flags = NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_CAN_MULTI_CONN;
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
    let (unix, tcp) = listeners.into_parts();
    eprintln!("[ramsharedd] NBD Unix listener at {sock}");
    if let Err(error) = acceptors.start(unix, tcp, exports, tx_flags, jobs_tx.clone()) {
        let _ = broker_shutdown.request();
        let _ = broker.join();
        cleanup_unix_socket_path(Path::new(sock));
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
            slice_io,
            vram,
        },
        broker,
        shutdown: broker_shutdown,
    })
}

/// Bounded broker worker loop. Production uses a 500 ms shutdown poll; tests
/// inject a shorter positive interval to prove that an idle worker cannot wait
/// indefinitely after its explicit stop signal.
fn serve_broker_jobs_with_poll<B: BlockBackend>(
    mut backend: B,
    rt: BrokerWorkerRuntime,
    mut residency: impl FnMut(u64) -> Option<DemoteReason>,
    poll_interval: Duration,
) -> B {
    let poll_interval = if poll_interval.is_zero() {
        Duration::from_millis(1)
    } else {
        poll_interval
    };
    let mut demoted = false;
    eprintln!("[ramsharedd] em transmissão (worker único; multi-slice/broker)");
    loop {
        let msg = match rt.jobs_rx.recv_timeout(poll_interval) {
            Ok(m) => m,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if rt.shutdown.load(Ordering::SeqCst) {
                    break; // DT-28: encerra só no SIGINT/SIGTERM
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let job = match msg {
            // DT-28: NBD connections coming and going do NOT terminate the daemon (the broker persists).
            WMsg::Opened | WMsg::Closed => continue,
            WMsg::Shutdown => break,
            WMsg::Job(job) => job,
            WMsg::ZeroExport { base, len, done } => {
                let ok = zero_window(&mut backend, base, len).is_ok();
                let _ = done.send(ok);
                continue;
            }
        };

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
        let _ = job.reply.send(Reply {
            reply: out.reply,
            data: out.read_data,
            disconnect: out.disconnect,
        });

        // Telemetry RF-1: bytes/IO served on this slice (atomic, cheap hot path — gate ITEM-2).
        if touches && let Some(c) = rt.slice_io.get(job.export) {
            c.bytes_served
                .fetch_add(u64::from(job.req.len), Ordering::Relaxed);
            c.io_count.fetch_add(1, Ordering::Relaxed);
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
    sock: String,
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
    // CUDA/VRAM have already been allocated above -> safe to lock MCL_FUTURE all at once.
    if let Err(error) = lock(force, true) {
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
            cleanup_unix_socket_path(Path::new(&sock));
            backend_zeroed?;
            probe_zeroed?;
            return Err(error);
        }
    };
    let (worker, broker) = rt.into_parts();
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

    let _ = broker.join();
    let zeroed = backend.zero();
    let _ = probe.zero(); // DT-12/DT-17: zeroes the canary-region as well
    cleanup_unix_socket_path(Path::new(&sock));
    zeroed?;
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
    sock: String,
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
    let (worker, broker) = rt.into_parts();
    let _ = serve_broker_jobs_with_poll(backend, worker, |_| None, worker_poll); // RAM: no residency

    let _ = broker.join();
    cleanup_unix_socket_path(Path::new(&sock));
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
    let sectors = size / SECTOR;
    if let Err(error) = runtime.set_params(device, sectors) {
        let _ = runtime.delete_device(device);
        return Err(error);
    }
    let char_path = format!("/dev/ublkc{}", device.id);
    let block_path = format!("/dev/ublkb{}", device.id);
    let server =
        match runtime.start_server(backend, &char_path, &block_path, device.queue_depth, size) {
            Ok(server) => server,
            Err(error) => {
                let _ = runtime.delete_device(device);
                return Err(error);
            }
        };
    if let Err(error) = runtime.start_device(device) {
        let _ = runtime.stop_device(device);
        let _ = server.join();
        let _ = runtime.delete_device(device);
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

    // STOP_DEV aborts FETCHes, then the server can join, then DEL_DEV removes
    // the node. Cleanup still runs if the wait or stop stage reported an error.
    let wait = runtime.wait_for_shutdown();
    let stop = runtime.stop_device(device);
    let joined = server.join();
    let deleted = runtime.delete_device(device);
    wait?;
    stop?;
    joined?;
    deleted?;
    eprintln!("[ramsharedd] ublk device removed");
    Ok(())
}

/// Refuses to serve ublk on WSL2 (unless conscious override
/// `RAMSHARED_ALLOW_UBLK_ON_WSL2=1`). Reason: teardown of the standalone ublk daemon,
/// if it fails (late SIGTERM -> SIGKILL race, or bug in STOP_DEV/join), leaves
/// `/dev/ublkbN` WITHOUT a server with I/O in flight -> processes in D-state in the
/// writeback/memory path -> with `mlockall(MCL_FUTURE)` + `drop_caches` the kernel does not
/// progress -> global stall -> WSL2 FREEZES (incident 2026-06-09). Validate the complete
/// daemon only in VM/QEMU (`scripts/kernel/qemu-validate.sh`), where a stall is
/// recoverable without dropping the host.
fn guard_not_wsl2() -> Result<(), Box<dyn std::error::Error>> {
    let allow_override = std::env::var("RAMSHARED_ALLOW_UBLK_ON_WSL2")
        .ok()
        .as_deref()
        == Some("1");
    if allow_override {
        eprintln!("[ramsharedd] WARNING: RAMSHARED_ALLOW_UBLK_ON_WSL2=1 — WSL2 lock ignored");
    }
    let osrelease = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    ublk_osrelease_guard(&osrelease, allow_override).map_err(Into::into)
}

/// Pure WSL2 safety policy: only an explicit operator override permits ublk
/// on a Microsoft/WSL kernel. Keeping the environment/proc read outside this
/// function makes the refusal matrix testable without inspecting host state.
fn ublk_osrelease_guard(osrelease: &str, allow_override: bool) -> Result<(), String> {
    if allow_override {
        return Ok(());
    }
    let lower = osrelease.to_ascii_lowercase();
    if lower.contains("microsoft") || lower.contains("wsl") {
        return Err(format!(
            "refused: --transport ublk on WSL2 ({}) can freeze the system if daemon teardown \
             fails (orphaned device -> D-state I/O). Validate the daemon in VM/QEMU. \
             Conscious override: RAMSHARED_ALLOW_UBLK_ON_WSL2=1.",
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
/// `lock_future`: includes `MCL_FUTURE` (locks future mmaps too) or only `MCL_CURRENT`
/// (only what is already mapped now). NBD/broker paths (`run_nbd`/`run_broker`)
/// call this AFTER `provider.alloc()` — the CUDA context and VRAM itself have already been
/// allocated, so `MCL_FUTURE` at once is safe. The `run_ublk` path with VRAM backend
/// is different: needs to be called with `lock_future=false` BEFORE the backend
/// initializes CUDA, and only arm `MCL_FUTURE` later via `arm_future_lock` — see the
/// comment there (incident 2026-07-03: kernel BUG due to collision with dxgkrnl).
fn lock_memory(force: bool, lock_future: bool) -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: mlockall is a syscall with no unsafe memory side effects.
    let flags = if lock_future {
        MCL_CURRENT | MCL_FUTURE
    } else {
        MCL_CURRENT
    };
    let locked = unsafe { mlockall(flags) } == 0;
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
    use std::cell::RefCell;

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
        let (worker, broker) = runtime.into_parts();
        let _backend = serve_broker_jobs_with_poll(
            RamBackend::new(4096),
            worker,
            |_| None,
            Duration::from_millis(1),
        );
        broker
            .join()
            .expect("broker exits after its injected shutdown");
        cleanup_unix_socket_path(&path);
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

            fn publish_demote(&mut self, total: u64, reason: &Option<String>, in_progress: bool) {
                self.status_updates
                    .push((total, reason.clone(), in_progress));
            }

            fn elapsed_us(&mut self, _started: Instant) -> u64 {
                10
            }

            fn spawn_swapoff(&mut self, _nbd_dev: &str) -> std::sync::mpsc::Receiver<bool> {
                panic!("the safe prealloc fixture must not request swapoff")
            }

            fn spawn_recovery_activation(
                &mut self,
                _nbd_dev: &str,
                _priority: i16,
            ) -> Result<std::sync::mpsc::Receiver<bool>, Box<dyn std::error::Error>> {
                panic!("the safe prealloc fixture must not activate swap")
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
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
            true,
            &mut starter,
        )
        .expect("fake prealloc worker must complete without a CUDA, swap, or NBD device");

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
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
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
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            true,
            false,
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
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            true,
            false,
            &mut starter,
        )
        .expect("healthy hysteresis recovers the fake sparse daemon");
        assert_eq!(starter.swapoff_calls, 1);
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
                4096,
                thread_path.to_string_lossy().into_owned(),
                false,
                "/dev/ramshared-test-nbd".into(),
                true,
                false,
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
            usage: [64, 0].into(),
            swapoff_calls: 0,
        };
        run_nbd_with_startup(
            TestProvider,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
            true,
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
                Ok((8 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024))
            }
        }

        struct DemoteStarter {
            latencies: std::collections::VecDeque<u64>,
            swapoff_calls: usize,
            replies: Option<std::sync::mpsc::Receiver<Reply>>,
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
                jobs_tx.send(WMsg::Opened)?;
                for handle in 0..20 {
                    jobs_tx.send(WMsg::Job(ramshared_wsl2d::conn::Job {
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
                    }))?;
                }
                jobs_tx.send(WMsg::Closed)?;
                jobs_tx.send(WMsg::Shutdown)?;
                self.replies = Some(reply_rx);
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
            latencies: std::iter::repeat_n(10, 16)
                .chain(std::iter::repeat_n(1_000, 3))
                .chain(std::iter::once(10))
                .collect(),
            swapoff_calls: 0,
            replies: None,
            status_updates: Vec::new(),
        };
        run_nbd_with_startup(
            TestProvider,
            4096,
            path.to_string_lossy().into_owned(),
            false,
            "/dev/ramshared-test-nbd".into(),
            false,
            true,
            &mut starter,
        )
        .expect("injected terminal DEMOTE must tear down the fake prealloc backend");

        assert_eq!(
            starter.swapoff_calls, 1,
            "one DEMOTE starts one bounded swapoff"
        );
        assert_eq!(
            starter.status_updates,
            vec![
                (0, None, false),
                (0, Some("Latency".into()), true),
                (1, Some("Latency".into()), false),
            ]
        );
        let replies = starter
            .replies
            .take()
            .expect("injected reply receiver")
            .try_iter()
            .count();
        assert_eq!(
            replies, 20,
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
                "guard", "lock", "signal", "add", "params", "server", "start", "wait", "stop",
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
    fn daemon_ublk_runtime_failures_delete_candidate_before_return() {
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
                vec!["guard", "lock", "signal", "add", "params", "delete"],
            ),
            (
                Failure::Server,
                vec![
                    "guard", "lock", "signal", "add", "params", "server", "delete",
                ],
            ),
            (
                Failure::Start,
                vec![
                    "guard", "lock", "signal", "add", "params", "server", "start", "stop", "join",
                    "delete",
                ],
            ),
            (
                Failure::Wait,
                vec![
                    "guard", "lock", "signal", "add", "params", "server", "start", "wait", "stop",
                    "join", "delete",
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
    fn daemon_ublk_wsl_guard_and_memory_lock_policy_are_pure_and_fail_closed() {
        assert!(ublk_osrelease_guard("6.6.0-microsoft-standard-WSL2", false).is_err());
        assert!(ublk_osrelease_guard("6.6.0-wsl", false).is_err());
        assert!(ublk_osrelease_guard("6.8.0-generic", false).is_ok());
        assert!(
            ublk_osrelease_guard("6.6.0-microsoft-standard-WSL2", true).is_ok(),
            "explicit override is the only way past the WSL2 guard"
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
    fn daemon_worker_serves_job_counts_io_and_stops_on_shutdown() {
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
            slice_io: std::sync::Arc::clone(&slice_io),
            vram: std::sync::Arc::new(VramGauge::default()),
        };
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();

        std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                serve_broker_jobs_with_poll(
                    RamBackend::new(4096),
                    worker_rt,
                    |_| None,
                    Duration::from_millis(10),
                )
            });
            jobs_tx
                .send(WMsg::Job(ramshared_wsl2d::conn::Job {
                    export: 0,
                    req: ramshared_block::Request {
                        flags: 0,
                        cmd: Command::Write,
                        handle: 7,
                        offset: 0,
                        len: 512,
                    },
                    payload: vec![0x5A; 512],
                    reply: reply_tx,
                }))
                .expect("worker queue accepts bounded test job");

            let reply = reply_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("worker reply must arrive before the test deadline");
            assert!(!reply.disconnect);
            assert_eq!(slice_io[0].bytes_served.load(Ordering::Relaxed), 512);
            assert_eq!(slice_io[0].io_count.load(Ordering::Relaxed), 1);

            assert_eq!(stop.request(), BrokerShutdownWake::Queued);
            let started = Instant::now();
            let _backend = worker.join().expect("worker joins after shutdown");
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "worker shutdown must honor its bounded poll interval"
            );
        });
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
            shutdown,
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

    #[test]
    fn daemon_worker_shutdown_full_queue_is_nonblocking() {
        let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel(CHAN_CAP);
        for _ in 0..CHAN_CAP {
            jobs_tx
                .try_send(WMsg::Opened)
                .expect("manufactured queue has exact capacity");
        }
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let stop = BrokerShutdown::new(std::sync::Arc::clone(&shutdown), jobs_tx);

        assert_eq!(stop.request(), BrokerShutdownWake::QueueFull);
        assert!(shutdown.load(Ordering::SeqCst));

        let mut opened = 0;
        loop {
            match jobs_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("full-queue notifier appends a terminal wake")
            {
                WMsg::Opened => opened += 1,
                WMsg::Shutdown => break,
                _ => panic!("manufactured queue contains only Opened and Shutdown"),
            }
        }
        assert_eq!(opened, CHAN_CAP, "shutdown wake preserves prior FIFO work");
    }

    #[test]
    fn daemon_worker_shutdown_drains_queued_io_before_stop() {
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
        let reply = reply_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("queued write receives a reply before shutdown");
        assert!(!reply.disconnect);
        assert_eq!(slice_io[0].bytes_served.load(Ordering::Relaxed), 512);
        assert_eq!(slice_io[0].io_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn daemon_command_timeout_terminates_child_without_hang() {
        let output = command_stdout_with_timeout(
            "head",
            &["-c", "131072", "/dev/zero"],
            Duration::from_secs(1),
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
                .contains("refusing to replace non-socket path"),
            "the production runner must preserve a non-socket path before acceptor startup"
        );
        assert_eq!(
            std::fs::read(&sock).expect("regular preflight file remains readable"),
            b"do-not-replace"
        );
        std::fs::remove_dir_all(&dir).expect("remove isolated temporary refusal directory");
    }

    #[test]
    fn private_listen_accepts_loopback_and_lan() {
        assert_eq!(parse_private_listen("127.0.0.1:7777").unwrap().port(), 7777);
        assert!(parse_private_listen("tcp://192.168.0.50:10809").is_ok());
    }

    #[test]
    fn private_listen_rejects_unspecified() {
        // RNF-2 / #5 abort trigger: public bind rejected BEFORE any bind().
        assert!(parse_private_listen("0.0.0.0:10809").is_err());
        assert!(parse_private_listen("tcp://0.0.0.0:7777").is_err());
        assert!(parse_private_listen("[::]:7777").is_err());
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
        assert_eq!(nbd_used_kb_from_text(swaps, "/dev/nbd0"), 37);
        assert_eq!(nbd_used_kb_from_text(swaps, "/dev/nbd01"), 0);
        assert_eq!(nbd_used_kb_from_text(swaps, "/dev/nbd9"), 0);
    }

    #[test]
    fn nbd_used_parser_is_fail_safe_for_duplicate_and_deleted_rows() {
        let swaps = "\
Filename\t\t\tType\t\tSize\t\tUsed\t\tPriority
/dev/nbd0                              partition\t1024\t\t0\t\t-2
/dev/nbd0\\040(deleted)                 partition\t1024\t\t55\t\t-3
";
        assert_eq!(nbd_used_kb_from_text(swaps, "nbd0"), 55);
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
}
