//! Multi-connection NBD connection (§9.4 / H1): dedicated reader and writer per connection,
//! connected to the **single CUDA worker** (in `main`) via channels. The reader drains the socket
//! and enqueues `Job`s; the worker processes them (CUDA affinity) and returns `Reply`s via
//! the connection's **unbounded** replica channel; the writer writes to the socket.
//!
//! SPEC: `docs/specs/no-milestone/wsl2-cascade-swap/SPEC.md` (DT-7/DT-8/DT-15/DT-16). Deterministic design:
//! `Opened` comes from the acceptor (before spawning the reader), `Closed` comes from the reader (upon exit) —
//! the worker counts `live` connections and reports the transition to zero. The simple NBD
//! runtime remains quiescent at zero until explicit shutdown so it can accept a later generation.

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

/// Worker channel message (DT-15). `Opened`/`Closed` control connection
/// accounting; `Job` is work; `ZeroExport` is the broker slice cleanup
/// (DT-17); and `Shutdown` is the internal broker-worker wake (DT-50). These
/// control variants are never exposed as NBD wire values.
pub enum WMsg {
    Opened,
    Job(Job),
    Closed,
    Shutdown,
    ZeroExport {
        base: u64,
        len: u64,
        done: Sender<bool>,
    },
}

/// Count of live connections in the worker (DT-15). `Opened` (from acceptor) always precedes
/// `Closed` (from reader) per connection, so `live` stays balanced. Reaching zero is a
/// transition signal; the owning runtime decides whether zero is terminal. Pure logic
/// (testable without GPU/sockets).
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

    /// Registers the closing of a connection; returns `true` once when **all** open
    /// connections have closed. An unbalanced
    /// `Closed` is ignored so it cannot emit a second terminal transition.
    pub fn on_close(&mut self) -> bool {
        if self.live == 0 {
            return false;
        }
        self.live -= 1;
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
                eprintln!("[ramsharedd] conn: handshake failed: {e}");
                let _ = jobs.send(WMsg::Closed);
                return;
            }
        };
        drop(hs_writer); // handshake completed; from here on only the writer thread writes replies.
        let export_size = exports[idx].size; // anti-DoS based on negotiated export (RF-L1)

        let mut hdr = [0u8; REQUEST_LEN];
        loop {
            if reader.read_exact(&mut hdr).is_err() {
                break; // EOF or socket error
            }
            let req = match parse_request(&hdr) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[ramsharedd] conn: malformed request: {e}; disconnecting");
                    break;
                }
            };
            // Anti-DoS: a WRITE can never exceed the negotiated export (prevents allocating gigabytes).
            if req.cmd == Command::Write && req.len as u64 > export_size {
                eprintln!(
                    "[ramsharedd] conn: WRITE len {} exceeds export; disconnecting",
                    req.len
                );
                break;
            }
            // Anti-DoS: a WRITE can never exceed the physical IPC buffer upper bound (16 MiB).
            if req.cmd == Command::Write && req.len > 16 * 1024 * 1024 {
                eprintln!(
                    "[ramsharedd] conn: WRITE len {} exceeds 16 MiB IPC limit; disconnecting",
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
                    eprintln!("[ramsharedd] accept failed: {e}");
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
                    eprintln!("[ramsharedd] TCP accept failed: {e}");
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
    use ramshared_block::handshake::NBD_OPT_EXPORT_NAME;
    use ramshared_block::protocol::{IHAVEOPT, NBD_REQUEST_MAGIC};
    use std::io::{self, Cursor};
    use std::net::{Shutdown, TcpStream};
    use std::os::unix::net::{UnixListener as TestUnixListener, UnixStream as TestUnixStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{TryRecvError, sync_channel};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    static SOCKET_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[derive(Default)]
    struct WriterState {
        bytes: Mutex<Vec<u8>>,
        writes: AtomicUsize,
        flushes: AtomicUsize,
    }

    #[derive(Clone, Copy)]
    enum WriterFailure {
        Never,
        WriteAt(usize),
        Flush,
    }

    struct TestWriter {
        state: Arc<WriterState>,
        failure: WriterFailure,
    }

    impl TestWriter {
        fn new(state: Arc<WriterState>, failure: WriterFailure) -> Self {
            Self { state, failure }
        }
    }

    impl Write for TestWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let write_index = self.state.writes.fetch_add(1, Ordering::SeqCst);
            if let WriterFailure::WriteAt(expected) = self.failure
                && write_index == expected
            {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "test write failure",
                ));
            }
            self.state.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.state.flushes.fetch_add(1, Ordering::SeqCst);
            if matches!(self.failure, WriterFailure::Flush) {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "test flush failure",
                ));
            }
            Ok(())
        }
    }

    fn join_with_deadline(handle: JoinHandle<()>) {
        let (done_tx, done_rx) = sync_channel(1);
        std::thread::spawn(move || {
            let _ = done_tx.send(handle.join().is_ok());
        });
        assert!(
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap_or(false),
            "connection thread must terminate within the bounded test deadline"
        );
    }

    fn recv_with_deadline<T>(rx: &Receiver<T>) -> T {
        rx.recv_timeout(Duration::from_secs(1)).unwrap_or_else(|_| {
            panic!("connection message must arrive within the bounded test deadline")
        })
    }

    fn assert_only_closed(rx: &Receiver<WMsg>) {
        assert!(matches!(recv_with_deadline(rx), WMsg::Closed));
        assert!(matches!(
            rx.try_recv(),
            Err(TryRecvError::Empty | TryRecvError::Disconnected)
        ));
    }

    fn one_export(size: u64) -> Arc<Vec<Export>> {
        Arc::new(vec![Export {
            name: "default".to_string(),
            size,
        }])
    }

    fn export_name_handshake(name: &[u8]) -> Vec<u8> {
        let mut wire = Vec::new();
        wire.extend_from_slice(&(1u32 << 1).to_be_bytes()); // NBD_FLAG_C_NO_ZEROES
        wire.extend_from_slice(&IHAVEOPT.to_be_bytes());
        wire.extend_from_slice(&NBD_OPT_EXPORT_NAME.to_be_bytes());
        wire.extend_from_slice(&(name.len() as u32).to_be_bytes());
        wire.extend_from_slice(name);
        wire
    }

    fn request_bytes(command: u16, handle: u64, len: u32, magic: u32) -> [u8; REQUEST_LEN] {
        let mut wire = [0u8; REQUEST_LEN];
        wire[0..4].copy_from_slice(&magic.to_be_bytes());
        wire[4..6].copy_from_slice(&0u16.to_be_bytes());
        wire[6..8].copy_from_slice(&command.to_be_bytes());
        wire[8..16].copy_from_slice(&handle.to_be_bytes());
        wire[16..24].copy_from_slice(&0u64.to_be_bytes());
        wire[24..28].copy_from_slice(&len.to_be_bytes());
        wire
    }

    fn reply(header: u8, data: &[u8], disconnect: bool) -> Reply {
        Reply {
            reply: [header; SIMPLE_REPLY_LEN],
            data: data.to_vec(),
            disconnect,
        }
    }

    fn socket_path(label: &str) -> PathBuf {
        let suffix = SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "ramshared-conn-{label}-{}-{suffix}.sock",
            std::process::id()
        ))
    }

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
            "must reject beyond the cap (backpressure)"
        );
    }

    // DT-18 / F-3/F-5: deterministic zero-live transition.
    #[test]
    fn live_count_reports_zero_after_all_closed() {
        let mut lc = LiveCount::new();
        lc.on_open(); // live=1
        lc.on_open(); // live=2
        assert!(!lc.on_close(), "live=1 still"); // live=1
        assert!(lc.on_close(), "live=0 emits the quiescent transition"); // live=0
    }

    // DT-18 / F-6: failed handshake = immediate Opened (acceptor) + Closed (reader); balanced.
    #[test]
    fn live_count_balanced_open_then_close() {
        let mut lc = LiveCount::new();
        lc.on_open();
        assert!(lc.on_close(), "1 connection opened and closed -> zero live");
    }

    #[test]
    fn live_count_never_stops_before_any_open() {
        let mut lc = LiveCount::new();
        assert!(
            !lc.on_close(),
            "without Opened does not transition spuriously"
        );
        assert_eq!(lc.live(), 0);
    }

    #[test]
    fn live_count_refuses_duplicate_closed() {
        let mut lc = LiveCount::new();
        lc.on_open();
        assert!(lc.on_close(), "the balanced close reaches zero live");
        assert!(
            !lc.on_close(),
            "a duplicate Closed must not emit another zero-live transition"
        );
        assert_eq!(lc.live(), 0);
    }

    #[test]
    fn writer_writes_header_data_and_flushes() {
        let state = Arc::new(WriterState::default());
        let (tx, rx) = channel();
        let writer = spawn_writer(
            TestWriter::new(Arc::clone(&state), WriterFailure::Never),
            rx,
        );
        tx.send(reply(0x11, &[0x22, 0x33], false)).unwrap();
        drop(tx);
        join_with_deadline(writer);

        let mut expected = vec![0x11; SIMPLE_REPLY_LEN];
        expected.extend_from_slice(&[0x22, 0x33]);
        assert_eq!(*state.bytes.lock().unwrap(), expected);
        assert_eq!(state.flushes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn writer_stops_on_disconnect_or_io_error() {
        let disconnect_state = Arc::new(WriterState::default());
        let (disconnect_tx, disconnect_rx) = channel();
        let disconnect_writer = spawn_writer(
            TestWriter::new(Arc::clone(&disconnect_state), WriterFailure::Never),
            disconnect_rx,
        );
        disconnect_tx.send(reply(0x44, &[0x55], true)).unwrap();
        disconnect_tx.send(reply(0x66, &[0x77], false)).unwrap();
        drop(disconnect_tx);
        join_with_deadline(disconnect_writer);
        let mut disconnect_expected = vec![0x44; SIMPLE_REPLY_LEN];
        disconnect_expected.push(0x55);
        assert_eq!(*disconnect_state.bytes.lock().unwrap(), disconnect_expected);
        assert_eq!(disconnect_state.flushes.load(Ordering::SeqCst), 1);

        let write_error_state = Arc::new(WriterState::default());
        let (write_error_tx, write_error_rx) = channel();
        let write_error_writer = spawn_writer(
            TestWriter::new(Arc::clone(&write_error_state), WriterFailure::WriteAt(1)),
            write_error_rx,
        );
        write_error_tx.send(reply(0x88, &[0x99], false)).unwrap();
        drop(write_error_tx);
        join_with_deadline(write_error_writer);
        assert_eq!(
            *write_error_state.bytes.lock().unwrap(),
            vec![0x88; SIMPLE_REPLY_LEN]
        );
        assert_eq!(write_error_state.flushes.load(Ordering::SeqCst), 0);

        let flush_error_state = Arc::new(WriterState::default());
        let (flush_error_tx, flush_error_rx) = channel();
        let flush_error_writer = spawn_writer(
            TestWriter::new(Arc::clone(&flush_error_state), WriterFailure::Flush),
            flush_error_rx,
        );
        flush_error_tx.send(reply(0xaa, &[], false)).unwrap();
        drop(flush_error_tx);
        join_with_deadline(flush_error_writer);
        assert_eq!(
            *flush_error_state.bytes.lock().unwrap(),
            vec![0xaa; SIMPLE_REPLY_LEN]
        );
        assert_eq!(flush_error_state.flushes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reader_enqueues_write_payload_then_closed() {
        let mut wire = export_name_handshake(b"");
        wire.extend_from_slice(&request_bytes(1, 0x0102_0304, 3, NBD_REQUEST_MAGIC));
        wire.extend_from_slice(&[7, 8, 9]);
        let (jobs_tx, jobs_rx) = sync_channel(2);
        let (reply_tx, _reply_rx) = channel();
        let reader = spawn_reader(
            Cursor::new(wire),
            TestWriter::new(Arc::new(WriterState::default()), WriterFailure::Never),
            one_export(4096),
            0,
            jobs_tx,
            reply_tx,
        );
        join_with_deadline(reader);

        match recv_with_deadline(&jobs_rx) {
            WMsg::Job(job) => {
                assert_eq!(job.export, 0);
                assert_eq!(job.req.cmd, Command::Write);
                assert_eq!(job.req.handle, 0x0102_0304);
                assert_eq!(job.payload, vec![7, 8, 9]);
            }
            _ => panic!("reader must enqueue the negotiated write job"),
        }
        assert_only_closed(&jobs_rx);
    }

    #[test]
    fn reader_refusal_and_eof_emit_closed() {
        let (refusal_tx, refusal_rx) = sync_channel(1);
        let (reply_tx, _reply_rx) = channel();
        let refusal_reader = spawn_reader(
            Cursor::new(export_name_handshake(b"missing")),
            TestWriter::new(Arc::new(WriterState::default()), WriterFailure::Never),
            one_export(4096),
            0,
            refusal_tx,
            reply_tx,
        );
        join_with_deadline(refusal_reader);
        assert_only_closed(&refusal_rx);

        let (eof_tx, eof_rx) = sync_channel(1);
        let (reply_tx, _reply_rx) = channel();
        let eof_reader = spawn_reader(
            Cursor::new(export_name_handshake(b"")),
            TestWriter::new(Arc::new(WriterState::default()), WriterFailure::Never),
            one_export(4096),
            0,
            eof_tx,
            reply_tx,
        );
        join_with_deadline(eof_reader);
        assert_only_closed(&eof_rx);
    }

    #[test]
    fn reader_malformed_and_oversized_write_emit_closed() {
        let mut malformed_wire = export_name_handshake(b"");
        malformed_wire.extend_from_slice(&request_bytes(0, 1, 0, 0));
        let (malformed_tx, malformed_rx) = sync_channel(1);
        let (reply_tx, _reply_rx) = channel();
        let malformed_reader = spawn_reader(
            Cursor::new(malformed_wire),
            TestWriter::new(Arc::new(WriterState::default()), WriterFailure::Never),
            one_export(4096),
            0,
            malformed_tx,
            reply_tx,
        );
        join_with_deadline(malformed_reader);
        assert_only_closed(&malformed_rx);

        let mut oversized_wire = export_name_handshake(b"");
        oversized_wire.extend_from_slice(&request_bytes(1, 2, 4097, NBD_REQUEST_MAGIC));
        let (oversized_tx, oversized_rx) = sync_channel(1);
        let (reply_tx, _reply_rx) = channel();
        let oversized_reader = spawn_reader(
            Cursor::new(oversized_wire),
            TestWriter::new(Arc::new(WriterState::default()), WriterFailure::Never),
            one_export(4096),
            0,
            oversized_tx,
            reply_tx,
        );
        join_with_deadline(oversized_reader);
        assert_only_closed(&oversized_rx);

        let mut over_ipc_limit_wire = export_name_handshake(b"");
        // 16 MiB + 1
        over_ipc_limit_wire.extend_from_slice(&request_bytes(1, 2, 16 * 1024 * 1024 + 1, NBD_REQUEST_MAGIC));
        let (over_ipc_tx, over_ipc_rx) = sync_channel(1);
        let (reply_tx, _reply_rx) = channel();
        let over_ipc_reader = spawn_reader(
            Cursor::new(over_ipc_limit_wire),
            TestWriter::new(Arc::new(WriterState::default()), WriterFailure::Never),
            one_export(32 * 1024 * 1024), // Export size is 32 MiB, so it doesn't fail the export size check
            0,
            over_ipc_tx,
            reply_tx,
        );
        join_with_deadline(over_ipc_reader);
        assert_only_closed(&over_ipc_rx);
    }

    #[test]
    fn reader_stops_when_worker_is_closed() {
        let mut wire = export_name_handshake(b"");
        wire.extend_from_slice(&request_bytes(0, 3, 0, NBD_REQUEST_MAGIC));
        let (jobs_tx, jobs_rx) = sync_channel(1);
        drop(jobs_rx);
        let (reply_tx, _reply_rx) = channel();
        let reader = spawn_reader(
            Cursor::new(wire),
            TestWriter::new(Arc::new(WriterState::default()), WriterFailure::Never),
            one_export(4096),
            0,
            jobs_tx,
            reply_tx,
        );
        join_with_deadline(reader);
    }

    #[test]
    fn wire_conn_balances_opened_and_closed() {
        let (server, mut client) = TestUnixStream::pair().unwrap();
        let writer = server.try_clone().unwrap();
        let hs_writer = server.try_clone().unwrap();
        let (jobs_tx, jobs_rx) = sync_channel(2);
        let exports = one_export(4096);
        assert!(wire_conn(server, writer, hs_writer, &exports, 0, &jobs_tx));
        assert!(matches!(recv_with_deadline(&jobs_rx), WMsg::Opened));

        client.write_all(&export_name_handshake(b"")).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut handshake_reply = [0u8; 28];
        client.read_exact(&mut handshake_reply).unwrap();
        assert_eq!(
            &handshake_reply[0..8],
            &ramshared_block::protocol::NBDMAGIC.to_be_bytes()
        );
        assert_only_closed(&jobs_rx);
    }

    #[test]
    fn acceptors_stop_when_worker_is_closed() {
        let unix_path = socket_path("acceptor");
        let _ = std::fs::remove_file(&unix_path);
        let unix_listener = TestUnixListener::bind(&unix_path).unwrap();
        let (unix_jobs_tx, unix_jobs_rx) = sync_channel(1);
        drop(unix_jobs_rx);
        let unix_acceptor = spawn_acceptor(unix_listener, one_export(4096), 0, unix_jobs_tx);
        let unix_client = TestUnixStream::connect(&unix_path).unwrap();
        join_with_deadline(unix_acceptor);
        drop(unix_client);
        std::fs::remove_file(&unix_path).unwrap();

        let tcp_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let tcp_addr = tcp_listener.local_addr().unwrap();
        let (tcp_jobs_tx, tcp_jobs_rx) = sync_channel(1);
        drop(tcp_jobs_rx);
        let tcp_acceptor = spawn_acceptor_tcp(tcp_listener, one_export(4096), 0, tcp_jobs_tx);
        let tcp_client = TcpStream::connect(tcp_addr).unwrap();
        join_with_deadline(tcp_acceptor);
        drop(tcp_client);
    }

    // DT-7 / DT-18: unbounded replica — worker progresses even with the writer stopped.
    // If the replica were bounded and the writer did not drain, the worker would block →
    // Jobs channel would fill up → reader would block → deadlock (this test would hang).
    #[test]
    fn slow_writer_does_not_deadlock() {
        let (jobs_tx, jobs_rx) = sync_channel::<WMsg>(2); // small Jobs channel
        let (reply_tx, reply_rx) = channel::<Reply>(); // UNBOUNDED replica (DT-7)
        let _stalled_writer = reply_rx; // holds without draining (simulates a hung socket)

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
            "worker processed every job without deadlock"
        );
    }
}
