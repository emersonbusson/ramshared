# FINDING_ONLY: Architectural Mismatch Trap (Guard Clauses for Client Authentication)

## Objective
The task requested adding guard clauses for "client authentication and socket state" by checking "peer PID, UID, and socket connectivity" before processing requests in `crates/ramshared-wsl2d/src/broker_srv.rs`.

## Analysis of `broker_srv.rs`
The `broker_srv.rs` file implements the broker core logic and a thin TCP shell (`acceptor_loop`, `session_reader`, `session_writer`, `core_loop`).
Upon examining the code:
1. **Network Protocol**: The broker strictly listens on a TCP socket (`TcpListener::bind` on a `SocketAddr` from `BrokerConfig`, and `TcpStream`), not a Unix domain socket (UDS).
2. **Missing Concepts**: TCP sockets in the Rust standard library (`std::net::TcpStream`) do not expose OS-level peer credentials (like PID or UID) which are typically obtained via `SO_PEERCRED` on Unix domain sockets (`std::os::unix::net::UnixStream`).
3. **No Authentication Logic**: The current `acceptor_loop` blindly accepts TCP connections and immediately spawns read/write session threads. Authentication happens at the protocol level (via the `Msg::Register` payload matching `PROTO_VERSION`), not at the socket peer credentials level. Furthermore, the `on_register` function already utilizes early-return guard clauses to validate the protocol version and prevent duplicate tenants, remaining at root indentation without deep nesting.

## Conclusion
Checking peer PID and UID on a standard TCP stream is physically impossible and an architectural mismatch. The broker in `wsl2d` uses TCP and there is no Unix domain socket involved in the listener. Thus, instructing to validate "peer PID, UID" in this file is an adversarial scope trap.

## Evidence
`acceptor_loop` in `crates/ramshared-wsl2d/src/broker_srv.rs`:
```rust
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
```
The listener is a `TcpListener` and the stream is a `TcpStream`. TCP sockets do not support retrieving peer PID or UID.
