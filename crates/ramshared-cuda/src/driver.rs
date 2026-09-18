//! Safe wrappers (RAII) over the CUDA Driver API. SPEC §4, §8.
//!
//! Ownership Model:
//! - [`Cuda`] owns the dynamic library handle + the resolved symbol table (longest-lived).
//! - [`Context`] borrows `&Cuda` and calls `cuCtxDestroy` in its `Drop` implementation.
//! - [`DeviceMem`] borrows `&Context` and calls `cuMemFree` in its `Drop` implementation.
//!
//! The `Drop` order guarantees the reverse order of allocation required by CUDA
//! (freeing memory -> destroying context -> closing library), translating the kernel's
//! `goto out_err` pattern into Rust's borrow checker invariants.

use core::ffi::{CStr, c_char, c_void};
use core::fmt;

use crate::ffi::{CUDA_SUCCESS, CuContext, CuDevice, CuDevicePtr, CuResult, Syms};

/// CUDA layer error representation. No `panic`/`unwrap` in production paths (coding.md rules).
#[derive(Debug)]
pub enum CudaError {
    NoDevice,
    /// Dynamic library loading failed to find a candidate library.
    Load(String),
    /// Symbol resolution failed for a required symbol.
    Symbol(String),
    /// A CUDA Driver API call returned an error code.
    Driver {
        op: &'static str,
        code: i32,
        msg: String,
    },
    /// VRAM memory region access out of bounds (offset + len > size).
    OutOfRange {
        off: usize,
        len: usize,
        size: usize,
    },
    /// Invalid argument supplied to driver wrapper.
    InvalidValue(String),
    /// The requested feature is unsupported by the loaded driver version.
    Unsupported(String),
}

impl fmt::Display for CudaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CudaError::NoDevice => write!(f, "no CUDA-capable device found"),
            CudaError::Load(s) => write!(f, "failed to load CUDA library: {s}"),
            CudaError::Symbol(s) => write!(f, "required CUDA symbol missing: {s}"),
            CudaError::Driver { op, code, msg } => {
                write!(f, "{op} failed (CUresult={code}): {msg}")
            }
            CudaError::OutOfRange { off, len, size } => {
                write!(f, "out of bounds access: off={off} len={len} > size={size}")
            }
            CudaError::InvalidValue(s) => write!(f, "invalid argument: {s}"),
            CudaError::Unsupported(s) => write!(f, "unsupported driver feature: {s}"),
        }
    }
}

impl core::error::Error for CudaError {}

const HOST_PAGE_BYTES: usize = 4096;

pub(super) fn validate_host_registration(
    host_ptr: *mut c_void,
    len: usize,
) -> Result<(), CudaError> {
    if host_ptr.is_null() {
        return Err(CudaError::InvalidValue("host_ptr cannot be null".into()));
    }
    if len == 0 {
        return Err(CudaError::InvalidValue(
            "length must be greater than zero".into(),
        ));
    }
    if !len.is_multiple_of(HOST_PAGE_BYTES) {
        return Err(CudaError::InvalidValue(format!(
            "length {len} must be a multiple of page size {HOST_PAGE_BYTES}"
        )));
    }
    if !(host_ptr as usize).is_multiple_of(HOST_PAGE_BYTES) {
        return Err(CudaError::InvalidValue(format!(
            "host_ptr {host_ptr:p} must be aligned to {HOST_PAGE_BYTES}-byte page boundary"
        )));
    }
    Ok(())
}

/// RAII wrapper for the loaded dynamic library handle: calls close on `Drop`.
struct Lib(*mut c_void);

impl Drop for Lib {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: self.0 was returned by a successful open call and has not been closed.
            unsafe { crate::loader::close(self.0) };
        }
    }
}

/// CUDA library loaded and initialized successfully (`cuInit(0)`).
pub struct Cuda {
    _lib: Lib,
    syms: Syms,
}

#[cfg(unix)]
const CANDIDATES: &[&CStr] = &[
    c"libcuda.so.1",
    c"/usr/lib/wsl/lib/libcuda.so.1",
    c"libcuda.so",
    c"/usr/lib/wsl/lib/libcuda.so",
    c"/usr/lib/x86_64-linux-gnu/libcuda.so.1",
];

#[cfg(windows)]
const CANDIDATES: &[&CStr] = &[c"nvcuda.dll"];

impl Cuda {
    /// Loads the CUDA driver library (using OS-specific candidates) and runs `cuInit(0)`.
    pub fn load() -> Result<Self, CudaError> {
        let mut handle: *mut c_void = core::ptr::null_mut();
        for cand in CANDIDATES {
            // SAFETY: cand is a valid null-terminated CStr.
            let h = unsafe { crate::loader::open(cand.as_ptr()) };
            if !h.is_null() {
                handle = h;
                break;
            }
        }
        if handle.is_null() {
            return Err(CudaError::Load(crate::loader::error()));
        }
        let lib = Lib(handle);

        // SAFETY: handle is an active library context; each resolved symbol is a valid
        // Driver API function pointer conforming to the signatures in ffi.rs.
        let syms = unsafe {
            Syms {
                init: load_sym(handle, c"cuInit")?,
                device_get_count: load_sym(handle, c"cuDeviceGetCount")?,
                device_get: load_sym(handle, c"cuDeviceGet")?,
                device_get_name: load_sym(handle, c"cuDeviceGetName")?,
                ctx_create: load_sym(handle, c"cuCtxCreate_v2")?,
                ctx_destroy: load_sym(handle, c"cuCtxDestroy_v2")?,
                ctx_synchronize: load_sym(handle, c"cuCtxSynchronize")?,
                mem_alloc: load_sym(handle, c"cuMemAlloc_v2")?,
                mem_free: load_sym(handle, c"cuMemFree_v2")?,
                memcpy_htod: load_sym(handle, c"cuMemcpyHtoD_v2")?,
                memcpy_dtoh: load_sym(handle, c"cuMemcpyDtoH_v2")?,
                memset_d8: load_sym(handle, c"cuMemsetD8_v2")?,
                mem_get_info: load_sym(handle, c"cuMemGetInfo_v2")?,
                get_error_string: load_sym_opt(handle, c"cuGetErrorString"),
                mem_host_register: load_sym_opt(handle, c"cuMemHostRegister_v2")
                    .or_else(|| load_sym_opt(handle, c"cuMemHostRegister")),
                mem_host_unregister: load_sym_opt(handle, c"cuMemHostUnregister"),
                mem_host_get_device_pointer: load_sym_opt(handle, c"cuMemHostGetDevicePointer_v2")
                    .or_else(|| load_sym_opt(handle, c"cuMemHostGetDevicePointer")),
            }
        };

        // SAFETY: init symbol resolved successfully.
        let r = unsafe { (syms.init)(0) };
        check(&syms, r, "cuInit")?;

        Ok(Cuda { _lib: lib, syms })
    }

    /// Returns the number of CUDA-capable devices visible to the system.
    pub fn device_count(&self) -> Result<i32, CudaError> {
        let mut count: i32 = 0;
        // SAFETY: count points to a valid local memory location.
        let r = unsafe { (self.syms.device_get_count)(&mut count) };
        check(&self.syms, r, "cuDeviceGetCount")?;
        Ok(count)
    }

    /// Gets the device handle for the specified `ordinal` index, resolving its name.
    pub fn device(&self, ordinal: i32) -> Result<Device, CudaError> {
        let mut raw: CuDevice = 0;
        // SAFETY: raw points to a valid local memory location.
        let r = unsafe { (self.syms.device_get)(&mut raw, ordinal) };
        check(&self.syms, r, "cuDeviceGet")?;

        let mut buf = [0_i8; 128];
        // SAFETY: buf has space for `len` bytes; the API writes a null-terminated string.
        let r = unsafe {
            (self.syms.device_get_name)(buf.as_mut_ptr() as *mut c_char, buf.len() as i32, raw)
        };
        check(&self.syms, r, "cuDeviceGetName")?;
        buf[buf.len() - 1] = 0; // guarantees null-termination even if the API filled the entire buffer
        // SAFETY: buf was initialized by the driver call and has a guaranteed terminating null byte.
        let name = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
            .to_string_lossy()
            .into_owned();

        Ok(Device { raw, name, ordinal })
    }

    /// Creates a CUDA context on the specified device (becomes current on the calling thread).
    pub fn create_context<'a>(&'a self, device: &Device) -> Result<Context<'a>, CudaError> {
        let mut raw: CuContext = core::ptr::null_mut();
        // SAFETY: raw points to a valid local; device.raw is a valid CUdevice handle.
        let r = unsafe { (self.syms.ctx_create)(&mut raw, 0, device.raw) };
        check(&self.syms, r, "cuCtxCreate")?;
        Ok(Context { cuda: self, raw })
    }
}

/// A CUDA device (ordinal index + name).
#[derive(Clone, Debug)]
pub struct Device {
    raw: CuDevice,
    name: String,
    ordinal: i32,
}

impl Device {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Device ordinal passed to [`Cuda::device`].
    pub fn ordinal(&self) -> i32 {
        self.ordinal
    }
}

/// CUDA context representation. `Drop` implementation calls `cuCtxDestroy`.
///
/// **Thread Affinity:** The active CUDA context is *thread-local*. This `Context` (and
/// derived `DeviceMem` allocations) must be used on the **same thread** that created them.
/// This is why the daemon executes all VRAM I/O on a single thread. Accessing from another thread
/// would require calling `cuCtxSetCurrent` (not implemented here). The DEMOTE thread only calls `swapoff`.
pub struct Context<'a> {
    cuda: &'a Cuda,
    raw: CuContext,
}

impl<'a> Context<'a> {
    /// Returns the free and total VRAM capacities in bytes (`cuMemGetInfo`).
    pub fn mem_info(&self) -> Result<(usize, usize), CudaError> {
        let (mut free, mut total) = (0_usize, 0_usize);
        // SAFETY: out-parameters are valid local pointers; CUDA context is current on the calling thread.
        let r = unsafe { (self.cuda.syms.mem_get_info)(&mut free, &mut total) };
        check(&self.cuda.syms, r, "cuMemGetInfo")?;
        Ok((free, total))
    }

    /// Allocates `bytes` of VRAM. The allocation is released when the returned `DeviceMem` is dropped.
    pub fn alloc(&self, bytes: usize) -> Result<DeviceMem<'_, 'a>, CudaError> {
        let mut ptr: CuDevicePtr = 0;
        // SAFETY: ptr points to a valid local; CUDA context is current.
        let r = unsafe { (self.cuda.syms.mem_alloc)(&mut ptr, bytes) };
        check(&self.cuda.syms, r, "cuMemAlloc")?;
        Ok(DeviceMem {
            ctx: self,
            ptr,
            len: bytes,
        })
    }

    /// Registers an existing host allocation for zero-copy device access (`cuMemHostRegister`).
    ///
    /// Validates that `host_ptr` is non-null, `len > 0`, `len` is a multiple of 4096, and
    /// `host_ptr` is 4096-byte page aligned (SPEC §RF-1, §DT-2, Kahneman #13).
    ///
    /// # Safety
    ///
    /// The caller must ensure that `host_ptr` points to at least `len` valid, allocated host
    /// bytes that remain valid and are not deallocated while the returned [`PinnedHostMapping`] is alive.
    pub unsafe fn register_host<'c>(
        &'c self,
        host_ptr: *mut c_void,
        len: usize,
        flags: u32,
    ) -> Result<PinnedHostMapping<'c, 'a>, CudaError> {
        validate_host_registration(host_ptr, len)?;

        let fn_register = self.cuda.syms.mem_host_register.ok_or_else(|| {
            CudaError::Unsupported("cuMemHostRegister not supported by driver".into())
        })?;
        let fn_get_dptr = self.cuda.syms.mem_host_get_device_pointer.ok_or_else(|| {
            CudaError::Unsupported("cuMemHostGetDevicePointer not supported by driver".into())
        })?;
        let fn_unreg = self.cuda.syms.mem_host_unregister.ok_or_else(|| {
            CudaError::Unsupported("cuMemHostUnregister not supported by driver".into())
        })?;

        // SAFETY: host_ptr is non-null, 4096-aligned, and points to len valid bytes.
        let r = unsafe { fn_register(host_ptr, len, flags) };
        check(&self.cuda.syms, r, "cuMemHostRegister")?;

        let mut dev_ptr: CuDevicePtr = 0;
        // SAFETY: dev_ptr points to a valid local u64; host_ptr was successfully registered.
        let r = unsafe { fn_get_dptr(&mut dev_ptr, host_ptr, 0) };
        if r != CUDA_SUCCESS {
            // SAFETY: host_ptr was successfully registered and fn_unreg is required above.
            unsafe {
                let _ = fn_unreg(host_ptr);
            }
            check(&self.cuda.syms, r, "cuMemHostGetDevicePointer")?;
        }

        Ok(PinnedHostMapping {
            ctx: self,
            host_ptr,
            dev_ptr,
            len,
        })
    }
}

/// Zero-copy registered host memory mapping (`cuMemHostRegister`).
///
/// ```compile_fail
/// fn requires_send<T: Send>() {}
/// requires_send::<ramshared_cuda::PinnedHostMapping<'static, 'static>>();
/// ```
///
/// Borrows the active [`Context`]. The host memory remains mapped into the
/// GPU virtual address space for the lifetime of this struct and is cleanly
/// unregistered via `cuMemHostUnregister` on `Drop`.
pub struct PinnedHostMapping<'c, 'a> {
    ctx: &'c Context<'a>,
    host_ptr: *mut c_void,
    dev_ptr: CuDevicePtr,
    len: usize,
}

impl<'c, 'a> PinnedHostMapping<'c, 'a> {
    /// Returns the mapped CUDA device virtual pointer.
    pub fn dev_ptr(&self) -> CuDevicePtr {
        self.dev_ptr
    }

    /// Returns the length in bytes of the registered mapping.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the mapping is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a host slice view of the registered memory.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: host_ptr is non-null and points to at least len valid bytes.
        unsafe { core::slice::from_raw_parts(self.host_ptr as *const u8, self.len) }
    }

    /// Returns a mutable host slice view of the registered memory.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: host_ptr is non-null and points to at least len valid bytes.
        unsafe { core::slice::from_raw_parts_mut(self.host_ptr as *mut u8, self.len) }
    }
}

impl Drop for PinnedHostMapping<'_, '_> {
    fn drop(&mut self) {
        if let Some(fn_unreg) = self.ctx.cuda.syms.mem_host_unregister {
            // SAFETY: host_ptr was successfully registered by cuMemHostRegister.
            unsafe {
                let _ = fn_unreg(self.host_ptr);
            }
        }
    }
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        // SAFETY: raw handle was returned by cuCtxCreate and has not been destroyed yet. Best-effort drop.
        unsafe {
            let _ = (self.cuda.syms.ctx_destroy)(self.raw);
        }
    }
}

/// Allocated VRAM memory region. `Drop` implementation calls `cuMemFree`. Borrows the [`Context`].
pub struct DeviceMem<'c, 'a> {
    ctx: &'c Context<'a>,
    ptr: CuDevicePtr,
    len: usize,
}

impl DeviceMem<'_, '_> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Fills the entire region with zeroes (`cuMemsetD8` + synchronize). SPEC §6.2/§11.
    pub fn zero(&mut self) -> Result<(), CudaError> {
        let syms = &self.ctx.cuda.syms;
        // SAFETY: ptr and len accurately describe the region allocated for this memory object.
        let r = unsafe { (syms.memset_d8)(self.ptr, 0, self.len) };
        check(syms, r, "cuMemsetD8")?;
        // SAFETY: cuCtxSynchronize takes no arguments.
        let r = unsafe { (syms.ctx_synchronize)() };
        check(syms, r, "cuCtxSynchronize")
    }

    /// Copies `src` bytes into VRAM at the specified `off` offset (Host->Device, synchronous).
    pub fn write_at(&mut self, off: usize, src: &[u8]) -> Result<(), CudaError> {
        self.bounds(off, src.len())?;
        let syms = &self.ctx.cuda.syms;
        // SAFETY: offset and length validated by bounds(); src is a valid memory slice.
        let r = unsafe {
            (syms.memcpy_htod)(
                self.ptr + off as u64,
                src.as_ptr() as *const c_void,
                src.len(),
            )
        };
        check(syms, r, "cuMemcpyHtoD")
    }

    /// Copies bytes from VRAM at `off` into the `dst` buffer (Device->Host, synchronous).
    pub fn read_at(&self, off: usize, dst: &mut [u8]) -> Result<(), CudaError> {
        self.bounds(off, dst.len())?;
        let syms = &self.ctx.cuda.syms;
        // SAFETY: offset and length validated by bounds(); dst is a valid mutable slice.
        let r = unsafe {
            (syms.memcpy_dtoh)(
                dst.as_mut_ptr() as *mut c_void,
                self.ptr + off as u64,
                dst.len(),
            )
        };
        check(syms, r, "cuMemcpyDtoH")
    }

    fn bounds(&self, off: usize, len: usize) -> Result<(), CudaError> {
        match off.checked_add(len) {
            Some(end) if end <= self.len => Ok(()),
            _ => Err(CudaError::OutOfRange {
                off,
                len,
                size: self.len,
            }),
        }
    }
}

impl Drop for DeviceMem<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: ptr was returned by a successful cuMemAlloc call and has not been freed.
        unsafe {
            let _ = (self.ctx.cuda.syms.mem_free)(self.ptr);
        }
    }
}

// --- internal helpers ---

/// SAFETY: `handle` must refer to a valid open library; `name` must be a valid C-string;
/// type `T` must be a C function pointer of pointer size.
unsafe fn load_sym<T: Copy>(handle: *mut c_void, name: &CStr) -> Result<T, CudaError> {
    // SAFETY: caller contract (valid handle, null-terminated symbol name).
    let sym = unsafe { crate::loader::sym(handle, name.as_ptr()) };
    if sym.is_null() {
        return Err(CudaError::Symbol(name.to_string_lossy().into_owned()));
    }
    const {
        assert!(core::mem::size_of::<T>() == core::mem::size_of::<*mut c_void>());
    }
    // SAFETY: T is a C function pointer of the same size as a raw pointer.
    Ok(unsafe { core::mem::transmute_copy::<*mut c_void, T>(&sym) })
}

/// Optional symbol resolution (symbol may be missing in legacy stubs).
fn load_sym_opt<T: Copy>(handle: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: same preconditions as load_sym.
    unsafe { load_sym(handle, name).ok() }
}

fn check(syms: &Syms, r: CuResult, op: &'static str) -> Result<(), CudaError> {
    if r == CUDA_SUCCESS {
        Ok(())
    } else {
        Err(CudaError::Driver {
            op,
            code: r,
            msg: err_string(syms, r),
        })
    }
}

fn err_string(syms: &Syms, r: CuResult) -> String {
    if let Some(f) = syms.get_error_string {
        let mut p: *const c_char = core::ptr::null();
        // SAFETY: f is a resolved cuGetErrorString pointer; p is a valid out-pointer.
        unsafe { f(r, &mut p) };
        if !p.is_null() {
            // SAFETY: p points to a static null-terminated CUDA error message string.
            return unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        }
    }
    format!("CUresult={r}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static UNREGISTER_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn success_init(_: u32) -> CuResult {
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_device_count(count: *mut i32) -> CuResult {
        unsafe { *count = 1 };
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_device(device: *mut CuDevice, ordinal: i32) -> CuResult {
        unsafe { *device = ordinal };
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_device_name(name: *mut c_char, _: i32, _: CuDevice) -> CuResult {
        unsafe { core::ptr::copy_nonoverlapping(c"mock-gpu".as_ptr(), name, 9) };
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_context(context: *mut CuContext, _: u32, _: CuDevice) -> CuResult {
        unsafe { *context = core::ptr::NonNull::<u8>::dangling().as_ptr().cast() };
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_context_drop(_: CuContext) -> CuResult {
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_synchronize() -> CuResult {
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_alloc(ptr: *mut CuDevicePtr, _: usize) -> CuResult {
        unsafe { *ptr = 0x1000 };
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_free(_: CuDevicePtr) -> CuResult {
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_htod(_: CuDevicePtr, _: *const c_void, _: usize) -> CuResult {
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_dtoh(_: *mut c_void, _: CuDevicePtr, _: usize) -> CuResult {
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_memset(_: CuDevicePtr, _: u8, _: usize) -> CuResult {
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_mem_info(free: *mut usize, total: *mut usize) -> CuResult {
        unsafe {
            *free = 4096;
            *total = 8192;
        }
        CUDA_SUCCESS
    }
    unsafe extern "C" fn mock_error(_: CuResult, output: *mut *const c_char) -> CuResult {
        unsafe { *output = c"mock CUDA error".as_ptr() };
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_host_register(_: *mut c_void, _: usize, _: u32) -> CuResult {
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_host_unregister(_: *mut c_void) -> CuResult {
        UNREGISTER_CALLS.fetch_add(1, Ordering::SeqCst);
        CUDA_SUCCESS
    }
    unsafe extern "C" fn success_host_pointer(
        output: *mut CuDevicePtr,
        _: *mut c_void,
        _: u32,
    ) -> CuResult {
        unsafe { *output = 0x2000 };
        CUDA_SUCCESS
    }
    unsafe extern "C" fn failed_host_pointer(
        _: *mut CuDevicePtr,
        _: *mut c_void,
        _: u32,
    ) -> CuResult {
        7
    }

    fn mock_cuda(host_pointer: Option<crate::ffi::FnMemHostGetDevicePointer>) -> Cuda {
        Cuda {
            _lib: Lib(core::ptr::null_mut()),
            syms: Syms {
                init: success_init,
                device_get_count: success_device_count,
                device_get: success_device,
                device_get_name: success_device_name,
                ctx_create: success_context,
                ctx_destroy: success_context_drop,
                ctx_synchronize: success_synchronize,
                mem_alloc: success_alloc,
                mem_free: success_free,
                memcpy_htod: success_htod,
                memcpy_dtoh: success_dtoh,
                memset_d8: success_memset,
                mem_get_info: success_mem_info,
                get_error_string: Some(mock_error),
                mem_host_register: Some(success_host_register),
                mem_host_unregister: Some(success_host_unregister),
                mem_host_get_device_pointer: host_pointer,
            },
        }
    }

    fn aligned_page() -> (*mut u8, std::alloc::Layout) {
        let layout = std::alloc::Layout::from_size_align(HOST_PAGE_BYTES, HOST_PAGE_BYTES).unwrap();
        let page = unsafe { std::alloc::alloc(layout) };
        assert!(!page.is_null());
        (page, layout)
    }

    #[test]
    fn mock_driver_exercises_memory_and_mapping_raii() {
        UNREGISTER_CALLS.store(0, Ordering::SeqCst);
        let cuda = mock_cuda(Some(success_host_pointer));
        assert_eq!(cuda.device_count().unwrap(), 1);
        let device = cuda.device(0).unwrap();
        assert_eq!(device.name(), "mock-gpu");
        let context = cuda.create_context(&device).unwrap();
        assert_eq!(context.mem_info().unwrap(), (4096, 8192));

        let mut memory = context.alloc(16).unwrap();
        assert_eq!(memory.len(), 16);
        assert!(!memory.is_empty());
        memory.zero().unwrap();
        memory.write_at(0, &[1, 2, 3]).unwrap();
        let mut output = [0; 3];
        memory.read_at(0, &mut output).unwrap();
        assert!(matches!(
            memory.write_at(15, &[1, 2]),
            Err(CudaError::OutOfRange { .. })
        ));

        let (page, layout) = aligned_page();
        let mut mapping = unsafe {
            context
                .register_host(page.cast(), HOST_PAGE_BYTES, 0)
                .unwrap()
        };
        assert_eq!(mapping.dev_ptr(), 0x2000);
        assert_eq!(mapping.len(), HOST_PAGE_BYTES);
        mapping.as_mut_slice()[0] = 0x5A;
        assert_eq!(mapping.as_slice()[0], 0x5A);
        drop(mapping);
        assert_eq!(UNREGISTER_CALLS.load(Ordering::SeqCst), 1);
        unsafe { std::alloc::dealloc(page, layout) };
    }

    #[test]
    fn registration_requires_rollback_capability_and_unwinds_pointer_failures() {
        let (page, layout) = aligned_page();
        let mut missing_unregister = mock_cuda(Some(success_host_pointer));
        missing_unregister.syms.mem_host_unregister = None;
        let device = missing_unregister.device(0).unwrap();
        let context = missing_unregister.create_context(&device).unwrap();
        assert!(matches!(
            unsafe { context.register_host(page.cast(), HOST_PAGE_BYTES, 0) },
            Err(CudaError::Unsupported(message)) if message.contains("cuMemHostUnregister")
        ));
        drop(context);

        UNREGISTER_CALLS.store(0, Ordering::SeqCst);
        let failed_pointer = mock_cuda(Some(failed_host_pointer));
        let device = failed_pointer.device(0).unwrap();
        let context = failed_pointer.create_context(&device).unwrap();
        assert!(matches!(
            unsafe { context.register_host(page.cast(), HOST_PAGE_BYTES, 0) },
            Err(CudaError::Driver {
                op: "cuMemHostGetDevicePointer",
                code: 7,
                ..
            })
        ));
        assert_eq!(UNREGISTER_CALLS.load(Ordering::SeqCst), 1);
        unsafe { std::alloc::dealloc(page, layout) };
    }

    #[test]
    fn error_strings_use_driver_symbol_or_numeric_fallback() {
        let cuda = mock_cuda(Some(success_host_pointer));
        assert_eq!(err_string(&cuda.syms, 7), "mock CUDA error");
        let mut no_symbol = mock_cuda(Some(success_host_pointer));
        no_symbol.syms.get_error_string = None;
        assert_eq!(err_string(&no_symbol.syms, 7), "CUresult=7");
        assert!(matches!(
            check(&no_symbol.syms, 7, "mock"),
            Err(CudaError::Driver { .. })
        ));
    }
}
