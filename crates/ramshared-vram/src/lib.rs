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
}

impl fmt::Display for VramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VramError::Provider(m) => write!(f, "vram provider: {m}"),
            VramError::OutOfRange { off, len, size } => {
                write!(f, "vram out-of-range: off={off} len={len} size={size}")
            }
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
