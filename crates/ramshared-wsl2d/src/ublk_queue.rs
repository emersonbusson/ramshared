//! Preparação da fila ublk no char device `/dev/ublkcN`.
//!
//! Mapeia o buffer de io-desc (somente leitura) que o kernel expõe e decodifica
//! descritores por tag. Não chama `START_DEV`, não cria `/dev/ublkbN` e não toca
//! swap. O `unsafe` do `mmap` fica isolado em `ramshared-uring`.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use crate::ublk;

/// Mapeia a fila 0 do char device `char_path` (read-only) e decodifica o
/// `ublksrv_io_desc` da `tag`. O tamanho do mapa é `round_up(queue_depth * 24, page)`
/// e o offset é 0 (fila 0); filas adicionais exigem `ublk_max_cmd_buf_size` — ver
/// `docs/ublk-backend/SPEC-ring-loop.md` §3.
pub fn read_io_desc(
    char_path: impl AsRef<Path>,
    queue_depth: u16,
    tag: u16,
) -> io::Result<ublk::IoDesc> {
    if tag >= queue_depth {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tag must be < queue_depth",
        ));
    }

    let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
    let len = ramshared_uring::round_up_to_page(usize::from(queue_depth) * ublk::UBLK_IO_DESC_SIZE);
    let map = ramshared_uring::MmapRo::map_readonly(char_dev.as_raw_fd(), len, 0)?;

    let start = usize::from(tag) * ublk::UBLK_IO_DESC_SIZE;
    let end = start + ublk::UBLK_IO_DESC_SIZE;
    let bytes = map.as_bytes();
    if end > bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "io-desc fora do buffer mapeado",
        ));
    }

    ublk::IoDesc::from_ne_bytes(&bytes[start..end])
        .ok_or_else(|| io::Error::other("io-desc menor que 24 bytes"))
}

/// Sessão de FETCH em uma fila ublk: segura o `File` do char device `/dev/ublkcN` e
/// o ring `ramshared-uring` que submeteu os `FETCH_REQ`. Não chama `START_DEV`, não
/// cria `/dev/ublkbN` e não toca swap. O ring é dropado antes do `File` (o fd
/// precisa seguir aberto enquanto o ring existe).
pub struct FetchSession {
    ring: ramshared_uring::UblkFetchRing,
    /// `File` do char device, mantido aberto enquanto o ring vive (drop guard).
    #[allow(dead_code)]
    char_dev: File,
}

impl FetchSession {
    /// Abre `char_path`, submete `FETCH_REQ` para as `queue_depth` tags da fila 0
    /// (buffer de `buf_size` por tag) e retorna sem esperar CQE.
    pub fn open(
        char_path: impl AsRef<Path>,
        queue_depth: u16,
        buf_size: usize,
    ) -> io::Result<Self> {
        let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
        let ring = ramshared_uring::UblkFetchRing::submit_fetch_all(
            char_dev.as_raw_fd(),
            queue_depth,
            buf_size,
        )?;

        Ok(Self { ring, char_dev })
    }

    /// Drena os CQEs disponíveis (não bloqueia).
    pub fn drain(&mut self) -> Vec<ramshared_uring::UblkCompletion> {
        self.ring.drain()
    }
}
