//! FFI cru sobre a CUDA Driver API, carregada em runtime via `dlopen`.
//!
//! SPEC: `SPECv3-WSL2.md` §4 (CUDA via FFI sobre `libcuda.so`, sem toolkit) e §0.2
//! (a referência `nbd-vram` usa exatamente esses símbolos `_v2`).
//!
//! Carregamento em runtime (não link-time) porque no WSL2 a `libcuda` é a stub do
//! host (`/usr/lib/wsl/lib`) e não queremos dependência de toolkit. Todo o
//! `unsafe` de FFI vive aqui e em `driver.rs`; o resto do workspace não toca CUDA
//! cru.

use core::ffi::{c_char, c_int, c_uint, c_void};

#[link(name = "dl")]
unsafe extern "C" {
    pub fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    pub fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    pub fn dlclose(handle: *mut c_void) -> c_int;
    pub fn dlerror() -> *const c_char;
}

pub const RTLD_NOW: c_int = 2;

pub type CuResult = c_int;
pub const CUDA_SUCCESS: CuResult = 0;

pub type CuDevice = c_int;
pub type CuContext = *mut c_void;
pub type CuDevicePtr = u64;

// Assinaturas da Driver API (ABI _v2 onde aplicável — igual à `nbd-vram`).
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

/// Tabela de símbolos resolvidos da `libcuda`.
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
