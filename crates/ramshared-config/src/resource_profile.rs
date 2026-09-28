//! Versioned end-user resource ceilings and stable storage targets.
//!
//! This module only parses and validates profile data. It performs no host or
//! guest mutation; providers must revalidate live identity and capacity before
//! acting on a target.

use std::collections::{BTreeMap, HashSet};
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

pub const RESOURCE_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const DISK_RESERVE_FLOOR_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub const MAX_RESOURCE_PROFILE_BYTES: usize = 64 * 1024;
const MAX_IDENTITY_BYTES: usize = 512;
const MAX_LINUX_RELATIVE_PATH_BYTES: usize = 4096;
const MAX_WINDOWS_PATH_BYTES: usize = 32_767;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourcePlatform {
    NativeLinux,
    Wsl2,
}

impl ResourcePlatform {
    fn as_str(self) -> &'static str {
        match self {
            Self::NativeLinux => "native_linux",
            Self::Wsl2 => "wsl2",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TierCaps {
    #[serde(default)]
    pub zram_bytes: Option<u64>,
    #[serde(default)]
    pub vram_bytes: BTreeMap<String, u64>,
    #[serde(default)]
    pub origin_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceProfile {
    pub schema_version: u32,
    #[serde(default)]
    pub caps: TierCaps,
    #[serde(default)]
    pub targets: Vec<ResourceTarget>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceTarget {
    LinuxSwapfile {
        filesystem_uuid: String,
        device_identity: String,
        managed_relative_path: String,
        bytes: u64,
        priority: i32,
    },
    LinuxFileOrigin {
        filesystem_uuid: String,
        device_identity: String,
        managed_relative_path: String,
        inode: u64,
        allocated_bytes: u64,
        identity_field_hash: String,
    },
    LinuxFileOriginRequest {
        filesystem_uuid: String,
        device_identity: String,
        managed_relative_path: String,
        allocated_bytes: u64,
    },
    WslFallback {
        windows_volume_id: String,
        path: String,
        bytes: u64,
    },
    WslOrigin {
        windows_volume_id: String,
        path: String,
        allocated_bytes: u64,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StorageVolumeIdentity {
    Linux {
        filesystem_uuid: String,
        device_identity: String,
    },
    Windows {
        volume_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceProfileError {
    Parse(String),
    UnsupportedSchemaVersion(u32),
    PlatformMismatch {
        target: &'static str,
        platform: &'static str,
    },
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
    CapacityOverflow,
}

impl Display for ResourceProfileError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(message) => write!(formatter, "invalid resource profile: {message}"),
            Self::UnsupportedSchemaVersion(version) => {
                write!(
                    formatter,
                    "unsupported resource profile schema version {version}"
                )
            }
            Self::PlatformMismatch { target, platform } => write!(
                formatter,
                "resource target {target} is not valid for platform {platform}"
            ),
            Self::Invalid { field, reason } => {
                write!(
                    formatter,
                    "invalid resource profile field {field}: {reason}"
                )
            }
            Self::CapacityOverflow => {
                formatter.write_str("managed storage requirement overflows byte capacity")
            }
        }
    }
}

impl std::error::Error for ResourceProfileError {}

impl ResourceProfile {
    pub fn parse(text: &str) -> Result<Self, ResourceProfileError> {
        if text.len() > MAX_RESOURCE_PROFILE_BYTES {
            return Err(invalid("profile", "exceeds the 64 KiB input limit"));
        }
        toml::from_str(text).map_err(|error| ResourceProfileError::Parse(error.to_string()))
    }

    pub fn to_toml(&self) -> Result<String, ResourceProfileError> {
        toml::to_string(self).map_err(|error| ResourceProfileError::Parse(error.to_string()))
    }

    pub fn validate_for(&self, platform: ResourcePlatform) -> Result<(), ResourceProfileError> {
        if self.schema_version != RESOURCE_PROFILE_SCHEMA_VERSION {
            return Err(ResourceProfileError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }

        for identity in self.caps.vram_bytes.keys() {
            validate_identity("caps.vram_bytes adapter identity", identity)?;
        }

        let mut managed_paths = HashSet::new();
        for target in &self.targets {
            self.validate_target(platform, target)?;
            if !managed_paths.insert(managed_path_identity(target)) {
                return Err(invalid(
                    "targets",
                    "contains duplicate managed storage paths",
                ));
            }
        }

        Ok(())
    }

    pub fn required_free_bytes_by_volume(
        &self,
    ) -> Result<BTreeMap<StorageVolumeIdentity, u64>, ResourceProfileError> {
        let mut requirements = BTreeMap::new();
        for target in &self.targets {
            let required = requirements
                .entry(target.storage_volume_identity())
                .or_insert(DISK_RESERVE_FLOOR_BYTES);
            *required = required
                .checked_add(target.allocated_bytes())
                .ok_or(ResourceProfileError::CapacityOverflow)?;
        }
        Ok(requirements)
    }

    fn validate_target(
        &self,
        platform: ResourcePlatform,
        target: &ResourceTarget,
    ) -> Result<(), ResourceProfileError> {
        match (platform, target) {
            (
                ResourcePlatform::NativeLinux,
                ResourceTarget::LinuxSwapfile {
                    filesystem_uuid,
                    device_identity,
                    managed_relative_path,
                    bytes,
                    priority,
                },
            ) => {
                validate_linux_storage_identity(filesystem_uuid, device_identity)?;
                validate_linux_relative_path(managed_relative_path)?;
                validate_positive_bytes("target.bytes", *bytes)?;
                if !(-1..=32_767).contains(priority) {
                    return Err(invalid("target.priority", "must be between -1 and 32767"));
                }
            }
            (
                ResourcePlatform::NativeLinux,
                ResourceTarget::LinuxFileOrigin {
                    filesystem_uuid,
                    device_identity,
                    managed_relative_path,
                    inode,
                    allocated_bytes,
                    identity_field_hash,
                },
            ) => {
                validate_linux_storage_identity(filesystem_uuid, device_identity)?;
                validate_linux_relative_path(managed_relative_path)?;
                validate_positive_bytes("target.allocated_bytes", *allocated_bytes)?;
                if *inode == 0 {
                    return Err(invalid("target.inode", "must be non-zero"));
                }
                if identity_field_hash.len() != 64
                    || !identity_field_hash
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(invalid(
                        "target.identity_field_hash",
                        "must contain exactly 64 hexadecimal characters",
                    ));
                }
            }
            (
                ResourcePlatform::NativeLinux,
                ResourceTarget::LinuxFileOriginRequest {
                    filesystem_uuid,
                    device_identity,
                    managed_relative_path,
                    allocated_bytes,
                },
            ) => {
                validate_linux_storage_identity(filesystem_uuid, device_identity)?;
                validate_linux_relative_path(managed_relative_path)?;
                validate_positive_bytes("target.allocated_bytes", *allocated_bytes)?;
            }
            (
                ResourcePlatform::Wsl2,
                ResourceTarget::WslFallback {
                    windows_volume_id,
                    path,
                    bytes,
                },
            ) => {
                validate_identity("target.windows_volume_id", windows_volume_id)?;
                validate_windows_path(path)?;
                validate_positive_bytes("target.bytes", *bytes)?;
            }
            (
                ResourcePlatform::Wsl2,
                ResourceTarget::WslOrigin {
                    windows_volume_id,
                    path,
                    allocated_bytes,
                },
            ) => {
                validate_identity("target.windows_volume_id", windows_volume_id)?;
                validate_windows_path(path)?;
                validate_positive_bytes("target.allocated_bytes", *allocated_bytes)?;
            }
            (_, ResourceTarget::LinuxSwapfile { .. }) => {
                return Err(platform_mismatch("linux_swapfile", platform));
            }
            (_, ResourceTarget::LinuxFileOrigin { .. }) => {
                return Err(platform_mismatch("linux_file_origin", platform));
            }
            (_, ResourceTarget::LinuxFileOriginRequest { .. }) => {
                return Err(platform_mismatch("linux_file_origin_request", platform));
            }
            (_, ResourceTarget::WslFallback { .. }) => {
                return Err(platform_mismatch("wsl_fallback", platform));
            }
            (_, ResourceTarget::WslOrigin { .. }) => {
                return Err(platform_mismatch("wsl_origin", platform));
            }
        }

        Ok(())
    }
}

impl ResourceTarget {
    fn storage_volume_identity(&self) -> StorageVolumeIdentity {
        match self {
            Self::LinuxSwapfile {
                filesystem_uuid,
                device_identity,
                ..
            }
            | Self::LinuxFileOrigin {
                filesystem_uuid,
                device_identity,
                ..
            }
            | Self::LinuxFileOriginRequest {
                filesystem_uuid,
                device_identity,
                ..
            } => StorageVolumeIdentity::Linux {
                filesystem_uuid: filesystem_uuid.clone(),
                device_identity: device_identity.clone(),
            },
            Self::WslFallback {
                windows_volume_id, ..
            }
            | Self::WslOrigin {
                windows_volume_id, ..
            } => StorageVolumeIdentity::Windows {
                volume_id: windows_volume_id.to_lowercase(),
            },
        }
    }

    fn allocated_bytes(&self) -> u64 {
        match self {
            Self::LinuxSwapfile { bytes, .. } | Self::WslFallback { bytes, .. } => *bytes,
            Self::LinuxFileOrigin {
                allocated_bytes, ..
            }
            | Self::LinuxFileOriginRequest {
                allocated_bytes, ..
            }
            | Self::WslOrigin {
                allocated_bytes, ..
            } => *allocated_bytes,
        }
    }
}

#[derive(Eq, Hash, PartialEq)]
enum ManagedPathIdentity {
    Linux(String, String, String),
    Windows(String, String),
}

fn managed_path_identity(target: &ResourceTarget) -> ManagedPathIdentity {
    match target {
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid,
            device_identity,
            managed_relative_path,
            ..
        }
        | ResourceTarget::LinuxFileOrigin {
            filesystem_uuid,
            device_identity,
            managed_relative_path,
            ..
        }
        | ResourceTarget::LinuxFileOriginRequest {
            filesystem_uuid,
            device_identity,
            managed_relative_path,
            ..
        } => ManagedPathIdentity::Linux(
            filesystem_uuid.clone(),
            device_identity.clone(),
            managed_relative_path.clone(),
        ),
        ResourceTarget::WslFallback {
            windows_volume_id,
            path,
            ..
        }
        | ResourceTarget::WslOrigin {
            windows_volume_id,
            path,
            ..
        } => ManagedPathIdentity::Windows(windows_volume_id.to_lowercase(), path.to_lowercase()),
    }
}

pub fn checked_required_free_bytes(
    managed_allocations: &[u64],
) -> Result<u64, ResourceProfileError> {
    managed_allocations
        .iter()
        .try_fold(DISK_RESERVE_FLOOR_BYTES, |required, allocation| {
            required
                .checked_add(*allocation)
                .ok_or(ResourceProfileError::CapacityOverflow)
        })
}

fn validate_linux_storage_identity(
    filesystem_uuid: &str,
    device_identity: &str,
) -> Result<(), ResourceProfileError> {
    validate_identity("target.filesystem_uuid", filesystem_uuid)?;
    validate_identity("target.device_identity", device_identity)?;
    Ok(())
}

fn validate_identity(field: &'static str, value: &str) -> Result<(), ResourceProfileError> {
    if value.trim().is_empty() {
        return Err(invalid(field, "must not be empty"));
    }
    if value.len() > MAX_IDENTITY_BYTES {
        return Err(invalid(field, "exceeds the identity length limit"));
    }
    if value.chars().any(char::is_control) {
        return Err(invalid(field, "contains a control character"));
    }
    Ok(())
}

fn validate_linux_relative_path(path: &str) -> Result<(), ResourceProfileError> {
    if path.is_empty() || path.len() > MAX_LINUX_RELATIVE_PATH_BYTES {
        return Err(invalid(
            "target.managed_relative_path",
            "has invalid length",
        ));
    }
    if path.starts_with('/') || path.starts_with('\\') || path.contains('\\') {
        return Err(invalid(
            "target.managed_relative_path",
            "must be a relative Linux path",
        ));
    }
    if path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(invalid(
            "target.managed_relative_path",
            "contains an unsafe path component",
        ));
    }
    Ok(())
}

fn validate_windows_path(path: &str) -> Result<(), ResourceProfileError> {
    let drive_path = path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes().get(2) == Some(&b'\\');
    let volume_prefix = r"\\?\Volume{";
    let volume_path = path
        .get(..volume_prefix.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(volume_prefix));
    if path.is_empty() || path.len() > MAX_WINDOWS_PATH_BYTES || (!drive_path && !volume_path) {
        return Err(invalid(
            "target.path",
            "must be an absolute drive or volume-GUID path",
        ));
    }

    let components_start = if drive_path {
        3
    } else {
        let guid_start = volume_prefix.len();
        let guid_end = guid_start + 36;
        let guid = path.get(guid_start..guid_end).ok_or_else(|| {
            invalid(
                "target.path",
                "volume path must contain a canonical volume GUID",
            )
        })?;
        if !is_canonical_guid(guid)
            || !path
                .as_bytes()
                .get(guid_end..guid_end + 2)
                .is_some_and(|separator| separator == b"}\\")
        {
            return Err(invalid(
                "target.path",
                "volume path must contain a canonical volume GUID",
            ));
        }
        guid_end + 2
    };
    let Some(remainder) = path.get(components_start..) else {
        return Err(invalid("target.path", "has an invalid root"));
    };
    if remainder.is_empty() || path.contains('/') || path.chars().any(char::is_control) {
        return Err(invalid(
            "target.path",
            "must name a file using canonical Windows separators and characters",
        ));
    }

    for component in remainder.split('\\') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(' ')
            || component.ends_with('.')
            || component.contains(':')
            || component
                .chars()
                .any(|character| matches!(character, '<' | '>' | '"' | '|' | '?' | '*'))
            || component.encode_utf16().count() > 255
            || is_reserved_windows_device_name(component)
        {
            return Err(invalid("target.path", "contains an unsafe path component"));
        }
    }
    Ok(())
}

fn is_canonical_guid(guid: &str) -> bool {
    guid.len() == 36
        && guid.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn is_reserved_windows_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            (suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit() && suffix != "0")
                || matches!(suffix, "¹" | "²" | "³")
        })
    })
}

fn validate_positive_bytes(field: &'static str, bytes: u64) -> Result<(), ResourceProfileError> {
    if bytes == 0 {
        return Err(invalid(field, "must be greater than zero"));
    }
    Ok(())
}

fn platform_mismatch(target: &'static str, platform: ResourcePlatform) -> ResourceProfileError {
    ResourceProfileError::PlatformMismatch {
        target,
        platform: platform.as_str(),
    }
}

fn invalid(field: &'static str, reason: &'static str) -> ResourceProfileError {
    ResourceProfileError::Invalid { field, reason }
}
