//! `VramBackend` — binds a region of VRAM (`ramshared_vram::VramMemory`) to the
//! [`BlockBackend`] trait. Promoted from `ramshared-wsl2d` (SPEC windows-swap-driver
//! ITEM-2 / DT-6) so Linux daemon and Windows service share one adapter.
//!
//! SPEC: `docs/specs/no-milestone/windows-swap-driver/SPEC.md` ITEM-2.

use ramshared_vram::{VramError, VramMemory};

use crate::{BlockBackend, IoError};

/// Block device backed by a region of VRAM (`M: VramMemory`).
pub struct VramBackend<M: VramMemory> {
    mem: Option<M>,
    block_size: u32,
}

impl<M: VramMemory> VramBackend<M> {
    pub fn new(mem: M, block_size: u32) -> Self {
        Self { mem: Some(mem), block_size }
    }

    /// Zeroes all VRAM (secure wipe on release/stop).
    pub fn zero(&mut self) -> Result<(), VramError> {
        if let Some(mem) = &mut self.mem {
            mem.zero()
        } else {
            Ok(())
        }
    }

    /// Access to the underlying VRAM region (e.g. for `mem_info` co-residency gates).
    pub fn mem(&self) -> &M {
        if let Some(mem) = &self.mem {
            mem
        } else {
            unreachable!("mem always present except during drop/into_inner")
        }
    }

    /// Mutable access to the underlying VRAM region.
    pub fn mem_mut(&mut self) -> &mut M {
        if let Some(mem) = &mut self.mem {
            mem
        } else {
            unreachable!("mem always present except during drop/into_inner")
        }
    }

    /// Consume the adapter so callers can explicitly order memory release
    /// before releasing an external lease.
    pub fn into_inner(mut self) -> M {
        // Secure wipe on manual release to prevent data leakage.
        let _ = self.zero();
        if let Some(mem) = self.mem.take() {
            mem
        } else {
            unreachable!("mem always present except during drop/into_inner")
        }
    }
}

impl<M: VramMemory> Drop for VramBackend<M> {
    fn drop(&mut self) {
        // Secure wipe on Drop to prevent data leakage.
        let _ = self.zero();
    }
}

impl<M: VramMemory> BlockBackend for VramBackend<M> {
    fn size_bytes(&self) -> u64 {
        self.mem().len() as u64
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
        self.mem_mut()
            .read_at(off, buf)
            .map_err(|e| IoError(e.to_string()))
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        self.mem_mut()
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
    use std::sync::{Arc, Mutex};

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

    /// Observable memory to ensure drops trigger wiping.
    struct ObservableMem {
        data: Arc<Mutex<Vec<u8>>>,
        size: usize,
    }

    impl ObservableMem {
        fn new(size: usize) -> Self {
            Self {
                data: Arc::new(Mutex::new(vec![0xFF; size])),
                size,
            }
        }
        fn get_data_ref(&self) -> Arc<Mutex<Vec<u8>>> {
            self.data.clone()
        }
    }

    impl VramMemory for ObservableMem {
        fn len(&self) -> usize {
            self.size
        }

        fn zero(&mut self) -> Result<(), VramError> {
            let mut d = self.data.lock().unwrap();
            d.fill(0);
            Ok(())
        }

        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            let off = off as usize;
            let d = self.data.lock().unwrap();
            let end = off.checked_add(dst.len()).unwrap();
            dst.copy_from_slice(&d[off..end]);
            Ok(())
        }

        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            let off = off as usize;
            let mut d = self.data.lock().unwrap();
            let end = off.checked_add(src.len()).unwrap();
            d[off..end].copy_from_slice(src);
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

    #[test]
    fn vram_backend_into_inner_allows_explicit_release_order() {
        let be = VramBackend::new(FakeVram::new(4096), 4096);
        let mem = be.into_inner();
        assert_eq!(mem.len(), 4096);
    }

    #[test]
    fn vram_backend_into_inner_secure_wipe() {
        let mem = ObservableMem::new(4096);
        let data_ref = mem.get_data_ref();
        let be = VramBackend::new(mem, 4096);
        let _inner = be.into_inner();
        // Inner was consumed, data should be zeroed
        let d = data_ref.lock().unwrap();
        assert!(d.iter().all(|&x| x == 0), "Memory must be wiped on into_inner");
    }

    #[test]
    fn vram_backend_drop_secure_wipe() {
        let mem = ObservableMem::new(4096);
        let data_ref = mem.get_data_ref();
        {
            let _be = VramBackend::new(mem, 4096);
            // when `_be` goes out of scope, it should trigger drop() which calls zero()
        }
        let d = data_ref.lock().unwrap();
        assert!(d.iter().all(|&x| x == 0), "Memory must be wiped on Drop");
    }
}
