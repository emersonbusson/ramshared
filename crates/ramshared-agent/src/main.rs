//! `ramshared-agent` — agente (tenant) do Memory Broker. Conecta ao broker por TCP, reporta
//! PSI/swaps 1×/s e executa os comandos `SwapOn`/`SwapOff`/`DemoteAll` sobre NBD (DT-27).
//!
//! Arquitetura de 3 threads com **escritor único** (DT-27/R8):
//! - **reader**: bloqueia em `read_msg(socket)` e encaminha cada `Msg` ao loop principal;
//! - **exec**: executa `attach`/`detach` (bloqueante) fora do caminho do socket e devolve o
//!   resultado por canal — assim um `swapon` lento nunca trava o heartbeat;
//! - **main**: dono do socket de escrita — manda `Psi`, despacha comandos ao exec, drena os
//!   resultados de volta como `SwapOnDone`/`SwapOffDone` e arma o watchdog (DT-18).
//!
//! SPEC: docs/memory-broker/SPECv2.md (ITEM-9). Sem `unsafe`.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::BufReader;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ramshared_agent::watchdog::Watchdog;
use ramshared_agent::{psi, swap};
use ramshared_broker::model::{SliceId, TransportKind};
use ramshared_broker::protocol::{Msg, NbdEndpoint, PROTO_VERSION, read_msg, write_msg};

/// Cadência de envio do `Psi` (control-plane de baixa taxa, ~1 msg/s).
const PSI_PERIOD: Duration = Duration::from_secs(1);
/// Fatia de espera do loop principal (responsividade do timer/exec sem busy-loop).
const POLL_SLICE: Duration = Duration::from_millis(200);
/// Backoff de reconexão ao broker: começa em [`INITIAL_BACKOFF`] e dobra até [`MAX_BACKOFF`]
/// enquanto a conexão falha (broker down) — evita thrash de reconexão; reseta após uma sessão
/// produtiva (≥ [`PRODUCTIVE_SESSION`], i.e. que de fato conectou e rodou).
const INITIAL_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const PRODUCTIVE_SESSION: Duration = Duration::from_secs(10);

/// Próximo backoff (dobra com teto). Pura/testável.
fn next_backoff(cur: Duration) -> Duration {
    (cur * 2).min(MAX_BACKOFF)
}

struct Config {
    broker: String,
    tenant: String,
    swap_prio: Option<i32>,
    nbd_base: String,
    transport: TransportKind,
    watchdog: Duration,
    status_only: bool,
}

/// Comando do loop principal para a thread de execução.
enum ExecCmd {
    On {
        slice: SliceId,
        export: String,
        endpoint: NbdEndpoint,
        dev: String,
        prio: Option<i32>,
    },
    Off {
        slice: SliceId,
        dev: String,
    },
}

/// Resultado devolvido pela thread de execução ao loop principal.
enum ExecResult {
    On {
        slice: SliceId,
        ok: bool,
        detail: String,
    },
    Off {
        slice: SliceId,
        ok: bool,
        detail: String,
    },
}

fn usage() -> String {
    "uso:\n  \
     ramshared-agent --broker HOST:PORT --tenant NOME [--swap-prio P] \
     [--nbd-base /dev/nbd] [--transport tcp|unix] [--watchdog-secs 90]\n  \
     ramshared-agent --broker HOST:PORT --status"
        .to_string()
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut broker = None;
    let mut tenant = None;
    let mut swap_prio = None;
    let mut nbd_base = "/dev/nbd".to_string();
    let mut transport = TransportKind::NbdTcp;
    let mut watchdog = Duration::from_secs(90);
    let mut status_only = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut take = |name: &str| -> Result<String, String> {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} exige um valor"))
        };
        match arg.as_str() {
            "--broker" => broker = Some(take("--broker")?),
            "--tenant" => tenant = Some(take("--tenant")?),
            "--swap-prio" => {
                let v = take("--swap-prio")?;
                swap_prio = Some(
                    v.parse()
                        .map_err(|_| format!("--swap-prio inválido: {v}"))?,
                );
            }
            "--nbd-base" => nbd_base = take("--nbd-base")?,
            "--transport" => {
                transport = match take("--transport")?.as_str() {
                    "tcp" => TransportKind::NbdTcp,
                    "unix" => TransportKind::NbdUnix,
                    other => return Err(format!("--transport inválido: {other} (use tcp|unix)")),
                };
            }
            "--watchdog-secs" => {
                let v = take("--watchdog-secs")?;
                let s: u64 = v
                    .parse()
                    .map_err(|_| format!("--watchdog-secs inválido: {v}"))?;
                watchdog = Duration::from_secs(s);
            }
            "--status" => status_only = true,
            "-h" | "--help" => return Err(usage()),
            other => return Err(format!("argumento desconhecido: {other}\n{}", usage())),
        }
    }

    Ok(Config {
        broker: broker.ok_or_else(|| format!("--broker é obrigatório\n{}", usage()))?,
        tenant: tenant.unwrap_or_default(),
        swap_prio,
        nbd_base,
        transport,
        watchdog,
        status_only,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = parse_args(&args).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    if cfg.status_only {
        return run_status(&cfg);
    }
    if cfg.tenant.is_empty() {
        return Err(format!("--tenant é obrigatório no modo agente\n{}", usage()).into());
    }

    // DT-26: swap exige privilégio. Lê o euid via /proc (sem libc) e recusa cedo, com número.
    let euid = psi::read_euid()?;
    if euid != 0 {
        return Err(format!("precisa de root para swap (euid atual={euid}, esperado 0)").into());
    }

    run_agent(&cfg)
}

/// Modo `--status`: consulta one-shot (não registra; o broker responde `StatusReply` a qualquer
/// sessão) e imprime o estado.
fn run_status(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(&cfg.broker)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut w = stream.try_clone()?;
    let mut r = BufReader::new(stream);
    write_msg(&mut w, &Msg::Status)?;
    for _ in 0..50 {
        match read_msg(&mut r)? {
            Some(Msg::StatusReply {
                tenants,
                slices,
                last_rebalance_secs,
            }) => {
                println!("tenants ({}):", tenants.len());
                for t in &tenants {
                    let mark = if t.present { "+" } else { "-" };
                    println!(
                        "  {mark} id={} name={} slices={:?} psi.avg10={:.2}",
                        t.id, t.name, t.slices, t.psi.avg10
                    );
                }
                println!("slices ({}):", slices.len());
                for s in &slices {
                    println!(
                        "  s{} off={} len={} tenant={:?} state={:?}",
                        s.id, s.offset, s.len, s.tenant, s.state
                    );
                }
                println!("last_rebalance_secs={last_rebalance_secs:?}");
                return Ok(());
            }
            Some(Msg::Error { reason }) => return Err(reason.into()),
            Some(_) => continue,
            None => break,
        }
    }
    Err("broker não respondeu StatusReply".into())
}

/// Sobe a thread de execução (vive por todo o processo) e roda o loop de sessão com reconexão.
fn run_agent(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<ExecCmd>();
    let (res_tx, res_rx) = mpsc::channel::<ExecResult>();
    let _exec = thread::spawn(move || exec_loop(cmd_rx, res_tx));

    eprintln!(
        "[agent] tenant={} broker={} transport={:?} watchdog={}s",
        cfg.tenant,
        cfg.broker,
        cfg.transport,
        cfg.watchdog.as_secs()
    );

    let mut backoff = INITIAL_BACKOFF;
    loop {
        let t0 = Instant::now();
        let result = session(cfg, &cmd_tx, &res_rx);
        let ran = t0.elapsed();
        match result {
            Ok(()) => eprintln!("[agent] sessão encerrada (EOF); reconectando em {backoff:?}…"),
            Err(e) => eprintln!("[agent] sessão caiu: {e}; reconectando em {backoff:?}"),
        }
        thread::sleep(backoff);
        // Sessão produtiva (conectou + rodou) → volta ao mínimo; falha rápida (broker down) → cresce.
        backoff = if ran >= PRODUCTIVE_SESSION {
            INITIAL_BACKOFF
        } else {
            next_backoff(backoff)
        };
    }
}

/// Uma sessão TCP: conecta, registra, e roda o loop até EOF/erro/watchdog. Ao sair, faz
/// `swapoff` best-effort das slices ainda ativas (broker morto ⇒ NBD morto).
fn session(
    cfg: &Config,
    cmd_tx: &Sender<ExecCmd>,
    res_rx: &Receiver<ExecResult>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(&cfg.broker)?;
    let mut w = stream.try_clone()?;
    let reader = BufReader::new(stream);

    // reader thread: socket → canal de Msg; sai (dropa o sender) em EOF/erro.
    let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
    let reader_handle = thread::spawn(move || reader_loop(reader, msg_tx));

    write_msg(
        &mut w,
        &Msg::Register {
            proto: PROTO_VERSION,
            tenant: cfg.tenant.clone(),
            transport: cfg.transport,
        },
    )?;

    let mut active: HashMap<SliceId, String> = HashMap::new();
    let mut wd = Watchdog::new(cfg.watchdog, Instant::now());
    let mut next_psi = Instant::now();
    let mut session_err: Option<Box<dyn std::error::Error>> = None;

    loop {
        let now = Instant::now();

        // (1) heartbeat de PSI na cadência. Erro de leitura de /proc é transiente: loga e segue.
        if now >= next_psi {
            match (psi::read_psi(), psi::read_swaps()) {
                (Ok(sample), Ok(swaps)) => {
                    if let Err(e) = write_msg(&mut w, &Msg::Psi { sample, swaps }) {
                        session_err = Some(e.into());
                        break;
                    }
                }
                (s, sw) => eprintln!(
                    "[agent] PSI ilegível (psi={:?} swaps={:?}); pulando ciclo",
                    s.err(),
                    sw.err()
                ),
            }
            next_psi = now + PSI_PERIOD;
        }

        // (2) drena resultados do exec → Done de volta ao broker (escritor único = esta thread).
        while let Ok(res) = res_rx.try_recv() {
            let done = match res {
                ExecResult::On { slice, ok, detail } => {
                    if !ok {
                        active.remove(&slice);
                    }
                    Msg::SwapOnDone { slice, ok, detail }
                }
                ExecResult::Off { slice, ok, detail } => {
                    active.remove(&slice);
                    Msg::SwapOffDone { slice, ok, detail }
                }
            };
            if let Err(e) = write_msg(&mut w, &done) {
                session_err = Some(e.into());
                break;
            }
        }
        if session_err.is_some() {
            break;
        }

        // (3) espera por uma mensagem do broker (com fatia curta p/ manter timer/exec vivos).
        match msg_rx.recv_timeout(POLL_SLICE) {
            Ok(msg) => {
                wd.touch(Instant::now());
                if !handle_msg(cfg, msg, &mut active, cmd_tx) {
                    break; // broker mandou Error / pediu para encerrar
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break, // reader saiu (EOF/erro de socket)
        }

        // (4) watchdog: broker silencioso além do deadline ⇒ sessão morta.
        if wd.expired(Instant::now()) {
            eprintln!(
                "[agent] watchdog: broker silencioso por {}s; encerrando sessão",
                cfg.watchdog.as_secs()
            );
            break;
        }
    }

    // Cleanup: solta as slices ativas (best-effort; broker reconcilia no re-register).
    for (slice, dev) in active.drain() {
        if let Err(e) = swap::detach_swap(&dev) {
            eprintln!("[agent] cleanup swapoff s{slice} ({dev}) falhou: {e}");
        }
    }
    let _ = w.shutdown(std::net::Shutdown::Both);
    let _ = reader_handle.join();

    match session_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Trata uma mensagem do broker. Retorna `false` se a sessão deve encerrar.
fn handle_msg(
    cfg: &Config,
    msg: Msg,
    active: &mut HashMap<SliceId, String>,
    cmd_tx: &Sender<ExecCmd>,
) -> bool {
    match msg {
        Msg::Registered { tenant_id } => {
            eprintln!("[agent] registrado: tenant_id={tenant_id}");
            true
        }
        Msg::Ack => true, // heartbeat (já tocou o watchdog)
        Msg::SwapOn {
            slice,
            export,
            endpoint,
            swap_prio,
        } => {
            let dev = format!("{}{}", cfg.nbd_base, slice);
            active.insert(slice, dev.clone());
            let prio = swap_prio.or(cfg.swap_prio); // DT-7: broker é autoritativo; CLI é fallback
            cmd_tx
                .send(ExecCmd::On {
                    slice,
                    export,
                    endpoint,
                    dev,
                    prio,
                })
                .is_ok()
        }
        Msg::SwapOff { slice } => {
            let dev = active
                .get(&slice)
                .cloned()
                .unwrap_or_else(|| format!("{}{}", cfg.nbd_base, slice));
            cmd_tx.send(ExecCmd::Off { slice, dev }).is_ok()
        }
        Msg::DemoteAll => {
            eprintln!("[agent] DemoteAll: soltando {} slice(s)", active.len());
            for (slice, dev) in active.iter() {
                if cmd_tx
                    .send(ExecCmd::Off {
                        slice: *slice,
                        dev: dev.clone(),
                    })
                    .is_err()
                {
                    return false;
                }
            }
            true
        }
        Msg::Error { reason } => {
            eprintln!("[agent] broker recusou a sessão: {reason}");
            false
        }
        other => {
            eprintln!("[agent] msg ignorada: {other:?}");
            true
        }
    }
}

/// Loop da thread de execução: roda attach/detach (bloqueante) e devolve o resultado.
fn exec_loop(cmd_rx: Receiver<ExecCmd>, res_tx: Sender<ExecResult>) {
    for cmd in cmd_rx.iter() {
        let res = match cmd {
            ExecCmd::On {
                slice,
                export,
                endpoint,
                dev,
                prio,
            } => {
                let (ok, detail) = match swap::attach_swap(&endpoint, &export, &dev, prio) {
                    Ok(()) => (true, dev),
                    Err(e) => (false, e),
                };
                ExecResult::On { slice, ok, detail }
            }
            ExecCmd::Off { slice, dev } => {
                let (ok, detail) = match swap::detach_swap(&dev) {
                    Ok(()) => (true, dev),
                    Err(e) => (false, e),
                };
                ExecResult::Off { slice, ok, detail }
            }
        };
        if res_tx.send(res).is_err() {
            break; // loop principal sumiu; nada a fazer
        }
    }
}

/// Loop da thread leitora: encaminha cada `Msg` ao loop principal; sai em EOF/erro (dropando o
/// sender, o que o loop principal detecta como `Disconnected`).
fn reader_loop(mut reader: BufReader<TcpStream>, msg_tx: Sender<Msg>) {
    loop {
        match read_msg(&mut reader) {
            Ok(Some(msg)) => {
                if msg_tx.send(msg).is_err() {
                    break;
                }
            }
            Ok(None) => break, // EOF limpo
            Err(e) => {
                eprintln!("[agent] erro de leitura do socket: {e}");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn backoff_doubles_up_to_cap() {
        // dobra: 2→4→8→16→32→60(teto)→60
        assert_eq!(next_backoff(INITIAL_BACKOFF), Duration::from_secs(4));
        assert_eq!(next_backoff(Duration::from_secs(4)), Duration::from_secs(8));
        assert_eq!(
            next_backoff(Duration::from_secs(16)),
            Duration::from_secs(32)
        );
        // 32*2=64 → satura no teto de 60
        assert_eq!(next_backoff(Duration::from_secs(32)), MAX_BACKOFF);
        assert_eq!(next_backoff(MAX_BACKOFF), MAX_BACKOFF);
    }

    #[test]
    fn parse_minimal_agent() {
        let c = parse_args(&args(&["--broker", "10.0.0.1:7000", "--tenant", "wsl2"])).unwrap();
        assert_eq!(c.broker, "10.0.0.1:7000");
        assert_eq!(c.tenant, "wsl2");
        assert_eq!(c.nbd_base, "/dev/nbd");
        assert!(matches!(c.transport, TransportKind::NbdTcp));
        assert_eq!(c.watchdog, Duration::from_secs(90));
        assert!(!c.status_only);
        assert!(c.swap_prio.is_none());
    }

    #[test]
    fn parse_full_flags() {
        let c = parse_args(&args(&[
            "--broker",
            "h:1",
            "--tenant",
            "t",
            "--swap-prio",
            "-3",
            "--nbd-base",
            "/dev/nbd",
            "--transport",
            "unix",
            "--watchdog-secs",
            "30",
        ]))
        .unwrap();
        assert_eq!(c.swap_prio, Some(-3));
        assert!(matches!(c.transport, TransportKind::NbdUnix));
        assert_eq!(c.watchdog, Duration::from_secs(30));
    }

    #[test]
    fn status_mode_needs_no_tenant() {
        let c = parse_args(&args(&["--broker", "h:1", "--status"])).unwrap();
        assert!(c.status_only);
        assert!(c.tenant.is_empty());
    }

    #[test]
    fn missing_broker_errors() {
        assert!(parse_args(&args(&["--tenant", "x"])).is_err());
    }

    #[test]
    fn unknown_flag_errors() {
        assert!(parse_args(&args(&["--broker", "h:1", "--bogus"])).is_err());
    }

    #[test]
    fn bad_transport_errors() {
        assert!(parse_args(&args(&["--broker", "h:1", "--transport", "rdma"])).is_err());
    }

    #[test]
    fn bad_swap_prio_errors() {
        assert!(parse_args(&args(&["--broker", "h:1", "--swap-prio", "x"])).is_err());
    }

    #[test]
    fn flag_without_value_errors() {
        assert!(parse_args(&args(&["--broker"])).is_err());
    }
}
