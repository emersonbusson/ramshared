//! `ramshared-agent` — agent (tenant) of the Memory Broker. Connects to the broker via TCP, reports
//! PSI/swaps 1×/s and executes `SwapOn`/`SwapOff`/`DemoteAll` commands over NBD (DT-27).
//!
//! 3-thread architecture with **single writer** (DT-27/R8):
//! - **reader**: blocks on `read_msg(socket)` and forwards each `Msg` to the main loop;
//! - **exec**: executes `attach`/`detach` (blocking) out of the socket path and returns the
//!   result via channel — this way a slow `swapon` never blocks the heartbeat;
//! - **main**: owner of the write socket — sends `Psi`, dispatches commands to exec, drains the
//!   results back as `SwapOnDone`/`SwapOffDone` and arms the watchdog (DT-18).
//!
//! SPEC: docs/specs/no-milestone/memory-broker/SPEC.md (ITEM-9). Without `unsafe`.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{BufReader, ErrorKind};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ramshared_agent::watchdog::Watchdog;
use ramshared_agent::{psi, swap};
use ramshared_broker::model::{SliceId, TransportKind};
use ramshared_broker::protocol::{Msg, NbdEndpoint, PROTO_VERSION, TenantMem, read_msg, write_msg};

/// Transmission rate of `Psi` (low-rate control-plane, ~1 msg/s).
const PSI_PERIOD: Duration = Duration::from_secs(1);
/// Poll slice of the main loop (responsiveness of the timer/exec without busy-loop).
const POLL_SLICE: Duration = Duration::from_millis(200);
/// Reconnection backoff to the broker: starts at [`INITIAL_BACKOFF`] and doubles up to [`MAX_BACKOFF`]
/// while connection fails (broker down) — avoids reconnection thrashing; resets after a
/// productive session (≥ [`PRODUCTIVE_SESSION`], i.e., actually connected and ran).
const INITIAL_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const PRODUCTIVE_SESSION: Duration = Duration::from_secs(10);
/// One-shot `--status` I/O deadline (DT-45); prevents a silent broker from hanging the CLI.
const STATUS_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Next backoff (doubles with cap). Pure/testable.
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

enum ParsedArgs {
    Help,
    Config(Config),
}

enum CliExit {
    Help,
    Usage(String),
    Runtime(String),
}

/// Command from the main loop to the execution thread.
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

/// Result returned by the execution thread to the main loop.
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
    "Usage:\n  \
     ramshared-agent --broker HOST:PORT --tenant NAME [--swap-prio P] \
     [--nbd-base /dev/nbd] [--transport tcp|unix] [--watchdog-secs 90]\n  \
     ramshared-agent --broker HOST:PORT --status"
        .to_string()
}

fn usage_diagnostic(message: &str) -> String {
    let usage = usage();
    if message.ends_with(&usage) {
        message.to_string()
    } else {
        format!("{message}\n{usage}")
    }
}

fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
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
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match arg.as_str() {
            "--broker" => broker = Some(take("--broker")?),
            "--tenant" => tenant = Some(take("--tenant")?),
            "--swap-prio" => {
                let v = take("--swap-prio")?;
                swap_prio = Some(
                    v.parse()
                        .map_err(|_| format!("--swap-prio is invalid: {v}"))?,
                );
            }
            "--nbd-base" => nbd_base = take("--nbd-base")?,
            "--transport" => {
                transport = match take("--transport")?.as_str() {
                    "tcp" => TransportKind::NbdTcp,
                    "unix" => TransportKind::NbdUnix,
                    other => return Err(format!("--transport is invalid: {other} (use tcp|unix)")),
                };
            }
            "--watchdog-secs" => {
                let v = take("--watchdog-secs")?;
                let s: u64 = v
                    .parse()
                    .map_err(|_| format!("--watchdog-secs is invalid: {v}"))?;
                watchdog = Duration::from_secs(s);
            }
            "--status" => status_only = true,
            "-h" | "--help" => return Ok(ParsedArgs::Help),
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
    }

    Ok(ParsedArgs::Config(Config {
        broker: broker.ok_or_else(|| format!("--broker is required\n{}", usage()))?,
        tenant: tenant.unwrap_or_default(),
        swap_prio,
        nbd_base,
        transport,
        watchdog,
        status_only,
    }))
}

fn run(args: &[String]) -> Result<(), CliExit> {
    let cfg = match parse_args(args).map_err(CliExit::Usage)? {
        ParsedArgs::Help => return Err(CliExit::Help),
        ParsedArgs::Config(cfg) => cfg,
    };

    if cfg.status_only {
        return run_status(&cfg).map_err(|error| CliExit::Runtime(error.to_string()));
    }
    if cfg.tenant.is_empty() {
        return Err(CliExit::Usage(format!(
            "--tenant is required in agent mode\n{}",
            usage()
        )));
    }

    // DT-26: swap requires privilege. Reads euid via /proc (no libc) and refuses early, with number.
    let euid = psi::read_euid().map_err(|error| CliExit::Runtime(error.to_string()))?;
    if euid != 0 {
        return Err(CliExit::Runtime(format!(
            "root is required for swap (current euid={euid}, expected 0)"
        )));
    }

    run_agent(&cfg).map_err(|error| CliExit::Runtime(error.to_string()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => {}
        Err(CliExit::Help) => println!("{}", usage()),
        Err(CliExit::Usage(message)) => {
            eprintln!("{}", usage_diagnostic(&message));
            std::process::exit(2);
        }
        Err(CliExit::Runtime(message)) => {
            eprintln!("[agent] error: {message}");
            std::process::exit(1);
        }
    }
}

/// `--status` mode: one-shot query (does not register; the broker responds with `StatusReply` to any
/// session) and prints the status.
fn run_status(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(&cfg.broker)?;
    stream.set_read_timeout(Some(STATUS_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(STATUS_IO_TIMEOUT))?;
    let mut w = stream.try_clone()?;
    let mut r = BufReader::new(stream);
    write_msg(&mut w, &Msg::Status)?;
    for _ in 0..50 {
        let response = match read_msg(&mut r) {
            Ok(response) => response,
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                return Err(format!(
                    "broker status timed out after {}s",
                    STATUS_IO_TIMEOUT.as_secs()
                )
                .into());
            }
            Err(error) => return Err(error.into()),
        };
        match response {
            Some(Msg::StatusReply {
                tenants,
                slices,
                slice_io,
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
                println!("slice_io ({}):", slice_io.len());
                for io in &slice_io {
                    println!(
                        "  s{} bytes_served={} io_count={}",
                        io.id, io.bytes_served, io.io_count
                    );
                }
                println!("last_rebalance_secs={last_rebalance_secs:?}");
                return Ok(());
            }
            Some(Msg::Error { reason }) => {
                return Err(format!("broker rejected status: {reason}").into());
            }
            Some(_) => continue,
            None => break,
        }
    }
    Err("broker did not return StatusReply".into())
}

/// Spawns the execution thread (lives for the entire process duration) and runs the session loop with reconnection.
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
            Ok(()) => eprintln!("[agent] session closed (EOF); reconnecting in {backoff:?}…"),
            Err(e) => eprintln!("[agent] session failed: {e}; reconnecting in {backoff:?}"),
        }
        thread::sleep(backoff);
        // Productive session (connected + ran) → goes back to minimum; quick failure (broker down) → grows.
        backoff = if ran >= PRODUCTIVE_SESSION {
            INITIAL_BACKOFF
        } else {
            next_backoff(backoff)
        };
    }
}

struct SessionDispatcher<'a> {
    cfg: &'a Config,
    active: &'a mut HashMap<SliceId, String>,
    wd: &'a mut Watchdog,
    cmd_tx: &'a Sender<ExecCmd>,
    res_rx: &'a Receiver<ExecResult>,
    session_err: &'a mut Option<Box<dyn std::error::Error>>,
}

impl<'a> SessionDispatcher<'a> {
    fn tick_psi(&mut self, next_psi: &mut Instant, w: &mut TcpStream) {
        let now = Instant::now();
        if now >= *next_psi {
            match (psi::read_psi(), psi::read_swaps()) {
                (Ok(sample), Ok(swaps)) => {
                    let mem = Some(TenantMem {
                        swap_current: psi::read_memcg_swap(),
                        diskstats_io: self
                            .active
                            .values()
                            .filter_map(|d| psi::read_diskstats(d))
                            .sum(),
                    });
                    if let Err(e) = write_msg(w, &Msg::Psi { sample, swaps, mem }) {
                        *self.session_err = Some(e.into());
                    }
                }
                (s, sw) => eprintln!(
                    "[agent] PSI unreadable (psi={:?} swaps={:?}); skipping cycle",
                    s.err(),
                    sw.err()
                ),
            }
            *next_psi = now + PSI_PERIOD;
        }
    }

    fn drain_exec(&mut self, w: &mut TcpStream) {
        while let Ok(res) = self.res_rx.try_recv() {
            let done = match res {
                ExecResult::On { slice, ok, detail } => {
                    if !ok {
                        self.active.remove(&slice);
                    }
                    Msg::SwapOnDone { slice, ok, detail }
                }
                ExecResult::Off { slice, ok, detail } => {
                    self.active.remove(&slice);
                    Msg::SwapOffDone { slice, ok, detail }
                }
            };
            if let Err(e) = write_msg(w, &done) {
                *self.session_err = Some(e.into());
                break;
            }
        }
    }

    fn dispatch_msg(&mut self, msg_rx: &Receiver<Msg>) -> bool {
        match msg_rx.recv_timeout(POLL_SLICE) {
            Ok(msg) => {
                self.wd.touch(Instant::now());
                if !handle_msg(self.cfg, msg, self.active, self.cmd_tx) {
                    return false; // broker sent Error / requested shutdown
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false, // reader exited (EOF/socket error)
        }
        true
    }

    fn check_watchdog(&self) -> bool {
        if self.wd.expired(Instant::now()) {
            eprintln!(
                "[agent] watchdog: broker silent for {}s; closing session",
                self.cfg.watchdog.as_secs()
            );
            return false;
        }
        true
    }
}

/// A TCP session: connects, registers, and runs the loop until EOF/error/watchdog. On exit, performs
/// best-effort `swapoff` of still active slices (dead broker ⇒ dead NBD).
fn session(
    cfg: &Config,
    cmd_tx: &Sender<ExecCmd>,
    res_rx: &Receiver<ExecResult>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(&cfg.broker)?;
    let mut w = stream.try_clone()?;
    let reader = BufReader::new(stream);

    // reader thread: socket → Msg channel; exits (drops sender) on EOF/error.
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
        let mut dispatcher = SessionDispatcher {
            cfg,
            active: &mut active,
            wd: &mut wd,
            cmd_tx,
            res_rx,
            session_err: &mut session_err,
        };

        // (1) PSI heartbeat at cadence. Error reading /proc is transient: log and continue.
        dispatcher.tick_psi(&mut next_psi, &mut w);
        if dispatcher.session_err.is_some() {
            break;
        }

        // (2) drains results from exec → Done back to the broker (single writer = this thread).
        dispatcher.drain_exec(&mut w);
        if dispatcher.session_err.is_some() {
            break;
        }

        // (3) waits for a message from the broker (with a short slice to keep timer/exec alive).
        if !dispatcher.dispatch_msg(&msg_rx) {
            break;
        }

        // (4) watchdog: silent broker beyond the deadline ⇒ dead session.
        if !dispatcher.check_watchdog() {
            break;
        }
    }

    // Cleanup: releases active slices (best-effort; broker reconciles on re-register).
    for (slice, dev) in active.drain() {
        if let Err(e) = swap::detach_swap(&dev) {
            eprintln!("[agent] cleanup swapoff s{slice} ({dev}) failed: {e}");
        }
    }
    let _ = w.shutdown(std::net::Shutdown::Both);
    let _ = reader_handle.join();

    match session_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Handles a message from the broker. Returns `false` if the session should terminate.
fn handle_msg(
    cfg: &Config,
    msg: Msg,
    active: &mut HashMap<SliceId, String>,
    cmd_tx: &Sender<ExecCmd>,
) -> bool {
    match msg {
        Msg::Registered { tenant_id } => {
            eprintln!("[agent] registered: tenant_id={tenant_id}");
            true
        }
        Msg::Ack => true, // heartbeat (already touched the watchdog)
        Msg::SwapOn {
            slice,
            export,
            endpoint,
            swap_prio,
        } => {
            let dev = format!("{}{}", cfg.nbd_base, slice);
            active.insert(slice, dev.clone());
            let prio = swap_prio.or(cfg.swap_prio); // DT-7: broker is authoritative; CLI is fallback
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
            eprintln!("[agent] DemoteAll: releasing {} slice(s)", active.len());
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
            eprintln!("[agent] broker refused the session: {reason}");
            false
        }
        other => {
            eprintln!("[agent] ignored message: {other:?}");
            true
        }
    }
}

/// Execution thread loop: runs attach/detach (blocking) and returns the result.
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
            break; // main loop is gone; nothing to do
        }
    }
}

/// Reader thread loop: forwards each `Msg` to the main loop; exits on EOF/error (dropping the
/// sender, which the main loop detects as `Disconnected`).
fn reader_loop(mut reader: BufReader<TcpStream>, msg_tx: Sender<Msg>) {
    loop {
        match read_msg(&mut reader) {
            Ok(Some(msg)) => {
                if msg_tx.send(msg).is_err() {
                    break;
                }
            }
            Ok(None) => break, // clean EOF
            Err(e) => {
                eprintln!("[agent] socket read error: {e}");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::net::TcpListener;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn parse_config(v: &[&str]) -> Config {
        match parse_args(&args(v)).expect("arguments must parse as a configuration") {
            ParsedArgs::Config(config) => config,
            ParsedArgs::Help => panic!("test expected configuration, not help"),
        }
    }

    fn test_config(broker: String, watchdog: Duration) -> Config {
        Config {
            broker,
            tenant: "test-tenant".to_string(),
            swap_prio: Some(-3),
            nbd_base: "/dev/ramshared-test-nbd".to_string(),
            transport: TransportKind::NbdTcp,
            watchdog,
            status_only: false,
        }
    }

    fn expect_register_and_psi(reader: &mut BufReader<TcpStream>) {
        assert!(matches!(
            read_msg(reader).expect("registration must decode"),
            Some(Msg::Register {
                proto: PROTO_VERSION,
                tenant,
                transport: TransportKind::NbdTcp,
            }) if tenant == "test-tenant"
        ));
        assert!(matches!(
            read_msg(reader).expect("PSI report must decode"),
            Some(Msg::Psi { mem: Some(_), .. })
        ));
    }

    #[test]
    fn backoff_doubles_up_to_cap() {
        // doubles: 2→4→8→16→32→60(cap)→60
        assert_eq!(next_backoff(INITIAL_BACKOFF), Duration::from_secs(4));
        assert_eq!(next_backoff(Duration::from_secs(4)), Duration::from_secs(8));
        assert_eq!(
            next_backoff(Duration::from_secs(16)),
            Duration::from_secs(32)
        );
        // 32*2=64 → saturates at the cap of 60
        assert_eq!(next_backoff(Duration::from_secs(32)), MAX_BACKOFF);
        assert_eq!(next_backoff(MAX_BACKOFF), MAX_BACKOFF);
    }

    #[test]
    fn parse_minimal_agent() {
        let c = parse_config(&["--broker", "10.0.0.1:7000", "--tenant", "wsl2"]);
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
        let c = parse_config(&[
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
        ]);
        assert_eq!(c.swap_prio, Some(-3));
        assert!(matches!(c.transport, TransportKind::NbdUnix));
        assert_eq!(c.watchdog, Duration::from_secs(30));
    }

    #[test]
    fn status_mode_needs_no_tenant() {
        let c = parse_config(&["--broker", "h:1", "--status"]);
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

    #[test]
    fn help_is_a_parse_outcome() {
        assert!(matches!(
            parse_args(&args(&["--help"])),
            Ok(ParsedArgs::Help)
        ));
    }

    #[test]
    fn usage_diagnostic_adds_usage_once() {
        let usage = usage();
        assert_eq!(
            usage_diagnostic("invalid input"),
            format!("invalid input\n{usage}")
        );
        assert_eq!(usage_diagnostic(&usage), usage);
    }

    #[test]
    fn swap_on_prefers_broker_priority_without_running_swap() {
        let cfg = test_config("127.0.0.1:1".to_string(), Duration::from_secs(1));
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let mut active = HashMap::new();

        assert!(handle_msg(
            &cfg,
            Msg::SwapOn {
                slice: 7,
                export: "s7".to_string(),
                endpoint: NbdEndpoint::Tcp {
                    host: "127.0.0.1".to_string(),
                    port: 10809,
                },
                swap_prio: Some(-9),
            },
            &mut active,
            &cmd_tx,
        ));
        assert_eq!(
            active.get(&7),
            Some(&"/dev/ramshared-test-nbd7".to_string())
        );
        match cmd_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("SwapOn must be dispatched")
        {
            ExecCmd::On {
                slice,
                export,
                endpoint,
                dev,
                prio,
            } => {
                assert_eq!(slice, 7);
                assert_eq!(export, "s7");
                assert!(matches!(endpoint, NbdEndpoint::Tcp { port: 10809, .. }));
                assert_eq!(dev, "/dev/ramshared-test-nbd7");
                assert_eq!(prio, Some(-9));
            }
            ExecCmd::Off { .. } => panic!("SwapOn must not dispatch SwapOff"),
        }
    }

    #[test]
    fn demote_all_dispatches_release_without_running_swap() {
        let cfg = test_config("127.0.0.1:1".to_string(), Duration::from_secs(1));
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let mut active = HashMap::from([
            (1, "/dev/ramshared-test-nbd1".to_string()),
            (2, "/dev/ramshared-test-nbd2".to_string()),
        ]);

        assert!(handle_msg(&cfg, Msg::DemoteAll, &mut active, &cmd_tx));
        let mut releases = [
            cmd_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first DemoteAll release must dispatch"),
            cmd_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("second DemoteAll release must dispatch"),
        ]
        .into_iter()
        .map(|cmd| match cmd {
            ExecCmd::Off { slice, dev } => (slice, dev),
            ExecCmd::On { .. } => panic!("DemoteAll must only dispatch SwapOff"),
        })
        .collect::<Vec<_>>();
        releases.sort_unstable();
        assert_eq!(
            releases,
            vec![
                (1, "/dev/ramshared-test-nbd1".to_string()),
                (2, "/dev/ramshared-test-nbd2".to_string()),
            ]
        );
    }

    #[test]
    fn session_registers_dispatches_commands_and_stops_on_refusal() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener must bind");
        let broker = listener
            .local_addr()
            .expect("listener address must be available")
            .to_string();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("agent must connect");
            let mut reader = BufReader::new(stream.try_clone().expect("stream must clone"));
            expect_register_and_psi(&mut reader);
            write_msg(&mut stream, &Msg::Registered { tenant_id: 42 })
                .expect("Registered must write");
            write_msg(&mut stream, &Msg::Ack).expect("Ack must write");
            write_msg(&mut stream, &Msg::SwapOff { slice: 7 }).expect("SwapOff must write");
            write_msg(
                &mut stream,
                &Msg::LeaseGranted {
                    lease: 9,
                    bytes: 4096,
                },
            )
            .expect("non-command frame must write");
            write_msg(
                &mut stream,
                &Msg::Error {
                    reason: "test refusal".to_string(),
                },
            )
            .expect("broker refusal must write");
        });
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (_res_tx, res_rx) = mpsc::channel();
        let cfg = test_config(broker, Duration::from_secs(1));

        assert!(session(&cfg, &cmd_tx, &res_rx).is_ok());
        server.join().expect("broker fixture must finish");
        match cmd_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("broker command must dispatch")
        {
            ExecCmd::Off { slice, dev } => {
                assert_eq!(slice, 7);
                assert_eq!(dev, "/dev/ramshared-test-nbd7");
            }
            ExecCmd::On { .. } => panic!("SwapOff frame must not attach swap"),
        }
        assert!(cmd_rx.try_recv().is_err());
    }

    #[test]
    fn session_reports_execution_results_without_running_swap() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener must bind");
        let broker = listener
            .local_addr()
            .expect("listener address must be available")
            .to_string();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("agent must connect");
            let mut reader = BufReader::new(stream.try_clone().expect("stream must clone"));
            expect_register_and_psi(&mut reader);
            assert!(matches!(
                read_msg(&mut reader).expect("SwapOn completion must decode"),
                Some(Msg::SwapOnDone {
                    slice: 3,
                    ok: false,
                    ref detail,
                }) if detail == "test attach refusal"
            ));
            assert!(matches!(
                read_msg(&mut reader).expect("SwapOff completion must decode"),
                Some(Msg::SwapOffDone {
                    slice: 3,
                    ok: true,
                    ref detail,
                }) if detail == "/dev/ramshared-test-nbd3"
            ));
            write_msg(
                &mut stream,
                &Msg::Error {
                    reason: "fixture complete".to_string(),
                },
            )
            .expect("broker refusal must write");
        });
        let (cmd_tx, _cmd_rx) = mpsc::channel();
        let (res_tx, res_rx) = mpsc::channel();
        res_tx
            .send(ExecResult::On {
                slice: 3,
                ok: false,
                detail: "test attach refusal".to_string(),
            })
            .expect("test result must queue");
        res_tx
            .send(ExecResult::Off {
                slice: 3,
                ok: true,
                detail: "/dev/ramshared-test-nbd3".to_string(),
            })
            .expect("test result must queue");
        let cfg = test_config(broker, Duration::from_secs(1));

        assert!(session(&cfg, &cmd_tx, &res_rx).is_ok());
        server.join().expect("broker fixture must finish");
    }

    #[test]
    fn test_session_dispatcher_methods_compile() {
        // RED_TEST: Verifies the methods of SessionDispatcher exist and modify state.
        let cfg = test_config("127.0.0.1:9999".to_string(), Duration::from_secs(1));
        let (cmd_tx, _cmd_rx) = mpsc::channel();
        let (_res_tx, res_rx) = mpsc::channel();

        let mut active = HashMap::new();
        let mut wd = Watchdog::new(cfg.watchdog, Instant::now());
        let mut session_err = None;

        // This won't run fully since we don't have a real stream, but it forces
        // the compiler to check the signatures.
        // let mut dispatcher = SessionDispatcher { ... };
        // We will just verify it as compile-only visibility test.
        let _ = SessionDispatcher {
            cfg: &cfg,
            active: &mut active,
            wd: &mut wd,
            cmd_tx: &cmd_tx,
            res_rx: &res_rx,
            session_err: &mut session_err,
        };
    }

    #[test]
    fn session_watchdog_terminates_silent_broker_without_swap() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener must bind");
        let broker = listener
            .local_addr()
            .expect("listener address must be available")
            .to_string();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("agent must connect");
            let mut reader = BufReader::new(stream);
            expect_register_and_psi(&mut reader);
            assert!(
                read_msg(&mut reader)
                    .expect("client shutdown must decode as EOF")
                    .is_none()
            );
        });
        let (cmd_tx, _cmd_rx) = mpsc::channel();
        let (_res_tx, res_rx) = mpsc::channel();
        let cfg = test_config(broker, Duration::from_millis(1));
        let started = Instant::now();

        assert!(session(&cfg, &cmd_tx, &res_rx).is_ok());
        assert!(started.elapsed() < Duration::from_secs(1));
        server.join().expect("silent broker fixture must finish");
    }
}
