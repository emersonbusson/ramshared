//! Raw FFI bindings over the CUDA Driver API, loaded at runtime via OS-specific dynamic loaders.
//!
//! SPEC: `SPECv3-WSL2.md` §4 (CUDA via FFI over `libcuda.so`, no toolkit) and §0.2
//! (the `nbd-vram` reference uses precisely these `_v2` symbols).
//!
//! Runtime loading (not link-time) is required because in WSL2 the `libcuda` is a host stub
//! (`/usr/lib/wsl/lib`) and we want to avoid build-time dependencies on the CUDA toolkit. All
//! FFI-related `unsafe` blocks are isolated here and in `driver.rs`; the rest of the workspace
//! does not touch raw CUDA.

use core::ffi::{c_char, c_int, c_uint, c_void};

// CUDA types and constants remain unchanged.

pub type CuResult = c_int;
pub const CUDA_SUCCESS: CuResult = 0;

pub type CuDevice = c_int;
pub type CuContext = *mut c_void;
pub type CuDevicePtr = u64;

// Driver API signatures (ABI _v2 where applicable — matching `nbd-vram`).
pub type FnInit = unsafe extern "C" fn(c_uint) -> CuResult;
pub type FnDeviceGetCount = unsafe extern "C" fn(*mut c_int) -> CuResult;
pub type FnDeviceGet = unsafe extern "C" fn(*mut CuDevice, c_int) -> CuResult;
pub type FnDeviceGetName = unsafe extern "C" fn(*mut c_char, c_int, CuDevice) -> CuResult;
pub type FnCtxCreate = unsafe extern "C" fn(*mut CuContext, c_uint, CuDevice) -> CuResult;
pub type FnCtxDestroy = unsafe extern "C" fn(CuContext) -> CuResult;
pub type FnCtxSynchronize = unsafe extern "C" fn() -> CuResult;
pub type FnMemAlloc = unsafe extern "C" fn(*mut CuDevicePtr, usize) -> CuResult;
pub type FnMemFree = unsafe extern "C" fn(CuDevicePtr) -> CuResult;
pub type FnMemcpyHtoD = unsafe extern "C" fn(CuDevicePtr, *const c_void, usize) -> CuResult;
pub type FnMemcpyDtoH = unsafe extern "C" fn(*mut c_void, CuDevicePtr, usize) -> CuResult;
pub type FnMemsetD8 = unsafe extern "C" fn(CuDevicePtr, u8, usize) -> CuResult;
pub type FnMemGetInfo = unsafe extern "C" fn(*mut usize, *mut usize) -> CuResult;
pub type FnGetErrorString = unsafe extern "C" fn(CuResult, *mut *const c_char) -> CuResult;
pub type FnMemHostRegister = unsafe extern "C" fn(*mut c_void, usize, c_uint) -> CuResult;
pub type FnMemHostUnregister = unsafe extern "C" fn(*mut c_void) -> CuResult;
pub type FnMemHostGetDevicePointer =
    unsafe extern "C" fn(*mut CuDevicePtr, *mut c_void, c_uint) -> CuResult;

// Host memory registration flags (cuMemHostRegister)
pub const CU_MEMHOSTREGISTER_PORTABLE: c_uint = 0x01;
pub const CU_MEMHOSTREGISTER_DEVICEMAP: c_uint = 0x02;
pub const CU_MEMHOSTREGISTER_IOMEMORY: c_uint = 0x04;
pub const CU_MEMHOSTREGISTER_READ_ONLY: c_uint = 0x08;

/// Table of resolved symbols from the CUDA driver library.
pub struct Syms {
    pub init: FnInit,
    pub device_get_count: FnDeviceGetCount,
    pub device_get: FnDeviceGet,
    pub device_get_name: FnDeviceGetName,
    pub ctx_create: FnCtxCreate,
    pub ctx_destroy: FnCtxDestroy,
    pub ctx_synchronize: FnCtxSynchronize,
    pub mem_alloc: FnMemAlloc,
    pub mem_free: FnMemFree,
    pub memcpy_htod: FnMemcpyHtoD,
    pub memcpy_dtoh: FnMemcpyDtoH,
    pub memset_d8: FnMemsetD8,
    pub mem_get_info: FnMemGetInfo,
    pub get_error_string: Option<FnGetErrorString>,
    pub mem_host_register: Option<FnMemHostRegister>,
    pub mem_host_unregister: Option<FnMemHostUnregister>,
    pub mem_host_get_device_pointer: Option<FnMemHostGetDevicePointer>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_registration_flags_are_disjoint_single_bits() {
        let flags = [
            CU_MEMHOSTREGISTER_PORTABLE,
            CU_MEMHOSTREGISTER_DEVICEMAP,
            CU_MEMHOSTREGISTER_IOMEMORY,
            CU_MEMHOSTREGISTER_READ_ONLY,
        ];
        for (index, flag) in flags.iter().enumerate() {
            assert_eq!(flag.count_ones(), 1);
            for other in flags.iter().skip(index + 1) {
                assert_eq!(flag & other, 0);
            }
        }
    }

    #[test]
    fn driver_handle_types_match_the_cuda_abi_widths() {
        assert_eq!(
            core::mem::size_of::<CuResult>(),
            core::mem::size_of::<c_int>()
        );
        assert_eq!(
            core::mem::size_of::<CuDevice>(),
            core::mem::size_of::<c_int>()
        );
        assert_eq!(
            core::mem::size_of::<CuDevicePtr>(),
            core::mem::size_of::<u64>()
        );
        assert_eq!(
            core::mem::size_of::<CuContext>(),
            core::mem::size_of::<*mut c_void>()
        );
    }
}
