//! `broker_srv` — **pure** core of the broker (decision/state), testable without threads/sockets/GPU.
//!
//! Same discipline as the arbiter: [`BrokerCore`] receives [`CoreEvent`]s and returns [`Outbound`]s
//! that the IO layer ([`spawn_broker`]) executes — send `Msg` to the session, close session, request
//! the worker to zero a slice (DT-17), or log (RF-B4).
//!
//! SPEC: docs/specs/no-milestone/memory-broker/SPEC.md ITEM-8. Covers: sessions + Register/Psi/Ack/Status/Disconnect
//! (DT-18/20/22), reconciliation (DT-9/21), rebalancing with hygiene (DT-17), DemoteAll and **revocable
//! lease (RF-B3/DT-19)**. The IO layer ([`spawn_broker`]) runs the core over TCP (DT-2/24);
//! only the wiring in the daemon's `run_nbd` is missing (`--slices`/`--arbiter-listen`).

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ramshared_broker::arbiter::{Arbiter, ArbiterConfig};
use ramshared_broker::lease::LeaseBook;
use ramshared_broker::model::{PsiSample, SliceId, TenantId, TransportKind};
pub mod dispatcher;

pub use crate::broker_srv::dispatcher::{CoreEvent, Outbound, EndpointCfg};
use ramshared_broker::protocol::{
    Msg, TenantMem, read_msg,
    write_msg,
};
use ramshared_broker::slices::SliceMap;

use crate::conn::WMsg;
use crate::residency::DemoteReason;
use crate::telemetry::{
    ReconcileFlag, SliceIoCounters, TelemetryCore, VramGauge,
};
#[derive(Clone, Debug)]
pub(crate) struct TenantState {
    pub(crate) name: String,
    pub(crate) transport: TransportKind,
    pub(crate) present: bool,
    pub(crate) sid: Option<usize>,
    pub(crate) psi: PsiSample,
    pub(crate) reconciled: bool,
    /// Last memory telemetry of the tenant (RF-2); `None` if the agent does not report cgroup/diskstats.
    pub(crate) mem: Option<TenantMem>,
    /// Occupied swap (bytes) in the `Active` slices of this tenant, derived in `Psi` (DT-10/F-v2-2).
    pub(crate) occupied_bytes: u64,
}

/// Named configuration for the pure broker core.
pub struct BrokerCoreConfig {
    pub arbiter_cfg: ArbiterConfig,
    pub endpoints: EndpointCfg,
    pub swap_prio: Option<i32>,
    pub slice_io: Arc<Vec<SliceIoCounters>>,
    pub vram: Arc<VramGauge>,
    pub tol_frac: f64,
    pub recon_streak: u32,
}

/// Broker core: sole owner of `SliceMap` + `Arbiter` + session table (without locks; the
/// IO layer runs this in a single thread). `BTreeMap` by `TenantId` gives stable iteration
/// (deterministic round-robin).
pub struct BrokerCore {
    pub(crate) slice_map: SliceMap,
    pub(crate) arbiter: Arbiter,
    pub(crate) endpoints: EndpointCfg,
    pub(crate) swap_prio: Option<i32>,
    pub(crate) tenants: BTreeMap<TenantId, TenantState>,
    pub(crate) sessions: HashMap<usize, TenantId>, // live connection → tenant
    pub(crate) name_to_id: HashMap<String, TenantId>, // stable ID by name (DT-22)
    pub(crate) next_tenant: TenantId,
    pub(crate) pending_dest: HashMap<SliceId, TenantId>, // destination of a MoveSlice in flight (post-zero)
    pub(crate) lease_book: LeaseBook,
    pub(crate) last_rebalance: Option<Instant>,
    // Telemetria/reconciliação (SPEC broker-telemetry-reconciliation).
    pub(crate) slice_io: Arc<Vec<SliceIoCounters>>, // counters per slice (data-plane writes, RF-1/DT-1)
    pub(crate) vram: Arc<VramGauge>,                // VRAM gauge published by the worker (RF-3/DT-5)
    pub(crate) demotes_total: u64,                  // accumulated canary DEMOTEs (RF-4)
    pub(crate) last_demote_reason: Option<String>,
    pub(crate) demotes_at_last_sample: u64, // baseline for the `demotes_delta` per tick
    pub(crate) recon_flag: ReconcileFlag,   // candidate flag (hysteresis DT-12)
    pub(crate) recon_count: u32,            // consecutive ticks with the same candidate flag
    pub(crate) tol_frac: f64,               // reconciliation tolerance (DT-7)
    pub(crate) recon_streak: u32,           // ticks to confirm the flag (DT-12)
    // R4: slices post-`SwapOffDone` waiting for zero confirmation (`ZeroDone`), with number of ticks
    // without confirmation. If `try_send(ZeroExport)` failed (channel full), `ZeroDone` does not arrive → the tick
    // re-emits zero (retry) until confirmed; escalates to ERROR after N. Only slices HERE have already been
    // swapped-off by the tenant (safe to re-zero); slices in Draining awaiting SwapOffDone are NOT included.
    pub(crate) pending_zero: HashMap<SliceId, u32>,
}
impl BrokerCore {
    pub fn new(slice_map: SliceMap, cfg: BrokerCoreConfig) -> Self {
        let lease_capacity = slice_map.total_bytes();
        Self {
            slice_map,
            arbiter: Arbiter::new(cfg.arbiter_cfg),
            endpoints: cfg.endpoints,
            swap_prio: cfg.swap_prio,
            tenants: BTreeMap::new(),
            sessions: HashMap::new(),
            name_to_id: HashMap::new(),
            next_tenant: 1,
            pending_dest: HashMap::new(),
            lease_book: LeaseBook::new(lease_capacity),
            last_rebalance: None,
            slice_io: cfg.slice_io,
            vram: cfg.vram,
            demotes_total: 0,
            last_demote_reason: None,
            demotes_at_last_sample: 0,
            recon_flag: ReconcileFlag::None,
            recon_count: 0,
            tol_frac: cfg.tol_frac,
            recon_streak: cfg.recon_streak,
            pending_zero: HashMap::new(),
        }
    }

}

// ===================== IO Layer (DT-2/DT-24) =====================
//
// `BrokerCore` is pure; `spawn_broker` is the thin shell of threads running it: TCP acceptor,
// reader/writer per session (writer = bounded channel 64, DT-24), the core loop (`recv_timeout`
// = tick) and DEMOTE and zero-done forwarders. The core never does socket IO.

/// Broker config (DT-2: in-process in the daemon; RNF-2: `listen` already validated as non-unspecified).
pub struct BrokerConfig {
    pub listen: SocketAddr,
    pub endpoints: EndpointCfg,
    pub swap_prio: Option<i32>,
    pub arbiter: ArbiterConfig,
    pub tick: Duration,
    /// Telemetry (SPEC): counters per slice + VRAM gauge (shared with the worker).
    pub slice_io: Arc<Vec<SliceIoCounters>>,
    pub vram: Arc<VramGauge>,
    pub tol_frac: f64,
    pub recon_streak: u32,
    /// Destino do JSONL de telemetria (RF-5); `None` = telemetria silenciosa.
    pub telemetry_jsonl: Option<PathBuf>,
}

/// Evento interno do IO (multiplexa registro de sessão e eventos do core).
enum IoEvent {
    NewSession(usize, SyncSender<Msg>),
    Core(CoreEvent),
}

/// Starts the broker: acceptor + sessions + single-thread core. Returns the core handle and the
/// bound `SocketAddr` (useful with port 0 in tests). `shutdown` triggers `DemoteAll` + exit.
pub fn spawn_broker(
    cfg: BrokerConfig,
    slice_map: SliceMap,
    demote_rx: Receiver<DemoteReason>,
    jobs: SyncSender<WMsg>,
    shutdown: Arc<AtomicBool>,
) -> io::Result<(JoinHandle<()>, SocketAddr)> {
    let listener = TcpListener::bind(cfg.listen)?;
    let addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let (io_tx, io_rx) = mpsc::channel::<IoEvent>();

    // Acceptor.
    {
        let io_tx = io_tx.clone();
        let shutdown = Arc::clone(&shutdown);
        thread::spawn(move || acceptor_loop(&listener, &io_tx, &shutdown));
    }
    // DEMOTE forwarder (canary/residency) → CoreEvent::Demote.
    {
        let io_tx = io_tx.clone();
        thread::spawn(move || {
            for reason in demote_rx.iter() {
                if io_tx
                    .send(IoEvent::Core(CoreEvent::Demote(format!("{reason:?}"))))
                    .is_err()
                {
                    break;
                }
            }
        });
    }
    // Core (single thread owner of BrokerCore). Keeps an `io_tx` for the zero-done forwarders.
    let core = BrokerCore::new(
        slice_map,
        BrokerCoreConfig {
            arbiter_cfg: cfg.arbiter,
            endpoints: cfg.endpoints,
            swap_prio: cfg.swap_prio,
            slice_io: cfg.slice_io,
            vram: cfg.vram,
            tol_frac: cfg.tol_frac,
            recon_streak: cfg.recon_streak,
        },
    );
    let tick = cfg.tick;
    let sink = cfg.telemetry_jsonl.and_then(TelemetrySink::open);
    let handle =
        thread::spawn(move || core_loop(core, &io_rx, &io_tx, &jobs, tick, &shutdown, sink));
    Ok((handle, addr))
}

fn acceptor_loop(listener: &TcpListener, io_tx: &Sender<IoEvent>, shutdown: &AtomicBool) {
    let mut next_sid = 0usize;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nodelay(true);
                let Ok(wsock) = stream.try_clone() else {
                    continue;
                };
                let sid = next_sid;
                next_sid += 1;
                let (wtx, wrx) = mpsc::sync_channel::<Msg>(64); // DT-24: bounded
                thread::spawn(move || session_writer(wsock, &wrx));
                if io_tx.send(IoEvent::NewSession(sid, wtx)).is_err() {
                    break; // core encerrou
                }
                let io2 = io_tx.clone();
                thread::spawn(move || session_reader(stream, sid, &io2));
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn session_writer(mut sock: TcpStream, rx: &Receiver<Msg>) {
    for msg in rx.iter() {
        if write_msg(&mut sock, &msg).is_err() {
            break;
        }
    }
    // closed channel (CloseSession/backpressure) or error → closes the socket (the reader sees EOF).
    let _ = sock.shutdown(Shutdown::Both);
}

fn session_reader(sock: TcpStream, sid: usize, io_tx: &Sender<IoEvent>) {
    let mut r = BufReader::new(sock);
    while let Ok(Some(m)) = read_msg(&mut r) {
        if io_tx.send(IoEvent::Core(CoreEvent::Msg(sid, m))).is_err() {
            return;
        }
    }
    let _ = io_tx.send(IoEvent::Core(CoreEvent::Disconnected(sid)));
}

#[allow(clippy::too_many_arguments)] // casca de IO do core: canais + tick + shutdown + sink
fn core_loop(
    mut core: BrokerCore,
    io_rx: &Receiver<IoEvent>,
    io_tx: &Sender<IoEvent>,
    jobs: &SyncSender<WMsg>,
    tick: Duration,
    shutdown: &AtomicBool,
    mut sink: Option<TelemetrySink>,
) {
    let mut sessions: HashMap<usize, SyncSender<Msg>> = HashMap::new();
    // Wall-clock deadline for the next Tick. CRITICAL: the Arbiter's Tick MUST NOT be starved
    // by messages. Pure `recv_timeout(tick)` never expires under normal `Psi` flow
    // (~1/s per tenant) → the arbiter would never run `AssignFree`/rebalance. Here the wait shrinks
    // as messages arrive, and the Tick fires when the deadline passes, regardless of
    // message rate. (Bug caught in e2e cross-host multi-tenant; the QEMU drill passed by luck of timing.)
    let mut next_tick = Instant::now() + tick;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            let outs = core.handle(CoreEvent::Demote("shutdown".into()), Instant::now());
            dispatch(outs, &mut sessions, jobs, io_tx, &mut sink);
            break;
        }
        let wait = next_tick.saturating_duration_since(Instant::now());
        match io_rx.recv_timeout(wait) {
            Ok(IoEvent::NewSession(sid, wtx)) => {
                sessions.insert(sid, wtx);
            }
            Ok(IoEvent::Core(ev)) => {
                let outs = core.handle(ev, Instant::now());
                dispatch(outs, &mut sessions, jobs, io_tx, &mut sink);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if Instant::now() >= next_tick {
            let outs = core.handle(CoreEvent::Tick, Instant::now());
            dispatch(outs, &mut sessions, jobs, io_tx, &mut sink);
            next_tick = Instant::now() + tick;
        }
    }
}

fn dispatch(
    outs: Vec<Outbound>,
    sessions: &mut HashMap<usize, SyncSender<Msg>>,
    jobs: &SyncSender<WMsg>,
    io_tx: &Sender<IoEvent>,
    sink: &mut Option<TelemetrySink>,
) {
    for o in outs {
        match o {
            Outbound::ToSession(sid, msg) => {
                // DT-24: try_send; channel full/dead → drops the session (without blocking the core).
                let dead = match sessions.get(&sid) {
                    Some(wtx) => wtx.try_send(msg).is_err(),
                    None => false,
                };
                if dead {
                    sessions.remove(&sid);
                }
            }
            Outbound::CloseSession(sid) => {
                sessions.remove(&sid);
            }
            Outbound::ZeroSlice { slice, base, len } => {
                let (dtx, drx) = mpsc::channel::<bool>();
                if jobs
                    .try_send(WMsg::ZeroExport {
                        base,
                        len,
                        done: dtx,
                    })
                    .is_ok()
                {
                    let io2 = io_tx.clone();
                    thread::spawn(move || {
                        let ok = drx.recv().unwrap_or(false);
                        let _ = io2.send(IoEvent::Core(CoreEvent::ZeroDone(slice, ok)));
                    });
                } else {
                    eprintln!(
                        "[ramsharedd] WARN jobs channel full; zeroing of s{slice} postponed (R4)"
                    );
                }
            }
            Outbound::Log(s) => eprintln!("{s}"),
            Outbound::Telemetry(core) => {
                if let Some(s) = sink.as_mut()
                    && let Err(error) = s.emit(&core)
                {
                    eprintln!("[ramsharedd] WARN telemetry disabled after write failure: {error}");
                    *sink = None;
                }
            }
        }
    }
}

/// JSONL telemetry sink (RF-5/DT-8): timestamps `t`/`branch`/`commit` on the core's `TelemetryCore` and
/// appends 1 JSON object per line. `branch`/`commit` come from env
/// (`RAMSHARED_BUILD_BRANCH`/`RAMSHARED_BUILD_COMMIT`; `None` if absent — F-v2-4).
pub(crate) struct TelemetrySink {
    file: File,
    branch: Option<String>,
    commit: Option<String>,
}

impl TelemetrySink {
    pub(crate) fn open(path: PathBuf) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| eprintln!("[ramsharedd] WARN telemetria off: {path:?}: {e}"))
            .ok()?;
        Some(Self {
            file,
            branch: std::env::var("RAMSHARED_BUILD_BRANCH").ok(),
            commit: std::env::var("RAMSHARED_BUILD_COMMIT").ok(),
        })
    }

    pub(crate) fn emit(&mut self, core: &TelemetryCore) -> std::io::Result<()> {
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let sample = crate::telemetry::TelemetrySample {
            t,
            branch: self.branch.clone(),
            commit: self.commit.clone(),
            core: core.clone(),
        };
        let mut line = serde_json::to_string(&sample)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        line.push('\n');
        self.file.write_all(line.as_bytes())
    }
}
