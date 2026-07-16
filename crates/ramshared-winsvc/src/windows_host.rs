//! Windows host helpers: config open, elevation, pagefile, LUN identity (SPEC DT-1/DT-8/DT-11).
//!
//! Cover target: N/A — E2E-only for COM/WMI/VPD. Pure identity helpers tested below.

#![cfg(windows)]
#![allow(unsafe_code)]

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::ptr;
use std::time::Duration;

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
    FILE_SHARE_READ, GetFileAttributesW, OPEN_EXISTING, ReadFile,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::config::{ConfigError, MAX_CONFIG_BYTES, WinDriveConfig};
use crate::service::{ObservedVolumeIdentity, parse_product_friendly_name};

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

// HANDLE is an OS resource token; exclusive ownership may move across threads
// (e.g. bounded lock helper) as long as only one owner uses the handle.
unsafe impl Send for LockedVolume {}

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
            return Err(HostError::Io(last_err("CreateFile config")));
        }
        let mut buf = vec![0u8; MAX_CONFIG_BYTES + 1];
        let mut total = 0usize;
        while total < buf.len() {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    buf[total..].as_mut_ptr(),
                    (buf.len() - total) as u32,
                    &mut read,
                    ptr::null_mut(),
                )
            };
            if ok == FALSE {
                unsafe {
                    CloseHandle(handle);
                }
                return Err(HostError::Io(last_err("ReadFile config")));
            }
            if read == 0 {
                break;
            }
            total += read as usize;
        }
        unsafe {
            CloseHandle(handle);
        }
        if total > MAX_CONFIG_BYTES {
            return Err(HostError::Config(ConfigError::Invalid {
                field: "config",
                detail: format!("exceeds {MAX_CONFIG_BYTES}"),
            }));
        }
        buf.truncate(total);
        WinDriveConfig::from_reader(&buf).map_err(HostError::Config)
    }

    /// Gate A/B pagefile query. Fail-closed on any error (DT-8).
    ///
    /// Prefer registry `PagingFiles` (configured pagefiles) — no PowerShell. CIM
    /// `Win32_PageFileUsage` hangs under GPU-PV guest load and blocked teardown.
    /// Configured paths are a strict super-set for "is a pagefile on this letter?"
    /// (DT-8 refuse if product letter hosts a pagefile).
    pub fn active_pagefiles() -> Result<Vec<PagefileIdentity>, HostError> {
        Self::configured_pagefiles_registry()
    }

    /// Read `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PagingFiles`.
    fn configured_pagefiles_registry() -> Result<Vec<PagefileIdentity>, HostError> {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::System::Registry::{
            HKEY_LOCAL_MACHINE, KEY_READ, REG_MULTI_SZ, RegCloseKey, RegOpenKeyExW,
            RegQueryValueExW,
        };

        let subkey = to_wide(r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management");
        let mut hkey = std::ptr::null_mut();
        let open =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
        if open != 0 {
            return Err(HostError::Pagefile(format!(
                "RegOpenKeyExW Memory Management failed status={open}"
            )));
        }
        let name = to_wide("PagingFiles");
        let mut ty = 0u32;
        let mut size = 0u32;
        let q1 = unsafe {
            RegQueryValueExW(
                hkey,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if q1 != 0 || size == 0 {
            unsafe {
                RegCloseKey(hkey);
            }
            // Empty / missing = no configured pagefiles.
            return Ok(Vec::new());
        }
        if ty != REG_MULTI_SZ {
            unsafe {
                RegCloseKey(hkey);
            }
            return Err(HostError::Pagefile(format!(
                "PagingFiles unexpected type {ty}"
            )));
        }
        let mut buf = vec![0u8; size as usize];
        let q2 = unsafe {
            RegQueryValueExW(
                hkey,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                buf.as_mut_ptr(),
                &mut size,
            )
        };
        unsafe {
            RegCloseKey(hkey);
        }
        if q2 != 0 {
            return Err(HostError::Pagefile(format!(
                "RegQueryValueExW PagingFiles failed status={q2}"
            )));
        }
        // MULTI_SZ is UTF-16LE double-null terminated.
        let wide: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let mut out = Vec::new();
        let mut start = 0usize;
        for i in 0..wide.len() {
            if wide[i] == 0 {
                if i == start {
                    break; // double-null terminator
                }
                let s = std::ffi::OsString::from_wide(&wide[start..i]);
                let name = s.to_string_lossy().to_string();
                // Entries look like "C:\pagefile.sys 0 0" — take path token.
                let path = name.split_whitespace().next().unwrap_or("").to_string();
                if !path.is_empty() {
                    let volume = path.get(..3).unwrap_or("").to_string();
                    out.push(PagefileIdentity { name: path, volume });
                }
                start = i + 1;
            }
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

    /// Observe exactly one product disk and confirm the configured volume letter.
    ///
    /// Prefer `Get-Disk` by serial+size (stable under GPU-PV). Avoid
    /// `Get-Partition -DriveLetter X | Get-Disk`, which hangs for minutes after
    /// volume churn and blocked graceful stop.
    pub fn observe_volume_identity(letter: char) -> Result<ObservedVolumeIdentity, HostError> {
        let letter = letter.to_ascii_uppercase();
        if !('D'..='Z').contains(&letter) {
            return Err(HostError::Volume("letter must be D..=Z".into()));
        }
        // Letter-only discovery is a fallback; callers that know serial should use
        // observe_product_volume.
        let script = format!(
            concat!(
                "$letter='{letter}'; ",
                "$parts=@(Get-Partition -ErrorAction SilentlyContinue | ",
                "Where-Object {{ $_.DriveLetter -eq $letter }}); ",
                "if($parts.Count -ne 1){{ exit 42 }}; ",
                "$d=Get-Disk -Number $parts[0].DiskNumber -ErrorAction Stop; ",
                "$n=($d.FriendlyName -replace '\\s+',' ').Trim(); ",
                "Write-Output ($n+'|'+$d.Size+'|'+$d.SerialNumber)"
            ),
            letter = letter
        );
        Self::parse_identity_output(
            letter,
            &run_powershell_bounded(&script, Duration::from_secs(8))
                .map_err(HostError::Identity)?,
        )
    }

    /// Observe product volume by exact serial+size (unique VPD identity).
    ///
    /// Letter is taken from config (Day-0 controlled), not from `Get-Partition`,
    /// which hangs under GPU-PV volume churn and burned the graceful-stop budget.
    pub fn observe_product_volume(
        letter: char,
        serial: &str,
        size_bytes: u64,
    ) -> Result<ObservedVolumeIdentity, HostError> {
        let letter = letter.to_ascii_uppercase();
        if !('D'..='Z').contains(&letter) {
            return Err(HostError::Volume("letter must be D..=Z".into()));
        }
        if serial.len() != 16 {
            return Err(HostError::Identity("serial must be 16 hex chars".into()));
        }
        // Get-Disk only — no Get-Partition. Unique serial+size is the hard identity.
        let script = format!(
            concat!(
                "$wantSerial='{serial}'; $wantSize=[uint64]{size}; ",
                "$d=@(Get-Disk -ErrorAction SilentlyContinue | Where-Object {{ ",
                "  ($_.Size -eq $wantSize) -and ",
                "  ((([string]$_.SerialNumber).Trim()) -ieq $wantSerial) -and ",
                "  ($_.FriendlyName -match 'RAMSHARE') ",
                "}}); ",
                "if($d.Count -ne 1){{ Write-Error ('disk_count='+$d.Count); exit 42 }}; ",
                "$n=($d[0].FriendlyName -replace '\\s+',' ').Trim(); ",
                "Write-Output ($n+'|'+$d[0].Size+'|'+$d[0].SerialNumber.Trim())"
            ),
            serial = serial,
            size = size_bytes
        );
        Self::parse_identity_output(
            letter,
            &run_powershell_bounded(&script, Duration::from_secs(2))
                .map_err(HostError::Identity)?,
        )
    }

    fn parse_identity_output(
        letter: char,
        output: &Output,
    ) -> Result<ObservedVolumeIdentity, HostError> {
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(HostError::Identity(format!(
                "volume identity query failed status={:?} stderr={}",
                output.status,
                stderr.chars().take(200).collect::<String>()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut parts = stdout.trim().split('|');
        let name = parts.next().unwrap_or_default();
        let size_bytes = parts
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| HostError::Identity("missing disk size".into()))?;
        let serial = parts.next().unwrap_or_default().trim().to_string();
        if parts.next().is_some() {
            return Err(HostError::Identity("ambiguous identity output".into()));
        }
        let (vendor, product) = parse_product_friendly_name(name).map_err(HostError::Identity)?;
        Ok(ObservedVolumeIdentity {
            letter,
            vendor,
            product,
            serial,
            size_bytes,
        })
    }

    pub fn find_lun(serial: &str, size_bytes: u64) -> Result<Option<LunIdentity>, HostError> {
        // Storage module via PowerShell (VPD serial when exposed).
        let script = format!(
            "Get-Disk | Where-Object {{ $_.Size -eq {size_bytes} -and $_.FriendlyName -match 'RAMSHARE|VRAMDISK' }} | Select-Object -First 1 Number,FriendlyName,Size,SerialNumber | ConvertTo-Json -Compress"
        );
        let output =
            run_powershell_bounded(&script, Duration::from_secs(5)).map_err(HostError::Identity)?;
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
            serial: sn,
            size_bytes: size,
            disk_number: number,
        };
        if lun.matches_expected(serial, size_bytes) {
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

fn run_powershell_bounded(script: &str, timeout: Duration) -> Result<Output, String> {
    // Channel + helper thread so a hung Get-Partition/WMI cannot block teardown
    // forever even if kill/wait races on Windows.
    use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
    let script = script.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let done_w = std::sync::Arc::clone(&done);
    let worker = std::thread::spawn(move || {
        let child = match Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                done_w.store(true, AtomicOrdering::Release);
                let _ = tx.send(Err(format!("spawn: {e}")));
                return;
            }
        };
        let pid = child.id();
        let done_k = std::sync::Arc::clone(&done_w);
        // Watchdog: force-kill tree only if still running after timeout.
        std::thread::spawn(move || {
            let slice = Duration::from_millis(50);
            let mut waited = Duration::ZERO;
            while waited < timeout {
                if done_k.load(AtomicOrdering::Acquire) {
                    return;
                }
                std::thread::sleep(slice);
                waited += slice;
            }
            if !done_k.load(AtomicOrdering::Acquire) {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        });
        let out = child.wait_with_output().map_err(|e| format!("output: {e}"));
        done_w.store(true, AtomicOrdering::Release);
        let _ = tx.send(out);
    });
    let result = match rx.recv_timeout(timeout + Duration::from_millis(750)) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            done.store(true, AtomicOrdering::Release);
            Err(format!(
                "PowerShell timeout after {} ms",
                timeout.as_millis()
            ))
        }
    };
    // Join only on success path; on timeout the worker may still be reaping.
    if result.is_ok() {
        let _ = worker.join();
    }
    result
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
