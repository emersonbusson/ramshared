//! `VramBackend` — liga uma região de VRAM (`ramshared_vram::VramMemory`) à trait `BlockBackend`
//! do `ramshared_block`. É o ponto onde "VRAM" vira "block device NBD" (SPEC §8). Genérico sobre o
//! backend de VRAM (CUDA hoje via `ramshared-cuda`; Vulkan amanhã pelo mesmo trait — RF-G1).

use ramshared_block::{BlockBackend, IoError};
use ramshared_vram::{VramError, VramMemory};

use crate::ublk;

/// Block device respaldado por uma região de VRAM (`M: VramMemory`).
pub struct VramBackend<M> {
    mem: M,
    block_size: u32,
}

impl<M: VramMemory> VramBackend<M> {
    pub fn new(mem: M, block_size: u32) -> Self {
        Self { mem, block_size }
    }

    /// Zera toda a VRAM (SPEC §11 — zerar ao liberar/parar).
    pub fn zero(&mut self) -> Result<(), VramError> {
        self.mem.zero()
    }
}

impl<M: VramMemory> BlockBackend for VramBackend<M> {
    fn size_bytes(&self) -> u64 {
        self.mem.len() as u64
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
        self.mem
            .read_at(off, buf)
            .map_err(|e| IoError(e.to_string()))
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        self.mem
            .write_at(off, data)
            .map_err(|e| IoError(e.to_string()))
    }

    fn flush(&mut self) -> Result<(), IoError> {
        // cuMemcpy*_v2 são síncronas (a referência usa o mesmo modelo); nada a drenar.
        // A coerência multi-conexão (NBD_FLAG_CAN_MULTI_CONN, H1/DT-10) depende desta
        // sincronicidade: WRITE durável no ack ⇒ FLUSH no-op ⇒ um FLUSH cobre todas as
        // WRITEs ackadas. NÃO trocar `write_at` para cópia assíncrona sem revisar isto.
        Ok(())
    }
}

/// Janela `[base, base+len)` sobre um [`BlockBackend`] — uma slice da VRAM (RF-L1, DT-4).
///
/// `serve()` valida contra `size_bytes()` = `len` (o bounds-check da slice sai de graça); os
/// acessos somam `base` ao offset. Empacota um `&mut B`, então o worker (única thread CUDA)
/// constrói uma `SliceView` por `Job` sobre o backend único, sem tocar CUDA.
pub struct SliceView<'b, B: BlockBackend> {
    inner: &'b mut B,
    base: u64,
    len: u64,
}

impl<'b, B: BlockBackend> SliceView<'b, B> {
    /// Constrói a janela. Em debug, garante que `[base, base+len)` cabe em `inner` (a construção
    /// é interna ao worker, que deriva `base`/`len` do `SliceMap`).
    pub fn new(inner: &'b mut B, base: u64, len: u64) -> Self {
        debug_assert!(
            base.checked_add(len)
                .is_some_and(|end| end <= inner.size_bytes()),
            "SliceView [{base}, {base}+{len}) excede o backend ({} bytes)",
            inner.size_bytes()
        );
        Self { inner, base, len }
    }
}

impl<B: BlockBackend> BlockBackend for SliceView<'_, B> {
    fn size_bytes(&self) -> u64 {
        self.len
    }

    fn block_size(&self) -> u32 {
        self.inner.block_size()
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
        let abs = self
            .base
            .checked_add(off)
            .ok_or_else(|| IoError("SliceView read offset overflow".into()))?;
        self.inner.read_at(abs, buf)
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        let abs = self
            .base
            .checked_add(off)
            .ok_or_else(|| IoError("SliceView write offset overflow".into()))?;
        self.inner.write_at(abs, data)
    }

    fn flush(&mut self) -> Result<(), IoError> {
        self.inner.flush()
    }
}

/// Disco volátil em memória que implementa [`BlockBackend`] — valida os loops (NBD/ublk) sem
/// CUDA (drill em qemu, e2e sem GPU). O backend de produção é o [`VramBackend`] (mesmo trait),
/// então os caminhos servem qualquer um dos dois sem mudança.
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use ramshared_block::{Command, Request, serve};
    use ramshared_cuda::Cuda;

    /// Composição cuda + block em VRAM real: serve um WRITE e um READ NBD.
    /// `cargo test -p ramshared-wsl2d -- --ignored` num host com GPU.
    #[test]
    #[ignore = "requer GPU CUDA funcional (WSL2/GPU-PV)"]
    fn vram_backend_serves_nbd_write_then_read() {
        let cuda = Cuda::load().expect("libcuda");
        let dev = cuda.device(0).unwrap();
        let ctx = cuda.create_context(&dev).unwrap();
        let mut mem = ctx.alloc(1 << 20).unwrap();
        mem.zero().unwrap();
        let mut be = VramBackend::new(mem, 4096);

        let payload = vec![0x5Au8; 4096];
        let w = serve(
            &Request {
                flags: 0,
                cmd: Command::Write,
                handle: 1,
                offset: 4096,
                len: 4096,
            },
            &payload,
            &mut be,
        );
        assert_eq!(
            u32::from_be_bytes([w.reply[4], w.reply[5], w.reply[6], w.reply[7]]),
            0,
            "WRITE deve dar NBD_OK"
        );

        let r = serve(
            &Request {
                flags: 0,
                cmd: Command::Read,
                handle: 2,
                offset: 4096,
                len: 4096,
            },
            &[],
            &mut be,
        );
        assert_eq!(r.read_data, payload, "READ deve devolver o que foi escrito");
    }

    #[test]
    fn slice_view_isolates_neighbors() {
        // RamBackend de 128B = 2 slices de 64B; escrever na slice 1 não vaza para a slice 0.
        let mut be = RamBackend::new(128);
        {
            let mut s1 = SliceView::new(&mut be, 64, 64);
            assert_eq!(s1.size_bytes(), 64);
            s1.write_at(0, &[0xAB; 64]).unwrap();
        }
        {
            let s0 = SliceView::new(&mut be, 0, 64);
            let mut buf = [0xFFu8; 64];
            s0.read_at(0, &mut buf).unwrap();
            assert_eq!(buf, [0u8; 64], "slice 0 não pode ver a escrita na slice 1");
        }
        let mut raw = [0u8; 64];
        be.read_at(64, &mut raw).unwrap();
        assert_eq!(raw, [0xAB; 64], "a escrita caiu na janela certa do backend");
    }

    #[test]
    fn slice_view_serve_rejects_out_of_range() {
        // Acesso além do len da slice → EINVAL via serve (bounds da slice de graça).
        let mut be = RamBackend::new(128);
        let mut view = SliceView::new(&mut be, 64, 64);
        let out = serve(
            &Request {
                flags: 0,
                cmd: Command::Read,
                handle: 1,
                offset: 64, // 64 + 64 = 128 > len(64) da slice
                len: 64,
            },
            &[],
            &mut view,
        );
        let errno = u32::from_be_bytes([out.reply[4], out.reply[5], out.reply[6], out.reply[7]]);
        assert_ne!(errno, 0, "fora da janela da slice deve falhar (EINVAL)");
    }

    #[test]
    #[should_panic(expected = "excede o backend")]
    fn slice_view_new_panics_when_window_exceeds_backend() {
        let mut be = RamBackend::new(64);
        let _ = SliceView::new(&mut be, 32, 64); // 32 + 64 = 96 > 64 → debug_assert
    }
}
