//! MiniDumpWriteDump integration for assertion failure and panics.
//!
//! Generates crash dumps for post-mortem analysis on Windows.

#[cfg(windows)]
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, GetLastError};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, GENERIC_WRITE,
};
#[cfg(windows)]
use windows_sys::Win32::System::Diagnostics::Debug::{
    MiniDumpWriteDump, MINIDUMP_EXCEPTION_INFORMATION, MiniDumpWithFullMemory, MINIDUMP_TYPE,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};
#[cfg(windows)]
use std::ptr;

/// Configuration for minidump generation.
pub struct DumpConfig {
    /// Directory where the dump file will be created.
    pub directory: std::path::PathBuf,
    /// Prefix for the generated dump filename.
    pub prefix: String,
    /// Dump type (e.g., MiniDumpWithFullMemory). Defaults to standard dump if not full.
    pub full_memory: bool,
}

impl Default for DumpConfig {
    fn default() -> Self {
        Self {
            directory: std::env::temp_dir(),
            prefix: "ramshared-winsvc-crash".to_string(),
            full_memory: false,
        }
    }
}

/// Generate a minidump at the specified directory.
/// Returns the path to the written dump file on success.
#[cfg(windows)]
pub fn write_minidump(config: &DumpConfig) -> Result<std::path::PathBuf, u32> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let filename = format!("{}-{}-{}.dmp", config.prefix, unsafe { GetCurrentProcessId() }, now);
    let dump_path = config.directory.join(filename);

    let path_w: Vec<u16> = dump_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: We provide a null-terminated wide string for the path.
    let h_file = unsafe {
        CreateFileW(
            path_w.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ,
            ptr::null(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            0 as _, // NULL handle
        )
    };

    if h_file == INVALID_HANDLE_VALUE {
        // SAFETY: GetLastError is safe to call.
        return Err(unsafe { GetLastError() });
    }

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            // SAFETY: CloseHandle is safe to call on a valid handle.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
        }
    }
    let _guard = HandleGuard(h_file);

    let process = unsafe { GetCurrentProcess() };
    let process_id = unsafe { GetCurrentProcessId() };

    let dump_type: MINIDUMP_TYPE = if config.full_memory {
        MiniDumpWithFullMemory
    } else {
        windows_sys::Win32::System::Diagnostics::Debug::MiniDumpNormal
    };

    // SAFETY: MiniDumpWriteDump writes to a valid file handle.
    let success = unsafe {
        MiniDumpWriteDump(
            process,
            process_id,
            h_file,
            dump_type,
            ptr::null(), // No exception info
            ptr::null(), // No user streams
            ptr::null(), // No callback
        )
    };

    if success == 0 {
        // SAFETY: GetLastError is safe to call.
        return Err(unsafe { GetLastError() });
    }

    Ok(dump_path)
}

/// Generates a minidump if configured. Returns an error on non-Windows platforms.
#[cfg(not(windows))]
pub fn write_minidump(_config: &DumpConfig) -> Result<std::path::PathBuf, u32> {
    Err(50) // ERROR_NOT_SUPPORTED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_config_default_is_sane() {
        let config = DumpConfig::default();
        assert_eq!(config.prefix, "ramshared-winsvc-crash");
        assert!(!config.full_memory);
        assert!(config.directory.exists());
    }

    #[test]
    #[cfg(not(windows))]
    fn write_minidump_returns_error_on_non_windows() {
        let config = DumpConfig::default();
        assert_eq!(write_minidump(&config), Err(50));
    }
}
