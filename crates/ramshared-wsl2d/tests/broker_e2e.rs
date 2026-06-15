//! e2e in-process do broker (ITEM-10): `spawn_broker` real + agentes falsos por TCP (loopback) +
//! um worker drenador do canal `jobs`. Valida a **fiação de IO** (acceptor → reader → core → tick
//! → dispatch → writer → socket); a lógica de decisão já é coberta pelos testes puros do
//! `BrokerCore`. **Tudo in-process** (threads + loopback), nenhum daemon standalone — seguro no
//! WSL2 (regra de sessão). Rodar com `timeout` (deadlock vira teste-que-estoura, não freeze).
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

const SLICE: u64 = 64 * 1024 * 1024;

/// Sobe o broker + worker drenador; derruba tudo no `Drop`.
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
    };
    let (core, addr) = spawn_broker(
        cfg,
        SliceMap::new(k, SLICE),
        demote_rx,
        jobs_tx,
        Arc::clone(&shutdown),
    )
    .unwrap();
    // Worker: drena o canal jobs; confirma os ZeroExport (higiene) com done(true).
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
    write_msg(
        s,
        &Msg::Register {
            proto: PROTO_VERSION,
            tenant: name.into(),
            transport: TransportKind::NbdTcp,
        },
    )
    .unwrap();
}

/// Lê mensagens até `pred` casar (ou timeout/EOF → None).
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
        "deve receber Registered"
    );
    write_msg(
        &mut s,
        &Msg::Psi {
            sample: Default::default(),
            swaps: vec![],
        },
    )
    .unwrap();
    assert!(
        matches!(
            read_until(&mut r, |m| matches!(m, Msg::Ack)),
            Some(Msg::Ack)
        ),
        "Psi deve receber Ack (heartbeat DT-18)"
    );
}

#[test]
fn e2e_tick_assigns_swapon_over_socket() {
    // 1 slice, 1 agente → o tick (round-robin) assina s0 e envia SwapOn pelo socket.
    let h = setup(1, 30);
    let (mut s, mut r) = connect(h.addr);
    register(&mut s, "solo");
    let m = read_until(&mut r, |m| matches!(m, Msg::SwapOn { .. }));
    assert!(
        matches!(m, Some(Msg::SwapOn { slice: 0, .. })),
        "tick deve assinar s0 e enviar SwapOn (fiação IO completa)"
    );
}

#[test]
fn e2e_duplicate_register_closes_second() {
    let h = setup(2, 30);
    let (mut a, mut ar) = connect(h.addr);
    register(&mut a, "dup");
    read_until(&mut ar, |m| matches!(m, Msg::Registered { .. }));
    let (mut b, mut br) = connect(h.addr);
    register(&mut b, "dup"); // mesmo nome, outra conexão → duplicado
    let m = read_until(&mut br, |m| matches!(m, Msg::Error { .. }));
    assert!(
        matches!(m, Some(Msg::Error { .. })),
        "registro duplicado deve receber Error e a sessão fecha (CloseSession)"
    );
}

#[test]
fn e2e_psi_flood_does_not_starve_arbiter_tick() {
    // Regressão (bug pego no e2e cross-host civm): sob `Psi` em taxa alta (>> tick), o Tick do
    // árbitro NÃO pode ser starvado — senão `AssignFree` nunca roda e o tenant nunca recebe
    // `SwapOn`. O `core_loop` dispara o Tick por deadline de wall-clock, não só no timeout do
    // recv. Aqui o agente floda `Psi` a cada ~5ms (tick=50ms) e ainda assim deve receber SwapOn.
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
        "árbitro deve assinar s0 mesmo sob flood de Psi (Tick não pode ser starvado)"
    );
}
