//! Backend de RAM e lógica de serviço de I/O para o loop ublk.
//!
//! `serve_request` é puro: dado o `IoDesc` do request e o buffer da tag, serve
//! contra o backend e devolve o `result` (bytes `>= 0`, ou `-errno`) que o COMMIT
//! deve carregar. `RamBackend` é um disco volátil em memória para validar o loop
//! end-to-end antes de ligar VRAM/swap.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ramshared_block::{BlockBackend, IoError};

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

/// Serve um request ublk contra qualquer [`BlockBackend`] usando `buf` (o buffer da
/// tag) e devolve o `result` do COMMIT: bytes transferidos (`>= 0`) ou `-errno`.
/// Serve **in-place** no buffer (sem alloc no hot path — DT-8), diferente do
/// `serve()` NBD que aloca um `Vec`.
///
/// Em WRITE o kernel já copiou os dados do bio para `buf`; em READ o backend
/// preenche `buf` e o kernel copia `result` bytes de volta no COMMIT — por isso
/// `result` precisa ser exatamente os bytes servidos.
pub fn serve_request<B: BlockBackend + ?Sized>(
    backend: &mut B,
    iod: &ublk::IoDesc,
    buf: &mut [u8],
) -> i32 {
    let sector = ublk::UBLK_SECTOR_SIZE as usize;
    let len = match usize::try_from(iod.nr_sectors_or_zones)
        .ok()
        .and_then(|n| n.checked_mul(sector))
    {
        Some(len) if len <= buf.len() => len,
        _ => return EINVAL, // request ausente, overflow ou maior que o buffer da tag
    };
    let offset = match iod.start_sector.checked_mul(ublk::UBLK_SECTOR_SIZE) {
        Some(off) => off,
        None => return EINVAL,
    };

    let served = match iod.operation() {
        ublk::UBLK_IO_OP_READ => backend.read_at(offset, &mut buf[..len]).map(|()| len),
        ublk::UBLK_IO_OP_WRITE => backend.write_at(offset, &buf[..len]).map(|()| len),
        ublk::UBLK_IO_OP_FLUSH => backend.flush().map(|()| 0),
        _ => return EINVAL,
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
            let result = serve_request(&mut backend, &iod, server.buffer_mut(completion.tag));
            server.commit_and_fetch(completion.tag, result)?;
        }
    }
}
