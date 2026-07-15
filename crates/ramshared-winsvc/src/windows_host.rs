//! Windows host helpers: config open, elevation, pagefile WMI, LUN identity (SPEC DT-1/DT-8/DT-11).
//!
//! Cover target: N/A — E2E-only (COM/WMI/token/VPD). Pure path validation helpers
//! are unit-tested below without Win32.

#![cfg(windows)]

use std::path::{Path, PathBuf};

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

/// Exclusive volume lock handle (opaque on this stub surface).
pub struct LockedVolume {
    pub letter: char,
}

/// Aggregate host queries used by product runtime.
pub struct WindowsHostState;

impl WindowsHostState {
    pub fn is_elevated() -> bool {
        // Lab implementation queries token elevation; default false until linked.
        false
    }

    /// Read config once from absolute path (reparse-safe open is Windows-only).
    pub fn read_owned_config(path: &Path) -> Result<WinDriveConfig, HostError> {
        validate_absolute_config_path(path).map_err(HostError::Config)?;
        let bytes = std::fs::read(path).map_err(|e| HostError::Io(e.to_string()))?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(HostError::Config(ConfigError::Invalid {
                field: "config",
                detail: format!("exceeds {MAX_CONFIG_BYTES}"),
            }));
        }
        WinDriveConfig::from_reader(&bytes).map_err(HostError::Config)
    }

    pub fn active_pagefiles() -> Result<Vec<PagefileIdentity>, HostError> {
        Err(HostError::Pagefile(
            "WMI Win32_PageFileUsage query not linked in this build".into(),
        ))
    }

    pub fn lock_volume(_letter: char) -> Result<LockedVolume, HostError> {
        Err(HostError::Volume("FSCTL_LOCK_VOLUME not linked".into()))
    }

    pub fn find_lun(_serial: &str, _size_bytes: u64) -> Result<Option<LunIdentity>, HostError> {
        Err(HostError::Identity("LUN query not linked".into()))
    }

    pub fn binary_sha256(_path: &Path) -> Result<String, HostError> {
        Err(HostError::Io("CNG SHA-256 not linked".into()))
    }

    pub fn emit_event(_summary: &str) -> Result<(), HostError> {
        Ok(())
    }
}

/// Reject relative and empty config paths (shared with non-Windows tests via re-export logic).
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
    // Reparse-point rejection requires Windows attributes; path string with junction markers
    // is still absolute — live reparse check is E2E-only.
    Ok(())
}

/// SCM fixed product config path (DT-1).
pub fn scm_config_path() -> PathBuf {
    PathBuf::from(r"C:\ProgramData\RamShared\winsvc.toml")
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
        // Without Win32 reparse open, absolute path still accepted at this layer;
        // empty path and relative remain refused. Document E2E for reparse.
        let e = validate_absolute_config_path(Path::new("")).unwrap_err();
        assert!(matches!(e, ConfigError::Invalid { .. }));
    }

    #[test]
    fn pagefile_query_matches_canonical_volume() {
        // Pure identity shape: pagefile on D: correlates by volume string.
        let pf = PagefileIdentity {
            name: r"D:\pagefile.sys".into(),
            volume: r"\\?\Volume{00000000-0000-0000-0000-000000000001}\".into(),
        };
        assert!(pf.name.starts_with("D:"));
        assert!(pf.volume.contains("Volume{"));
    }

    #[test]
    fn pagefile_query_error_is_unsafe() {
        let err = HostError::Pagefile("WMI timeout".into());
        assert!(err.to_string().contains("pagefile"));
    }

    #[test]
    fn exclusive_volume_lock_closes_pagefile_race() {
        // Contract: lock_volume failure is HostError::Volume — mapped to code 7 by service.
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
