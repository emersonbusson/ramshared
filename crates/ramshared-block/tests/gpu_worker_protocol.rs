#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;

use ramshared_block::{
    FRAME_HEADER_LEN, FrameHeader, GpuWorkerConfig, IpcCacheClient,
    gpu_cache_worker::{MSG_HANDSHAKE_REQ, MSG_UPDATE, STATUS_OK},
    run_gpu_worker_loop, run_gpu_worker_loop_with_frame_read_timeout,
};
use ramshared_vram::{GpuBudgetSnapshot, VramError, VramMemory, VramProvider};

struct NoopMemory;

impl VramMemory for NoopMemory {
    fn len(&self) -> usize {
        0
    }

    fn zero(&mut self) -> Result<(), VramError> {
        Ok(())
    }

    fn read_at(&self, _off: u64, _dst: &mut [u8]) -> Result<(), VramError> {
        Err(VramError::OutOfMemory)
    }

    fn write_at(&mut self, _off: u64, _src: &[u8]) -> Result<(), VramError> {
        Err(VramError::OutOfMemory)
    }
}

struct NoopProvider;

impl VramProvider for NoopProvider {
    type Mem<'p> = NoopMemory;

    fn alloc(&self, _bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        Err(VramError::OutOfMemory)
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        Err(VramError::Provider("unused in invalid-frame test".into()))
    }

    fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
        Err(VramError::Provider("unused in invalid-frame test".into()))
    }
}

fn worker_config() -> GpuWorkerConfig {
    GpuWorkerConfig {
        target_bytes: 0,
        chunk_bytes: 4096,
        reserve_floor_bytes: 0,
    }
}

#[test]
fn worker_rejects_unknown_message_types_instead_of_silently_dropping_them() {
    let (mut client, worker_socket) = UnixStream::pair().expect("socketpair");
    let worker = std::thread::spawn(move || {
        run_gpu_worker_loop(worker_socket, NoopProvider, worker_config())
    });

    let invalid = FrameHeader {
        msg_type: u8::MAX,
        status: STATUS_OK,
        correlation_id: 1,
        offset: 0,
        payload_len: 0,
        aux: 0,
    };
    client
        .write_all(&invalid.encode())
        .expect("write invalid frame");
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set timeout");

    let mut response = [0; FRAME_HEADER_LEN];
    let response_len = client.read(&mut response);
    drop(client);
    let worker_result = worker.join().expect("worker thread");

    assert!(
        matches!(response_len, Ok(0)),
        "invalid frame must close the worker stream: {response_len:?}"
    );
    assert!(
        worker_result.is_err(),
        "invalid frame must fail the worker loop"
    );
}

#[test]
fn worker_rejects_nonzero_reserved_header_bytes() {
    let (mut client, worker_socket) = UnixStream::pair().expect("socketpair");
    let worker = std::thread::spawn(move || {
        run_gpu_worker_loop(worker_socket, NoopProvider, worker_config())
    });

    let handshake = FrameHeader {
        msg_type: MSG_HANDSHAKE_REQ,
        status: STATUS_OK,
        correlation_id: 2,
        offset: 0,
        payload_len: 0,
        aux: 0,
    };
    let mut malformed = handshake.encode();
    malformed[2] = 1;
    client.write_all(&malformed).expect("write malformed frame");
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set timeout");

    let mut response = [0; FRAME_HEADER_LEN];
    let response_len = client.read(&mut response);
    drop(client);
    let worker_result = worker.join().expect("worker thread");

    assert!(
        matches!(response_len, Ok(0)),
        "malformed header must close the worker stream: {response_len:?}"
    );
    assert!(
        worker_result.is_err(),
        "malformed header must fail the worker loop"
    );
}

#[test]
fn client_refuses_nonzero_reserved_response_header_bytes() {
    let (client_socket, mut worker_socket) = UnixStream::pair().expect("socketpair");
    let fake_worker = std::thread::spawn(move || {
        let mut request = [0; FRAME_HEADER_LEN];
        worker_socket
            .read_exact(&mut request)
            .expect("read handshake request");
        let request = FrameHeader::decode(&request).expect("valid handshake request");
        let response = FrameHeader {
            msg_type: ramshared_block::gpu_cache_worker::MSG_HANDSHAKE_RESP,
            status: STATUS_OK,
            correlation_id: request.correlation_id,
            offset: 4096,
            payload_len: 0,
            aux: 0,
        };
        let mut malformed = response.encode();
        malformed[7] = 1;
        worker_socket
            .write_all(&malformed)
            .expect("write malformed response");
    });
    let mut client = IpcCacheClient::new(client_socket, Duration::from_secs(1), 4096);

    assert!(client.perform_handshake().is_err());
    fake_worker.join().expect("fake worker thread");
}

const STALL_FRAME_READ_TIMEOUT: Duration = Duration::from_millis(100);
const STALL_WATCHDOG: Duration = Duration::from_secs(5);

fn spawn_worker_with_stall_budget(worker_socket: UnixStream) -> mpsc::Receiver<Result<(), String>> {
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = run_gpu_worker_loop_with_frame_read_timeout(
            worker_socket,
            NoopProvider,
            worker_config(),
            STALL_FRAME_READ_TIMEOUT,
        );
        let _ = done_tx.send(result);
    });
    done_rx
}

fn expect_stalled_worker_fails_closed(
    done_rx: mpsc::Receiver<Result<(), String>>,
    case: &str,
    client: UnixStream,
) {
    let result = done_rx
        .recv_timeout(STALL_WATCHDOG)
        .unwrap_or_else(|_| panic!("worker must not block forever on {case}"));
    drop(client);
    let error = result.expect_err("stalled peer must fail closed");
    assert!(
        error.to_ascii_lowercase().contains("deadline"),
        "stall failure must report the read deadline for {case}: {error}"
    );
}

#[test]
fn worker_frame_read_deadline_fails_closed_on_a_stalled_partial_header() {
    let (mut client, worker_socket) = UnixStream::pair().expect("socketpair");
    let done_rx = spawn_worker_with_stall_budget(worker_socket);

    let handshake = FrameHeader {
        msg_type: MSG_HANDSHAKE_REQ,
        status: STATUS_OK,
        correlation_id: 1,
        offset: 0,
        payload_len: 0,
        aux: 0,
    };
    let encoded = handshake.encode();
    client
        .write_all(&encoded[..FRAME_HEADER_LEN / 2])
        .expect("write stalled partial header");

    expect_stalled_worker_fails_closed(done_rx, "a stalled partial header", client);
}

#[test]
fn worker_frame_read_deadline_fails_closed_on_a_stalled_partial_payload() {
    let (mut client, worker_socket) = UnixStream::pair().expect("socketpair");
    let done_rx = spawn_worker_with_stall_budget(worker_socket);

    let update = FrameHeader {
        msg_type: MSG_UPDATE,
        status: STATUS_OK,
        correlation_id: 1,
        offset: 0,
        payload_len: 4,
        aux: 0,
    };
    client
        .write_all(&update.encode())
        .expect("write frame header");
    client.write_all(&[1, 2]).expect("write half the payload");

    expect_stalled_worker_fails_closed(done_rx, "a stalled partial payload", client);
}

#[test]
fn worker_frame_read_deadline_fails_closed_on_a_silent_peer() {
    let (client, worker_socket) = UnixStream::pair().expect("socketpair");
    let done_rx = spawn_worker_with_stall_budget(worker_socket);

    expect_stalled_worker_fails_closed(done_rx, "a silent peer", client);
}
