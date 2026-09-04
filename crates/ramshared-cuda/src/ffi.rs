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
}

impl Syms {
    /// Validates that all mandatory driver function pointers in the table are non-null.
    /// Does not enforce non-nullness for optional symbols (e.g., `get_error_string`).
    pub fn validate_pointers(&self) -> bool {
        // We cast the function pointers to *const () to check for null
        let init_ptr: *const () = self.init as *const ();
        let dgc_ptr: *const () = self.device_get_count as *const ();
        let dg_ptr: *const () = self.device_get as *const ();
        let dgn_ptr: *const () = self.device_get_name as *const ();
        let cc_ptr: *const () = self.ctx_create as *const ();
        let cd_ptr: *const () = self.ctx_destroy as *const ();
        let cs_ptr: *const () = self.ctx_synchronize as *const ();
        let ma_ptr: *const () = self.mem_alloc as *const ();
        let mf_ptr: *const () = self.mem_free as *const ();
        let mhtod_ptr: *const () = self.memcpy_htod as *const ();
        let mdtoh_ptr: *const () = self.memcpy_dtoh as *const ();
        let msd8_ptr: *const () = self.memset_d8 as *const ();
        let mgi_ptr: *const () = self.mem_get_info as *const ();

        !init_ptr.is_null()
            && !dgc_ptr.is_null()
            && !dg_ptr.is_null()
            && !dgn_ptr.is_null()
            && !cc_ptr.is_null()
            && !cd_ptr.is_null()
            && !cs_ptr.is_null()
            && !ma_ptr.is_null()
            && !mf_ptr.is_null()
            && !mhtod_ptr.is_null()
            && !mdtoh_ptr.is_null()
            && !msd8_ptr.is_null()
            && !mgi_ptr.is_null()
    }
}
