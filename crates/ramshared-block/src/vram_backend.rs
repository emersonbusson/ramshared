//! `VramBackend` — binds a region of VRAM (`ramshared_vram::VramMemory`) to the
//! [`BlockBackend`] trait. Promoted from `ramshared-wsl2d` (SPEC windows-swap-driver
//! ITEM-2 / DT-6) so Linux daemon and Windows service share one adapter.
//!
//! SPEC: `docs/specs/no-milestone/windows-swap-driver/SPEC.md` ITEM-2.

use ramshared_vram::{VramError, VramMemory};

use crate::{BlockBackend, IoError};

/// Block device backed by a region of VRAM (`M: VramMemory`).
pub struct VramBackend<M> {
    mem: M,
    block_size: u32,
}

impl<M: VramMemory> VramBackend<M> {
    pub fn new(mem: M, block_size: u32) -> Self {
        Self { mem, block_size }
    }

    /// Zeroes all VRAM (secure wipe on release/stop).
    pub fn zero(&mut self) -> Result<(), VramError> {
        self.mem.zero()
    }

    /// Access to the underlying VRAM region (e.g. for `mem_info` co-residency gates).
    pub fn mem(&self) -> &M {
        &self.mem
    }

    /// Mutable access to the underlying VRAM region.
    pub fn mem_mut(&mut self) -> &mut M {
        &mut self.mem
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
        // cuMemcpy*_v2 are synchronous (the reference uses the same model); nothing to drain.
        // Multi-connection coherence (NBD_FLAG_CAN_MULTI_CONN) depends on this
        // synchronicity: WRITE is durable upon ack ⇒ FLUSH is a no-op ⇒ a FLUSH covers all
        // acked WRITEs. Do NOT change `write_at` to asynchronous copy without reviewing this.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::{Command, Request, ServeOutcome, serve};
    use ramshared_vram::VramMemory;

    /// In-memory stand-in for VRAM (no GPU required).
    struct FakeVram(Vec<u8>);

    impl FakeVram {
        fn new(size: usize) -> Self {
            Self(vec![0u8; size])
        }
    }

    impl VramMemory for FakeVram {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn zero(&mut self) -> Result<(), VramError> {
            self.0.fill(0);
            Ok(())
        }

        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            let off = off as usize;
            let end = off
                .checked_add(dst.len())
                .filter(|&e| e <= self.0.len())
                .ok_or(VramError::OutOfRange {
                    off: off as u64,
                    len: dst.len() as u64,
                    size: self.0.len() as u64,
                })?;
            dst.copy_from_slice(&self.0[off..end]);
            Ok(())
        }

        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            let off = off as usize;
            let end = off
                .checked_add(src.len())
                .filter(|&e| e <= self.0.len())
                .ok_or(VramError::OutOfRange {
                    off: off as u64,
                    len: src.len() as u64,
                    size: self.0.len() as u64,
                })?;
            self.0[off..end].copy_from_slice(src);
            Ok(())
        }
    }

    fn errno(out: &ServeOutcome) -> u32 {
        u32::from_be_bytes([out.reply[4], out.reply[5], out.reply[6], out.reply[7]])
    }

    #[test]
    fn vram_backend_write_then_read_roundtrip() {
        let mut be = VramBackend::new(FakeVram::new(1 << 20), 4096);
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
        assert_eq!(errno(&w), 0, "WRITE must succeed");

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
        assert_eq!(r.read_data, payload, "READ must return written bytes");
    }

    #[test]
    fn vram_backend_oob_is_error() {
        let mut be = VramBackend::new(FakeVram::new(8192), 4096);
        let r = serve(
            &Request {
                flags: 0,
                cmd: Command::Read,
                handle: 1,
                offset: 8192,
                len: 4096,
            },
            &[],
            &mut be,
        );
        assert_ne!(errno(&r), 0, "OOB must fail before/with backend");
        assert!(r.read_data.is_empty());
    }

    #[test]
    fn vram_backend_zero_wipes() {
        let mut be = VramBackend::new(FakeVram::new(4096), 4096);
        be.write_at(0, &[0xFFu8; 4096]).unwrap();
        be.zero().unwrap();
        let mut buf = [0xAAu8; 4096];
        be.read_at(0, &mut buf).unwrap();
        assert_eq!(buf, [0u8; 4096]);
    }
}
