//! Test of worker DT-3 (`spawn_ublk_worker`) — validates the "worker" half of the
//! architecture (`IoWork`/`WorkerReply` channels + service against a `BlockBackend`)
//! without ublk device and without GPU.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test: unwrap/expect is idiomatic

use ramshared_block::{BlockBackend, Command, Request};
use ramshared_wsl2d::{RamBackend, ublk, ublk_server};
use std::sync::mpsc;

fn work(tag: u16, cmd: Command, offset: u64, len: u32, payload: Vec<u8>) -> ublk::IoWork {
    ublk::IoWork {
        qid: 0,
        tag,
        buffer_addr: 0,
        req: Request {
            flags: 0,
            cmd,
            handle: u64::from(tag),
            offset,
            len,
        },
        payload,
    }
}

#[test]
fn ublk_worker_serves_read_and_write_over_channels() {
    let mut backend = RamBackend::new(8192);
    let pattern: Vec<u8> = (0..512u32).map(|i| (i % 251) as u8).collect();
    backend
        .write_at(1024, &pattern)
        .expect("pre-load sector 2");

    let (work_tx, work_rx) = mpsc::sync_channel::<ublk::IoWork>(8);
    let (reply_tx, reply_rx) = mpsc::channel::<ublk_server::WorkerReply>();
    let worker = ublk_server::spawn_ublk_worker(backend, work_rx, reply_tx);

    // READ of sector 2: the ring owner yields a buffer sized to `len`; the worker
    // fills it in-place and returns it in `buf` (no alloc in worker — DT-8).
    work_tx
        .send(work(3, Command::Read, 1024, 512, vec![0u8; 512]))
        .expect("envia READ");
    let reply = reply_rx.recv().expect("reply READ");
    assert_eq!(reply.tag, 3);
    assert_eq!(reply.result, 512);
    assert!(reply.is_read);
    assert_eq!(reply.buf, pattern);

    // WRITE: the payload (yielded buffer) already brings the data; they go to the backend and the
    // same buffer returns in `buf` for the ring owner to recycle.
    let pattern2: Vec<u8> = (0..512u32).map(|i| ((i * 3 + 1) % 251) as u8).collect();
    work_tx
        .send(work(4, Command::Write, 2048, 512, pattern2.clone()))
        .expect("envia WRITE");
    let reply = reply_rx.recv().expect("reply WRITE");
    assert_eq!(reply.tag, 4);
    assert_eq!(reply.result, 512);
    assert!(!reply.is_read);
    assert_eq!(reply.buf.len(), 512); // buffer returned for recycling

    // READ back from sector 4 confirms the WRITE.
    work_tx
        .send(work(5, Command::Read, 2048, 512, vec![0u8; 512]))
        .expect("envia READ 2");
    let r2 = reply_rx.recv().expect("reply READ 2");
    assert!(r2.is_read);
    assert_eq!(r2.buf, pattern2);

    // Closing the work channel terminates the worker, which returns the backend.
    drop(work_tx);
    let backend = worker.join().expect("worker terminated ok");
    let mut got = vec![0u8; 512];
    backend
        .read_at(2048, &mut got)
        .expect("reads the returned backend");
    assert_eq!(got, pattern2);
}
