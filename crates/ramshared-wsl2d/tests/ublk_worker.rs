//! Teste do worker DT-3 (`spawn_ublk_worker`) — valida a metade "worker" da
//! arquitetura (canais `IoWork`/`WorkerReply` + serviço contra um `BlockBackend`)
//! sem device ublk e sem GPU.

use ramshared_block::{BlockBackend, Command, Request};
use ramshared_wsl2d::{ublk, ublk_server};
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
    let mut backend = ublk_server::RamBackend::new(8192);
    let pattern: Vec<u8> = (0..512u32).map(|i| (i % 251) as u8).collect();
    backend
        .write_at(1024, &pattern)
        .expect("pre-carrega setor 2");

    let (work_tx, work_rx) = mpsc::sync_channel::<ublk::IoWork>(8);
    let (reply_tx, reply_rx) = mpsc::channel::<ublk_server::WorkerReply>();
    let worker = ublk_server::spawn_ublk_worker(backend, work_rx, reply_tx);

    // READ do setor 2: o ring owner cede um buffer dimensionado a `len`; o worker
    // o preenche in-place e o devolve em `buf` (sem alloc no worker — DT-8).
    work_tx
        .send(work(3, Command::Read, 1024, 512, vec![0u8; 512]))
        .expect("envia READ");
    let reply = reply_rx.recv().expect("reply READ");
    assert_eq!(reply.tag, 3);
    assert_eq!(reply.result, 512);
    assert!(reply.is_read);
    assert_eq!(reply.buf, pattern);

    // WRITE: o payload (buffer cedido) já traz os dados; vão para o backend e o
    // mesmo buffer volta em `buf` para o ring owner reciclar.
    let pattern2: Vec<u8> = (0..512u32).map(|i| ((i * 3 + 1) % 251) as u8).collect();
    work_tx
        .send(work(4, Command::Write, 2048, 512, pattern2.clone()))
        .expect("envia WRITE");
    let reply = reply_rx.recv().expect("reply WRITE");
    assert_eq!(reply.tag, 4);
    assert_eq!(reply.result, 512);
    assert!(!reply.is_read);
    assert_eq!(reply.buf.len(), 512); // buffer devolvido para reciclagem

    // READ de volta o setor 4 confirma o WRITE.
    work_tx
        .send(work(5, Command::Read, 2048, 512, vec![0u8; 512]))
        .expect("envia READ 2");
    let r2 = reply_rx.recv().expect("reply READ 2");
    assert!(r2.is_read);
    assert_eq!(r2.buf, pattern2);

    // Fechar o canal de work encerra o worker, que devolve o backend.
    drop(work_tx);
    let backend = worker.join().expect("worker terminou ok");
    let mut got = vec![0u8; 512];
    backend
        .read_at(2048, &mut got)
        .expect("le o backend devolvido");
    assert_eq!(got, pattern2);
}
