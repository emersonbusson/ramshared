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
    OutOfRange { off: usize, len: usize, size: usize },
}

impl fmt::Display for CudaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CudaError::Load(s) => write!(f, "failed to load CUDA library: {s}"),
            CudaError::Symbol(s) => write!(f, "required CUDA symbol missing: {s}"),
            CudaError::Driver { op, code, msg } => {
                write!(f, "{op} failed (CUresult={code}): {msg}")
            }
            CudaError::OutOfRange { off, len, size } => {
                write!(f, "out of bounds access: off={off} len={len} > size={size}")
            }
        }
    }
}

impl core::error::Error for CudaError {}

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
const CANDIDATES: &[&CStr] = &[
    c"nvcuda.dll",
];

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

        Ok(Device { raw, name })
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
}

impl Device {
    pub fn name(&self) -> &str {
        &self.name
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
