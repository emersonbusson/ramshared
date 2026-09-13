//! ramshared-cuda — Safe wrapper over the CUDA Driver API (`libcuda`) loaded at runtime
//! for the VRAM cascade tier (SPECv3-WSL2.md §4, §8).
//!
//! Reusable library: `Cuda::load()` -> `device()` -> `create_context()` -> `alloc()`
//! with synchronous Host<->Device copies and RAII-based resource deallocation. Ported from the design of
//! reference `c0deJedi/nbd-vram` (C, MIT), not copied — see SPECv3 §0.2.
//!
//! ```no_run
//! use ramshared_cuda::Cuda;
//! let cuda = Cuda::load()?;
//! let dev = cuda.device(0)?;
//! let ctx = cuda.create_context(&dev)?;
//! let (free, total) = ctx.mem_info()?;
//! let mut mem = ctx.alloc(256 * 1024 * 1024)?; // 256 MiB de VRAM
//! mem.zero()?;
//! mem.write_at(0, b"ping")?;
//! let mut out = [0u8; 4];
//! mem.read_at(0, &mut out)?;
//! assert_eq!(&out, b"ping");
//! # Ok::<(), ramshared_cuda::CudaError>(())
//! ```

#[cfg(unix)]
mod loader_unix;
#[cfg(unix)]
use loader_unix as loader;

#[cfg(windows)]
mod loader_win;
#[cfg(windows)]
use loader_win as loader;

mod driver;
mod ffi;
pub mod probe;
mod vram_impl; // impl VramProvider/VramMemory for CUDA types (RF-G1)

pub use driver::{Context, Cuda, CudaError, Device, DeviceMem, PinnedHostMapping};
pub use ffi::{
    CU_MEMHOSTREGISTER_DEVICEMAP, CU_MEMHOSTREGISTER_IOMEMORY, CU_MEMHOSTREGISTER_PORTABLE,
    CU_MEMHOSTREGISTER_READ_ONLY,
};
pub use probe::{PROBE_PATTERN_LEN, ProbePlanError, pattern_for_offset, plan_probe_offsets};

#[cfg(test)]
mod tests {
    // unwrap/expect allowed in tests only (coding.md rules), despite the crate-level deny.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn error_display_is_descriptive() {
        let e = CudaError::OutOfRange {
            off: 4096,
            len: 8192,
            size: 8192,
        };
        let s = e.to_string();
        assert!(s.contains("off=4096"));
        assert!(s.contains("size=8192"));

        let e_inv = CudaError::InvalidValue("misaligned pointer".into());
        assert!(e_inv.to_string().contains("misaligned pointer"));

        let e_unsupp = CudaError::Unsupported("feature missing".into());
        assert!(e_unsupp.to_string().contains("feature missing"));
    }

    #[test]
    fn driver_error_carries_op_and_code() {
        let e = CudaError::Driver {
            op: "cuMemAlloc",
            code: 2,
            msg: "out of memory".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("cuMemAlloc"));
        assert!(s.contains("CUresult=2"));
    }

    #[test]
    fn pinned_host_mapping_validation_rejects_invalid_inputs() {
        let layout = std::alloc::Layout::from_size_align(4096, 4096).expect("valid page layout");
        let aligned = unsafe { std::alloc::alloc(layout) };
        assert!(!aligned.is_null(), "aligned host allocation must succeed");
        assert!(super::driver::validate_host_registration(aligned.cast(), 4096).is_ok());
        assert!(matches!(
            super::driver::validate_host_registration(core::ptr::null_mut(), 4096),
            Err(CudaError::InvalidValue(message)) if message.contains("null")
        ));
        assert!(matches!(
            super::driver::validate_host_registration(aligned.cast(), 0),
            Err(CudaError::InvalidValue(message)) if message.contains("greater than zero")
        ));
        assert!(matches!(
            super::driver::validate_host_registration(aligned.cast(), 1024),
            Err(CudaError::InvalidValue(message)) if message.contains("multiple of page size")
        ));
        let misaligned = unsafe { aligned.add(1) };
        assert!(matches!(
            super::driver::validate_host_registration(misaligned.cast(), 4096),
            Err(CudaError::InvalidValue(message)) if message.contains("aligned")
        ));
        unsafe {
            std::alloc::dealloc(aligned, layout);
        }
    }
}
