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
