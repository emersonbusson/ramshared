use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use ramshared_block::{
    FRAME_HEADER_LEN, FrameHeader, GpuWorkerConfig, run_gpu_worker_loop,
    gpu_cache_worker::{MSG_HANDSHAKE_REQ, STATUS_OK},
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
    client.write_all(&invalid.encode()).expect("write invalid frame");
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set timeout");

    let mut response = [0; FRAME_HEADER_LEN];
    let response_len = client.read(&mut response);
    drop(client);
    let worker_result = worker.join().expect("worker thread");

    assert!(matches!(response_len, Ok(0)), "invalid frame must close the worker stream: {response_len:?}");
    assert!(worker_result.is_err(), "invalid frame must fail the worker loop");
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

    assert!(matches!(response_len, Ok(0)), "malformed header must close the worker stream: {response_len:?}");
    assert!(worker_result.is_err(), "malformed header must fail the worker loop");
}
