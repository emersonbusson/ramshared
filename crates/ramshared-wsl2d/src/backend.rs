//! Block backends for the daemon: re-exports shared [`VramBackend`] (ITEM-2 / DT-6)
//! plus daemon-local [`SliceView`] and [`RamBackend`].

use ramshared_block::{BlockBackend, IoError};

use crate::ublk;

// SPEC windows-swap-driver ITEM-2 / DT-6: single adapter in ramshared-block.
pub use ramshared_block::VramBackend;

/// Window `[base, base+len)` over a [`BlockBackend`] — a slice of VRAM (RF-L1, DT-4).
///
/// `serve()` validates against `size_bytes()` = `len` (bounds-check of the slice is free);
/// accesses add `base` to the offset. Wraps a `&mut B`, so the worker (single CUDA thread)
/// builds a `SliceView` per `Job` over the single backend, without touching CUDA.
pub struct SliceView<'b, B: BlockBackend> {
    inner: &'b mut B,
    base: u64,
    len: u64,
}

impl<'b, B: BlockBackend> SliceView<'b, B> {
    /// Constructs the window. In debug, ensures that `[base, base+len)` fits in `inner` (construction
    /// is internal to the worker, which derives `base`/`len` from `SliceMap`).
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

/// Volatile memory disk implementing [`BlockBackend`] — validates the loops (NBD/ublk) without
/// CUDA (drill in QEMU, E2E without GPU). The production backend is [`VramBackend`] (same trait),
/// so the paths serve either one without changes.
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

    /// Composition of cuda + block in real VRAM: serves an NBD WRITE and READ.
    /// `cargo test -p ramshared-wsl2d -- --ignored` on a host with a GPU.
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

    /// VRAM gauge with REAL `mem_info` (RF-3): `vram_outros` (subtração) captures graphics/Windows
    /// usage. `cargo test -p ramshared-wsl2d -- --ignored` on a host with a GPU.
    #[test]
    #[ignore = "requer GPU CUDA funcional (WSL2/GPU-PV)"]
    fn vram_gauge_outros_captures_real_graphics_usage() {
        use crate::telemetry::{VramGauge, vram_outros};
        use std::sync::atomic::Ordering;
        let cuda = Cuda::load().expect("libcuda");
        let dev = cuda.device(0).unwrap();
        let ctx = cuda.create_context(&dev).unwrap();
        let chunk = 64 * 1024 * 1024usize; // the "daemon" allocates 64 MiB
        let _mem = ctx.alloc(chunk).unwrap();
        let (free, total) = ctx.mem_info().unwrap();
        let gauge = VramGauge::default();
        gauge.free.store(free as u64, Ordering::Relaxed);
        gauge.total.store(total as u64, Ordering::Relaxed);
        assert!(total > 0 && free <= total, "mem_info coerente");
        let used = (total - free) as u64;
        let alloc_daemon = chunk as u64;
        let outros = vram_outros(used, alloc_daemon);
        // On a desktop in use, total usage > what the daemon allocated (due to graphics) → outros > 0.
        assert!(
            used > alloc_daemon,
            "uso total ({used}) > daemon ({alloc_daemon})"
        );
        assert!(
            outros > 0,
            "vram_outros capta gráficos/Windows: {outros} bytes"
        );
        eprintln!(
            "VRAM real (MiB): total={} free={} used={} daemon={} outros={}",
            total >> 20,
            free >> 20,
            used >> 20,
            alloc_daemon >> 20,
            outros >> 20
        );
    }

    #[test]
    fn slice_view_isolates_neighbors() {
        // RamBackend of 128B = 2 slices de 64B; writing to slice 1 does not leak to slice 0.
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
        // Access beyond the slice's len → EINVAL via serve (bounds-check of slice is free).
        let mut be = RamBackend::new(128);
        let mut view = SliceView::new(&mut be, 64, 64);
        let out = serve(
            &Request {
                flags: 0,
                cmd: Command::Read,
                handle: 1,
                offset: 64, // 64 + 64 = 128 > len(64) of the slice
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
