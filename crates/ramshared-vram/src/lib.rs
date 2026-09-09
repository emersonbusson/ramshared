//! `ramshared-vram` — VRAM backend abstraction (RF-G1, preparation for P3).
//!
//! Separates the VRAM **control plane** (lifecycle + allocation + wipe + free-floor) from
//! the concrete backend (currently CUDA; Vulkan in the future). The **data plane** (block I/O) is
//! already abstracted by `ramshared_block::BlockBackend`; this crate handles VRAM-specific operations.
//!
//! Safe Rust only, completely driver-agnostic. The concrete CUDA implementation lives in
//! `ramshared-cuda` (which re-exports the types + impl); a future `ramshared-vulkan` would do the same.
//!
//! SPEC: docs/vram-provider/SPEC.md.
#![forbid(unsafe_code)]

use std::fmt;

/// VRAM operation error (mapped from the backend-specific error, e.g., `CudaError`).
#[derive(Debug)]
pub enum VramError {
    /// Backend provider failure: initialization/driver/allocation error.
    Provider(String),
    /// Attempted access out of the allocated memory range.
    OutOfRange { off: u64, len: u64, size: u64 },
    /// Allocation failed due to out-of-memory.
    OutOfMemory,
    /// Invalid alignment for VRAM operation.
    InvalidAlignment,
    /// VRAM operation failed because resource is busy.
    Busy,
}

impl fmt::Display for VramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VramError::Provider(m) => write!(f, "vram provider: {m}"),
            VramError::OutOfRange { off, len, size } => {
                write!(f, "vram out-of-range: off={off} len={len} size={size}")
            }
            VramError::OutOfMemory => write!(f, "vram out of memory"),
            VramError::InvalidAlignment => write!(f, "vram invalid alignment"),
            VramError::Busy => write!(f, "vram busy"),
        }
    }
}

impl std::error::Error for VramError {}

/// An allocated VRAM memory region. Synchronous operations (wipe/zeroing is blocking, DT-17/§11).
///
/// **Thread Affinity:** The implementation can be thread-local (CUDA is). It must be used on the
/// same thread that allocated it. This is why the daemon handles all VRAM I/O on a single thread.
/// The trait does NOT require `Send`.
pub trait VramMemory {
    /// Size of the region in bytes.
    fn len(&self) -> usize;
    /// Returns `true` if the region has 0 bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Fills the entire region with zeroes (secure wipe + synchronize). DT-17/§11.
    fn zero(&mut self) -> Result<(), VramError>;
    /// Reads `dst.len()` bytes starting at `off`.
    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError>;
    /// Writes `src` bytes starting at `off`.
    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError>;
}

/// VRAM Provider (representing an initialized thread-affinity context): Allocates regions and reports capacity metrics.
///
/// The driver lifecycle (driver load, device selection, and context creation) is the responsibility
/// of the concrete backend constructor (e.g., `Cuda::load()` + `create_context()`), as it differs per
/// backend; the daemon receives an initialized provider and communicates solely via this trait.
pub trait VramProvider {
    /// Type of the allocated region (GAT: borrows `&self`, preserving thread affinity without `Arc`).
    type Mem<'p>: VramMemory
    where
        Self: 'p;

    /// Allocates `bytes` of VRAM. The region is released when dropped (RAII).
    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError>;

    /// Returns free and total VRAM capacities in bytes (used by the residency canary — DT-3/9/11).
    fn mem_info(&self) -> Result<(u64, u64), VramError>;
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vram_error_display() {
        assert_eq!(
            VramError::Provider("test".to_string()).to_string(),
            "vram provider: test"
        );
        assert_eq!(
            VramError::OutOfRange {
                off: 0,
                len: 10,
                size: 5
            }
            .to_string(),
            "vram out-of-range: off=0 len=10 size=5"
        );
        assert_eq!(VramError::OutOfMemory.to_string(), "vram out of memory");
        assert_eq!(
            VramError::InvalidAlignment.to_string(),
            "vram invalid alignment"
        );
        assert_eq!(VramError::Busy.to_string(), "vram busy");
    }

    #[derive(Debug)]
    struct MockMem {
        len: usize,
    }

    impl VramMemory for MockMem {
        fn len(&self) -> usize {
            self.len
        }
        fn zero(&mut self) -> Result<(), VramError> {
            Ok(())
        }
        fn read_at(&self, _off: u64, _dst: &mut [u8]) -> Result<(), VramError> {
            Ok(())
        }
        fn write_at(&mut self, _off: u64, _src: &[u8]) -> Result<(), VramError> {
            Ok(())
        }
    }

    struct MockProvider {
        capacity: u64,
    }

    impl VramProvider for MockProvider {
        type Mem<'p> = MockMem where Self: 'p;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            if bytes as u64 > self.capacity {
                return Err(VramError::OutOfMemory);
            }
            if bytes % 4096 != 0 {
                return Err(VramError::InvalidAlignment);
            }
            Ok(MockMem { len: bytes })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((self.capacity, self.capacity))
        }
    }

    #[test]
    fn test_vram_alloc_zero_success() {
        let provider = MockProvider { capacity: 8192 };
        let mem = provider.alloc(0).unwrap();
        assert_eq!(mem.len(), 0);
        assert!(mem.is_empty());
    }

    #[test]
    fn test_vram_alloc_page_boundary_success() {
        let provider = MockProvider { capacity: 8192 };
        let mem = provider.alloc(4096).unwrap();
        assert_eq!(mem.len(), 4096);
        assert!(!mem.is_empty());
    }

    #[test]
    fn test_vram_alloc_max_capacity_error() {
        let provider = MockProvider { capacity: 8192 };
        let err = provider.alloc(16384).unwrap_err();
        assert!(matches!(err, VramError::OutOfMemory));
    }

    #[test]
    fn test_vram_alloc_misaligned_error() {
        let provider = MockProvider { capacity: 8192 };
        let err = provider.alloc(4097).unwrap_err();
        assert!(matches!(err, VramError::InvalidAlignment));
    }

    #[test]
    fn test_vram_mem_methods() {
        let mut mem = MockMem { len: 4096 };
        assert!(mem.zero().is_ok());
        let mut buf = [0u8; 10];
        assert!(mem.read_at(0, &mut buf).is_ok());
        assert!(mem.write_at(0, &buf).is_ok());
    }

    #[test]
    fn test_vram_provider_mem_info() {
        let provider = MockProvider { capacity: 8192 };
        let (free, total) = provider.mem_info().unwrap();
        assert_eq!(free, 8192);
        assert_eq!(total, 8192);
    }
}
