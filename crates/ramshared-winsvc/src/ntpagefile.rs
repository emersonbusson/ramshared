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
