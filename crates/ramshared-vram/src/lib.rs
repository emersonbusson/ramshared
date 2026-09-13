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



/// A unified vRAM allocation dispatcher that checks hardware constraints
/// upfront using guard clauses instead of nested matching.
pub struct VramDispatcher<P> {
    provider: P,
}

impl<P> VramDispatcher<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

impl<P: VramProvider> VramProvider for VramDispatcher<P> {
    type Mem<'p> = P::Mem<'p> where Self: 'p;

    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        if bytes == 0 {
            return Err(VramError::InvalidAlignment);
        }

        // Use #[allow(clippy::manual_is_multiple_of)] to suppress the warning without using the unstable feature
        #[allow(clippy::manual_is_multiple_of)]
        if bytes % 4096 != 0 {
            return Err(VramError::InvalidAlignment);
        }

        // 4 GiB VRAM allocation safety bound
        let max_alloc = 4 * 1024 * 1024 * 1024;
        if bytes > max_alloc {
            return Err(VramError::OutOfMemory);
        }

        let (free, _total) = self.provider.mem_info()?;

        if (bytes as u64) > free {
            return Err(VramError::OutOfMemory);
        }

        // Flattened happy path dispatch
        self.provider.alloc(bytes)
    }

    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        self.provider.mem_info()
    }
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

    struct DummyMem;
    impl VramMemory for DummyMem {
        fn len(&self) -> usize { 4096 }
        fn zero(&mut self) -> Result<(), VramError> { Ok(()) }
        fn read_at(&self, _off: u64, _dst: &mut [u8]) -> Result<(), VramError> { Ok(()) }
        fn write_at(&mut self, _off: u64, _src: &[u8]) -> Result<(), VramError> { Ok(()) }
    }

    struct DummyProvider { free: u64 }
    impl VramProvider for DummyProvider {
        type Mem<'p> = DummyMem where Self: 'p;
        fn alloc(&self, _bytes: usize) -> Result<Self::Mem<'_>, VramError> { Ok(DummyMem) }
        fn mem_info(&self) -> Result<(u64, u64), VramError> { Ok((self.free, 8 * 1024 * 1024 * 1024)) }
    }

    #[test]
    fn test_vram_dispatcher_guard_clauses() {
        let provider = DummyProvider { free: 8 * 1024 * 1024 * 1024 };
        let dispatcher = VramDispatcher::new(provider);

        // Valid allocation
        assert!(dispatcher.alloc(4096).is_ok());

        // Invalid alignment
        match dispatcher.alloc(4095) {
            Err(VramError::InvalidAlignment) => (),
            _ => panic!("Expected InvalidAlignment for 4095"),
        }

        // Invalid size (0)
        match dispatcher.alloc(0) {
            Err(VramError::InvalidAlignment) => (),
            _ => panic!("Expected InvalidAlignment for 0"),
        }

        // Exceeds 4 GiB max
        let over_4gib = (4 * 1024 * 1024 * 1024) + 4096;
        match dispatcher.alloc(over_4gib) {
            Err(VramError::OutOfMemory) => (),
            _ => panic!("Expected OutOfMemory for > 4 GiB"),
        }

        // Exceeds free memory
        let tight_provider = DummyProvider { free: 4096 };
        let tight_dispatcher = VramDispatcher::new(tight_provider);
        match tight_dispatcher.alloc(8192) {
            Err(VramError::OutOfMemory) => (),
            _ => panic!("Expected OutOfMemory for exceeding free memory"),
        }
    }

}
