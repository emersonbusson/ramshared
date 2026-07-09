//! Multi-connection NBD connection (§9.4 / H1): dedicated reader and writer per connection,
//! connected to the **single CUDA worker** (in `main`) via channels. The reader drains the socket
//! and enqueues `Job`s; the worker processes them (CUDA affinity) and returns `Reply`s via
//! the connection's **unbounded** replica channel; the writer writes to the socket.
//!
//! SPEC: `docs/daemon-multiconn/SPECv3.md` (DT-7/DT-8/DT-15/DT-16). Deterministic design:
//! `Opened` comes from the acceptor (before spawning the reader), `Closed` comes from the reader (upon exit) —
//! the worker counts `live` connections and terminates when all open connections close.

use std::io::{BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel};
use std::thread::JoinHandle;

use ramshared_block::handshake::Export;
use ramshared_block::protocol::SIMPLE_REPLY_LEN;
use ramshared_block::{Command, Request, parse_request, protocol::REQUEST_LEN, server_handshake};

/// Capacity of the worker message channel (`WMsg`): the **single** point of backpressure.
/// The replica channel per connection is unbounded (DT-7), so the worker never blocks when
/// responding — only the readers apply backpressure when enqueuing `Job`s.
pub const CHAN_CAP: usize = 64;

/// A request to be processed by the CUDA worker, with the replica route of the source connection.
/// The canary latency is measured in the worker around `serve()` (serve-only, DT-16
/// revised): measuring the wait time in the queue caused false positives for DEMOTE under normal load.
pub struct Job {
    /// Index of the export (slice) negotiated in the handshake — which window the worker serves (RF-L1).
    pub export: usize,
    pub req: Request,
    pub payload: Vec<u8>,
    pub reply: Sender<Reply>,
}

/// Outcome of `serve()` to be written to the connection socket. `reply` is the 16-byte
/// NBD header (fixed `Copy` array, without allocation in the hot path — DT-8).
pub struct Reply {
    pub reply: [u8; SIMPLE_REPLY_LEN],
    pub data: Vec<u8>,
    pub disconnect: bool,
}

/// Worker channel message (DT-15). `Opened`/`Closed` control the deterministic
/// termination; `Job` is work; `ZeroExport` is the broker slice cleanup (DT-17): the
/// worker (single thread owning the backend) zeroes the `[base, base+len)` window and confirms via `done`.
pub enum WMsg {
    Opened,
    Job(Job),
    Closed,
    ZeroExport {
        base: u64,
        len: u64,
        done: Sender<bool>,
    },
}

/// Count of live connections in the worker (DT-15). `Opened` (from acceptor) always precedes
/// `Closed` (from reader) per connection, so `live` stays balanced; the worker stops when
/// all open connections have closed. Pure logic (testable without GPU/sockets).
#[derive(Default)]
pub struct LiveCount {
    live: u32,
    opened: bool,
}

impl LiveCount {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_open(&mut self) {
        self.live += 1;
        self.opened = true;
    }

    /// Registers the closing of a connection; returns `true` when **all** open
    /// connections have closed (the worker should terminate). `saturating_sub` avoids underflow if
    /// a `Closed` arrives unbalanced (it shouldn't, but it is defensive).
    pub fn on_close(&mut self) -> bool {
        self.live = self.live.saturating_sub(1);
        self.live == 0 && self.opened
    }

    pub fn live(&self) -> u32 {
        self.live
    }
}

/// Writer thread: drains `Reply`s and writes to the socket. Replies can go out of
/// order (each carries the NBD `handle`). Terminates on socket error, on `disconnect`,
/// or when the channel closes (reader exited and all replies were drained).
pub fn spawn_writer<S: Write + Send + 'static>(
    stream: S,
    replies: Receiver<Reply>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut w = stream;
        for r in replies.iter() {
            if w.write_all(&r.reply).is_err() {
                break;
            }
            if !r.data.is_empty() && w.write_all(&r.data).is_err() {
                break;
            }
            if w.flush().is_err() {
                break;
            }
            if r.disconnect {
                break;
            }
        }
    })
}

/// Reader thread (generic over stream — Unix or TCP, RF-L2): handshake on its own thread
/// (DT-15 — error confined to the connection), negotiates export by name (RF-L1) and enqueues `Job`s with
/// the export index. `hs_writer` is the write handle (clone made by the acceptor) used only during the
/// handshake. Upon exiting (EOF/error/handshake failure) sends `WMsg::Closed` to balance `Opened`.
pub fn spawn_reader<S: Read + Send + 'static, W2: Write + Send + 'static>(
    stream: S,
    mut hs_writer: W2,
    exports: Arc<Vec<Export>>,
    tx_flags: u16,
    jobs: SyncSender<WMsg>,
    reply_tx: Sender<Reply>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let idx = match server_handshake(&mut reader, &mut hs_writer, &exports, tx_flags) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[ramsharedd] conn: handshake falhou: {e}");
                let _ = jobs.send(WMsg::Closed);
                return;
            }
        };
        drop(hs_writer); // handshake completed; from here on only the writer thread writes replies.
        let export_size = exports[idx].size; // anti-DoS based on negotiated export (RF-L1)

        let mut hdr = [0u8; REQUEST_LEN];
        loop {
            if reader.read_exact(&mut hdr).is_err() {
                break; // EOF ou erro de socket
            }
            let req = match parse_request(&hdr) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[ramsharedd] conn: request malformado: {e}; desconectando");
                    break;
                }
            };
            // Anti-DoS: a WRITE can never exceed the negotiated export (prevents allocating gigabytes).
            if req.cmd == Command::Write && req.len as u64 > export_size {
                eprintln!(
                    "[ramsharedd] conn: WRITE len {} excede o export; desconectando",
                    req.len
                );
                break;
            }
            let payload = if req.cmd == Command::Write {
                let mut p = vec![0u8; req.len as usize];
                if reader.read_exact(&mut p).is_err() {
                    break;
                }
                p
            } else {
                Vec::new()
            };
            let job = Job {
                export: idx,
                req,
                payload,
                reply: reply_tx.clone(),
            };
            if jobs.send(WMsg::Job(job)).is_err() {
                break; // worker terminated
            }
        }
        let _ = jobs.send(WMsg::Closed);
    })
}

/// Wires an accepted connection to the worker: `WMsg::Opened` **before** spawning the reader (balances
/// `live`, DT-15), **unbounded** replica channel (DT-7), writer + reader. Generic over the
/// handles (Unix/TCP). Returns `false` if the worker terminated (the acceptor should stop).
fn wire_conn<RS, WS>(
    rstream: RS,
    wstream: WS,
    hs_writer: WS,
    exports: &Arc<Vec<Export>>,
    tx_flags: u16,
    jobs: &SyncSender<WMsg>,
) -> bool
where
    RS: Read + Send + 'static,
    WS: Write + Send + 'static,
{
    if jobs.send(WMsg::Opened).is_err() {
        return false; // worker terminated
    }
    let (reply_tx, reply_rx) = channel::<Reply>(); // unbounded (DT-7)
    spawn_writer(wstream, reply_rx);
    spawn_reader(
        rstream,
        hs_writer,
        Arc::clone(exports),
        tx_flags,
        jobs.clone(),
        reply_tx,
    );
    true
}

/// **Unix** Acceptor: accepts connections in a loop (N-agnostic) and wires each to the worker, negotiating
/// the export by name via `exports` (RF-L1). Each connection needs 2 clones of the stream (writer +
/// handshake) in addition to the read handle.
pub fn spawn_acceptor(
    listener: UnixListener,
    exports: Arc<Vec<Export>>,
    tx_flags: u16,
    jobs: SyncSender<WMsg>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            let stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) => {
                    eprintln!("[ramsharedd] accept falhou: {e}");
                    break;
                }
            };
            let (wstream, hs_writer) = match (stream.try_clone(), stream.try_clone()) {
                (Ok(w), Ok(h)) => (w, h),
                _ => {
                    eprintln!("[ramsharedd] try_clone (unix) failed; skipping connection");
                    continue;
                }
            };
            if !wire_conn(stream, wstream, hs_writer, &exports, tx_flags, &jobs) {
                break;
            }
        }
    })
}

/// **TCP** Acceptor (RF-L2): same design as Unix over `TcpListener`, feeding the SAME
/// `jobs` channel (worker is unique). `TCP_NODELAY` per connection (swap latency).
pub fn spawn_acceptor_tcp(
    listener: TcpListener,
    exports: Arc<Vec<Export>>,
    tx_flags: u16,
    jobs: SyncSender<WMsg>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            let stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) => {
                    eprintln!("[ramsharedd] accept tcp falhou: {e}");
                    break;
                }
            };
            let _ = stream.set_nodelay(true); // TCP_NODELAY: swap latency
            let (wstream, hs_writer) = match (stream.try_clone(), stream.try_clone()) {
                (Ok(w), Ok(h)) => (w, h),
                _ => {
                    eprintln!("[ramsharedd] try_clone (tcp) failed; skipping connection");
                    continue;
                }
            };
            if !wire_conn(stream, wstream, hs_writer, &exports, tx_flags, &jobs) {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::sync::mpsc::sync_channel;

    fn dummy_req() -> Request {
        Request {
            flags: 0,
            cmd: Command::Read,
            handle: 1,
            offset: 0,
            len: 0,
        }
    }

    #[test]
    fn job_reply_roundtrip() {
        let (tx, _rx) = channel::<Reply>();
        let job = Job {
            export: 0,
            req: dummy_req(),
            payload: vec![1, 2, 3],
            reply: tx,
        };
        assert_eq!(job.req.handle, 1);
        assert_eq!(job.payload, vec![1, 2, 3]);
        let rep = Reply {
            reply: [0u8; SIMPLE_REPLY_LEN],
            data: vec![9, 8, 7],
            disconnect: false,
        };
        assert_eq!(rep.data, vec![9, 8, 7]);
        assert!(!rep.disconnect);
    }

    #[test]
    fn chan_cap_is_bounded() {
        let (tx, _rx) = sync_channel::<u8>(2);
        assert!(tx.try_send(1).is_ok());
        assert!(tx.try_send(2).is_ok());
        assert!(
            tx.try_send(3).is_err(),
            "deve recusar além do cap (backpressure)"
        );
    }

    // DT-18 / F-3/F-5: deterministic termination — stops exactly when live==0.
    #[test]
    fn live_count_terminates_on_all_closed() {
        let mut lc = LiveCount::new();
        lc.on_open(); // live=1
        lc.on_open(); // live=2
        assert!(!lc.on_close(), "live=1 still"); // live=1
        assert!(lc.on_close(), "live=0 + opened -> stops"); // live=0
    }

    // DT-18 / F-6: failed handshake = immediate Opened (acceptor) + Closed (reader); balanced.
    #[test]
    fn live_count_balanced_open_then_close() {
        let mut lc = LiveCount::new();
        lc.on_open();
        assert!(lc.on_close(), "1 connection opened and closed -> stops");
    }

    #[test]
    fn live_count_never_stops_before_any_open() {
        let mut lc = LiveCount::new();
        assert!(!lc.on_close(), "without Opened does not stop spuriously");
        assert_eq!(lc.live(), 0);
    }

    // DT-7 / DT-18: unbounded replica — worker progresses even with the writer stopped.
    // If the replica were bounded and the writer did not drain, the worker would block →
    // Jobs channel would fill up → reader would block → deadlock (this test would hang).
    #[test]
    fn slow_writer_does_not_deadlock() {
        let (jobs_tx, jobs_rx) = sync_channel::<WMsg>(2); // small Jobs channel
        let (reply_tx, reply_rx) = channel::<Reply>(); // UNBOUNDED replica (DT-7)
        let _writer_parado = reply_rx; // holds without draining (simulates a hung socket)

        let worker = std::thread::spawn(move || {
            let mut served = 0u32;
            for m in jobs_rx.iter() {
                if let WMsg::Job(job) = m {
                    // worker never blocks: unbounded replica
                    let _ = job.reply.send(Reply {
                        reply: [0u8; SIMPLE_REPLY_LEN],
                        data: Vec::new(),
                        disconnect: false,
                    });
                    served += 1;
                    if served >= 10 {
                        break;
                    }
                }
            }
            served
        });

        for _ in 0..10 {
            jobs_tx
                .send(WMsg::Job(Job {
                    export: 0,
                    req: dummy_req(),
                    payload: Vec::new(),
                    reply: reply_tx.clone(),
                }))
                .unwrap();
        }
        assert_eq!(
            worker.join().unwrap(),
            10,
            "worker processou tudo sem deadlock"
        );
    }
}
