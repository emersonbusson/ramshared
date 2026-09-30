//! NVML (NVIDIA Management Library) device-wide VRAM occupancy.
//!
//! `cuMemGetInfo` is the wrong authority for a VRAM budget on WSL2 GPU-PV: the
//! paravirtual CUDA shim accounts the calling process's channel, so free/total
//! never move when another process allocates device memory. A containment chain
//! built on that number cannot yield to a GPU application.
//!
//! `nvmlDeviceGetMemoryInfo` reports the whole adapter and does observe every
//! process, which is what the budget must describe. NVML ships with the same
//! NVIDIA driver package as `libcuda.so.1`, so it is loaded the same way: at
//! runtime, through the platform loader, with no build-time toolkit dependency.

use core::ffi::{c_char, c_int, c_uint, c_void};
use core::fmt;
use std::ffi::CStr;

use crate::driver::{Lib, load_sym, load_sym_opt};

/// `nvmlReturn_t`. `NVML_SUCCESS` shares the zero value with `CUDA_SUCCESS`.
pub type NvmlResult = c_int;
/// `NVML_SUCCESS`.
pub const NVML_SUCCESS: NvmlResult = 0;

/// `nvmlDevice_t` — opaque device handle.
type NvmlDevice = *mut c_void;

/// `nvmlMemory_t` — device-wide memory accounting in bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmlMemory {
    /// Total physical device memory.
    pub total: u64,
    /// Unallocated device memory.
    pub free: u64,
    /// Device memory allocated by all contexts on the adapter.
    pub used: u64,
}

type FnNvmlInit = unsafe extern "C" fn() -> NvmlResult;
type FnNvmlShutdown = unsafe extern "C" fn() -> NvmlResult;
type FnNvmlDeviceGetHandleByIndex = unsafe extern "C" fn(c_uint, *mut NvmlDevice) -> NvmlResult;
type FnNvmlDeviceGetMemoryInfo = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> NvmlResult;
type FnNvmlErrorString = unsafe extern "C" fn(NvmlResult) -> *const c_char;

struct NvmlSyms {
    init: FnNvmlInit,
    shutdown: FnNvmlShutdown,
    device_get_handle_by_index: FnNvmlDeviceGetHandleByIndex,
    device_get_memory_info: FnNvmlDeviceGetMemoryInfo,
    error_string: Option<FnNvmlErrorString>,
}

/// A loaded and initialized NVML library.
pub struct Nvml {
    _lib: Lib,
    syms: NvmlSyms,
}

#[cfg(unix)]
const CANDIDATES: &[&CStr] = &[
    c"libnvidia-ml.so.1",
    c"/usr/lib/wsl/lib/libnvidia-ml.so.1",
    c"libnvidia-ml.so",
    c"/usr/lib/x86_64-linux-gnu/libnvidia-ml.so.1",
];

#[cfg(windows)]
const CANDIDATES: &[&CStr] = &[c"nvml.dll"];

/// Error raised while loading NVML or reading device memory.
#[derive(Debug)]
pub enum NvmlError {
    /// No candidate library could be opened.
    Load(String),
    /// A required exported symbol is missing.
    Symbol(String),
    /// An NVML call returned a non-success status.
    Call {
        /// Operation that failed.
        op: &'static str,
        /// `nvmlReturn_t` code.
        code: NvmlResult,
        /// Driver-provided description, when available.
        msg: String,
    },
}

impl fmt::Display for NvmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NvmlError::Load(s) => write!(f, "failed to load NVML library: {s}"),
            NvmlError::Symbol(s) => write!(f, "required NVML symbol missing: {s}"),
            NvmlError::Call { op, code, msg } => {
                write!(f, "{op} failed (nvmlReturn_t={code}): {msg}")
            }
        }
    }
}

impl std::error::Error for NvmlError {}

impl Nvml {
    /// Loads NVML and runs `nvmlInit_v2`.
    pub fn load() -> Result<Self, NvmlError> {
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
            return Err(NvmlError::Load(crate::loader::error()));
        }
        let lib = Lib(handle);

        // SAFETY: handle is an active library context; each resolved symbol is a
        // valid NVML function pointer conforming to the signatures above.
        let syms = unsafe {
            NvmlSyms {
                init: load_sym(handle, c"nvmlInit_v2").map_err(map_sym)?,
                shutdown: load_sym(handle, c"nvmlShutdown").map_err(map_sym)?,
                device_get_handle_by_index: load_sym(handle, c"nvmlDeviceGetHandleByIndex_v2")
                    .or_else(|_| load_sym(handle, c"nvmlDeviceGetHandleByIndex"))
                    .map_err(map_sym)?,
                device_get_memory_info: load_sym(handle, c"nvmlDeviceGetMemoryInfo")
                    .map_err(map_sym)?,
                error_string: load_sym_opt(handle, c"nvmlErrorString"),
            }
        };

        // SAFETY: init symbol resolved successfully.
        let r = unsafe { (syms.init)() };
        if r != NVML_SUCCESS {
            return Err(NvmlError::Call {
                op: "nvmlInit_v2",
                code: r,
                msg: err_string(&syms, r),
            });
        }

        Ok(Nvml { _lib: lib, syms })
    }

    /// Returns device-wide used/free/total memory in bytes for `ordinal`.
    ///
    /// `used` is the sum over every process on the adapter, which is exactly
    /// the occupancy a VRAM budget must track to yield to other applications.
    pub fn device_memory(&self, ordinal: i32) -> Result<NvmlMemory, NvmlError> {
        if ordinal < 0 {
            return Err(NvmlError::Call {
                op: "nvmlDeviceGetHandleByIndex_v2",
                code: -1,
                msg: format!("negative device ordinal {ordinal}"),
            });
        }
        let mut device: NvmlDevice = core::ptr::null_mut();
        // SAFETY: device is a valid out-pointer for an opaque handle.
        let r = unsafe { (self.syms.device_get_handle_by_index)(ordinal as c_uint, &mut device) };
        self.check(r, "nvmlDeviceGetHandleByIndex_v2")?;
        if device.is_null() {
            return Err(NvmlError::Call {
                op: "nvmlDeviceGetHandleByIndex_v2",
                code: -1,
                msg: "null device handle".into(),
            });
        }

        let mut memory = NvmlMemory::default();
        // SAFETY: device is a resolved handle; memory is a valid out-pointer.
        let r = unsafe { (self.syms.device_get_memory_info)(device, &mut memory) };
        self.check(r, "nvmlDeviceGetMemoryInfo")?;
        Ok(memory)
    }

    fn check(&self, r: NvmlResult, op: &'static str) -> Result<(), NvmlError> {
        if r == NVML_SUCCESS {
            Ok(())
        } else {
            Err(NvmlError::Call {
                op,
                code: r,
                msg: err_string(&self.syms, r),
            })
        }
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: the library was initialized successfully and is still mapped.
        unsafe {
            (self.syms.shutdown)();
        }
    }
}

fn map_sym(error: crate::driver::CudaError) -> NvmlError {
    match error {
        crate::driver::CudaError::Symbol(name) => NvmlError::Symbol(name),
        other => NvmlError::Symbol(other.to_string()),
    }
}

fn err_string(syms: &NvmlSyms, r: NvmlResult) -> String {
    if let Some(f) = syms.error_string {
        // SAFETY: f is a resolved nvmlErrorString; the returned pointer is a
        // driver-owned NUL-terminated string valid for the duration of the call.
        let p = unsafe { f(r) };
        if !p.is_null() {
            // SAFETY: p is a non-null NUL-terminated C string from the driver.
            let s = unsafe { CStr::from_ptr(p) };
            return s.to_string_lossy().into_owned();
        }
    }
    format!("nvmlReturn_t={r}")
}

/// Test-only NVML whose device-wide numbers are injected.
///
/// A budget test must be able to make NVML disagree with `cuMemGetInfo` on
/// purpose: that divergence is exactly the WSL2 GPU-PV failure this module
/// exists to correct, and a mock that cannot express it cannot guard the fix.
#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use core::cell::Cell;

    thread_local! {
        static MEM: Cell<(u64, u64, u64)> = const { Cell::new((0, 0, 0)) };
    }

    /// Sets `(used, free, total)` returned by the mock, in bytes.
    pub(crate) fn set_device_memory(used: u64, free: u64, total: u64) {
        MEM.with(|cell| cell.set((used, free, total)));
    }

    unsafe extern "C" fn mock_init() -> NvmlResult {
        NVML_SUCCESS
    }

    unsafe extern "C" fn mock_shutdown() -> NvmlResult {
        NVML_SUCCESS
    }

    unsafe extern "C" fn mock_handle(ordinal: c_uint, device: *mut NvmlDevice) -> NvmlResult {
        // SAFETY: device is a valid out-pointer provided by the caller.
        unsafe { *device = (ordinal as usize + 1) as NvmlDevice };
        NVML_SUCCESS
    }

    unsafe extern "C" fn mock_memory(device: NvmlDevice, memory: *mut NvmlMemory) -> NvmlResult {
        if device.is_null() {
            return -1;
        }
        let (used, free, total) = MEM.with(|cell| cell.get());
        // SAFETY: memory is a valid out-pointer provided by the caller.
        unsafe { *memory = NvmlMemory { total, free, used } };
        NVML_SUCCESS
    }

    /// Builds a mock NVML handle for unit tests.
    pub(crate) fn build() -> Nvml {
        Nvml {
            _lib: Lib(core::ptr::null_mut()),
            syms: NvmlSyms {
                init: mock_init,
                shutdown: mock_shutdown,
                device_get_handle_by_index: mock_handle,
                device_get_memory_info: mock_memory,
                error_string: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    // unwrap/expect allowed in tests only (coding.md rules).
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn nvml_memory_layout_matches_driver_struct() {
        // nvmlMemory_t is three unsigned long long fields with no padding.
        assert_eq!(core::mem::size_of::<NvmlMemory>(), 24);
        assert_eq!(core::mem::align_of::<NvmlMemory>(), 8);
        assert_eq!(core::mem::offset_of!(NvmlMemory, total), 0);
        assert_eq!(core::mem::offset_of!(NvmlMemory, free), 8);
        assert_eq!(core::mem::offset_of!(NvmlMemory, used), 16);
    }

    #[test]
    fn error_display_is_descriptive() {
        let e = NvmlError::Call {
            op: "nvmlDeviceGetMemoryInfo",
            code: 3,
            msg: "not supported".into(),
        };
        let s = e.to_string();
        assert!(s.contains("nvmlDeviceGetMemoryInfo"));
        assert!(s.contains("nvmlReturn_t=3"));
    }

    #[test]
    fn negative_ordinal_is_rejected() {
        // No library needed: the guard runs before any driver call.
        let e = NvmlError::Call {
            op: "nvmlDeviceGetHandleByIndex_v2",
            code: -1,
            msg: format!("negative device ordinal {}", -1),
        };
        assert!(e.to_string().contains("negative device ordinal"));
    }
}
