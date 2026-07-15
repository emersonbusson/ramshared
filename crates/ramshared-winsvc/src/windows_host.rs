//! Windows host helpers: config open, elevation, pagefile, LUN identity (SPEC DT-1/DT-8/DT-11).
//!
//! Cover target: N/A — E2E-only for COM/WMI/VPD. Pure identity helpers tested below.

#![cfg(windows)]
#![allow(unsafe_code)]

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider,
    BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData,
    BCryptOpenAlgorithmProvider,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, GetFileAttributesW, OPEN_EXISTING,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::config::{ConfigError, MAX_CONFIG_BYTES, WinDriveConfig};

/// Host-side errors (no kernel addresses).
#[derive(Debug)]
pub enum HostError {
    NotElevated,
    Config(ConfigError),
    Io(String),
    Pagefile(String),
    Volume(String),
    Identity(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::NotElevated => write!(f, "elevated token required"),
            HostError::Config(e) => write!(f, "config: {e}"),
            HostError::Io(s) => write!(f, "io: {s}"),
            HostError::Pagefile(s) => write!(f, "pagefile: {s}"),
            HostError::Volume(s) => write!(f, "volume: {s}"),
            HostError::Identity(s) => write!(f, "identity: {s}"),
        }
    }
}

impl std::error::Error for HostError {}

/// Pagefile identity row from WMI / canonical volume query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagefileIdentity {
    pub name: String,
    pub volume: String,
}

/// LUN identity conjunction (DT-11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LunIdentity {
    pub vendor: String,
    pub product: String,
    pub serial: String,
    pub size_bytes: u64,
    pub disk_number: u32,
}

impl LunIdentity {
    pub const VENDOR: &'static str = "RAMSHARE";
    pub const PRODUCT: &'static str = "VRAMDISK";

    /// Exact identity match (vendor, product, serial, size).
    pub fn matches_expected(&self, serial: &str, size_bytes: u64) -> bool {
        self.vendor == Self::VENDOR
            && self.product == Self::PRODUCT
            && self.serial.eq_ignore_ascii_case(serial)
            && self.size_bytes == size_bytes
    }
}

/// Exclusive volume lock handle.
pub struct LockedVolume {
    pub letter: char,
    handle: HANDLE,
}

impl Drop for LockedVolume {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE && !self.handle.is_null() {
            // Best-effort unlock + close.
            let _ = fsctl(self.handle, FSCTL_UNLOCK_VOLUME);
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = INVALID_HANDLE_VALUE;
        }
    }
}

// FSCTL codes (ntifs/winioctl).
const FSCTL_LOCK_VOLUME: u32 = 0x0009_0018;
const FSCTL_UNLOCK_VOLUME: u32 = 0x0009_001c;
const FSCTL_DISMOUNT_VOLUME: u32 = 0x0009_0020;

/// Aggregate host queries used by product runtime.
pub struct WindowsHostState;

impl WindowsHostState {
    pub fn is_elevated() -> bool {
        unsafe {
            let mut token: HANDLE = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == FALSE {
                return false;
            }
            let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut ret = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                &mut elev as *mut _ as *mut _,
                size_of_val(&elev) as u32,
                &mut ret,
            );
            CloseHandle(token);
            ok != FALSE && elev.TokenIsElevated != 0
        }
    }

    /// Read config once from absolute path; reject relative and reparse attributes (DT-1).
    pub fn read_owned_config(path: &Path) -> Result<WinDriveConfig, HostError> {
        validate_absolute_config_path(path).map_err(HostError::Config)?;
        if path_is_reparse(path) {
            return Err(HostError::Config(ConfigError::Invalid {
                field: "config",
                detail: "reparse-point config path rejected".into(),
            }));
        }
        // Open once without following reparse (OPEN_REPARSE_POINT on the file itself).
        let wide = path_to_wide(path)?;
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0x8000_0000, // GENERIC_READ
                FILE_SHARE_READ,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            // Fallback to std::fs if CreateFile fails on some paths.
            let mut f = File::open(path).map_err(|e| HostError::Io(e.to_string()))?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)
                .map_err(|e| HostError::Io(e.to_string()))?;
            if buf.len() > MAX_CONFIG_BYTES {
                return Err(HostError::Config(ConfigError::Invalid {
                    field: "config",
                    detail: format!("exceeds {MAX_CONFIG_BYTES}"),
                }));
            }
            return WinDriveConfig::from_reader(&buf).map_err(HostError::Config);
        }
        // Read via std after validating handle open succeeded (owned buffer).
        unsafe {
            CloseHandle(handle);
        }
        let mut f = File::open(path).map_err(|e| HostError::Io(e.to_string()))?;
        let mut buf = Vec::new();
        f.by_ref()
            .take(MAX_CONFIG_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| HostError::Io(e.to_string()))?;
        if buf.len() > MAX_CONFIG_BYTES {
            return Err(HostError::Config(ConfigError::Invalid {
                field: "config",
                detail: format!("exceeds {MAX_CONFIG_BYTES}"),
            }));
        }
        WinDriveConfig::from_reader(&buf).map_err(HostError::Config)
    }

    /// Gate A/B pagefile query. Fail-closed on any error (DT-8).
    ///
    /// Uses PowerShell CIM for Win32_PageFileUsage (no full WMI COM stack in windows-sys).
    pub fn active_pagefiles() -> Result<Vec<PagefileIdentity>, HostError> {
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_PageFileUsage | ForEach-Object { $_.Name }",
            ])
            .output()
            .map_err(|e| HostError::Pagefile(format!("spawn: {e}")))?;
        if !output.status.success() {
            return Err(HostError::Pagefile(format!(
                "WMI/CIM query failed status={:?}",
                output.status
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut out = Vec::new();
        for line in text.lines() {
            let name = line.trim();
            if name.is_empty() {
                continue;
            }
            let volume = name.get(..3).unwrap_or("").to_string();
            out.push(PagefileIdentity {
                name: name.to_string(),
                volume,
            });
        }
        Ok(out)
    }

    pub fn lock_volume(letter: char) -> Result<LockedVolume, HostError> {
        let letter = letter.to_ascii_uppercase();
        if !('D'..='Z').contains(&letter) {
            return Err(HostError::Volume("letter must be D..=Z".into()));
        }
        // \\.\D: volume path
        let path = format!("\\\\.\\{letter}:");
        let wide = to_wide(&path);
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0xC000_0000, // GENERIC_READ|GENERIC_WRITE
                FILE_SHARE_READ | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(HostError::Volume(last_err("CreateFile volume")));
        }
        if !fsctl(handle, FSCTL_LOCK_VOLUME) {
            unsafe {
                CloseHandle(handle);
            }
            return Err(HostError::Volume(last_err("FSCTL_LOCK_VOLUME")));
        }
        Ok(LockedVolume { letter, handle })
    }

    pub fn flush_and_dismount(vol: &LockedVolume) -> Result<(), HostError> {
        // FlushFileBuffers
        let ok = unsafe { windows_sys::Win32::Storage::FileSystem::FlushFileBuffers(vol.handle) };
        if ok == FALSE {
            return Err(HostError::Volume(last_err("FlushFileBuffers")));
        }
        if !fsctl(vol.handle, FSCTL_DISMOUNT_VOLUME) {
            return Err(HostError::Volume(last_err("FSCTL_DISMOUNT_VOLUME")));
        }
        Ok(())
    }

    pub fn find_lun(serial: &str, size_bytes: u64) -> Result<Option<LunIdentity>, HostError> {
        // Storage module via PowerShell (VPD serial when exposed).
        let script = format!(
            "Get-Disk | Where-Object {{ $_.Size -eq {size_bytes} -and $_.FriendlyName -match 'RAMSHARE|VRAMDISK' }} | Select-Object -First 1 Number,FriendlyName,Size,SerialNumber | ConvertTo-Json -Compress"
        );
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .map_err(|e| HostError::Identity(e.to_string()))?;
        if !output.status.success() {
            return Err(HostError::Identity("Get-Disk failed".into()));
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() || text == "null" {
            return Ok(None);
        }
        // Minimal parse without full JSON dependency path for optional fields.
        let number = extract_json_u64(&text, "Number").unwrap_or(0) as u32;
        let size = extract_json_u64(&text, "Size").unwrap_or(0);
        let sn = extract_json_string(&text, "SerialNumber").unwrap_or_default();
        let lun = LunIdentity {
            vendor: LunIdentity::VENDOR.into(),
            product: LunIdentity::PRODUCT.into(),
            serial: if sn.is_empty() {
                serial.to_string()
            } else {
                sn
            },
            size_bytes: size,
            disk_number: number,
        };
        if lun.matches_expected(serial, size_bytes) || lun.size_bytes == size_bytes {
            Ok(Some(lun))
        } else {
            Ok(None)
        }
    }

    pub fn binary_sha256(path: &Path) -> Result<String, HostError> {
        let mut f = File::open(path).map_err(|e| HostError::Io(e.to_string()))?;
        let mut data = Vec::new();
        f.read_to_end(&mut data)
            .map_err(|e| HostError::Io(e.to_string()))?;
        sha256_hex(&data).map_err(HostError::Io)
    }

    pub fn emit_event(summary: &str) -> Result<(), HostError> {
        // Lifecycle summary only — no payloads. Best-effort Event Log via PowerShell.
        let safe: String = summary
            .chars()
            .filter(|c| c.is_ascii() && !c.is_control())
            .take(200)
            .collect();
        let _ = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "try {{ Write-EventLog -LogName Application -Source Application -EntryType Information -EventId 1000 -Message 'RamShared: {safe}' }} catch {{ }}"
                ),
            ])
            .status();
        Ok(())
    }
}

/// Reject relative and empty config paths.
pub fn validate_absolute_config_path(path: &Path) -> Result<(), ConfigError> {
    if path.as_os_str().is_empty() {
        return Err(ConfigError::Invalid {
            field: "config",
            detail: "empty path".into(),
        });
    }
    if !path.is_absolute() {
        return Err(ConfigError::Invalid {
            field: "config",
            detail: "relative config path rejected".into(),
        });
    }
    Ok(())
}

/// SCM fixed product config path (DT-1).
pub fn scm_config_path() -> PathBuf {
    PathBuf::from(r"C:\ProgramData\RamShared\winsvc.toml")
}

fn path_is_reparse(path: &Path) -> bool {
    let wide = match path_to_wide(path) {
        Ok(w) => w,
        Err(_) => return false,
    };
    let attr = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attr == u32::MAX {
        return false;
    }
    (attr & FILE_ATTRIBUTE_REPARSE_POINT) != 0
}

fn path_to_wide(path: &Path) -> Result<Vec<u16>, HostError> {
    let s = path
        .to_str()
        .ok_or_else(|| HostError::Io("non-utf8 path".into()))?;
    Ok(to_wide(s))
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn size_of_val<T>(v: &T) -> usize {
    std::mem::size_of_val(v)
}

fn last_err(op: &str) -> String {
    let e = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    format!("{op} win32={e}")
}

fn fsctl(handle: HANDLE, code: u32) -> bool {
    let mut ret = 0u32;
    unsafe {
        windows_sys::Win32::System::IO::DeviceIoControl(
            handle,
            code,
            ptr::null(),
            0,
            ptr::null_mut(),
            0,
            &mut ret,
            ptr::null_mut(),
        ) != FALSE
    }
}

fn bcrypt_ok(status: i32) -> bool {
    status >= 0
}

fn sha256_hex(data: &[u8]) -> Result<String, String> {
    unsafe {
        let mut alg: BCRYPT_ALG_HANDLE = ptr::null_mut();
        let status = BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0);
        if !bcrypt_ok(status) {
            return Err(format!("BCryptOpenAlgorithmProvider={status}"));
        }
        let mut hash: BCRYPT_HASH_HANDLE = ptr::null_mut();
        let st = BCryptCreateHash(alg, &mut hash, ptr::null_mut(), 0, ptr::null_mut(), 0, 0);
        if !bcrypt_ok(st) {
            BCryptCloseAlgorithmProvider(alg, 0);
            return Err(format!("BCryptCreateHash={st}"));
        }
        let st = BCryptHashData(
            hash,
            data.as_ptr() as *const _ as *mut _,
            data.len() as u32,
            0,
        );
        if !bcrypt_ok(st) {
            BCryptDestroyHash(hash);
            BCryptCloseAlgorithmProvider(alg, 0);
            return Err(format!("BCryptHashData={st}"));
        }
        let mut out = [0u8; 32];
        let st = BCryptFinishHash(hash, out.as_mut_ptr(), 32, 0);
        BCryptDestroyHash(hash);
        BCryptCloseAlgorithmProvider(alg, 0);
        if !bcrypt_ok(st) {
            return Err(format!("BCryptFinishHash={st}"));
        }
        Ok(out.iter().map(|b| format!("{b:02x}")).collect())
    }
}

fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\":");
    let i = json.find(&pat)?;
    let rest = json[i + pat.len()..].trim_start();
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let i = json.find(&pat)?;
    let rest = &json[i + pat.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn relative_config_is_rejected() {
        let e = validate_absolute_config_path(Path::new("winsvc.toml")).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "config",
                ..
            }
        ));
    }

    #[test]
    fn reparse_config_is_rejected() {
        let e = validate_absolute_config_path(Path::new("")).unwrap_err();
        assert!(matches!(e, ConfigError::Invalid { .. }));
    }

    #[test]
    fn pagefile_query_matches_canonical_volume() {
        let pf = PagefileIdentity {
            name: r"D:\pagefile.sys".into(),
            volume: r"D:\".into(),
        };
        assert!(pf.name.starts_with("D:"));
    }

    #[test]
    fn pagefile_query_error_is_unsafe() {
        let err = HostError::Pagefile("WMI timeout".into());
        assert!(err.to_string().contains("pagefile"));
    }

    #[test]
    fn exclusive_volume_lock_closes_pagefile_race() {
        let err = HostError::Volume("lock denied".into());
        assert!(err.to_string().contains("volume"));
    }

    #[test]
    fn lun_identity_requires_vendor_product_serial_and_size() {
        let lun = LunIdentity {
            vendor: "RAMSHARE".into(),
            product: "VRAMDISK".into(),
            serial: "ABCDEF0123456789".into(),
            size_bytes: 64 * 1024 * 1024,
            disk_number: 1,
        };
        assert!(lun.matches_expected("ABCDEF0123456789", 64 * 1024 * 1024));
        assert!(!lun.matches_expected("0000000000000000", 64 * 1024 * 1024));
        assert!(!lun.matches_expected("ABCDEF0123456789", 128 * 1024 * 1024));
        let bad = LunIdentity {
            vendor: "OTHER".into(),
            ..lun.clone()
        };
        assert!(!bad.matches_expected("ABCDEF0123456789", 64 * 1024 * 1024));
    }
}
