//! Secondary pagefile activation via `NtCreatePagingFile` (SPEC ITEM-7 / DT-8 / DT-24).
//!
//! On non-Windows hosts every call returns a graceful error so unit tests run on Linux.

use std::path::Path;

/// Build major.minor.build (e.g. 10.0.26200) — allow-list is build series only (DT-24).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsBuild {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

/// Errors from pagefile helpers.
#[derive(Debug, PartialEq)]
pub enum PagefileError {
    UnsupportedBuild { build: u32 },
    NotWindows,
    Api(String),
    InvalidPath,
}

impl std::fmt::Display for PagefileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PagefileError::UnsupportedBuild { build } => {
                write!(
                    f,
                    "NtCreatePagingFile unsupported build {build} (need 26200.*)"
                )
            }
            PagefileError::NotWindows => write!(f, "pagefile API is Windows-only"),
            PagefileError::Api(s) => write!(f, "pagefile API: {s}"),
            PagefileError::InvalidPath => write!(f, "invalid pagefile path"),
        }
    }
}

impl std::error::Error for PagefileError {}

/// DT-24 allow-list: Windows 11 25H2 `26200.*` only for MVP.
pub fn is_supported_build(build: u32) -> bool {
    build == 26200
}

/// Returns true if the running OS is on the allow-list (Windows path).
///
/// On non-Windows, returns `false` without error (graceful degrade for DT-16 stub).
pub fn supported_build() -> bool {
    match current_build() {
        Ok(b) => is_supported_build(b.build),
        Err(_) => false,
    }
}

/// Read OS version. On Linux returns `PagefileError::NotWindows`.
pub fn current_build() -> Result<OsBuild, PagefileError> {
    #[cfg(windows)]
    {
        // RtlGetVersion via windows-sys is filled when ITEM-7 lands on a Windows host.
        // Placeholder that fails closed until linked: implementers replace with real call.
        Err(PagefileError::Api(
            "RtlGetVersion not linked in this build; use test injection".into(),
        ))
    }
    #[cfg(not(windows))]
    {
        Err(PagefileError::NotWindows)
    }
}

/// Create a secondary pagefile on `volume` if the build is allow-listed.
///
/// `volume` is a root like `V:\` or a path to the target volume.

/// Calculate NT pagefile size based on RAM size
pub fn calculate_pagefile_size(ram_size_bytes: u64) -> Result<(u64, u64), PagefileError> {
    if ram_size_bytes == 0 {
        return Err(PagefileError::Api("RAM size cannot be 0".into()));
    }

    let min_size: u64;
    let max_size: u64;

    const GB: u64 = 1024 * 1024 * 1024;

    if ram_size_bytes < 4 * GB {
        min_size = 256 * 1024 * 1024; // 256 MB
        max_size = ram_size_bytes * 2;
    } else if ram_size_bytes < 8 * GB {
        min_size = 1 * GB;
        max_size = ram_size_bytes;
    } else if ram_size_bytes <= 16 * GB {
        min_size = 2 * GB;
        max_size = 16 * GB;
    } else {
        min_size = 4 * GB;
        max_size = 32 * GB; // Cap max at 32 GB for large RAM
    }

    Ok((min_size, max_size))
}

pub fn create_secondary(
    volume: &Path,
    min_bytes: u64,
    max_bytes: u64,
    build: Option<OsBuild>,
) -> Result<(), PagefileError> {
    if volume.as_os_str().is_empty() {
        return Err(PagefileError::InvalidPath);
    }
    if min_bytes == 0 || min_bytes > max_bytes {
        return Err(PagefileError::Api("min/max pagefile sizes invalid".into()));
    }
    let b = match build {
        Some(b) => b,
        None => current_build()?,
    };
    if !is_supported_build(b.build) {
        return Err(PagefileError::UnsupportedBuild { build: b.build });
    }
    create_secondary_impl(volume, min_bytes, max_bytes)
}

/// Remove secondary pagefile (DT-9 first step). May require reboot if OS holds it hot.
pub fn remove_secondary(volume: &Path, build: Option<OsBuild>) -> Result<(), PagefileError> {
    if volume.as_os_str().is_empty() {
        return Err(PagefileError::InvalidPath);
    }
    let b = match build {
        Some(b) => b,
        None => current_build()?,
    };
    if !is_supported_build(b.build) {
        return Err(PagefileError::UnsupportedBuild { build: b.build });
    }
    remove_secondary_impl(volume)
}

#[cfg(windows)]
fn create_secondary_impl(
    _volume: &Path,
    _min_bytes: u64,
    _max_bytes: u64,
) -> Result<(), PagefileError> {
    // Real NtCreatePagingFile FFI lands with Windows host validation (ITEM-7).
    // Until then, fail closed with a clear message rather than silent no-op.
    Err(PagefileError::Api(
        "NtCreatePagingFile FFI not yet bound; see SPEC ITEM-7".into(),
    ))
}

#[cfg(not(windows))]
fn create_secondary_impl(
    _volume: &Path,
    _min_bytes: u64,
    _max_bytes: u64,
) -> Result<(), PagefileError> {
    Err(PagefileError::NotWindows)
}

#[cfg(windows)]
fn remove_secondary_impl(_volume: &Path) -> Result<(), PagefileError> {
    Err(PagefileError::Api(
        "NtSetSystemInformation remove not yet bound; see SPEC ITEM-7".into(),
    ))
}

#[cfg(not(windows))]
fn remove_secondary_impl(_volume: &Path) -> Result<(), PagefileError> {
    Err(PagefileError::NotWindows)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_ntpagefile_create_secondary_min_greater_than_max() {
        let vol = PathBuf::from("V:\\");
        let e = create_secondary(
            &vol,
            1024,
            512,
            Some(OsBuild { major: 10, minor: 0, build: 26200 }),
        ).unwrap_err();
        assert_eq!(e, PagefileError::Api("min/max pagefile sizes invalid".into()));
    }

    #[test]
    fn test_ntpagefile_create_secondary_invalid_path() {
        let vol = PathBuf::from("");
        let e = create_secondary(
            &vol,
            256,
            1024,
            Some(OsBuild { major: 10, minor: 0, build: 26200 }),
        ).unwrap_err();
        assert_eq!(e, PagefileError::InvalidPath);
    }

    #[test]
    fn test_ntpagefile_remove_secondary_invalid_path() {
        let vol = PathBuf::from("");
        let e = remove_secondary(
            &vol,
            Some(OsBuild { major: 10, minor: 0, build: 26200 }),
        ).unwrap_err();
        assert_eq!(e, PagefileError::InvalidPath);
    }

    #[test]
    fn test_ntpagefile_remove_secondary_unsupported_build() {
        let vol = PathBuf::from("V:\\");
        let e = remove_secondary(
            &vol,
            Some(OsBuild { major: 10, minor: 0, build: 22631 }),
        ).unwrap_err();
        assert_eq!(e, PagefileError::UnsupportedBuild { build: 22631 });
    }

    #[test]
    fn test_ntpagefile_size_under_4gb() {
        let (min, max) = calculate_pagefile_size(2 * 1024 * 1024 * 1024).unwrap();
        assert_eq!(min, 256 * 1024 * 1024);
        assert_eq!(max, 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_ntpagefile_size_at_4gb() {
        let (min, max) = calculate_pagefile_size(4 * 1024 * 1024 * 1024).unwrap();
        assert_eq!(min, 1024 * 1024 * 1024);
        assert_eq!(max, 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_ntpagefile_size_under_8gb() {
        let (min, max) = calculate_pagefile_size(6 * 1024 * 1024 * 1024).unwrap();
        assert_eq!(min, 1024 * 1024 * 1024);
        assert_eq!(max, 6 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_ntpagefile_size_at_8gb() {
        let (min, max) = calculate_pagefile_size(8 * 1024 * 1024 * 1024).unwrap();
        assert_eq!(min, 2 * 1024 * 1024 * 1024);
        assert_eq!(max, 16 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_ntpagefile_size_at_16gb() {
        let (min, max) = calculate_pagefile_size(16 * 1024 * 1024 * 1024).unwrap();
        assert_eq!(min, 2 * 1024 * 1024 * 1024);
        assert_eq!(max, 16 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_ntpagefile_size_over_16gb() {
        let (min, max) = calculate_pagefile_size(32 * 1024 * 1024 * 1024).unwrap();
        assert_eq!(min, 4 * 1024 * 1024 * 1024);
        assert_eq!(max, 32 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_ntpagefile_size_zero_ram() {
        let e = calculate_pagefile_size(0).unwrap_err();
        assert_eq!(e, PagefileError::Api("RAM size cannot be 0".into()));
    }

#[test]
    fn allow_list_26200_only() {
        assert!(is_supported_build(26200));
        assert!(!is_supported_build(26100));
        assert!(!is_supported_build(0));
    }

    #[test]
    fn unsupported_build_is_graceful() {
        let vol = PathBuf::from("V:\\");
        let e = create_secondary(
            &vol,
            256 * 1024 * 1024,
            1024 * 1024 * 1024,
            Some(OsBuild {
                major: 10,
                minor: 0,
                build: 22631,
            }),
        )
        .unwrap_err();
        assert!(matches!(
            e,
            PagefileError::UnsupportedBuild { build: 22631 }
        ));
    }

    #[test]
    fn invalid_sizes() {
        let vol = PathBuf::from("V:\\");
        let e = create_secondary(
            &vol,
            0,
            1,
            Some(OsBuild {
                major: 10,
                minor: 0,
                build: 26200,
            }),
        )
        .unwrap_err();
        assert!(matches!(e, PagefileError::Api(_)));
    }

    #[cfg(not(windows))]
    #[test]
    fn linux_create_is_not_windows() {
        let vol = PathBuf::from("V:\\");
        let e = create_secondary(
            &vol,
            1,
            2,
            Some(OsBuild {
                major: 10,
                minor: 0,
                build: 26200,
            }),
        )
        .unwrap_err();
        assert_eq!(e, PagefileError::NotWindows);
    }
}
