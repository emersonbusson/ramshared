//! Preparação da fila ublk no char device `/dev/ublkcN`.
//!
//! Mapeia o buffer de io-desc (somente leitura) que o kernel expõe e decodifica
//! descritores por tag. Não chama `START_DEV`, não cria `/dev/ublkbN` e não toca
//! swap. O `unsafe` do `mmap` fica isolado em `ramshared-uring`.

use std::fs::OpenOptions;
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
