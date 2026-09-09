//! Implementation of `ramshared_vram` traits for CUDA types (RF-G1): CUDA is the first VRAM
//! backend behind `VramProvider`/`VramMemory`. A future `ramshared-vulkan` would do the same,
//! without modifying the daemon. Orphan rule OK: the types (`Context`/`DeviceMem`) are local to this crate.

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
            other => VramError::Provider(other.to_string()),
        }
    }
}

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


    pub mod mock {
        use crate::ffi::*;
        use crate::Cuda;
        use core::ffi::{c_char, c_int, c_uint, c_void};
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        pub static ALLOC_CALLED: AtomicUsize = AtomicUsize::new(0);
        pub static FREE_CALLED: AtomicUsize = AtomicUsize::new(0);
        pub static FORCE_OOM: AtomicBool = AtomicBool::new(false);
        pub static DOUBLE_FREE: AtomicBool = AtomicBool::new(false);
        pub static FREED_PTR: AtomicUsize = AtomicUsize::new(0); // non-zero if freed

        pub fn reset() {
            ALLOC_CALLED.store(0, Ordering::SeqCst);
            FREE_CALLED.store(0, Ordering::SeqCst);
            FORCE_OOM.store(false, Ordering::SeqCst);
            DOUBLE_FREE.store(false, Ordering::SeqCst);
            FREED_PTR.store(0, Ordering::SeqCst);
        }

        unsafe extern "C" fn mock_init(_: c_uint) -> CuResult { CUDA_SUCCESS }
        unsafe extern "C" fn mock_device_get_count(count: *mut c_int) -> CuResult { unsafe { *count = 1 }; CUDA_SUCCESS }
        unsafe extern "C" fn mock_device_get(device: *mut CuDevice, _: c_int) -> CuResult { unsafe { *device = 0 }; CUDA_SUCCESS }
        unsafe extern "C" fn mock_device_get_name(name: *mut c_char, _: c_int, _: CuDevice) -> CuResult { unsafe { *name = 0 }; CUDA_SUCCESS }
        unsafe extern "C" fn mock_ctx_create(ctx: *mut CuContext, _: c_uint, _: CuDevice) -> CuResult { unsafe { *ctx = 1 as CuContext }; CUDA_SUCCESS }
        unsafe extern "C" fn mock_ctx_destroy(_: CuContext) -> CuResult { CUDA_SUCCESS }
        unsafe extern "C" fn mock_ctx_synchronize() -> CuResult { CUDA_SUCCESS }
        unsafe extern "C" fn mock_mem_alloc(ptr: *mut CuDevicePtr, _: usize) -> CuResult {
            if FORCE_OOM.load(Ordering::SeqCst) {
                return 2; // CUDA_ERROR_OUT_OF_MEMORY
            }
            ALLOC_CALLED.fetch_add(1, Ordering::SeqCst);
            unsafe { *ptr = 0x1000 }; // fake ptr
            CUDA_SUCCESS
        }
        unsafe extern "C" fn mock_mem_free(ptr: CuDevicePtr) -> CuResult {
            FREE_CALLED.fetch_add(1, Ordering::SeqCst);
            let prev = FREED_PTR.swap(ptr as usize, Ordering::SeqCst);
            if prev == ptr as usize {
                DOUBLE_FREE.store(true, Ordering::SeqCst);
            }
            CUDA_SUCCESS
        }
        unsafe extern "C" fn mock_memcpy_htod(_: CuDevicePtr, _: *const c_void, _: usize) -> CuResult { CUDA_SUCCESS }
        unsafe extern "C" fn mock_memcpy_dtoh(_: *mut c_void, _: CuDevicePtr, _: usize) -> CuResult { CUDA_SUCCESS }
        unsafe extern "C" fn mock_memset_d8(_: CuDevicePtr, _: u8, _: usize) -> CuResult { CUDA_SUCCESS }
        unsafe extern "C" fn mock_mem_get_info(free: *mut usize, total: *mut usize) -> CuResult {
            unsafe { *free = 1024 };
            unsafe { *total = 1024 };
            CUDA_SUCCESS
        }

        pub fn get_mock_cuda() -> Cuda {
            Cuda::mock(Syms {
                init: mock_init,
                device_get_count: mock_device_get_count,
                device_get: mock_device_get,
                device_get_name: mock_device_get_name,
                ctx_create: mock_ctx_create,
                ctx_destroy: mock_ctx_destroy,
                ctx_synchronize: mock_ctx_synchronize,
                mem_alloc: mock_mem_alloc,
                mem_free: mock_mem_free,
                memcpy_htod: mock_memcpy_htod,
                memcpy_dtoh: mock_memcpy_dtoh,
                memset_d8: mock_memset_d8,
                mem_get_info: mock_mem_get_info,
                get_error_string: None,
            })
        }
    }

    #[test]
    fn test_vram_alloc_success() {
        mock::reset();
        let cuda = mock::get_mock_cuda();
        let dev = cuda.device(0).unwrap();
        let ctx = cuda.create_context(&dev).unwrap();

        {
            let mut mem = VramProvider::alloc(&ctx, 1024).unwrap();
            assert_eq!(VramMemory::len(&mem), 1024);
            assert!(!VramMemory::is_empty(&mem));

            // test VramMemory methods
            VramMemory::zero(&mut mem).unwrap();
            VramMemory::write_at(&mut mem, 0, b"hi").unwrap();
            let mut buf = [0u8; 2];
            VramMemory::read_at(&mem, 0, &mut buf).unwrap();

            // test VramProvider mem_info
            let (free, total) = VramProvider::mem_info(&ctx).unwrap();
            assert_eq!(free, 1024);
            assert_eq!(total, 1024);

            assert_eq!(mock::ALLOC_CALLED.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(mock::FREE_CALLED.load(std::sync::atomic::Ordering::SeqCst), 0);
        }

        // Ensure free was called
        assert_eq!(mock::FREE_CALLED.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_vram_oom_failure() {
        mock::reset();
        mock::FORCE_OOM.store(true, std::sync::atomic::Ordering::SeqCst);
        let cuda = mock::get_mock_cuda();
        let dev = cuda.device(0).unwrap();
        let ctx = cuda.create_context(&dev).unwrap();

        let res = VramProvider::alloc(&ctx, 1024);
        assert!(matches!(res, Err(VramError::Provider(_))));
    }

    #[test]
    fn test_vram_double_free_guard() {
        mock::reset();
        let cuda = mock::get_mock_cuda();
        let dev = cuda.device(0).unwrap();
        let ctx = cuda.create_context(&dev).unwrap();

        let mem = VramProvider::alloc(&ctx, 1024).unwrap();

        drop(mem);
        assert_eq!(mock::FREE_CALLED.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(mock::DOUBLE_FREE.load(std::sync::atomic::Ordering::SeqCst), false);
    }

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
            op: "cuMemAlloc",
            code: 2,
            msg: "out of memory".to_string(),
        };
        let vram_err: VramError = cuda_err.into();

        match vram_err {
            VramError::Provider(msg) => {
                assert!(msg.contains("cuMemAlloc"));
                assert!(msg.contains("CUresult=2"));
            }
            _ => panic!("Expected VramError::Provider"),
        }
    }

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
