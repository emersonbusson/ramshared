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
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
use ramshared_wsl2d::swap::{activate_swap, spawn_swapoff};
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
            eprintln!("[ramsharedd] erro: {e}");
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
        .map_err(|_| format!("endereço inválido '{s}' (use IP:PORT)"))?;
    if addr.ip().is_unspecified() {
        return Err(format!(
            "bind em {} recusado — RNF-2: só rede privada/loopback, nunca 0.0.0.0/::",
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
fn residency_check<M: VramMemory, F: Fn() -> Option<u64>>(
    lat_us: u64,
    canary: &mut Option<Canary>,
    baseline: &mut Vec<u64>,
    sampler: &mut ResidencySampler,
    cadence: &mut Cadence,
    probe: &mut CanaryProbe<M>,
    free_floor_bytes: u64,
    mem_free: F,
) -> Option<DemoteReason> {
    // §9: per-request latency canary. content_ok=true/free=u64::MAX ON PURPOSE — the signal
    // here is latency; content and free-floor come from the probe §9.4 below.
    let mut latency_reason = None;
    match canary.as_mut() {
        None => {
            baseline.push(lat_us);
            if baseline.len() >= 16 {
                baseline.sort_unstable();
                let med = baseline[baseline.len() / 2].max(1);
                *canary = Some(Canary::new(ResidencyConfig::default(), med));
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
    if cadence.tick() {
        let content = probe.check_content().ok();
        let free = mem_free();
        let verdict = sampler.sample(content, free);
        let streak = sampler.bad_streak();
        if should_log_probe_sample(content, free, free_floor_bytes, streak) {
            eprintln!(
                "[ramsharedd] sonda §9.4 sample: content={content:?} free={free:?} \
                 floor={free_floor_bytes} streak={streak}"
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
) -> bool {
    content != Some(true)
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

        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--size" => {
                    i += 1;
                    let mb: u64 = args.get(i).ok_or("--size requer valor (MiB)")?.parse()?;
                    size = mb
                        .checked_mul(1024 * 1024)
                        .ok_or("--size: overflow (MiB grande demais)")?;
                }
                "--sock" => {
                    i += 1;
                    sock = args.get(i).ok_or("--sock requer caminho")?.clone();
                }
                "--force" => force = true,
                "--nbd" => {
                    i += 1;
                    nbd_dev = args.get(i).ok_or("--nbd requer caminho")?.clone();
                }
                "--transport" => {
                    i += 1;
                    transport = match args.get(i).map(String::as_str) {
                        Some("nbd") => Transport::Nbd,
                        Some("ublk") => Transport::Ublk,
                        _ => return Err("--transport requer 'nbd' ou 'ublk'".into()),
                    };
                }
                "--queue-depth" => {
                    i += 1;
                    queue_depth = args
                        .get(i)
                        .ok_or("--queue-depth requer valor")?
                        .parse()
                        .map_err(|_| "--queue-depth invalido")?;
                }
                "--backend" => {
                    i += 1;
                    backend = match args.get(i).map(String::as_str) {
                        Some("vram") => BackendKind::Vram,
                        Some("vulkan") => BackendKind::Vulkan,
                        Some("ram") => BackendKind::Ram,
                        _ => return Err("--backend requer 'vram', 'vulkan' ou 'ram'".into()),
                    };
                }
                "--slices" => {
                    i += 1;
                    slices = args
                        .get(i)
                        .ok_or("--slices requer valor")?
                        .parse()
                        .map_err(|_| "--slices inválido")?;
                }
                "--slice-mb" => {
                    i += 1;
                    slice_mb = args
                        .get(i)
                        .ok_or("--slice-mb requer valor (MiB)")?
                        .parse()
                        .map_err(|_| "--slice-mb inválido")?;
                }
                "--listen-nbd" => {
                    i += 1;
                    listen_nbd = Some(
                        args.get(i)
                            .ok_or("--listen-nbd requer tcp://IP:PORT")?
                            .clone(),
                    );
                }
                "--arbiter-listen" => {
                    i += 1;
                    arbiter = Some(
                        args.get(i)
                            .ok_or("--arbiter-listen requer IP:PORT")?
                            .clone(),
                    );
                }
                "--advertise-nbd" => {
                    i += 1;
                    advertise_nbd = Some(
                        args.get(i)
                            .ok_or("--advertise-nbd requer HOST:PORT")?
                            .clone(),
                    );
                }
                "--telemetry-jsonl" => {
                    i += 1;
                    telemetry_jsonl = Some(
                        args.get(i)
                            .ok_or("--telemetry-jsonl requer caminho")?
                            .clone(),
                    );
                }
                other => return Err(format!("argumento desconhecido: {other}").into()),
            }
            i += 1;
        }
        size -= size % BLOCK_SIZE as u64; // alinhar ao block size

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
                "--advertise-nbd exige --listen-nbd (anunciar um endpoint que se serve)".into(),
            );
        }

        let advertise_tcp = advertise_nbd_addr
            .or(listen_nbd_addr)
            .map(|a| (a.ip().to_string(), a.port()));
        let telemetry_jsonl = telemetry_jsonl.map(std::path::PathBuf::from);

        let slice_bytes = if slices > 0 {
            slice_mb
                .checked_mul(1024 * 1024)
                .ok_or("--slice-mb: overflow (MiB grande demais)")?
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

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = AppArgs::parse()?;

    // Broker mode (ITEM-8): --slices > 0 slices the memory and starts the arbiter. Requires --arbiter-listen
    // (the broker control point). --listen-nbd is optional (TCP/civm tenants besides Unix).
    // --backend ram serves without GPU (validation in QEMU, ITEM-11); vram is the production path.
    if args.slices > 0 {
        let arbiter_addr = args
            .arbiter_addr
            .ok_or("--slices exige --arbiter-listen IP:PORT (ponto de controle)")?;
        return match args.backend {
            BackendKind::Vram => {
                // CUDA Shell: creates the provider (Context impl VramProvider) and enters the
                // generic path.
                let cuda = Cuda::load()?;
                let dev = cuda.device(0)?;
                eprintln!("[ramsharedd] GPU: {}", dev.name());
                let ctx = cuda.create_context(&dev)?;
                run_broker(
                    ctx,
                    args.slice_bytes,
                    args.slices,
                    args.sock,
                    args.force,
                    args.listen_nbd_addr,
                    args.advertise_tcp,
                    arbiter_addr,
                    args.telemetry_jsonl,
                )
            }
            BackendKind::Vulkan => {
                // Vulkan Shell (RF-V4/DT-11): Vulkan provider in the SAME generic run_broker.
                let provider = VulkanProvider::open(0)?;
                eprintln!("[ramsharedd] GPU (vulkan): {}", provider.device_name());
                run_broker(
                    provider,
                    args.slice_bytes,
                    args.slices,
                    args.sock,
                    args.force,
                    args.listen_nbd_addr,
                    args.advertise_tcp,
                    arbiter_addr,
                    args.telemetry_jsonl,
                )
            }
            BackendKind::Ram => run_broker_ram(
                args.slice_bytes,
                args.slices,
                args.sock,
                args.listen_nbd_addr,
                args.advertise_tcp,
                arbiter_addr,
                args.telemetry_jsonl,
            ),
        };
    }
    // Without slices, there is nothing to arbitrate or export via TCP.
    if args.arbiter_addr.is_some() || args.listen_nbd_addr.is_some() {
        return Err("--arbiter-listen/--listen-nbd require --slices N (N > 0)".into());
    }

    match args.transport {
        Transport::Nbd => match args.backend {
            BackendKind::Vram => {
                // CUDA Shell: creates the provider and enters the generic path.
                let cuda = Cuda::load()?;
                let dev = cuda.device(0)?;
                eprintln!("[ramsharedd] GPU: {}", dev.name());
                let ctx = cuda.create_context(&dev)?;
                run_nbd(ctx, args.size, args.sock, args.force, args.nbd_dev, true)
            }
            BackendKind::Vulkan => {
                // Vulkan Shell (RF-V4/DT-11): Vulkan provider in the SAME generic run_nbd.
                let provider = VulkanProvider::open(0)?;
                eprintln!("[ramsharedd] GPU (vulkan): {}", provider.device_name());
                run_nbd(
                    provider,
                    args.size,
                    args.sock,
                    args.force,
                    args.nbd_dev,
                    false,
                )
            }
            BackendKind::Ram => Err(
                "--backend ram não tem caminho NBD single; use --slices (broker) ou ublk".into(),
            ),
        },
        Transport::Ublk => run_ublk(args.size, args.force, args.queue_depth, args.backend),
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
    let (free, total) = provider.mem_info()?;
    eprintln!(
        "[ramsharedd] VRAM livre={} MiB total={} MiB",
        free >> 20,
        total >> 20
    );

    let use_prealloc = prealloc_enabled();

    let dxg = if use_dxg_budget {
        match DxgBudgetProvider::open(None) {
            Ok(provider) => {
                eprintln!(
                    "[ramsharedd] budget_source=dxg adapter={} (WDDM authority)",
                    provider.adapter_luid()
                );
                Some(provider)
            }
            Err(error) if error.permits_startup_fallback() => {
                eprintln!(
                    "[ramsharedd] budget_source=cuda-fallback reason={error}; \
                     CUDA free-floor is secondary compatibility mode"
                );
                None
            }
            Err(error) => return Err(error.into()),
        }
    } else {
        None
    };
    struct DxgGate<'a> {
        provider: &'a DxgBudgetProvider,
        config: AutotierConfig,
    }
    impl CommitBudgetGate for DxgGate<'_> {
        fn allow_commit(&self, committed: u64, next_chunk: u64) -> Result<(), String> {
            let snapshot = self
                .provider
                .snapshot()
                .map_err(|error| error.to_string())?;
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
    let dxg_gate = dxg.as_ref().map(|provider| DxgGate {
        provider,
        config: AutotierConfig::default(),
    });
    let budget_gate = dxg_gate.as_ref().map(|gate| gate as &dyn CommitBudgetGate);
    // Discipline 3: mlock host pages; for sparse, CUDA commit is on-demand (SPEC).
    lock_memory(force, true)?;

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
        let sparse = SparseVramBackend::new_with_limits_and_gate(
            &provider,
            size,
            chunk,
            BLOCK_SIZE,
            reserve,
            Some(commit_cap),
            budget_gate,
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
    let _ = std::fs::remove_file(path);
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
    let _acceptor = spawn_acceptor(listener, exports, tx_flags, jobs_tx);
    eprintln!("[ramsharedd] transmitting (single CUDA worker; multi-connection)");

    let mut canary: Option<Canary> = None;
    let mut baseline: Vec<u64> = Vec::new();
    let mut demoted = false;
    let mut demote_rx: Option<std::sync::mpsc::Receiver<bool>> = None;
    let mut swapoff_attempted = false;
    let mut swapoff_confirmed = false;
    let mut observed_budget_refuses = 0;
    let mut recovery = RecoveryTracker::new(3);
    let mut live = LiveCount::new();
    // CLI status --json demote fields (cascade-lifecycle-observability ITEM-3)
    let mut demotes_total: u64 = 0;
    let mut last_demote_reason: Option<String> = None;
    let publish_demote = |total: u64, reason: &Option<String>, in_progress: bool| {
        let st = ramshared_wsl2d::DemoteStatusFile {
            total,
            last_reason: reason.clone(),
            in_progress,
        };
        if let Err(e) = ramshared_wsl2d::write_demote_status(
            std::path::Path::new(ramshared_wsl2d::DEMOTE_STATUS_PATH),
            &st,
        ) {
            eprintln!("[ramsharedd] demote-status write: {e}");
        }
    };
    publish_demote(0, &None, false);

    // recv_timeout so sparse reclaim runs even with no NBD I/O (idle free).
    const RECV_TICK: Duration = Duration::from_secs(5);

    loop {
        let msg = match jobs_rx.recv_timeout(RECV_TICK) {
            Ok(m) => Some(m),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if let Some(msg) = msg {
            let job = match msg {
                WMsg::Opened => {
                    live.on_open();
                    // fall through to reclaim tick
                    None
                }
                WMsg::Closed => {
                    if live.on_close() {
                        break;
                    }
                    None
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
                let lat_us = t0.elapsed().as_micros() as u64;
                let _ = job.reply.send(Reply {
                    reply: out.reply,
                    data: out.read_data,
                    disconnect: out.disconnect,
                });

                if let Some(rx) = demote_rx.take() {
                    match rx.try_recv() {
                        Ok(true) => {
                            demoted = true;
                            swapoff_confirmed = true;
                            demotes_total = demotes_total.saturating_add(1);
                            publish_demote(demotes_total, &last_demote_reason, false);
                            eprintln!(
                                "[ramsharedd] DEMOTE: swapoff {nbd_dev} OK (canario desarmado)"
                            );
                        }
                        Ok(false) => {
                            publish_demote(demotes_total, &last_demote_reason, false);
                            eprintln!(
                                "[ramsharedd] DEMOTE: swapoff {nbd_dev} FALHOU; canario re-armado"
                            );
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            demote_rx = Some(rx);
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            publish_demote(demotes_total, &last_demote_reason, false);
                            eprintln!(
                                "[ramsharedd] DEMOTE: thread de swapoff sumiu; canario re-armado"
                            );
                        }
                    }
                }

                if touches_vram
                    && !demoted
                    && demote_rx.is_none()
                    && let Some(reason) = residency_check(
                        lat_us,
                        &mut canary,
                        &mut baseline,
                        &mut sampler,
                        &mut cadence,
                        &mut probe,
                        free_floor,
                        || provider.mem_info().ok().map(|(f, _)| f),
                    )
                {
                    let sparse = matches!(backend, Be::Sparse(_));
                    let skip = sparse && !sparse_residency_requests_swapoff(reason);
                    if skip {
                        eprintln!("[ramsharedd] sparse skip swapoff for {reason:?} lat={lat_us}us");
                    }
                    if !skip {
                        eprintln!(
                            "[ramsharedd] DEMOTE ({reason:?}) lat={lat_us}us -> swapoff {nbd_dev}"
                        );
                        last_demote_reason = Some(format!("{reason:?}"));
                        demote_rx = Some(spawn_swapoff(&nbd_dev));
                        swapoff_attempted = true;
                        publish_demote(demotes_total, &last_demote_reason, true);
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
                    demote_rx = Some(spawn_swapoff(&nbd_dev));
                    swapoff_attempted = true;
                    publish_demote(demotes_total, &last_demote_reason, true);
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
                    publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: swapoff {nbd_dev} OK (parked)");
                }
                Ok(false) => {
                    recovery.reset();
                    publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: swapoff {nbd_dev} FALHOU");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => demote_rx = Some(rx),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    recovery.reset();
                    publish_demote(demotes_total, &last_demote_reason, false);
                    eprintln!("[ramsharedd] DEMOTE: thread de swapoff sumiu");
                }
            }
        }

        // SPEC ITEM-2: reclaim on worker thread (I/O or idle tick).
        if let Be::Sparse(ref mut sp) = backend {
            let used_kb = nbd_used_kb_from_proc(&nbd_dev);
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
            let budget_healthy = dxg_provider
                .snapshot()
                .ok()
                .and_then(|snapshot| {
                    commit_allowed(
                        BudgetInput {
                            budget: snapshot.budget,
                            current_usage: snapshot.current_usage,
                            cuda_committed: sparse.committed_bytes(),
                            sampled_at: snapshot.sampled_at,
                        },
                        sparse.committed_bytes(),
                        sparse.chunk_bytes(),
                        &AutotierConfig::default(),
                    )
                    .ok()
                })
                .is_some();

            if !demoted && demote_rx.is_none() && !budget_healthy {
                eprintln!("[ramsharedd] WDDM poll constrained -> swapoff {nbd_dev}");
                last_demote_reason = Some("WddmBudgetPoll".into());
                demote_rx = Some(spawn_swapoff(&nbd_dev));
                swapoff_attempted = true;
                publish_demote(demotes_total, &last_demote_reason, true);
            } else if demoted && recovery.observe(budget_healthy, sparse.chunks_live() == 0) {
                if activate_swap(&nbd_dev, 100) {
                    demoted = false;
                    swapoff_attempted = false;
                    swapoff_confirmed = false;
                    recovery.reset();
                    eprintln!("[ramsharedd] RECOVERING -> available: swapon {nbd_dev} prio=100");
                } else {
                    recovery.reset();
                    eprintln!("[ramsharedd] RECOVERING: swapon {nbd_dev} falhou; parked");
                }
            }
        }
    }

    if let Some(rx) = demote_rx.take() {
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(true) => {
                swapoff_confirmed = true;
                eprintln!("[ramsharedd] teardown: swapoff {nbd_dev} confirmado (DEMOTE limpo)")
            }
            Ok(false) => eprintln!(
                "[ramsharedd] teardown: AVISO swapoff {nbd_dev} NAO confirmou (swap pode estar inconsistente)"
            ),
            Err(_) => eprintln!(
                "[ramsharedd] teardown: AVISO swapoff {nbd_dev} sem confirmacao em 5s (timeout/thread sumiu)"
            ),
        }
    }
    let mut used_kb = nbd_used_kb_from_proc(&nbd_dev);
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
            && let Ok(true) = spawn_swapoff(&nbd_dev).recv_timeout(Duration::from_secs(30))
        {
            swapoff_confirmed = true;
        }
        std::thread::sleep(Duration::from_secs(5));
        used_kb = nbd_used_kb_from_proc(&nbd_dev);
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
    let _ = std::fs::remove_file(path);
    Ok(())
}

/// `used_kb` for the given nbd device path from `/proc/swaps` (0 if absent).
fn nbd_used_kb_from_proc(nbd_dev: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
    let key = nbd_dev.trim();
    let bare = key.rsplit('/').next().unwrap_or(key);
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 {
            continue;
        }
        let name = cols[0];
        if (name.contains(bare) || name == key)
            && let Ok(u) = cols[3].parse::<u64>()
        {
            return u;
        }
    }
    0
}

/// Broker control-plane parts consumed by the worker (data-plane). Backend-agnostic
/// (applies to VRAM and RAM) — only backend and residency differ between modes.
struct BrokerRuntime {
    geom: Vec<(u64, u64)>,
    jobs_rx: std::sync::mpsc::Receiver<WMsg>,
    demote_tx: std::sync::mpsc::Sender<DemoteReason>,
    shutdown: std::sync::Arc<AtomicBool>,
    broker: std::thread::JoinHandle<()>,
    /// IO counters per slice (telemetry RF-1): worker increments, broker reads in `Status`.
    pub slice_io: std::sync::Arc<Vec<SliceIoCounters>>,
    /// VRAM Gauge (RF-3): the residency closure publishes free/total; broker reads on tick.
    pub vram: std::sync::Arc<VramGauge>,
}

/// Reconciliation tolerance (DT-7, provisional — calibrate at P0).
const RECON_TOL_FRAC: f64 = 0.10;
/// Consecutive ticks to confirm a reconciliation flag (hysteresis DT-12).
const RECON_STREAK: u32 = 3;

/// Starts the broker control-plane (independent of backend): slices map + geometry +
/// NBD exports ("s0".."sN"), acceptors (always Unix; TCP if `--listen-nbd`) feeding the
/// SAME `jobs` worker channel, the arbiter (`spawn_broker`, sharing `jobs` for the
/// hygiene `ZeroExport` DT-17 and consuming the DEMOTE channel) and the `SHUTDOWN` bridge
/// (signal handler only touches the async-signal-safe static variable → mirrored in `Arc`).
#[allow(clippy::too_many_arguments)] // setup do control-plane: geometria + rede + telemetria
fn broker_setup(
    slices: u16,
    slice_bytes: u64,
    sock: &str,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
    telemetry_jsonl: Option<std::path::PathBuf>,
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

    unsafe {
        signal(SIGINT, handle_shutdown);
        signal(SIGTERM, handle_shutdown);
    }
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    {
        let mirror = std::sync::Arc::clone(&shutdown);
        std::thread::spawn(move || {
            while !SHUTDOWN.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
            }
            mirror.store(true, Ordering::SeqCst);
        });
    }

    let tx_flags = NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_CAN_MULTI_CONN;
    let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel::<WMsg>(CHAN_CAP);
    let (demote_tx, demote_rx) = std::sync::mpsc::channel::<DemoteReason>();

    let path = Path::new(sock);
    let _ = std::fs::remove_file(path);
    let unix = UnixListener::bind(path)?;
    eprintln!("[ramsharedd] NBD unix em {sock}");
    let _ = spawn_acceptor(
        unix,
        std::sync::Arc::clone(&exports),
        tx_flags,
        jobs_tx.clone(),
    );
    if let Some(addr) = listen_nbd_addr {
        let tcp = std::net::TcpListener::bind(addr)?;
        eprintln!("[ramsharedd] NBD tcp em {addr}");
        let _ = ramshared_wsl2d::conn::spawn_acceptor_tcp(
            tcp,
            std::sync::Arc::clone(&exports),
            tx_flags,
            jobs_tx.clone(),
        );
    }

    let slice_io = std::sync::Arc::new(
        (0..slices)
            .map(|_| SliceIoCounters::default())
            .collect::<Vec<_>>(),
    );
    let vram = std::sync::Arc::new(VramGauge::default());
    let bcfg = BrokerConfig {
        listen: arbiter_addr,
        endpoints: EndpointCfg {
            nbd_unix: Some(sock.to_string()),
            nbd_tcp: advertise_tcp,
        },
        swap_prio: None,
        arbiter: ArbiterConfig::default(),
        tick: Duration::from_secs(2), // SPEC §/DT-24: tick=2s (streak=5 → 10s window)
        slice_io: std::sync::Arc::clone(&slice_io),
        vram: std::sync::Arc::clone(&vram),
        tol_frac: RECON_TOL_FRAC,
        recon_streak: RECON_STREAK,
        telemetry_jsonl,
    };
    let (broker, broker_addr) = spawn_broker(
        bcfg,
        slice_map,
        demote_rx,
        jobs_tx.clone(),
        std::sync::Arc::clone(&shutdown),
    )?;
    eprintln!("[ramsharedd] broker (árbitro) em {broker_addr}");
    drop(jobs_tx); // clones (acceptors + broker) keep the channel; worker owns the rx

    Ok(BrokerRuntime {
        geom,
        jobs_rx,
        demote_tx,
        shutdown,
        broker,
        slice_io,
        vram,
    })
}

/// Broker worker (data-plane), generic over the backend (VRAM or RAM): serves each `Job`
/// via [`SliceView`] of the export's geometry. DT-28: runs until `shutdown`, does NOT terminate when
/// NBD connections drop (the broker persists). Residency is injected via closure — VRAM passes the
/// canary §9/§9.4; RAM passes `|_| None` (RAM does not suffer WDDM eviction). On DEMOTE, notifies the
/// broker (`DemoteAll` to all tenants; the shared VRAM compromises ALL slices) and
/// stops sampling. Returns backend for teardown (safe wipe is the owner's responsibility).
fn serve_broker_jobs<B: BlockBackend>(
    mut backend: B,
    rt: &BrokerRuntime,
    mut residency: impl FnMut(u64) -> Option<DemoteReason>,
) -> B {
    let mut demoted = false;
    eprintln!("[ramsharedd] em transmissão (worker único; multi-slice/broker)");
    loop {
        let msg = match rt.jobs_rx.recv_timeout(Duration::from_millis(500)) {
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
    let total = (slices as u64)
        .checked_mul(slice_bytes)
        .ok_or("--slices * --slice-mb: overflow")?;

    // O `provider` (CUDA hoje; Vulkan amanhã) já vem pronto do shell em `run()`. Daqui pra baixo o
    // caminho é genérico sobre `VramProvider`/`VramMemory` (RF-G1).
    let (free, total_vram) = provider.mem_info()?;
    eprintln!(
        "[ramsharedd] VRAM livre={} MiB total={} MiB",
        free >> 20,
        total_vram >> 20
    );
    let mut mem = provider.alloc(total as usize)?;
    mem.zero()?;
    // CUDA/VRAM have already been allocated above -> safe to lock MCL_FUTURE all at once.
    lock_memory(force, true)?; // Discipline 3: locks memory BEFORE serving swap
    let backend = VramBackend::new(mem, BLOCK_SIZE);
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

    let rt = broker_setup(
        slices,
        slice_bytes,
        &sock,
        listen_nbd_addr,
        advertise_tcp,
        arbiter_addr,
        telemetry_jsonl,
    )?;
    let vram = std::sync::Arc::clone(&rt.vram);
    let mut backend = serve_broker_jobs(backend, &rt, |lat_us| {
        residency_check(
            lat_us,
            &mut canary,
            &mut baseline,
            &mut sampler,
            &mut cadence,
            &mut probe,
            ResidencyConfig::default().free_floor_bytes,
            || {
                let (f, t) = provider.mem_info().ok()?;
                // RF-3/DT-5: publishes the gauge for reconciliation (free/total in bytes).
                vram.free.store(f, Ordering::Relaxed);
                vram.total.store(t, Ordering::Relaxed);
                Some(f)
            },
        )
    });

    let _ = rt.broker.join();
    let zeroed = backend.zero();
    let _ = probe.zero(); // DT-12/DT-17: zeroes the canary-region as well
    let _ = std::fs::remove_file(Path::new(&sock));
    zeroed?;
    eprintln!("[ramsharedd] broker VRAM encerrado (VRAM zerada)");
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
    let total = (slices as u64)
        .checked_mul(slice_bytes)
        .ok_or("--slices * --slice-mb: overflow")?;
    let backend = RamBackend::new(total as usize);
    eprintln!(
        "[ramsharedd] broker RAM (sem GPU): {slices} slices x {} MiB = {} MiB, block_size={BLOCK_SIZE}",
        slice_bytes >> 20,
        total >> 20
    );

    let rt = broker_setup(
        slices,
        slice_bytes,
        &sock,
        listen_nbd_addr,
        advertise_tcp,
        arbiter_addr,
        telemetry_jsonl,
    )?;
    let _ = serve_broker_jobs(backend, &rt, |_| None); // RAM: without residency

    let _ = rt.broker.join();
    let _ = std::fs::remove_file(Path::new(&sock));
    eprintln!("[ramsharedd] broker RAM encerrado");
    Ok(())
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
    // DT-11: ublk+Vulkan not yet implemented. The ublk residency server
    // (spawn_server_dt3_vram_with_residency) is CUDA-fixed, not generic over
    // VramProvider; generifying it and refactoring is a separate task (only validatable on native host).
    // Fails early, before any side-effects (add_device/mlockall).
    if let BackendKind::Vulkan = backend {
        return Err("ublk with --backend vulkan not yet supported (DT-11: ublk \
             residency server is CUDA-fixed). Use --backend vram (CUDA), or Vulkan \
             via --slices (broker) / --transport nbd."
            .into());
    }
    // SAFETY LOCK: refuses to serve ublk on WSL2. An unsuccessful daemon teardown
    // orphans `/dev/ublkbN` -> I/O in D-state -> FREEZES WSL2 (2026-06-09).
    guard_not_wsl2()?;
    // Discipline 3: locks memory + protects from OOM (entire process; worker owns
    // CUDA, but mlockall/oom_score_adj are process-wide). Only MCL_CURRENT here —
    // MCL_FUTURE is armed later, in arm_future_lock, see comment there.
    lock_memory(force, false)?;

    // SAFETY: registers async-signal-safe handler (just an atomic store) to exit
    // in an orderly fashion. signal() only stores the pointer; the old return is ignored.
    unsafe {
        signal(SIGINT, handle_shutdown);
        signal(SIGTERM, handle_shutdown);
    }

    let dev_sectors = size / SECTOR;
    let mut spec = ublk_control::DeviceSpec::smoke_auto();
    spec.queue_depth = queue_depth;
    let report = ublk_control::add_device(UBLK_CONTROL, spec)?;
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 12, 12),
    )?;

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);

    // Backend: VRAM (worker owning the CUDA context + residency §9/§9.4; swap_dev = the
    // device itself, which DEMOTE removes from swap) or RAM (without GPU; reuses the
    // spawn_server_dt3 already validated in-process, without residency — to validate the lifecycle/teardown
    // in QEMU).
    let server = match backend {
        BackendKind::Vram => UblkHandle::Vram(ublk_server::spawn_server_dt3_vram_with_residency(
            &char_path,
            report.queue_depth,
            BLOCK_SIZE as usize, // buf_size per tag: device requests are <= 4KB (smoke_auto)
            size as usize,
            BLOCK_SIZE,
            block_path.clone(),
            ResidencyConfig::default(),
        )?),
        BackendKind::Ram => UblkHandle::Ram(ublk_server::spawn_server_dt3(
            &char_path,
            report.queue_depth,
            BLOCK_SIZE as usize,
            RamBackend::new(size as usize),
        )?),
        // Unreachable: barred at the beginning of run_ublk (DT-11). Defensive, without panic.
        BackendKind::Vulkan => {
            return Err("ublk with --backend vulkan not supported (DT-11)".into());
        }
    };
    // dxgkrnl ANTI-BUG (incident 2026-07-03): We do NOT arm MCL_FUTURE in the
    // ublk+vram path. dxgkrnl's `dxg_map_iospace` maps VRAM in 2 steps (anonymous vm_mmap
    // -> io_remap_pfn_range over it); with MCL_FUTURE active, step 1
    // pre-populates the VMA and step 2 hits BUG_ON(!pte_none) -> kernel BUG with lock
    // held -> host hangs. And `spawn_server_dt3_vram_with_residency` runs
    // Cuda::load()/create_context() (= the dxg_map_iospace calls) in a THREAD that runs
    // ASYNCHRONOUS to this point — it is not possible to "arm MCL_FUTURE after CUDA" with
    // safety (there would be a race: main's MCL_FUTURE could fall in the middle of
    // the worker's create_context). Therefore we stick only to MCL_CURRENT (lock_memory
    // above, lock_future=false), which does NOT affect future mmaps -> zero collision with
    // dxgkrnl. Confirmed by reading mm/memory.c + Layers A/B.
    //
    // TRADE-OFF (Discipline 3 / anti-deadlock RNF-1): I/O buffers allocated AFTER
    // this point are not locked by MCL_FUTURE. Under extreme memory pressure,
    // this theoretically reopens the D-state vector. Correct future mitigation: explicit
    // mlock() only on the residency/staging buffers (uring/canary), instead of the
    // blind MCL_FUTURE. Until then: run only supervised / low swap priority.
    // SPEC: docs/reliability/BLACK-BOX-FORENSICS.md.
    eprintln!(
        "[ramsharedd] mlockall: MCL_CURRENT-only no caminho ublk+vram (anti-dxgkrnl-BUG; MCL_FUTURE desarmado de proposito)"
    );
    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())?;
    eprintln!(
        "[ramsharedd] ublk device: {block_path} ({} MiB, qd={}, backend={})",
        size >> 20,
        report.queue_depth,
        backend.label()
    );
    eprintln!("[ramsharedd] swapon: sudo swapon {block_path}");
    eprintln!("[ramsharedd] Ctrl-C / SIGTERM to exit");

    // Waits for the shutdown signal (polling the flag; sleep ignores EINTR).
    while !SHUTDOWN.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!("[ramsharedd] signal received — exiting");

    // Orderly teardown: STOP_DEV aborts FETCHes -> worker exits loop (and zeroes
    // VRAM at the end, in VRAM path) -> join -> DEL_DEV. (Whoever did swapon must swapoff
    // before: del_gendisk waits for the openers of the block device.)
    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id)?;
    server.join()?;
    ublk_control::delete_device(UBLK_CONTROL, report.dev_id)?;
    eprintln!("[ramsharedd] ublk device removido");
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
    if std::env::var("RAMSHARED_ALLOW_UBLK_ON_WSL2")
        .ok()
        .as_deref()
        == Some("1")
    {
        eprintln!("[ramsharedd] WARNING: RAMSHARED_ALLOW_UBLK_ON_WSL2=1 — WSL2 lock ignored");
        return Ok(());
    }
    let osrelease = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let lower = osrelease.to_ascii_lowercase();
    if lower.contains("microsoft") || lower.contains("wsl") {
        return Err(format!(
            "RECUSADO: --transport ublk no WSL2 ({}) pode CONGELAR o sistema se o teardown \
             do daemon falhar (device orfao -> I/O em D-state). Valide o daemon em VM/qemu. \
             Override consciente: RAMSHARED_ALLOW_UBLK_ON_WSL2=1.",
            osrelease.trim()
        )
        .into());
    }
    Ok(())
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
    if !locked && !force {
        return Err("mlockall failed; run as root or use --force".into());
    }
    let oom_ok = std::fs::write("/proc/self/oom_score_adj", "-1000").is_ok();
    if !oom_ok && !force {
        return Err("could not set oom_score_adj=-1000; run as root or use --force".into());
    }
    if locked && oom_ok {
        eprintln!("[ramsharedd] memory locked (mlockall) + oom_score_adj=-1000");
    } else {
        eprintln!(
            "[ramsharedd] WARNING --force: mlockall={} oom_score_adj={} (anti-deadlock NOT guaranteed)",
            if locked { "ok" } else { "FAILED" },
            if oom_ok { "ok" } else { "FAILED" }
        );
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
        assert!(should_log_probe_sample(Some(true), Some(128), 512, 1));
        assert!(should_log_probe_sample(None, Some(1024), 512, 0));
        assert!(should_log_probe_sample(Some(true), None, 512, 0));
        assert!(!should_log_probe_sample(Some(true), Some(2048), 512, 0));
    }
}
