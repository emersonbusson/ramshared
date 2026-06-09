//! Backend de RAM e lógica de serviço de I/O para o loop ublk.
//!
//! `serve_request` é puro: dado um `Request` e o buffer de dados, serve contra um
//! `BlockBackend` e devolve o `result` (bytes `>= 0`, ou `-errno`) que o COMMIT
//! deve carregar. `RamBackend` valida o loop sem CUDA; `spawn_ublk_worker` é o
//! worker DT-3 (thread dona do backend) que será usado com o `VramBackend`.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ramshared_block::{BlockBackend, Command, IoError, Request};

use crate::ublk;

const EIO: i32 = -5;
const EINVAL: i32 = -22;

/// Disco volátil em memória que implementa [`BlockBackend`] — valida o loop ublk
/// sem CUDA. O backend de produção é o `VramBackend` (mesmo trait), então o loop
/// serve qualquer um dos dois sem mudança.
pub struct RamBackend {
    data: Vec<u8>,
    block_size: u32,
}

impl RamBackend {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0u8; size],
            block_size: ublk::UBLK_SECTOR_SIZE as u32,
        }
    }

    fn range(&self, off: u64, len: usize) -> Option<(usize, usize)> {
        let start = usize::try_from(off).ok()?;
        let end = start.checked_add(len)?;
        (end <= self.data.len()).then_some((start, end))
    }
}

impl BlockBackend for RamBackend {
    fn size_bytes(&self) -> u64 {
        self.data.len() as u64
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
        let (start, end) = self
            .range(off, buf.len())
            .ok_or_else(|| IoError("RamBackend read out of range".into()))?;
        buf.copy_from_slice(&self.data[start..end]);
        Ok(())
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        let (start, end) = self
            .range(off, data.len())
            .ok_or_else(|| IoError("RamBackend write out of range".into()))?;
        self.data[start..end].copy_from_slice(data);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}

/// Serve um `Request` ublk contra qualquer [`BlockBackend`] usando `buf` (onde os
/// dados vivem) e devolve o `result` do COMMIT: bytes transferidos (`>= 0`) ou
/// `-errno`. Serve **in-place** no buffer (sem alloc no hot path — DT-8). `buf` é o
/// buffer da tag no loop single-thread, ou um buffer do worker no DT-3.
///
/// Em WRITE `buf` já traz os dados (o kernel copiou do bio); em READ o backend
/// preenche `buf` e o kernel copia `result` bytes de volta no COMMIT — por isso
/// `result` precisa ser exatamente os bytes servidos.
pub fn serve_request<B: BlockBackend + ?Sized>(
    req: &Request,
    backend: &mut B,
    buf: &mut [u8],
) -> i32 {
    let len = req.len as usize;
    if len > buf.len() {
        return EINVAL; // request maior que o buffer disponível
    }

    let served = match req.cmd {
        Command::Read => backend.read_at(req.offset, &mut buf[..len]).map(|()| len),
        Command::Write => backend.write_at(req.offset, &buf[..len]).map(|()| len),
        Command::Flush => backend.flush().map(|()| 0),
        Command::Trim => return 0, // descarte: no-op seguro no MVP
        Command::Disc | Command::Unknown(_) => return EINVAL,
    };

    match served {
        Ok(bytes) => i32::try_from(bytes).unwrap_or(EIO),
        Err(_) => EIO,
    }
}

/// Handle da thread servidora ublk; `join` aguarda o loop terminar (ao receber o
/// abort do STOP/DEL_DEV) e devolve o `RamBackend` para inspeção.
pub struct ServerHandle {
    thread: JoinHandle<io::Result<RamBackend>>,
}

impl ServerHandle {
    pub fn join(self) -> io::Result<RamBackend> {
        match self.thread.join() {
            Ok(result) => result,
            Err(_) => Err(io::Error::other("server thread panicked")),
        }
    }
}

/// Abre `char_path`, cria o `UblkServer` e roda o loop de serviço numa thread
/// própria (dona única do ring, DT-3). A thread submete FETCH, serve cada request
/// contra `backend` e re-arma via COMMIT_AND_FETCH; encerra ao receber o abort
/// (`UBLK_IO_RES_ABORT`) que o STOP/DEL_DEV dispara.
pub fn spawn_server(
    char_path: impl AsRef<Path>,
    queue_depth: u16,
    buf_size: usize,
    backend: RamBackend,
) -> io::Result<ServerHandle> {
    let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
    let server = ramshared_uring::UblkServer::new(char_dev.as_raw_fd(), queue_depth, buf_size)?;

    let thread = thread::spawn(move || {
        // Mantém o char device aberto enquanto o loop usa o ring (dropado depois).
        let _char_dev = char_dev;
        run_server_loop(server, backend)
    });

    Ok(ServerHandle { thread })
}

fn run_server_loop(
    mut server: ramshared_uring::UblkServer,
    mut backend: RamBackend,
) -> io::Result<RamBackend> {
    server.submit_initial_fetch()?;

    loop {
        let completions = server.drain();
        if completions.is_empty() {
            thread::sleep(Duration::from_micros(200));
            continue;
        }

        for completion in completions {
            if completion.result == ublk::UBLK_IO_RES_ABORT {
                return Ok(backend); // teardown: STOP/DEL_DEV abortou os FETCH
            }
            if completion.result < 0 {
                return Err(io::Error::other(format!(
                    "FETCH falhou: {}",
                    completion.result
                )));
            }

            // result == UBLK_IO_RES_OK (0): ha um request pronto na tag.
            let iod = ublk::IoDesc::from_ne_bytes(server.io_desc_bytes(completion.tag))
                .ok_or_else(|| io::Error::other("io-desc invalido no mmap"))?;
            let result = match iod.to_block_request(completion.tag) {
                Ok(req) => serve_request(&req, &mut backend, server.buffer_mut(completion.tag)),
                Err(_) => EINVAL, // op ublk sem equivalencia segura
            };
            server.commit_and_fetch(completion.tag, result)?;
        }
    }
}

/// Resposta do worker DT-3 para o ring owner: `result` do COMMIT e os dados de um
/// READ (que o ring owner copia no buffer da tag antes de `commit_and_fetch`).
#[derive(Clone, Debug)]
pub struct WorkerReply {
    pub qid: u16,
    pub tag: u16,
    pub result: i32,
    pub read_data: Vec<u8>,
}

/// Handle da thread worker DT-3; `join` aguarda o worker encerrar (quando o canal de
/// `IoWork` fecha) e devolve o backend.
pub struct WorkerHandle<B> {
    thread: JoinHandle<B>,
}

impl<B> WorkerHandle<B> {
    pub fn join(self) -> io::Result<B> {
        self.thread
            .join()
            .map_err(|_| io::Error::other("ublk worker panicked"))
    }
}

/// Sobe o worker DT-3: a thread dona do `backend` (a única a tocar a VRAM/CUDA).
/// Recebe `IoWork` pelo canal, serve contra o `backend` e devolve `WorkerReply`.
/// Encerra quando `work_rx` fecha (o ring owner caiu) ou `reply_tx` quebra.
pub fn spawn_ublk_worker<B: BlockBackend + Send + 'static>(
    backend: B,
    work_rx: Receiver<ublk::IoWork>,
    reply_tx: Sender<WorkerReply>,
) -> WorkerHandle<B> {
    let thread = thread::spawn(move || worker_loop(backend, work_rx, reply_tx));
    WorkerHandle { thread }
}

fn worker_loop<B: BlockBackend>(
    mut backend: B,
    work_rx: Receiver<ublk::IoWork>,
    reply_tx: Sender<WorkerReply>,
) -> B {
    while let Ok(mut work) = work_rx.recv() {
        let (result, read_data) = match work.req.cmd {
            Command::Read => {
                // O worker possui o buffer de leitura; o ring owner copia para a tag.
                let mut buf = vec![0u8; work.req.len as usize];
                let result = serve_request(&work.req, &mut backend, &mut buf);
                if result >= 0 {
                    (result, buf)
                } else {
                    (result, Vec::new())
                }
            }
            // WRITE/FLUSH/TRIM: os dados (se houver) já vêm no payload.
            _ => {
                let result = serve_request(&work.req, &mut backend, &mut work.payload);
                (result, Vec::new())
            }
        };

        let reply = WorkerReply {
            qid: work.qid,
            tag: work.tag,
            result,
            read_data,
        };
        if reply_tx.send(reply).is_err() {
            break; // ring owner caiu
        }
    }

    backend
}
