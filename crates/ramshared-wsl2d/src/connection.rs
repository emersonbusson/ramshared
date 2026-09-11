//! Network server connections for the broker, managing socket acceptors,
//! session state machines, and I/O tasks.
//!
//! SPEC: docs/specs/no-milestone/memory-broker/SPEC.md ITEM-8.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ramshared_broker::arbiter::ArbiterConfig;
use ramshared_broker::protocol::{Msg, read_msg, write_msg};
use ramshared_broker::slices::SliceMap;

use crate::broker_srv::{BrokerCore, BrokerCoreConfig, CoreEvent, EndpointCfg, Outbound};
use crate::conn::WMsg;
use crate::residency::DemoteReason;
use crate::telemetry::{SliceIoCounters, TelemetryCore, VramGauge};

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
    // message rate. (Bug caught in e2e cross-host civm; the QEMU drill passed by luck of timing.)
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
pub struct TelemetrySink {
    file: File,
    branch: Option<String>,
    commit: Option<String>,
}

impl TelemetrySink {
    pub fn open(path: PathBuf) -> Option<Self> {
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

    pub fn emit(&mut self, core: &TelemetryCore) -> std::io::Result<()> {
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


#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_broker_config_creation() {
        let cfg = BrokerConfig {
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            endpoints: EndpointCfg::default(),
            swap_prio: None,
            arbiter: ArbiterConfig::default(),
            tick: Duration::from_secs(1),
            slice_io: Arc::new(Vec::new()),
            vram: Arc::new(VramGauge::default()),
            tol_frac: 0.1,
            recon_streak: 3,
            telemetry_jsonl: None,
        };
        assert_eq!(cfg.recon_streak, 3);
    }
}
