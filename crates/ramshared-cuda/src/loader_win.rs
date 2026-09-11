//! Windows-specific dynamic library loader (using Win32 APIs).
//!
//! Covered requirements: RF-4, DT-5.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::CStr;
use windows_sys::Win32::Foundation::{FreeLibrary, GetLastError, HMODULE, MAX_PATH};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetSystemWow64DirectoryW};
use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process};

/// Checks if the current process is running under WoW64 (32-bit process on 64-bit OS).
fn is_wow64_process() -> bool {
    let mut is_wow64 = 0;
    // SAFETY: GetCurrentProcess returns a pseudo-handle that is always valid and does not need to be closed.
    let handle = unsafe { GetCurrentProcess() };
    // SAFETY: handle is valid, is_wow64 is a valid out pointer.
    let ok = unsafe { IsWow64Process(handle, &mut is_wow64) };
    ok != 0 && is_wow64 != 0
}

/// Resolves the system directory considering WoW64 redirection.
fn get_system_directory() -> String {
    let mut buf = [0u16; MAX_PATH as usize];

    if is_wow64_process() {
        // We are a 32-bit process on a 64-bit OS.
        // We might want to explicitly query the WoW64 directory if we need the 32-bit DLL.
        // SAFETY: buf is allocated with MAX_PATH which is exactly the capacity we pass.
        let len = unsafe { GetSystemWow64DirectoryW(buf.as_mut_ptr(), MAX_PATH) };
        if len > 0 && len < MAX_PATH {
            return String::from_utf16_lossy(&buf[..len as usize]);
        }
    }

    // Fallback for 64-bit processes on 64-bit OS, or 32-bit processes on 32-bit OS,
    // or if GetSystemWow64DirectoryW failed.
    // SAFETY: buf is allocated with MAX_PATH which is exactly the capacity we pass.
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), MAX_PATH) };
    if len > 0 && len < MAX_PATH {
        return String::from_utf16_lossy(&buf[..len as usize]);
    }

    String::new()
}

/// Resolves the absolute path for a filename, handling WoW64 System32/SysWOW64 paths.
pub fn resolve_path(filename: &str) -> Vec<u16> {
    let sys_dir = get_system_directory();
    if sys_dir.is_empty() {
        return filename.encode_utf16().chain(Some(0)).collect();
    }

    let full_path = format!("{}\\{}", sys_dir, filename);
    full_path.encode_utf16().chain(Some(0)).collect()
}

/// Fetches the driver version from a loaded library path or filename.
///
/// # Safety
/// The `filename` pointer must point to a valid null-terminated C-string.
#[allow(dead_code)]
pub unsafe fn driver_version(filename: *const c_char) -> Result<(u32, u32, u32, u32), String> {
    // SAFETY: the caller must provide a valid null-terminated string pointer
    let cstr = unsafe { CStr::from_ptr(filename) };
    let string_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return Err("Invalid filename string".to_string()),
    };

    let wide = resolve_path(string_str);

    // SAFETY: wide is a valid wide string buffer
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), core::ptr::null_mut()) };
    if size == 0 {
        return Err("GetFileVersionInfoSizeW failed".to_string());
    }

    let mut buf = vec![0u8; size as usize];
    // SAFETY: buf has size bytes allocated and wide is a valid wide string
    let ok = unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr() as *mut c_void) };
    if ok == 0 {
        return Err("GetFileVersionInfoW failed".to_string());
    }

    let mut info_ptr: *mut c_void = core::ptr::null_mut();
    let mut info_len = 0;
    let sub_block: Vec<u16> = "\\\0".encode_utf16().chain(Some(0)).collect();

    // SAFETY: buf contains valid version info, sub_block is a valid wide string, info_ptr and info_len are populated
    let ok = unsafe { VerQueryValueW(buf.as_ptr() as *const c_void, sub_block.as_ptr(), &mut info_ptr, &mut info_len) };
    if ok == 0 || info_ptr.is_null() || info_len == 0 {
        return Err("VerQueryValueW failed".to_string());
    }

    // SAFETY: info_ptr points to a valid VS_FIXEDFILEINFO structure
    let info = unsafe { &*(info_ptr as *const VS_FIXEDFILEINFO) };

    let v1 = info.dwFileVersionMS >> 16;
    let v2 = info.dwFileVersionMS & 0xFFFF;
    let v3 = info.dwFileVersionLS >> 16;
    let v4 = info.dwFileVersionLS & 0xFFFF;

    Ok((v1, v2, v3, v4))
}

/// Opens the specified dynamic library (converts CStr to UTF-16).
///
/// # Safety
/// The `filename` pointer must point to a valid null-terminated C-string.
pub unsafe fn open(filename: *const c_char) -> *mut c_void {
    // SAFETY: caller guarantees filename is a valid null-terminated C-string
    let cstr = unsafe { CStr::from_ptr(filename) };
    let string_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return core::ptr::null_mut(),
    };

    let wide = resolve_path(string_str);
    // SAFETY: wide is a valid wide string
    let mut handle = unsafe { LoadLibraryW(wide.as_ptr()) };

    // If not found, try the direct name as fallback (in case it's in PATH or app directory)
    if handle.is_null() {
        let direct_wide: Vec<u16> = string_str.encode_utf16().chain(Some(0)).collect();
        // SAFETY: direct_wide is a valid wide string
        handle = unsafe { LoadLibraryW(direct_wide.as_ptr()) };
    }

    if handle.is_null() {
        return core::ptr::null_mut();
    }
    handle
}

/// Loads the specified symbol from the library.
///
/// # Safety
/// `handle` must be a valid pointer returned by `open`.
/// `symbol` must point to a valid null-terminated C-string.
pub unsafe fn sym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    if handle.is_null() {
        return core::ptr::null_mut();
    }
    // GetProcAddress returns Option<unsafe extern "system" fn() -> isize> in windows-sys 0.61.
    // SAFETY: handle is a valid module handle and symbol is a valid C-string
    let addr = unsafe { GetProcAddress(handle as HMODULE, symbol as *const u8) };
    match addr {
        Some(f) => f as *mut c_void,
        None => core::ptr::null_mut(),
    }
}

/// Closes the dynamic library.
///
/// # Safety
/// `handle` must be a valid pointer returned by `open` that has not been closed yet.
pub unsafe fn close(handle: *mut c_void) -> c_int {
    if handle.is_null() {
        return 0;
    }
    // SAFETY: handle is a valid module handle
    let ok = unsafe { FreeLibrary(handle as HMODULE) };
    if ok != 0 {
        0 // Returns 0 for success, matching dlclose behavior
    } else {
        -1 // Failure
    }
}

/// Returns the last error message generated by the loader using GetLastError().
pub fn error() -> String {
    // SAFETY: calling GetLastError is always safe and has no requirements
    let code = unsafe { GetLastError() };
    format!("Windows error code: 0x{code:08X}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use windows_sys::Win32::Foundation::SetLastError;

    #[test]
    fn test_error_formatting() {
        // SAFETY: calling SetLastError in tests is safe as we immediately check it
        unsafe { SetLastError(0x00000005) };
        assert_eq!(error(), "Windows error code: 0x00000005");

        // SAFETY: calling SetLastError in tests is safe
        unsafe { SetLastError(0xC0000005) };
        assert_eq!(error(), "Windows error code: 0xC0000005");

        // SAFETY: calling SetLastError in tests is safe
        unsafe { SetLastError(0) };
        assert_eq!(error(), "Windows error code: 0x00000000");
    }

    #[test]
    fn test_resolve_path_not_empty() {
        let path = resolve_path("nvcuda.dll");
        assert!(!path.is_empty());
    }
}
