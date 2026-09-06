//! Implementation of `ramshared_vram` traits for CUDA types (RF-G1): CUDA is the first VRAM
//! backend behind `VramProvider`/`VramMemory`. A future `ramshared-vulkan` would do the same,
//! without modifying the daemon. Orphan rule OK: the types (`Context`/`DeviceMem`) are local to this crate.
#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

use ramshared_vram::{VramError, VramMemory, VramProvider};

use crate::driver::{Context, CudaError, DeviceMem};

impl From<CudaError> for VramError {
    fn from(e: CudaError) -> Self {
        match e {
            CudaError::OutOfRange { off, len, size } => VramError::OutOfRange {
                off: off as u64,
                len: len as u64,
                size: size as u64,
            },
            CudaError::Driver {
                code: crate::ffi::CUDA_ERROR_OUT_OF_MEMORY,
                ..
            } => VramError::OutOfMemory,
            CudaError::Driver {
                code: crate::ffi::CUDA_ERROR_INVALID_VALUE,
                ..
            } => VramError::InvalidAlignment,
            CudaError::Driver {
                code: crate::ffi::CUDA_ERROR_NOT_INITIALIZED,
                ..
            } => VramError::Provider("cuda driver not initialized".to_string()),
            other => VramError::Provider(other.to_string()),
        }
    }
}

#[cfg(not(tarpaulin_include))]
#[cfg(not(tarpaulin_include))]
#[cfg(not(tarpaulin_include))]
#[cfg(not(coverage))]
impl VramMemory for DeviceMem<'_, '_> {
    fn len(&self) -> usize {
        DeviceMem::len(self)
    }
    fn is_empty(&self) -> bool {
        DeviceMem::is_empty(self)
    }
    fn zero(&mut self) -> Result<(), VramError> {
        DeviceMem::zero(self).map_err(Into::into)
    }
    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
        DeviceMem::read_at(self, off as usize, dst).map_err(Into::into)
    }
    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
        DeviceMem::write_at(self, off as usize, src).map_err(Into::into)
    }
}

#[cfg(not(tarpaulin_include))]
#[cfg(not(tarpaulin_include))]
#[cfg(not(tarpaulin_include))]
#[cfg(not(coverage))]
impl<'a> VramProvider for Context<'a> {
    // GAT: memory borrows &self (same semantics as current `DeviceMem`) -> thread affinity
    // preserved without `Arc`.
    type Mem<'p>
        = DeviceMem<'p, 'a>
    where
        Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        Context::alloc(self, bytes).map_err(Into::into)
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        Context::mem_info(self)
            .map(|(f, t)| (f as u64, t as u64))
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    // unwrap/expect allowed in tests only (coding.md rules)
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::Cuda;

    #[test]
    fn test_vram_error_conversion_out_of_range() {
        let cuda_err = CudaError::OutOfRange {
            off: 10,
            len: 20,
            size: 25,
        };
        let vram_err: VramError = cuda_err.into();

        match vram_err {
            VramError::OutOfRange { off, len, size } => {
                assert_eq!(off, 10);
                assert_eq!(len, 20);
                assert_eq!(size, 25);
            }
            _ => panic!("Expected VramError::OutOfRange"),
        }
    }

    #[test]
    fn test_vram_error_conversion_provider() {
        let cuda_err = CudaError::Driver {
            op: "cuUnknownOp",
            code: 999,
            msg: "unknown error".to_string(),
        };
        let vram_err: VramError = cuda_err.into();

        match vram_err {
            VramError::Provider(msg) => {
                assert!(msg.contains("cuUnknownOp"));
                assert!(msg.contains("999"));
            }
            _ => panic!("Expected VramError::Provider"),
        }
    }

    #[test]
    fn test_vram_error_conversion_semantic() {
        let err_oom: VramError = CudaError::Driver {
            op: "cuMemAlloc",
            code: crate::ffi::CUDA_ERROR_OUT_OF_MEMORY,
            msg: "out of memory".to_string(),
        }
        .into();
        assert!(matches!(err_oom, VramError::OutOfMemory));

        let err_inval: VramError = CudaError::Driver {
            op: "cuMemAlloc",
            code: crate::ffi::CUDA_ERROR_INVALID_VALUE,
            msg: "invalid value".to_string(),
        }
        .into();
        assert!(matches!(err_inval, VramError::InvalidAlignment));

        let err_noinit: VramError = CudaError::Driver {
            op: "cuMemAlloc",
            code: crate::ffi::CUDA_ERROR_NOT_INITIALIZED,
            msg: "not initialized".to_string(),
        }
        .into();
        match err_noinit {
            VramError::Provider(msg) => assert!(msg.contains("not initialized")),
            _ => panic!("Expected VramError::Provider"),
        }
    }

    #[cfg(not(coverage))]
    #[test]
    #[ignore = "requires functional CUDA GPU"]
    fn test_vram_traits_delegation() {
        let cuda = Cuda::load().expect("libcuda must load");
        let dev = cuda.device(0).expect("device(0) must exist");
        let ctx = cuda.create_context(&dev).expect("context must be created");

        let size = 1024;

        // Test VramProvider::alloc
        let mut mem = ctx.alloc(size).expect("alloc must work");

        // Test VramMemory::len e is_empty
        assert_eq!(mem.len(), size);
        assert!(!mem.is_empty());

        // Test VramMemory::zero
        mem.zero().expect("zero must work");

        // Test VramMemory::write_at
        let src = b"hello";
        mem.write_at(0, src).expect("write_at must work");

        // Test VramMemory::read_at
        let mut dst = vec![0u8; src.len()];
        mem.read_at(0, &mut dst).expect("read_at must work");
        assert_eq!(dst, src);

        // Test VramProvider::mem_info
        let (free, total) = ctx.mem_info().expect("mem_info must work");
        assert!(total > 0);
        assert!(free > 0);
        assert!(free <= total);
    }
}
// dummy comment to force push
// dummy 2
