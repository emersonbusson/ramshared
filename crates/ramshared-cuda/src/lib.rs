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

    /// Verifies a real registration/unregistration lifecycle on CUDA hardware.
    #[test]
    #[ignore = "requires a working CUDA GPU (run with --ignored on a GPU host)"]
    fn pinned_host_mapping_legitimate_lifecycle() {
        let cuda = Cuda::load().expect("libcuda must load");
        let dev = cuda.device(0).expect("device(0) must exist");
        let ctx = cuda.create_context(&dev).expect("context must be created");

        let layout = std::alloc::Layout::from_size_align(4096, 4096).unwrap();
        let page_ptr = unsafe { std::alloc::alloc(layout) };
        assert!(!page_ptr.is_null(), "aligned host allocation must succeed");
        unsafe {
            core::ptr::write_bytes(page_ptr, 0x42, 4096);
        }
        let mut mapping = unsafe {
            ctx.register_host(page_ptr.cast(), 4096, CU_MEMHOSTREGISTER_DEVICEMAP)
                .expect("page registration must succeed")
        };
        assert_eq!(mapping.len(), 4096);
        assert!(!mapping.is_empty());
        assert!(mapping.dev_ptr() != 0);
        assert_eq!(mapping.as_slice()[0], 0x42);
        mapping.as_mut_slice()[0] = 0xAA;
        assert_eq!(mapping.as_slice()[0], 0xAA);
        drop(mapping);
        unsafe {
            std::alloc::dealloc(page_ptr, layout);
        }
    }

    /// Real Host→VRAM→Host roundtrip. Requires a working CUDA GPU (WSL2/GPU-PV).
    #[test]
    #[ignore = "requires a working CUDA GPU (run with --ignored on a GPU host)"]
    fn gpu_roundtrip_256mib() {
        let cuda = Cuda::load().expect("libcuda must load");
        assert!(cuda.device_count().expect("device count") >= 1);
        let dev = cuda.device(0).expect("device(0) must exist");
        assert!(!dev.name().is_empty());
        assert_eq!(dev.ordinal(), 0);
        let ctx = cuda.create_context(&dev).expect("context must be created");
        let (free_before, total) = ctx.mem_info().expect("memory info must be available");
        assert!(total > 0 && free_before > 0);

        let size = 256 * 1024 * 1024;
        let mut mem = ctx.alloc(size).expect("allocation must succeed");
        assert_eq!(mem.len(), size);
        assert!(!mem.is_empty());
        mem.zero().expect("zero must succeed");

        // Known pattern at three offsets.
        let pat: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        for off in [0usize, size / 2, size - pat.len()] {
            mem.write_at(off, &pat).expect("write must succeed");
            let mut out = vec![0u8; pat.len()];
            mem.read_at(off, &mut out).expect("read must succeed");
            assert_eq!(out, pat, "roundtrip diverged at off={off}");
        }

        // Out-of-range access is an error, not corruption.
        let mut tiny = [0u8; 16];
        assert!(matches!(
            mem.read_at(size - 8, &mut tiny),
            Err(CudaError::OutOfRange { .. })
        ));
        assert!(matches!(
            mem.write_at(size - 8, &tiny),
            Err(CudaError::OutOfRange { .. })
        ));
    }
}
