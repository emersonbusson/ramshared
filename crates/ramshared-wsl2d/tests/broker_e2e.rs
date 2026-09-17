//! e2e in-process broker (ITEM-10): real `spawn_broker` + fake agents over TCP (loopback) +
//! a worker draining the `jobs` channel. Validates **IO wiring** (acceptor → reader → core → tick
//! → dispatch → writer → socket); decision logic is already covered by pure tests of
//! `BrokerCore`. **Everything in-process** (threads + loopback), no standalone daemon — safe on
//! WSL2 (session rule). Run with `timeout` (deadlock becomes a test-that-blows-up, not a freeze).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::BufReader;
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use ramshared_broker::arbiter::ArbiterConfig;
use ramshared_broker::model::TransportKind;
use ramshared_broker::protocol::{Msg, PROTO_VERSION, read_msg, write_msg};
use ramshared_broker::slices::SliceMap;
use ramshared_wsl2d::DemoteReason;
use ramshared_wsl2d::WMsg;
use ramshared_wsl2d::broker_srv::{BrokerConfig, EndpointCfg, spawn_broker};
use ramshared_wsl2d::{SliceIoCounters, VramGauge};

const SLICE: u64 = 64 * 1024 * 1024;

/// Starts the broker + draining worker; drops everything on `Drop`.
struct Harness {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    core: Option<JoinHandle<()>>,
    _demote_tx: mpsc::Sender<DemoteReason>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(h) = self.core.take() {
            let _ = h.join();
        }
    }
}

fn setup(k: u16, tick_ms: u64) -> Harness {
    setup_with_lease_ttl(k, tick_ms, Duration::from_secs(30))
}

fn setup_with_lease_ttl(k: u16, tick_ms: u64, lease_ttl: Duration) -> Harness {
    let (demote_tx, demote_rx) = mpsc::channel::<DemoteReason>();
    let (jobs_tx, jobs_rx) = mpsc::sync_channel::<WMsg>(64);
    let shutdown = Arc::new(AtomicBool::new(false));
    let cfg = BrokerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        endpoints: EndpointCfg {
            nbd_unix: Some("/tmp/e2e.sock".into()),
            nbd_tcp: Some(("127.0.0.1".into(), 10809)), // agentes do e2e usam NbdTcp (DT-25)
        },
        swap_prio: None,
        arbiter: ArbiterConfig::default(),
        tick: Duration::from_millis(tick_ms),
        slice_io: Arc::new((0..k).map(|_| SliceIoCounters::default()).collect()),
        vram: Arc::new(VramGauge::default()),
        tol_frac: 0.10,
        recon_streak: 1,
        lease_ttl,
        telemetry_jsonl: None,
    };
    let (core, addr) = spawn_broker(
        cfg,
        SliceMap::new(k, SLICE, u64::from(k) * SLICE).expect("valid sizes"),
        demote_rx,
        jobs_tx,
        Arc::clone(&shutdown),
    )
    .unwrap();
    // Worker: drains the jobs channel; confirms ZeroExport (hygiene) with done(true).
    std::thread::spawn(move || {
        for m in jobs_rx.iter() {
            if let WMsg::ZeroExport { done, .. } = m {
                let _ = done.send(true);
            }
        }
    });
    Harness {
        addr,
        shutdown,
        core: Some(core),
        _demote_tx: demote_tx,
    }
}

fn connect(addr: SocketAddr) -> (TcpStream, BufReader<TcpStream>) {
    let s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let r = BufReader::new(s.try_clone().unwrap());
    (s, r)
}

fn register(s: &mut TcpStream, name: &str) {
    register_with_transport(s, name, TransportKind::NbdTcp);
}

fn register_with_transport(s: &mut TcpStream, name: &str, transport: TransportKind) {
    write_msg(
        s,
        &Msg::Register {
            proto: PROTO_VERSION,
            tenant: name.into(),
            transport,
        },
    )
    .unwrap();
}

fn request_status(
    s: &mut TcpStream,
    r: &mut BufReader<TcpStream>,
) -> Vec<ramshared_broker::model::Slice> {
    write_msg(s, &Msg::Status).unwrap();
    match read_until(r, |message| matches!(message, Msg::StatusReply { .. })) {
        Some(Msg::StatusReply { slices, .. }) => slices,
        _ => panic!("broker did not return a status reply"),
    }
}

/// Reads messages until `pred` matches (or timeout/EOF → None).
fn read_until(r: &mut BufReader<TcpStream>, pred: impl Fn(&Msg) -> bool) -> Option<Msg> {
    for _ in 0..30 {
        match read_msg(r) {
            Ok(Some(m)) => {
                if pred(&m) {
                    return Some(m);
                }
            }
            _ => return None,
        }
    }
    None
}

#[test]
fn e2e_register_and_ack() {
    let h = setup(2, 30);
    let (mut s, mut r) = connect(h.addr);
    register(&mut s, "a");
    assert!(
        matches!(
            read_until(&mut r, |m| matches!(m, Msg::Registered { .. })),
            Some(Msg::Registered { .. })
        ),
        "should receive Registered"
    );
    write_msg(
        &mut s,
        &Msg::Psi {
            sample: Default::default(),
            swaps: vec![],
            mem: None,
        },
    )
    .unwrap();
    assert!(
        matches!(
            read_until(&mut r, |m| matches!(m, Msg::Ack)),
            Some(Msg::Ack)
        ),
        "Psi must receive Ack (heartbeat DT-18)"
    );
}

#[test]
fn e2e_tick_assigns_swapon_over_socket() {
    // 1 slice, 1 agent → tick (round-robin) assigns s0 and sends SwapOn via socket.
    let h = setup(1, 30);
    let (mut s, mut r) = connect(h.addr);
    register(&mut s, "solo");
    let m = read_until(&mut r, |m| matches!(m, Msg::SwapOn { .. }));
    assert!(
        matches!(m, Some(Msg::SwapOn { slice: 0, .. })),
        "tick must assign s0 and send SwapOn (complete IO wiring)"
    );
}

#[test]
fn e2e_disconnected_lease_expires_without_reuse_before_deadline() {
    let h = setup_with_lease_ttl(1, 10, Duration::from_millis(400));
    let (mut holder, mut holder_reader) = connect(h.addr);
    register_with_transport(&mut holder, "holder", TransportKind::DccAgent);
    assert!(matches!(
        read_until(&mut holder_reader, |message| matches!(
            message,
            Msg::Registered { .. }
        )),
        Some(Msg::Registered { .. })
    ));
    write_msg(&mut holder, &Msg::LeaseRequest { bytes: SLICE }).unwrap();
    assert!(matches!(
        read_until(&mut holder_reader, |message| matches!(
            message,
            Msg::LeaseGranted { .. }
        )),
        Some(Msg::LeaseGranted { .. })
    ));

    let (mut observer, mut observer_reader) = connect(h.addr);
    register_with_transport(&mut observer, "observer", TransportKind::DccAgent);
    assert!(matches!(
        read_until(&mut observer_reader, |message| matches!(
            message,
            Msg::Registered { .. }
        )),
        Some(Msg::Registered { .. })
    ));
    drop(holder_reader);
    drop(holder);

    std::thread::sleep(Duration::from_millis(100));
    let before = request_status(&mut observer, &mut observer_reader);
    assert!(matches!(
        before[0].state,
        ramshared_broker::model::SliceState::Leased
    ));

    std::thread::sleep(Duration::from_millis(500));
    let after = request_status(&mut observer, &mut observer_reader);
    assert!(matches!(
        after[0].state,
        ramshared_broker::model::SliceState::Free
    ));
}

#[test]
fn e2e_duplicate_register_closes_second() {
    let h = setup(2, 30);
    let (mut a, mut ar) = connect(h.addr);
    register(&mut a, "dup");
    read_until(&mut ar, |m| matches!(m, Msg::Registered { .. }));
    let (mut b, mut br) = connect(h.addr);
    register(&mut b, "dup"); // same name, another connection → duplicate
    let m = read_until(&mut br, |m| matches!(m, Msg::Error { .. }));
    assert!(
        matches!(m, Some(Msg::Error { .. })),
        "duplicate registration must receive Error and the session closes (CloseSession)"
    );
}

#[test]
fn e2e_psi_flood_does_not_starve_arbiter_tick() {
    // Regression (bug caught in cross-host multi-tenant e2e): under high-rate `Psi` (>> tick), the Arbiter's
    // Tick MUST NOT be starved — otherwise `AssignFree` never runs and the tenant never receives
    // `SwapOn`. The `core_loop` fires the Tick by wall-clock deadline, not just on recv
    // timeout. Here the agent floods `Psi` every ~5ms (tick=50ms) and should still receive SwapOn.
    let h = setup(1, 50);
    let (mut s, mut r) = connect(h.addr);
    register(&mut s, "flood");
    let stop = Arc::new(AtomicBool::new(false));
    let mut writer = s.try_clone().unwrap();
    let stop_w = Arc::clone(&stop);
    let flood = std::thread::spawn(move || {
        while !stop_w.load(Ordering::SeqCst) {
            if write_msg(
                &mut writer,
                &Msg::Psi {
                    sample: Default::default(),
                    swaps: vec![],
                    mem: None,
                },
            )
            .is_err()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let got = read_until(&mut r, |m| matches!(m, Msg::SwapOn { .. }));
    stop.store(true, Ordering::SeqCst);
    let _ = flood.join();
    assert!(
        matches!(got, Some(Msg::SwapOn { slice: 0, .. })),
        "arbiter must assign s0 even under Psi flood (Tick cannot be starved)"
    );
}
