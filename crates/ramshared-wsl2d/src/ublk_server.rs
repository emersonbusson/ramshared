//! Backend de RAM e lógica de serviço de I/O para o loop ublk.
//!
//! `serve_request` é puro: dado o `IoDesc` do request e o buffer da tag, serve
//! contra o backend e devolve o `result` (bytes `>= 0`, ou `-errno`) que o COMMIT
//! deve carregar. `RamBackend` é um disco volátil em memória para validar o loop
//! end-to-end antes de ligar VRAM/swap.

use crate::ublk;

const EIO: i32 = -5;
const EINVAL: i32 = -22;

/// Disco volátil em memória (`Vec<u8>`), endereçado por byte.
pub struct RamBackend {
    data: Vec<u8>,
}

impl RamBackend {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0u8; size],
        }
    }

    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    /// READ: copia `data[offset..offset+buf.len()]` para `buf`. `-EIO` fora do range.
    pub fn read_into(&self, offset: u64, buf: &mut [u8]) -> i32 {
        match self.range(offset, buf.len()) {
            Some((start, end)) => {
                buf.copy_from_slice(&self.data[start..end]);
                buf.len() as i32
            }
            None => EIO,
        }
    }

    /// WRITE: copia `buf` para `data[offset..]`. `-EIO` fora do range.
    pub fn write_from(&mut self, offset: u64, buf: &[u8]) -> i32 {
        match self.range(offset, buf.len()) {
            Some((start, end)) => {
                self.data[start..end].copy_from_slice(buf);
                buf.len() as i32
            }
            None => EIO,
        }
    }

    fn range(&self, offset: u64, len: usize) -> Option<(usize, usize)> {
        let start = usize::try_from(offset).ok()?;
        let end = start.checked_add(len)?;
        (end <= self.data.len()).then_some((start, end))
    }
}

/// Serve um request ublk contra `backend` usando `buf` (o buffer da tag) e devolve
/// o `result` do COMMIT: bytes transferidos (`>= 0`) ou `-errno`.
///
/// Em WRITE o kernel já copiou os dados do bio para `buf`; em READ o backend
/// preenche `buf` e o kernel copia `result` bytes de volta no COMMIT — por isso
/// `result` precisa ser exatamente os bytes servidos.
pub fn serve_request(backend: &mut RamBackend, iod: &ublk::IoDesc, buf: &mut [u8]) -> i32 {
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

    match iod.operation() {
        ublk::UBLK_IO_OP_READ => backend.read_into(offset, &mut buf[..len]),
        ublk::UBLK_IO_OP_WRITE => backend.write_from(offset, &buf[..len]),
        ublk::UBLK_IO_OP_FLUSH => 0,
        _ => EINVAL,
    }
}
