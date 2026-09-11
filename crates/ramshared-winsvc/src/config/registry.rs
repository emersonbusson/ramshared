#[cfg(windows)]
use crate::config::{BrokerPipeV1, ConfigError, WinDriveConfig};
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE,
    KEY_READ, REG_DWORD, REG_NOTIFY_CHANGE_LAST_SET, REG_SZ,
};

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Reads WinDrive configuration from the Windows Registry.
///
/// Looks under `HKLM\SOFTWARE\RamShared\WinDrive`.
#[cfg(windows)]
pub fn read_config_from_registry() -> Result<WinDriveConfig, ConfigError> {
    let subkey = to_wide(r"SOFTWARE\RamShared\WinDrive");
    let mut hkey = std::ptr::null_mut();
    // SAFETY: FFI call to open registry key, handles are properly checked.
    let open =
        unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
    if open != 0 {
        return Err(ConfigError::Parse(format!(
            "RegOpenKeyExW failed status={open}"
        )));
    }

    let read_dword = |name: &str| -> Result<u32, ConfigError> {
        let name_w = to_wide(name);
        let mut ty = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut val = 0u32;
        // SAFETY: FFI call with valid buffers and pointers to read a DWORD.
        let q = unsafe {
            RegQueryValueExW(
                hkey,
                name_w.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                &mut val as *mut _ as *mut u8,
                &mut size,
            )
        };
        if q != 0 {
            return Err(ConfigError::Parse(format!(
                "RegQueryValueExW failed for {name} status={q}"
            )));
        }
        if ty != REG_DWORD {
            return Err(ConfigError::Parse(format!(
                "Unexpected registry type for {name} ty={ty}"
            )));
        }
        Ok(val)
    };

    let read_u64 = |name: &str| -> Result<u64, ConfigError> {
        let name_w = to_wide(name);
        let mut ty = 0u32;
        let mut size = std::mem::size_of::<u64>() as u32;
        let mut val = 0u64;
        // SAFETY: FFI call with valid buffers and pointers to read a QWORD.
        let q = unsafe {
            RegQueryValueExW(
                hkey,
                name_w.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                &mut val as *mut _ as *mut u8,
                &mut size,
            )
        };
        if q != 0 {
            return Err(ConfigError::Parse(format!(
                "RegQueryValueExW failed for {name} status={q}"
            )));
        }
        if ty != windows_sys::Win32::System::Registry::REG_QWORD {
            return Err(ConfigError::Parse(format!(
                "Unexpected registry type for {name} ty={ty}"
            )));
        }
        Ok(val)
    };

    let read_string = |name: &str| -> Result<String, ConfigError> {
        let name_w = to_wide(name);
        let mut ty = 0u32;
        let mut size = 0u32;
        // SAFETY: FFI call to read buffer size.
        let q1 = unsafe {
            RegQueryValueExW(
                hkey,
                name_w.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if q1 != 0 {
            return Err(ConfigError::Parse(format!(
                "RegQueryValueExW length query failed for {name} status={q1}"
            )));
        }
        if ty != REG_SZ {
            return Err(ConfigError::Parse(format!(
                "Unexpected registry type for {name} ty={ty}"
            )));
        }
        let mut buf = vec![0u8; size as usize];
        // SAFETY: FFI call to populate string buffer of known size.
        let q2 = unsafe {
            RegQueryValueExW(
                hkey,
                name_w.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                buf.as_mut_ptr(),
                &mut size,
            )
        };
        if q2 != 0 {
            return Err(ConfigError::Parse(format!(
                "RegQueryValueExW read failed for {name} status={q2}"
            )));
        }
        // SAFETY: Re-interpret u8 buffer safely into u16 slice.
        let u16_slice: &[u16] =
            unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u16, buf.len() / 2) };
        let mut len = u16_slice.len();
        if len > 0 && u16_slice[len - 1] == 0 {
            len -= 1;
        }
        let s = String::from_utf16(&u16_slice[..len])
            .map_err(|e| ConfigError::Parse(format!("Invalid UTF-16 for {name}: {e}")))?;
        Ok(s)
    };

    let size_bytes = read_u64("SizeBytes")?;
    let block_size = read_dword("BlockSize")?;
    let cuda_device = read_dword("CudaDevice")?;
    let reserve_bytes = read_u64("ReserveBytes")?;
    let queue_depth = read_dword("QueueDepth")?;
    let max_io_bytes = read_dword("MaxIoBytes")?;
    let evidence_path_str = read_string("EvidencePath")?;
    let evidence_path = PathBuf::from(evidence_path_str);
    let volume_letter_str = read_string("VolumeLetter")?;
    let volume_letter = volume_letter_str
        .chars()
        .next()
        .ok_or_else(|| ConfigError::Parse("VolumeLetter is empty".into()))?;

    let volume_mount_path_str = read_string("VolumeMountPath").ok();
    let volume_mount_path = volume_mount_path_str.map(PathBuf::from);

    let broker_pipe_str = read_string("BrokerPipe")?;
    let broker_pipe = if broker_pipe_str == "named_pipe_v1" {
        BrokerPipeV1::NamedPipeV1
    } else {
        return Err(ConfigError::Parse(format!(
            "Invalid BrokerPipe value: {broker_pipe_str}"
        )));
    };

    let broker_ready_timeout_secs = read_u64("BrokerReadyTimeoutSecs")?;
    let tenant = read_string("Tenant")?;
    let heartbeat_secs = read_u64("HeartbeatSecs").unwrap_or(5);

    // SAFETY: Closes registry key handle securely.
    unsafe { RegCloseKey(hkey) };

    let config = WinDriveConfig {
        size_bytes,
        block_size,
        cuda_device,
        reserve_bytes,
        queue_depth,
        max_io_bytes,
        evidence_path,
        volume_letter,
        volume_mount_path,
        broker_pipe,
        broker_ready_timeout_secs,
        tenant,
        heartbeat_secs,
    };

    config.validate()?;
    Ok(config)
}

/// Reads WinDrive configuration from the Windows Registry (stub for non-Windows platforms).
#[cfg(not(windows))]
pub fn read_config_from_registry(
) -> Result<crate::config::WinDriveConfig, crate::config::ConfigError> {
    Err(crate::config::ConfigError::Parse(
        "Registry config requires Windows".into(),
    ))
}

/// Blocks waiting for config changes or an explicit shutdown event.
///
/// Returns `true` if a configuration change occurred, and `false` if `shutdown_event` fired.
#[cfg(windows)]
pub fn watch_config_registry(
    shutdown_event: windows_sys::Win32::Foundation::HANDLE,
) -> Result<bool, ConfigError> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects};

    let subkey = to_wide(r"SOFTWARE\RamShared\WinDrive");
    let mut hkey = std::ptr::null_mut();
    // SAFETY: FFI call to open registry key.
    let open =
        unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
    if open != 0 {
        return Err(ConfigError::Parse(format!(
            "RegOpenKeyExW failed for notify status={open}"
        )));
    }

    // SAFETY: FFI call to create an event handle securely.
    let notify_event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if notify_event.is_null() {
        // SAFETY: Closes registry key handle.
        unsafe { RegCloseKey(hkey) };
        return Err(ConfigError::Parse("CreateEventW failed".into()));
    }

    // SAFETY: Subscribes the event to key changes.
    let notify_res = unsafe {
        RegNotifyChangeKeyValue(
            hkey,
            1, // watch subkeys
            REG_NOTIFY_CHANGE_LAST_SET,
            notify_event,
            1, // asynchronous
        )
    };

    if notify_res != 0 {
        // SAFETY: Safely dispose of resources before exiting.
        unsafe {
            CloseHandle(notify_event);
            RegCloseKey(hkey);
        }
        return Err(ConfigError::Parse(format!(
            "RegNotifyChangeKeyValue failed status={notify_res}"
        )));
    }

    let handles = [shutdown_event, notify_event];
    // SAFETY: Blocking wait on valid handles.
    let wait_res = unsafe {
        WaitForMultipleObjects(
            2,
            handles.as_ptr(),
            0,
            windows_sys::Win32::System::Threading::INFINITE,
        )
    };

    // SAFETY: Cleanup.
    unsafe {
        CloseHandle(notify_event);
        RegCloseKey(hkey);
    }

    if wait_res == WAIT_OBJECT_0 {
        // shutdown event
        Ok(false)
    } else if wait_res == WAIT_OBJECT_0 + 1 {
        // notify event -> config changed
        Ok(true)
    } else {
        Err(ConfigError::Parse(format!(
            "WaitForMultipleObjects failed or abandoned {wait_res}"
        )))
    }
}

/// Watches configuration (stub for non-Windows platforms).
#[cfg(not(windows))]
pub fn watch_config_registry(
) -> Result<bool, crate::config::ConfigError> {
    Err(crate::config::ConfigError::Parse(
        "Registry config watching requires Windows".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn registry_config_fails_on_non_windows() {
        let err = read_config_from_registry()
            .err()
            .unwrap_or_else(|| unreachable!());
        assert!(matches!(err, crate::config::ConfigError::Parse(_)));
    }
}
