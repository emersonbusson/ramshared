//! Read-only discovery and display for the cross-platform resource settings UI.

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write as FmtWrite};
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ramshared_config::resource_profile::{
    MAX_RESOURCE_PROFILE_BYTES, RESOURCE_PROFILE_SCHEMA_VERSION, ResourcePlatform, ResourceProfile,
    ResourceTarget, StorageVolumeIdentity, TierCaps,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use rustix::fs::{Mode, fchmod};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const LINUX_INVENTORY_OUTPUT_LIMIT: usize = 1024 * 1024;
const WINDOWS_INVENTORY_OUTPUT_LIMIT: usize = 256 * 1024;
const DEFAULT_RESOURCE_PROFILE_PATH: &str = "/etc/ramshared/resource-profile.toml";
const STORAGE_SAMPLE_MAX_AGE_MS: u64 = 30_000;
const STORAGE_SAMPLE_FUTURE_TOLERANCE_MS: u64 = 5_000;
const MAX_DRAFT_LINE_BYTES: usize = 128;
const MAX_DRAFT_TARGETS: usize = 16;
const DRAFT_LINUX_SWAP_PATH: &str = "swap/ramshared-fallback.swap";
const DRAFT_LINUX_ORIGIN_PATH: &str = "origin/ramshared-origin.img";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConfigMode {
    Interactive,
    Show {
        json: bool,
    },
    Draft {
        output_path: String,
    },
    Plan {
        json: bool,
        profile_path: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RuntimePlatform {
    NativeLinux,
    Wsl2,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct MemorySnapshot {
    total_bytes: Option<u64>,
    available_bytes: Option<u64>,
    swap_total_bytes: Option<u64>,
    swap_free_bytes: Option<u64>,
    required_counters_available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct SwapDevice {
    filename: String,
    kind: String,
    size_kib: u64,
    used_kib: u64,
    priority: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct BlockDevice {
    name: String,
    path: String,
    kind: String,
    size_bytes: Option<u64>,
    filesystem: Option<String>,
    uuid: Option<String>,
    mountpoints: Vec<String>,
    parent: Option<String>,
    major_minor: Option<String>,
    partition_uuid: Option<String>,
    hardware_identity: Option<String>,
    parent_hardware_identity: Option<String>,
    mounts: Vec<MountInfo>,
    read_only: Option<bool>,
    removable: Option<bool>,
    rotational: Option<bool>,
    transport: Option<String>,
    model: Option<String>,
    eligible_for_file_storage: bool,
    eligibility_reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct MountInfo {
    mount_id: u64,
    parent_mount_id: u64,
    major_minor: String,
    root: String,
    mountpoint: String,
    mount_options: Vec<String>,
    filesystem: String,
    source: String,
    super_options: Vec<String>,
    read_only: bool,
    total_bytes: Option<u64>,
    available_bytes: Option<u64>,
    capacity_observed_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct HostMemorySnapshot {
    total_bytes: Option<u64>,
    free_bytes: Option<u64>,
    committed_bytes: Option<u64>,
    commit_limit_bytes: Option<u64>,
}

impl HostMemorySnapshot {
    fn commit_headroom_bytes(&self) -> Option<u64> {
        self.commit_limit_bytes?.checked_sub(self.committed_bytes?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct WindowsVolume {
    drive_letter: Option<String>,
    label: Option<String>,
    file_system: Option<String>,
    drive_type: String,
    size_bytes: Option<u64>,
    free_bytes: Option<u64>,
    volume_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct WindowsSnapshot {
    observed_utc: String,
    observed_unix_ms: Option<u64>,
    host_memory: HostMemorySnapshot,
    volumes: Vec<WindowsVolume>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct PlannedTarget {
    kind: &'static str,
    volume_identity: String,
    path: String,
    observed_mount_id: Option<u64>,
    requested_bytes: u64,
    required_free_bytes: u64,
    observed_free_bytes: Option<u64>,
    status: &'static str,
    reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StorageObservation {
    free_bytes: u64,
    mount_id: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DraftVolumeCandidate {
    display: String,
    free_bytes: u64,
    storage: DraftStorageIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DraftStorageIdentity {
    NativeLinux {
        filesystem_uuid: String,
        device_identity: String,
    },
    Wsl2 {
        volume_id: String,
        drive_letter: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DraftTargetRole {
    FallbackSwap,
    RamSharedOrigin,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum PlannedPathIdentity {
    Linux {
        filesystem_uuid: String,
        device_identity: String,
        relative_path: String,
    },
    Windows {
        volume_id: String,
        relative_path: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ResourcePlan {
    schema_version: u32,
    platform: RuntimePlatform,
    observed_unix_ms: u64,
    profile_state: &'static str,
    profile_sha256: Option<String>,
    user_caps: Option<TierCaps>,
    targets: Vec<PlannedTarget>,
    gpu_budget_status: String,
    warnings: Vec<String>,
    writes_performed: bool,
    apply_enabled: bool,
    status: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ResourceSnapshot {
    platform: RuntimePlatform,
    observed_unix_ms: u64,
    guest_memory: MemorySnapshot,
    swaps: Vec<SwapDevice>,
    block_devices: Vec<BlockDevice>,
    windows: Option<WindowsSnapshot>,
    gpu_budget_status: String,
    warnings: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct InventoryError(String);

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn classify_platform(release: &str, version: &str) -> RuntimePlatform {
    let combined = format!("{release} {version}").to_ascii_lowercase();
    if combined.contains("microsoft-standard-wsl2")
        || combined.contains("wsl2")
        || (combined.contains("microsoft") && combined.contains("wsl"))
    {
        RuntimePlatform::Wsl2
    } else {
        RuntimePlatform::NativeLinux
    }
}

fn meminfo_kib(text: &str, key: &str) -> Option<u64> {
    let prefix = format!("{key}:");
    let line = text.lines().find(|line| line.starts_with(&prefix))?;
    let mut fields = line[prefix.len()..].split_whitespace();
    let value = fields.next()?.parse::<u64>().ok()?;
    if fields.next()? != "kB" || fields.next().is_some() {
        return None;
    }
    value.checked_mul(1024)
}

fn parse_meminfo(text: &str) -> MemorySnapshot {
    let total = meminfo_kib(text, "MemTotal");
    let available = meminfo_kib(text, "MemAvailable");
    let (total_bytes, available_bytes) = match (total, available) {
        (Some(total), Some(available)) if total > 0 && available <= total => {
            (Some(total), Some(available))
        }
        _ => (None, None),
    };
    let swap_total = meminfo_kib(text, "SwapTotal");
    let swap_free = meminfo_kib(text, "SwapFree");
    let (swap_total_bytes, swap_free_bytes) = match (swap_total, swap_free) {
        (Some(total), Some(free)) if free <= total => (Some(total), Some(free)),
        _ => (None, None),
    };
    let required_counters_available = total_bytes.is_some()
        && available_bytes.is_some()
        && swap_total_bytes.is_some()
        && swap_free_bytes.is_some();

    MemorySnapshot {
        total_bytes,
        available_bytes,
        swap_total_bytes,
        swap_free_bytes,
        required_counters_available,
    }
}

fn parse_swap_table(text: &str) -> Result<Vec<SwapDevice>, InventoryError> {
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| InventoryError("swap table is empty".into()))?;
    if header.split_whitespace().collect::<Vec<_>>()
        != ["Filename", "Type", "Size", "Used", "Priority"]
    {
        return Err(InventoryError("swap table header is malformed".into()));
    }

    let mut devices = Vec::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 5 {
            return Err(InventoryError(format!(
                "swap table row {} is malformed",
                index + 2
            )));
        }
        let size_kib = fields[2].parse::<u64>().map_err(|_| {
            InventoryError(format!("swap table row {} has invalid size", index + 2))
        })?;
        let used_kib = fields[3]
            .parse::<u64>()
            .map_err(|_| InventoryError(format!("swap table row {} has invalid use", index + 2)))?;
        let priority = fields[4].parse::<i32>().map_err(|_| {
            InventoryError(format!("swap table row {} has invalid priority", index + 2))
        })?;
        if used_kib > size_kib {
            return Err(InventoryError(format!(
                "swap table row {} reports use above capacity",
                index + 2
            )));
        }
        devices.push(SwapDevice {
            filename: fields[0].to_string(),
            kind: fields[1].to_string(),
            size_kib,
            used_kib,
            priority,
        });
    }
    Ok(devices)
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn json_u64(value: &Value, key: &str) -> Option<u64> {
    let value = value.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn json_bool(value: &Value, key: &str) -> Option<bool> {
    let value = value.get(key)?;
    value
        .as_bool()
        .or_else(|| value.as_u64().map(|number| number != 0))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| match text.to_ascii_lowercase().as_str() {
                    "true" | "1" => Some(true),
                    "false" | "0" => Some(false),
                    _ => None,
                })
        })
}

fn parse_mountpoints(value: &Value) -> Vec<String> {
    match value.get("mountpoints") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(item)) if !item.is_empty() => vec![item.clone()],
        _ => json_string(value, "mountpoint").into_iter().collect(),
    }
}

fn storage_eligibility(device: &BlockDevice, mount: Option<&MountInfo>) -> (bool, String) {
    match device.read_only {
        Some(false) => {}
        Some(true) => return (false, "block device is read-only".into()),
        None => return (false, "block-device read-only state is unavailable".into()),
    }
    match device.removable {
        Some(false) => {}
        Some(true) => return (false, "removable storage is not eligible".into()),
        None => return (false, "removable status is unavailable".into()),
    }
    let Some(transport) = device.transport.as_deref() else {
        return (
            false,
            "storage transport is unavailable; local backing cannot be verified".into(),
        );
    };
    if transport.eq_ignore_ascii_case("usb") {
        return (
            false,
            "USB storage is not eligible for managed files".into(),
        );
    }
    if [
        "aoe", "drbd", "fc", "fcoe", "iscsi", "nbd", "network", "nvme-of", "nvmeof", "nvmf", "rbd",
    ]
    .iter()
    .any(|known| transport.eq_ignore_ascii_case(known))
    {
        return (
            false,
            "network-backed storage is not eligible for managed files".into(),
        );
    }
    if ![
        "ata", "ide", "mmc", "nvme", "pci", "sas", "sata", "scsi", "virtio",
    ]
    .iter()
    .any(|known| transport.eq_ignore_ascii_case(known))
    {
        return (
            false,
            format!("storage transport {transport} is not qualified as local"),
        );
    }
    if device.kind != "part" && device.kind != "disk" {
        return (
            false,
            "select a filesystem on a partition or whole disk".into(),
        );
    }
    let Some(mount) = mount else {
        return (false, "no current mount record matches this device".into());
    };
    if device.major_minor.as_deref() != Some(mount.major_minor.as_str()) {
        return (false, "mounted device identity does not match".into());
    }
    if mount.root != "/" {
        return (false, "mount does not expose the filesystem root".into());
    }
    if mount.read_only {
        return (false, "filesystem is mounted read-only".into());
    }
    if mount.mount_id == 0 || mount.total_bytes.is_none() || mount.available_bytes.is_none() {
        return (
            false,
            "mount identity or free-space measurement is unavailable".into(),
        );
    }
    if device.uuid.as_deref().is_none_or(str::is_empty) {
        return (false, "filesystem UUID is unavailable".into());
    }
    let stable_storage_identity = match device.kind.as_str() {
        "part" => device.parent_hardware_identity.as_deref(),
        "disk" => device.hardware_identity.as_deref(),
        _ => None,
    };
    if stable_storage_identity.is_none_or(str::is_empty) {
        return (
            false,
            "stable storage-device identity is unavailable".into(),
        );
    }
    match mount.filesystem.to_ascii_lowercase().as_str() {
        "ext4" | "xfs" => (
            true,
            "verified ext4/XFS mount; recheck identity and free space before use".into(),
        ),
        other => (false, format!("filesystem {other} is not qualified")),
    }
}

fn hardware_identity(value: &Value) -> Option<String> {
    json_string(value, "wwn")
        .filter(|identity| !identity.trim().is_empty())
        .map(|identity| format!("wwn:{identity}"))
        .or_else(|| {
            json_string(value, "serial")
                .filter(|identity| !identity.trim().is_empty())
                .map(|identity| format!("serial:{identity}"))
        })
}

fn parse_block_device(
    value: &Value,
    parent_hardware_identity: Option<&str>,
    parent_transport: Option<&str>,
    parent_removable: Option<bool>,
    devices: &mut Vec<BlockDevice>,
) -> Result<(), InventoryError> {
    let name = json_string(value, "name")
        .ok_or_else(|| InventoryError("lsblk device is missing name".into()))?;
    let path = json_string(value, "path")
        .ok_or_else(|| InventoryError(format!("lsblk device {name} is missing path")))?;
    let kind = json_string(value, "type")
        .ok_or_else(|| InventoryError(format!("lsblk device {name} is missing type")))?;
    let filesystem = json_string(value, "fstype");
    let mountpoints = parse_mountpoints(value);
    let read_only = json_bool(value, "ro");
    let removable = match (json_bool(value, "rm"), parent_removable) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), _) | (_, Some(false)) => Some(false),
        (None, None) => None,
    };
    let transport = json_string(value, "tran").or_else(|| parent_transport.map(str::to_owned));
    let hardware_identity = hardware_identity(value);

    devices.push(BlockDevice {
        name,
        path,
        kind,
        size_bytes: json_u64(value, "size"),
        filesystem,
        uuid: json_string(value, "uuid"),
        mountpoints,
        parent: json_string(value, "pkname"),
        major_minor: json_string(value, "maj:min"),
        partition_uuid: json_string(value, "partuuid"),
        hardware_identity: hardware_identity.clone(),
        parent_hardware_identity: parent_hardware_identity.map(str::to_owned),
        mounts: Vec::new(),
        read_only,
        removable,
        rotational: json_bool(value, "rota"),
        transport: transport.clone(),
        model: json_string(value, "model"),
        eligible_for_file_storage: false,
        eligibility_reason: "current mount, capacity, and stable identity are not verified".into(),
    });

    if let Some(children) = value.get("children").and_then(Value::as_array) {
        let child_parent_identity = hardware_identity.as_deref().or(parent_hardware_identity);
        let child_transport = transport.as_deref().or(parent_transport);
        let child_removable = removable.or(parent_removable);
        for child in children {
            parse_block_device(
                child,
                child_parent_identity,
                child_transport,
                child_removable,
                devices,
            )?;
        }
    }
    Ok(())
}

fn parse_lsblk_json(text: &str) -> Result<Vec<BlockDevice>, InventoryError> {
    let root: Value = serde_json::from_str(text)
        .map_err(|error| InventoryError(format!("lsblk JSON is invalid: {error}")))?;
    let blockdevices = root
        .get("blockdevices")
        .and_then(Value::as_array)
        .ok_or_else(|| InventoryError("lsblk JSON has no blockdevices array".into()))?;
    let mut devices = Vec::new();
    for device in blockdevices {
        parse_block_device(device, None, None, None, &mut devices)?;
    }
    Ok(devices)
}

fn decode_mountinfo_field(field: &str) -> Result<String, InventoryError> {
    let bytes = field.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 3 >= bytes.len()
            || !bytes[index + 1..index + 4]
                .iter()
                .all(|digit| (b'0'..=b'7').contains(digit))
        {
            return Err(InventoryError(
                "mountinfo contains an invalid escape".into(),
            ));
        }
        let value = u16::from(bytes[index + 1] - b'0') * 64
            + u16::from(bytes[index + 2] - b'0') * 8
            + u16::from(bytes[index + 3] - b'0');
        decoded
            .push(u8::try_from(value).map_err(|_| {
                InventoryError("mountinfo escape is outside the byte range".into())
            })?);
        index += 4;
    }
    String::from_utf8(decoded)
        .map_err(|error| InventoryError(format!("mountinfo path is not UTF-8: {error}")))
}

fn parse_mountinfo(text: &str) -> Result<Vec<MountInfo>, InventoryError> {
    let mut mounts = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (mount_fields, filesystem_fields) = line.split_once(" - ").ok_or_else(|| {
            InventoryError(format!("mountinfo row {} has no separator", index + 1))
        })?;
        let mount_fields = mount_fields.split_whitespace().collect::<Vec<_>>();
        let filesystem_fields = filesystem_fields.split_whitespace().collect::<Vec<_>>();
        if mount_fields.len() < 6 || filesystem_fields.len() < 3 {
            return Err(InventoryError(format!(
                "mountinfo row {} is malformed",
                index + 1
            )));
        }
        let mount_id = mount_fields[0].parse::<u64>().map_err(|_| {
            InventoryError(format!("mountinfo row {} has invalid mount ID", index + 1))
        })?;
        let parent_mount_id = mount_fields[1].parse::<u64>().map_err(|_| {
            InventoryError(format!(
                "mountinfo row {} has invalid parent mount ID",
                index + 1
            ))
        })?;
        let mount_options = mount_fields[5]
            .split(',')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let super_options = filesystem_fields[2]
            .split(',')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let read_only = mount_options.iter().any(|option| option == "ro")
            || super_options.iter().any(|option| option == "ro");
        mounts.push(MountInfo {
            mount_id,
            parent_mount_id,
            major_minor: mount_fields[2].to_owned(),
            root: decode_mountinfo_field(mount_fields[3])?,
            mountpoint: decode_mountinfo_field(mount_fields[4])?,
            mount_options,
            filesystem: filesystem_fields[0].to_owned(),
            source: decode_mountinfo_field(filesystem_fields[1])?,
            super_options,
            read_only,
            total_bytes: None,
            available_bytes: None,
            capacity_observed_unix_ms: None,
        });
    }
    Ok(mounts)
}

fn mount_capacity_bytes(mountpoint: &str) -> Option<(u64, u64)> {
    let stats = rustix::fs::statvfs(mountpoint).ok()?;
    let available_blocks = stats.f_bavail;
    let total_blocks = stats.f_blocks;
    let fragment_size = if stats.f_frsize == 0 {
        stats.f_bsize
    } else {
        stats.f_frsize
    };
    let total_bytes = total_blocks.checked_mul(fragment_size)?;
    let available_bytes = available_blocks.checked_mul(fragment_size)?;
    (available_bytes <= total_bytes).then_some((total_bytes, available_bytes))
}

fn attach_mount_inventory(devices: &mut [BlockDevice], mut mounts: Vec<MountInfo>) {
    for mount in &mut mounts {
        (mount.total_bytes, mount.available_bytes) = mount_capacity_bytes(&mount.mountpoint)
            .map_or((None, None), |(total, available)| {
                mount.capacity_observed_unix_ms = Some(unix_millis());
                (Some(total), Some(available))
            });
    }
    let mounts_by_device = mounts.into_iter().fold(
        HashMap::<String, Vec<MountInfo>>::new(),
        |mut grouped, mount| {
            grouped
                .entry(mount.major_minor.clone())
                .or_default()
                .push(mount);
            grouped
        },
    );
    for device in devices {
        device.mounts = device
            .major_minor
            .as_ref()
            .and_then(|major_minor| mounts_by_device.get(major_minor))
            .cloned()
            .unwrap_or_default();
        if !device.mounts.is_empty() {
            device.mountpoints = device
                .mounts
                .iter()
                .map(|mount| mount.mountpoint.clone())
                .collect();
        }
        let eligibility = device
            .mounts
            .iter()
            .map(|mount| storage_eligibility(device, Some(mount)))
            .find(|result| result.0)
            .or_else(|| {
                device
                    .mounts
                    .first()
                    .map(|mount| storage_eligibility(device, Some(mount)))
            });
        if let Some((eligible, reason)) = eligibility {
            device.eligible_for_file_storage = eligible;
            device.eligibility_reason = reason;
        } else if device.mountpoints.is_empty() {
            device.eligibility_reason = "filesystem is not mounted".into();
        } else {
            device.eligibility_reason = "mount identity does not match a block device".into();
        }
    }
}

fn apply_platform_storage_policy(devices: &mut [BlockDevice], platform: RuntimePlatform) {
    if platform != RuntimePlatform::Wsl2 {
        return;
    }

    for device in devices {
        let has_mounted_supported_filesystem = device.mounts.iter().any(|mount| {
            !mount.read_only
                && matches!(
                    mount.filesystem.to_ascii_lowercase().as_str(),
                    "ext4" | "xfs"
                )
                && mount.total_bytes.is_some()
                && mount.available_bytes.is_some()
        });
        if has_mounted_supported_filesystem {
            let guest_reason = device.eligibility_reason.trim();
            device.eligible_for_file_storage = false;
            device.eligibility_reason = if guest_reason.is_empty() {
                "WSL2 host-volume identity and free capacity are not bound to this guest filesystem"
                    .into()
            } else {
                format!(
                    "WSL2 host-volume identity and free capacity are not bound to this guest filesystem; guest check: {guest_reason}"
                )
            };
        }
    }
}

fn parse_windows_volume(value: &Value) -> Result<WindowsVolume, InventoryError> {
    if !value.is_object() {
        return Err(InventoryError("Windows volume row is not an object".into()));
    }
    Ok(WindowsVolume {
        drive_letter: json_string(value, "drive_letter"),
        label: json_string(value, "label"),
        file_system: json_string(value, "file_system"),
        drive_type: json_string(value, "drive_type").unwrap_or_else(|| "unknown".into()),
        size_bytes: json_u64(value, "size_bytes"),
        free_bytes: json_u64(value, "free_bytes"),
        volume_id: json_string(value, "volume_id"),
    })
}

fn parse_windows_snapshot(text: &str) -> Result<WindowsSnapshot, InventoryError> {
    let root: Value = serde_json::from_str(text)
        .map_err(|error| InventoryError(format!("Windows inventory JSON is invalid: {error}")))?;
    let host = root
        .get("host_memory")
        .and_then(Value::as_object)
        .ok_or_else(|| InventoryError("Windows inventory has no host_memory object".into()))?;
    let volume_rows = root
        .get("volumes")
        .and_then(Value::as_array)
        .ok_or_else(|| InventoryError("Windows inventory has no volumes array".into()))?;
    let host_memory_value = Value::Object(host.clone());
    let volumes = volume_rows
        .iter()
        .map(parse_windows_volume)
        .collect::<Result<Vec<_>, _>>()?;
    let observed_utc = json_string(&root, "observed_utc")
        .ok_or_else(|| InventoryError("Windows inventory has no observation time".into()))?;

    Ok(WindowsSnapshot {
        observed_utc,
        observed_unix_ms: json_u64(&root, "observed_unix_ms"),
        host_memory: HostMemorySnapshot {
            total_bytes: json_u64(&host_memory_value, "total_bytes"),
            free_bytes: json_u64(&host_memory_value, "free_bytes"),
            committed_bytes: json_u64(&host_memory_value, "committed_bytes"),
            commit_limit_bytes: json_u64(&host_memory_value, "commit_limit_bytes"),
        },
        volumes,
    })
}

fn run_bounded_command(
    command: &mut Command,
    label: &str,
    output_limit: usize,
) -> Result<Vec<u8>, InventoryError> {
    let output = crate::bounded_process::run_capture_command(
        command,
        label,
        DISCOVERY_TIMEOUT,
        output_limit,
        |_| {},
    )
    .map_err(|error| InventoryError(error.to_string()))?;
    if !output.status.success() {
        return Err(InventoryError(format!("{label} exited unsuccessfully")));
    }
    Ok(output.stdout)
}

fn collect_linux_block_devices() -> Result<Vec<BlockDevice>, InventoryError> {
    let mut command = Command::new("lsblk");
    command.args([
        "--json",
        "--bytes",
        "--output",
        "name,path,type,size,fstype,uuid,partuuid,mountpoints,pkname,maj:min,wwn,serial,ro,rm,rota,tran,model",
    ]);
    let output = run_bounded_command(
        &mut command,
        "lsblk inventory",
        LINUX_INVENTORY_OUTPUT_LIMIT,
    )?;
    let text = String::from_utf8(output)
        .map_err(|error| InventoryError(format!("lsblk output is not UTF-8: {error}")))?;
    let mut devices = parse_lsblk_json(&text)?;
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")
        .map_err(|error| InventoryError(format!("cannot read Linux mount inventory: {error}")))?;
    let mounts = parse_mountinfo(&mountinfo)?;
    attach_mount_inventory(&mut devices, mounts);
    Ok(devices)
}

fn windows_inventory_script() -> &'static str {
    r#"
$ErrorActionPreference = 'Stop'
$os = Get-CimInstance Win32_OperatingSystem
$memory = Get-CimInstance Win32_PerfRawData_PerfOS_Memory
$volumes = @(Get-Volume -ErrorAction Stop | ForEach-Object {
    [pscustomobject]@{
        drive_letter = if ($null -ne $_.DriveLetter) { [string]$_.DriveLetter } else { $null }
        label = [string]$_.FileSystemLabel
        file_system = [string]$_.FileSystem
        drive_type = [string]$_.DriveType
        size_bytes = if ($null -ne $_.Size) { [uint64]$_.Size } else { $null }
        free_bytes = if ($null -ne $_.SizeRemaining) { [uint64]$_.SizeRemaining } else { $null }
        volume_id = [string]$_.UniqueId
    }
})
$result = [pscustomobject]@{
    observed_utc = [DateTime]::UtcNow.ToString('o')
    observed_unix_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    host_memory = [pscustomobject]@{
        total_bytes = [uint64]$os.TotalVisibleMemorySize * [uint64]1024
        free_bytes = [uint64]$os.FreePhysicalMemory * [uint64]1024
        committed_bytes = [uint64]$memory.CommittedBytes
        commit_limit_bytes = [uint64]$memory.CommitLimit
    }
    volumes = @($volumes)
}
ConvertTo-Json -InputObject $result -Depth 5 -Compress
"#
}

fn collect_windows_snapshot() -> Result<WindowsSnapshot, InventoryError> {
    let script = windows_inventory_script();
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    let output = run_bounded_command(
        &mut command,
        "Windows host inventory",
        WINDOWS_INVENTORY_OUTPUT_LIMIT,
    )?;
    let text = String::from_utf8(output)
        .map_err(|error| InventoryError(format!("Windows inventory is not UTF-8: {error}")))?;
    parse_windows_snapshot(&text)
}

fn collect_snapshot() -> Result<ResourceSnapshot, InventoryError> {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let version = std::fs::read_to_string("/proc/version").unwrap_or_default();
    let platform = classify_platform(&release, &version);
    let meminfo = std::fs::read_to_string("/proc/meminfo")
        .map_err(|error| InventoryError(format!("cannot read guest memory counters: {error}")))?;
    let guest_memory = parse_meminfo(&meminfo);
    let swaps_text = std::fs::read_to_string("/proc/swaps")
        .map_err(|error| InventoryError(format!("cannot read guest swap inventory: {error}")))?;
    let swaps = parse_swap_table(&swaps_text)?;
    let mut warnings = Vec::new();
    let mut block_devices = match collect_linux_block_devices() {
        Ok(devices) => devices,
        Err(error) => {
            warnings.push(format!("Linux block inventory unavailable: {error}"));
            Vec::new()
        }
    };
    apply_platform_storage_policy(&mut block_devices, platform);
    let windows = if platform == RuntimePlatform::Wsl2 {
        match collect_windows_snapshot() {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                warnings.push(format!("Windows host inventory unavailable: {error}"));
                None
            }
        }
    } else {
        None
    };
    let observed_unix_ms = unix_millis();

    Ok(ResourceSnapshot {
        platform,
        observed_unix_ms,
        guest_memory,
        swaps,
        block_devices,
        windows,
        gpu_budget_status: "not sampled by storage inventory; no GPU context opened".into(),
        warnings,
    })
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn sample_is_fresh(sample_unix_ms: Option<u64>, now_unix_ms: u64) -> bool {
    sample_unix_ms.is_some_and(|sample| {
        sample <= now_unix_ms.saturating_add(STORAGE_SAMPLE_FUTURE_TOLERANCE_MS)
            && now_unix_ms.saturating_sub(sample) <= STORAGE_SAMPLE_MAX_AGE_MS
    })
}

fn load_profile_text(
    path: &Path,
    require_root_owned: bool,
) -> Result<Option<String>, InventoryError> {
    if require_root_owned {
        let Some(parent) = path.parent() else {
            return Err(InventoryError(
                "profile path has no parent directory".into(),
            ));
        };
        match fs::symlink_metadata(parent) {
            Ok(metadata)
                if metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && metadata.uid() == 0
                    && metadata.mode() & 0o022 == 0 => {}
            Ok(_) => {
                return Err(InventoryError(
                    "system profile directory must be root-owned and not group/world writable"
                        .into(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(InventoryError(format!(
                    "cannot inspect system profile directory: {error}"
                )));
            }
        }
    }

    let path_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(InventoryError(format!(
                "cannot inspect profile file: {error}"
            )));
        }
    };
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(InventoryError(
            "profile input must be a regular, non-symlink file".into(),
        ));
    }
    if path_metadata.len() > MAX_RESOURCE_PROFILE_BYTES as u64 {
        return Err(InventoryError(
            "profile input exceeds the 64 KiB limit".into(),
        ));
    }

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| InventoryError(format!("cannot open profile file: {error}")))?;
    let opened_metadata = file
        .metadata()
        .map_err(|error| InventoryError(format!("cannot inspect open profile: {error}")))?;
    if !opened_metadata.is_file()
        || opened_metadata.dev() != path_metadata.dev()
        || opened_metadata.ino() != path_metadata.ino()
    {
        return Err(InventoryError(
            "profile file changed while it was being opened".into(),
        ));
    }
    if require_root_owned
        && (opened_metadata.uid() != 0
            || opened_metadata.mode() & 0o7777 != 0o600
            || opened_metadata.nlink() != 1)
    {
        return Err(InventoryError(
            "system profile must be a single-link root-owned file with mode 0600".into(),
        ));
    }

    let mut content = Vec::with_capacity(path_metadata.len() as usize);
    file.take((MAX_RESOURCE_PROFILE_BYTES + 1) as u64)
        .read_to_end(&mut content)
        .map_err(|error| InventoryError(format!("cannot read profile file: {error}")))?;
    if content.len() > MAX_RESOURCE_PROFILE_BYTES {
        return Err(InventoryError(
            "profile input exceeds the 64 KiB limit".into(),
        ));
    }
    String::from_utf8(content)
        .map(Some)
        .map_err(|error| InventoryError(format!("profile input is not UTF-8: {error}")))
}

fn profile_platform(platform: RuntimePlatform) -> ResourcePlatform {
    match platform {
        RuntimePlatform::NativeLinux => ResourcePlatform::NativeLinux,
        RuntimePlatform::Wsl2 => ResourcePlatform::Wsl2,
    }
}

fn resource_volume_identity(target: &ResourceTarget) -> StorageVolumeIdentity {
    match target {
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid,
            device_identity,
            ..
        }
        | ResourceTarget::LinuxFileOrigin {
            filesystem_uuid,
            device_identity,
            ..
        }
        | ResourceTarget::LinuxFileOriginRequest {
            filesystem_uuid,
            device_identity,
            ..
        } => StorageVolumeIdentity::Linux {
            filesystem_uuid: filesystem_uuid.clone(),
            device_identity: device_identity.clone(),
        },
        ResourceTarget::WslFallback {
            windows_volume_id, ..
        }
        | ResourceTarget::WslOrigin {
            windows_volume_id, ..
        } => StorageVolumeIdentity::Windows {
            volume_id: windows_volume_id.to_lowercase(),
        },
    }
}

fn target_kind_path_and_size(target: &ResourceTarget) -> (&'static str, String, u64) {
    match target {
        ResourceTarget::LinuxSwapfile {
            managed_relative_path,
            bytes,
            ..
        } => ("linux_swapfile", managed_relative_path.clone(), *bytes),
        ResourceTarget::LinuxFileOrigin {
            managed_relative_path,
            allocated_bytes,
            ..
        } => (
            "linux_file_origin",
            managed_relative_path.clone(),
            *allocated_bytes,
        ),
        ResourceTarget::LinuxFileOriginRequest {
            managed_relative_path,
            allocated_bytes,
            ..
        } => (
            "linux_file_origin_request",
            managed_relative_path.clone(),
            *allocated_bytes,
        ),
        ResourceTarget::WslFallback { path, bytes, .. } => ("wsl_fallback", path.clone(), *bytes),
        ResourceTarget::WslOrigin {
            path,
            allocated_bytes,
            ..
        } => ("wsl_origin", path.clone(), *allocated_bytes),
    }
}

fn volume_identity_text(identity: &StorageVolumeIdentity) -> String {
    match identity {
        StorageVolumeIdentity::Linux {
            filesystem_uuid,
            device_identity,
        } => format!("filesystem:{filesystem_uuid};device:{device_identity}"),
        StorageVolumeIdentity::Windows { volume_id } => volume_id.clone(),
    }
}

fn backing_device_identity(device: &BlockDevice) -> Option<&str> {
    device
        .parent_hardware_identity
        .as_deref()
        .or(device.hardware_identity.as_deref())
}

fn linux_target_free_bytes(
    snapshot: &ResourceSnapshot,
    filesystem_uuid: &str,
    device_identity: &str,
    now_unix_ms: u64,
) -> Result<StorageObservation, (&'static str, String)> {
    if snapshot.platform != RuntimePlatform::NativeLinux {
        return Err((
            "identity_unavailable",
            "native Linux targets are unavailable on this platform".into(),
        ));
    }

    let mut matched_filesystem = false;
    let mut current_mounts = Vec::new();
    for device in &snapshot.block_devices {
        if device.uuid.as_deref() != Some(filesystem_uuid)
            || backing_device_identity(device) != Some(device_identity)
        {
            continue;
        }
        matched_filesystem = true;
        for mount in &device.mounts {
            if device.major_minor.as_deref() == Some(mount.major_minor.as_str()) {
                current_mounts.push((device, mount));
            }
        }
    }

    if current_mounts.len() > 1 {
        return Err((
            "identity_ambiguous",
            "stable filesystem identity resolves to multiple current mounts".into(),
        ));
    }
    let Some((device, mount)) = current_mounts.first().copied() else {
        let reason = if matched_filesystem {
            "stable filesystem is not currently mounted on its reported device"
        } else {
            "stable filesystem and backing-device identity are not present in the inventory"
        };
        return Err(("identity_unavailable", reason.into()));
    };

    let (eligible, reason) = storage_eligibility(device, Some(mount));
    if !eligible {
        return Err(("target_ineligible", reason));
    }
    if !sample_is_fresh(mount.capacity_observed_unix_ms, now_unix_ms) {
        return Err((
            "stale_sample",
            "filesystem free-space sample is missing or stale".into(),
        ));
    }
    let (Some(total), Some(free)) = (mount.total_bytes, mount.available_bytes) else {
        return Err((
            "capacity_unavailable",
            "filesystem total/free capacity is unavailable".into(),
        ));
    };
    if free > total {
        return Err((
            "inconsistent_sample",
            "filesystem free capacity exceeds its total capacity".into(),
        ));
    }
    Ok(StorageObservation {
        free_bytes: free,
        mount_id: Some(mount.mount_id),
    })
}

fn windows_target_free_bytes(
    snapshot: &ResourceSnapshot,
    windows_volume_id: &str,
    target_path: &str,
    now_unix_ms: u64,
) -> Result<StorageObservation, (&'static str, String)> {
    if snapshot.platform != RuntimePlatform::Wsl2 {
        return Err((
            "identity_unavailable",
            "Windows volume targets are only available under WSL2".into(),
        ));
    }
    let Some(windows) = snapshot.windows.as_ref() else {
        return Err((
            "identity_unavailable",
            "Windows host volume inventory is unavailable".into(),
        ));
    };
    if !sample_is_fresh(windows.observed_unix_ms, now_unix_ms) {
        return Err((
            "stale_sample",
            "Windows volume sample is missing or stale".into(),
        ));
    }
    let matches = windows
        .volumes
        .iter()
        .filter(|volume| {
            volume
                .volume_id
                .as_deref()
                .is_some_and(|observed| observed.eq_ignore_ascii_case(windows_volume_id))
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err((
            "identity_unavailable",
            "Windows volume identity is absent or ambiguous".into(),
        ));
    }
    let volume = matches[0];
    if !windows_path_belongs_to_volume(target_path, volume) {
        return Err((
            "identity_unavailable",
            "selected path does not resolve under the bound Windows volume".into(),
        ));
    }
    let free = windows_volume_eligibility(volume, matches.len())
        .map_err(|(code, reason)| (code, reason.to_owned()))?;
    Ok(StorageObservation {
        free_bytes: free,
        mount_id: None,
    })
}

fn windows_volume_eligibility(
    volume: &WindowsVolume,
    matching_id_count: usize,
) -> Result<u64, (&'static str, &'static str)> {
    if !volume.drive_type.eq_ignore_ascii_case("Fixed") {
        return Err(("target_ineligible", "volume is not fixed"));
    }
    if !volume
        .volume_id
        .as_deref()
        .is_some_and(|identity| !identity.trim().is_empty())
    {
        return Err((
            "identity_unavailable",
            "stable volume identity is unavailable",
        ));
    }
    if matching_id_count != 1 {
        return Err(("identity_unavailable", "volume identity is ambiguous"));
    }
    let Some(filesystem) = volume.file_system.as_deref() else {
        return Err(("target_ineligible", "filesystem is unavailable"));
    };
    if !filesystem.eq_ignore_ascii_case("NTFS") && !filesystem.eq_ignore_ascii_case("ReFS") {
        return Err(("target_ineligible", "filesystem is not NTFS/ReFS"));
    }
    let (Some(total), Some(free)) = (volume.size_bytes, volume.free_bytes) else {
        return Err(("capacity_unavailable", "volume capacity is unavailable"));
    };
    if free > total {
        return Err((
            "inconsistent_sample",
            "reported free capacity exceeds total capacity",
        ));
    }
    Ok(free)
}

fn windows_path_belongs_to_volume(path: &str, volume: &WindowsVolume) -> bool {
    windows_path_relative_to_volume(path, volume).is_some()
}

fn windows_path_relative_to_volume(path: &str, volume: &WindowsVolume) -> Option<String> {
    let normalized_path = path.to_lowercase();
    let drive_relative = volume.drive_letter.as_deref().and_then(|drive| {
        let drive = drive.trim_end_matches(':').to_lowercase();
        normalized_path.strip_prefix(&format!("{drive}:\\"))
    });
    let volume_relative = volume.volume_id.as_deref().and_then(|identity| {
        let prefix = format!("{}\\", identity.trim_end_matches('\\').to_lowercase());
        normalized_path.strip_prefix(&prefix)
    });
    drive_relative.or(volume_relative).map(str::to_owned)
}

fn planned_path_identity(
    snapshot: &ResourceSnapshot,
    target: &ResourceTarget,
) -> Option<PlannedPathIdentity> {
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
        } => Some(PlannedPathIdentity::Linux {
            filesystem_uuid: filesystem_uuid.clone(),
            device_identity: device_identity.clone(),
            relative_path: managed_relative_path.clone(),
        }),
        ResourceTarget::WslFallback {
            windows_volume_id,
            path,
            ..
        }
        | ResourceTarget::WslOrigin {
            windows_volume_id,
            path,
            ..
        } => {
            let windows = snapshot.windows.as_ref()?;
            let mut matches = windows.volumes.iter().filter(|volume| {
                volume
                    .volume_id
                    .as_deref()
                    .is_some_and(|observed| observed.eq_ignore_ascii_case(windows_volume_id))
            });
            let volume = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            let relative_path = windows_path_relative_to_volume(path, volume)?;
            Some(PlannedPathIdentity::Windows {
                volume_id: windows_volume_id
                    .trim_end_matches('\\')
                    .trim_end_matches('/')
                    .to_lowercase(),
                relative_path: relative_path.to_lowercase(),
            })
        }
    }
}

fn unconfigured_resource_plan(snapshot: &ResourceSnapshot) -> ResourcePlan {
    ResourcePlan {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        platform: snapshot.platform,
        observed_unix_ms: snapshot.observed_unix_ms,
        profile_state: "not_configured",
        profile_sha256: None,
        user_caps: None,
        targets: Vec::new(),
        gpu_budget_status: snapshot.gpu_budget_status.clone(),
        warnings: snapshot.warnings.clone(),
        writes_performed: false,
        apply_enabled: false,
        status: "not_configured",
    }
}

fn target_free_bytes(
    snapshot: &ResourceSnapshot,
    target: &ResourceTarget,
    now_unix_ms: u64,
) -> Result<StorageObservation, (&'static str, String)> {
    match target {
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid,
            device_identity,
            ..
        }
        | ResourceTarget::LinuxFileOrigin {
            filesystem_uuid,
            device_identity,
            ..
        }
        | ResourceTarget::LinuxFileOriginRequest {
            filesystem_uuid,
            device_identity,
            ..
        } => linux_target_free_bytes(snapshot, filesystem_uuid, device_identity, now_unix_ms),
        ResourceTarget::WslFallback {
            windows_volume_id,
            path,
            ..
        }
        | ResourceTarget::WslOrigin {
            windows_volume_id,
            path,
            ..
        } => windows_target_free_bytes(snapshot, windows_volume_id, path, now_unix_ms),
    }
}

fn plan_target(
    snapshot: &ResourceSnapshot,
    target: &ResourceTarget,
    required_free_bytes: u64,
    now_unix_ms: u64,
    duplicate_path: bool,
) -> PlannedTarget {
    let volume_identity = resource_volume_identity(target);
    let (kind, path, requested_bytes) = target_kind_path_and_size(target);
    let assessment = if duplicate_path {
        Err((
            "duplicate_target_path",
            "target resolves to the same managed path as another profile entry".into(),
        ))
    } else {
        target_free_bytes(snapshot, target, now_unix_ms)
    };
    let (observed_free_bytes, observed_mount_id, status, reason) = match assessment {
        Ok(observation) if observation.free_bytes >= required_free_bytes => (
            Some(observation.free_bytes),
            observation.mount_id,
            "storage_ready",
            None,
        ),
        Ok(observation) => (
            Some(observation.free_bytes),
            observation.mount_id,
            "insufficient_space",
            Some("available free space is below the combined target and reserve".into()),
        ),
        Err((status, reason)) => (None, None, status, Some(reason)),
    };
    PlannedTarget {
        kind,
        volume_identity: volume_identity_text(&volume_identity),
        path,
        observed_mount_id,
        requested_bytes,
        required_free_bytes,
        observed_free_bytes,
        status,
        reason,
    }
}

fn plan_status(targets: &[PlannedTarget]) -> &'static str {
    if targets.is_empty() {
        "profile_loaded_no_storage_targets"
    } else if targets
        .iter()
        .all(|target| target.status == "storage_ready")
    {
        "ready_for_review"
    } else {
        "blocked"
    }
}

fn profile_warnings(snapshot: &ResourceSnapshot, caps: &TierCaps) -> Vec<String> {
    let mut warnings = snapshot.warnings.clone();
    if caps.zram_bytes.is_some() || !caps.vram_bytes.is_empty() || caps.origin_bytes.is_some() {
        warnings.push(
            "tier ceilings are displayed only; this plan does not sample their owning runtime budgets or authorize increases".into(),
        );
    }
    warnings
}

fn profile_digest(profile_text: &str) -> String {
    Sha256::digest(profile_text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn build_resource_plan(
    snapshot: &ResourceSnapshot,
    profile_text: Option<&str>,
) -> Result<ResourcePlan, InventoryError> {
    let Some(profile_text) = profile_text else {
        return Ok(unconfigured_resource_plan(snapshot));
    };
    let profile = ResourceProfile::parse(profile_text)
        .map_err(|error| InventoryError(format!("resource profile is invalid: {error}")))?;
    profile
        .validate_for(profile_platform(snapshot.platform))
        .map_err(|error| InventoryError(format!("resource profile is not valid here: {error}")))?;
    let required_by_volume = profile
        .required_free_bytes_by_volume()
        .map_err(|error| InventoryError(format!("cannot calculate profile capacity: {error}")))?;
    let now_unix_ms = unix_millis();
    let mut targets = Vec::with_capacity(profile.targets.len());
    let mut planned_paths = HashSet::new();

    for target in &profile.targets {
        let volume_identity = resource_volume_identity(target);
        let required_free_bytes = required_by_volume
            .get(&volume_identity)
            .copied()
            .ok_or_else(|| InventoryError("profile target has no volume requirement".into()))?;
        let duplicate_path = planned_path_identity(snapshot, target)
            .is_some_and(|identity| !planned_paths.insert(identity));
        targets.push(plan_target(
            snapshot,
            target,
            required_free_bytes,
            now_unix_ms,
            duplicate_path,
        ));
    }

    let status = plan_status(&targets);
    let profile_sha256 = profile_digest(profile_text);
    let warnings = profile_warnings(snapshot, &profile.caps);
    Ok(ResourcePlan {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        platform: snapshot.platform,
        observed_unix_ms: snapshot.observed_unix_ms,
        profile_state: "validated",
        profile_sha256: Some(profile_sha256),
        user_caps: Some(profile.caps),
        targets,
        gpu_budget_status: snapshot.gpu_budget_status.clone(),
        warnings,
        writes_performed: false,
        apply_enabled: false,
        status,
    })
}

fn render_plan_text(plan: &ResourcePlan) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "RamShared read-only resource plan: {}", plan.status);
    let _ = writeln!(output, "Profile: {}", plan.profile_state);
    if let Some(digest) = &plan.profile_sha256 {
        let _ = writeln!(output, "Profile SHA-256: {digest}");
    }
    for target in &plan.targets {
        let _ = writeln!(
            output,
            "{} {} — {} (needs {}, observed free {})",
            target.kind,
            target.path,
            target.status,
            format_gib(Some(target.required_free_bytes)),
            format_gib(target.observed_free_bytes)
        );
        let _ = writeln!(output, "  volume identity: {}", target.volume_identity);
        if let Some(mount_id) = target.observed_mount_id {
            let _ = writeln!(output, "  current mount ID (ephemeral): {mount_id}");
        }
        if let Some(reason) = &target.reason {
            let _ = writeln!(output, "  reason: {reason}");
        }
    }
    for warning in &plan.warnings {
        let _ = writeln!(output, "Warning: {warning}");
    }
    let _ = writeln!(
        output,
        "Read-only plan. No settings, swap, origin, disk, GPU, or running tier were changed."
    );
    let _ = writeln!(output, "Apply enabled: {}", plan.apply_enabled);
    output
}

fn render_plan_json(plan: &ResourcePlan) -> Result<String, InventoryError> {
    serde_json::to_string(plan)
        .map_err(|error| InventoryError(format!("cannot serialize resource plan: {error}")))
}

fn format_gib(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "unavailable".into(),
        |bytes| format!("{:.2} GiB", bytes as f64 / (1024_f64 * 1024_f64 * 1024_f64)),
    )
}

fn render_text(snapshot: &ResourceSnapshot) -> String {
    let mut output = String::new();
    let platform = match snapshot.platform {
        RuntimePlatform::NativeLinux => "Native Linux",
        RuntimePlatform::Wsl2 => "WSL2",
    };
    let _ = writeln!(output, "RamShared resource inventory — {platform}");
    match snapshot.platform {
        RuntimePlatform::NativeLinux => {
            let _ = writeln!(
                output,
                "System RAM: total {}, available {}",
                format_gib(snapshot.guest_memory.total_bytes),
                format_gib(snapshot.guest_memory.available_bytes)
            );
        }
        RuntimePlatform::Wsl2 => {
            let _ = writeln!(
                output,
                "WSL guest RAM: total {}, available {}",
                format_gib(snapshot.guest_memory.total_bytes),
                format_gib(snapshot.guest_memory.available_bytes)
            );
            if let Some(windows) = &snapshot.windows {
                let _ = writeln!(
                    output,
                    "Windows host RAM: total {}, physically free {}",
                    format_gib(windows.host_memory.total_bytes),
                    format_gib(windows.host_memory.free_bytes)
                );
                let _ = writeln!(
                    output,
                    "Windows commit headroom: {}",
                    format_gib(windows.host_memory.commit_headroom_bytes())
                );
            } else {
                let _ = writeln!(output, "Windows host RAM: unavailable");
                let _ = writeln!(output, "Windows commit headroom: unavailable");
            }
        }
    }

    let _ = writeln!(
        output,
        "Guest swap: total {}, free {}",
        format_gib(snapshot.guest_memory.swap_total_bytes),
        format_gib(snapshot.guest_memory.swap_free_bytes)
    );
    for swap in &snapshot.swaps {
        let _ = writeln!(
            output,
            "  {} ({}, priority {}): {} KiB / {} KiB used",
            swap.filename, swap.kind, swap.priority, swap.used_kib, swap.size_kib
        );
    }

    let _ = writeln!(output, "Linux block devices and filesystems:");
    if snapshot.block_devices.is_empty() {
        let _ = writeln!(output, "  unavailable");
    }
    for device in &snapshot.block_devices {
        let mount = if device.mountpoints.is_empty() {
            "unmounted".into()
        } else {
            device.mountpoints.join(", ")
        };
        let hardware_identity = device
            .parent_hardware_identity
            .as_deref()
            .or(device.hardware_identity.as_deref())
            .unwrap_or("unknown");
        let rotation = match device.rotational {
            Some(true) => "rotating",
            Some(false) => "non-rotating",
            None => "rotation unknown",
        };
        let _ = writeln!(
            output,
            "  {} {} ({}, {}, mounts {}, model {}, transport {}, {}, ID {}) — {}",
            device.name,
            format_gib(device.size_bytes),
            device.kind,
            device.filesystem.as_deref().unwrap_or("filesystem unknown"),
            mount,
            device.model.as_deref().unwrap_or("unknown"),
            device.transport.as_deref().unwrap_or("unknown"),
            rotation,
            hardware_identity,
            device.eligibility_reason
        );
        for mount in &device.mounts {
            let _ = writeln!(
                output,
                "    {} [{} mount {}, device {}] — free {} of {}{}",
                mount.mountpoint,
                mount.filesystem,
                mount.mount_id,
                mount.major_minor,
                format_gib(mount.available_bytes),
                format_gib(mount.total_bytes),
                if mount.read_only { "; read-only" } else { "" }
            );
        }
    }

    if snapshot.platform == RuntimePlatform::Wsl2 {
        let _ = writeln!(output, "Windows volumes:");
        match &snapshot.windows {
            Some(windows) if windows.volumes.is_empty() => {
                let _ = writeln!(output, "  none reported");
            }
            Some(windows) => {
                for volume in &windows.volumes {
                    let drive = volume
                        .drive_letter
                        .as_deref()
                        .map_or_else(|| "(no drive letter)".into(), |letter| format!("{letter}:"));
                    let identity = volume
                        .volume_id
                        .as_deref()
                        .filter(|identity| !identity.trim().is_empty());
                    let matching_id_count = identity.map_or(0, |identity| {
                        windows
                            .volumes
                            .iter()
                            .filter(|candidate| {
                                candidate
                                    .volume_id
                                    .as_deref()
                                    .is_some_and(|observed| observed.eq_ignore_ascii_case(identity))
                            })
                            .count()
                    });
                    let eligibility = match windows_volume_eligibility(volume, matching_id_count) {
                        Ok(_)
                            if identity.is_some_and(|identity| {
                                draft_wsl_target_path_is_valid(
                                    identity,
                                    volume.drive_letter.as_deref(),
                                )
                            }) =>
                        {
                            "eligible volume candidate".to_owned()
                        }
                        Ok(_) => "ineligible: no safe canonical target path".to_owned(),
                        Err((_, reason)) => format!("ineligible: {reason}"),
                    };
                    let identity = identity.map_or_else(
                        || "identity unknown".to_owned(),
                        |identity| format!("ID {identity}"),
                    );
                    let _ = writeln!(
                        output,
                        "  {drive} {} {} ({}) — {identity} — {eligibility} — free {} of {}",
                        volume.label.as_deref().unwrap_or("unlabeled"),
                        volume
                            .file_system
                            .as_deref()
                            .unwrap_or("filesystem unknown"),
                        volume.drive_type,
                        format_gib(volume.free_bytes),
                        format_gib(volume.size_bytes)
                    );
                }
            }
            None => {
                let _ = writeln!(output, "  unavailable");
            }
        }
    }
    let _ = writeln!(
        output,
        "Storage speed comparison: not measured by this read-only inventory."
    );
    let _ = writeln!(output, "GPU/VRAM budget: {}", snapshot.gpu_budget_status);
    for warning in &snapshot.warnings {
        let _ = writeln!(output, "Warning: {warning}");
    }
    let _ = writeln!(
        output,
        "Read-only inventory. No swap, origin, or GPU allocation was changed."
    );
    output
}

fn render_json(snapshot: &ResourceSnapshot) -> Result<String, InventoryError> {
    serde_json::to_string(snapshot)
        .map_err(|error| InventoryError(format!("cannot serialize inventory: {error}")))
}

fn draw_config_frame(frame: &mut Frame<'_>, snapshot: &ResourceSnapshot, scroll: u16) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let title = Paragraph::new("RamShared Resource Configuration — Read-only inventory")
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);
    let body = Paragraph::new(render_text(snapshot))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(body, chunks[1]);
    let footer = Line::styled(
        "↑/↓ scroll  r refresh  q close",
        Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
    );
    frame.render_widget(Paragraph::new(footer), chunks[2]);
}

fn run_interactive() -> Result<(), InventoryError> {
    if !io::stdin().is_terminal() {
        return Err(InventoryError(
            "interactive config needs a terminal; use `ramshared config show`".into(),
        ));
    }
    let mut snapshot = collect_snapshot()?;
    let mut terminal = ratatui::init();
    let result = (|| {
        let mut scroll = 0u16;
        loop {
            terminal
                .draw(|frame| draw_config_frame(frame, &snapshot, scroll))
                .map_err(|error| InventoryError(format!("cannot draw config screen: {error}")))?;
            if !ratatui::crossterm::event::poll(Duration::from_millis(250))
                .map_err(|error| InventoryError(format!("cannot read config input: {error}")))?
            {
                continue;
            }
            if let ratatui::crossterm::event::Event::Key(key) = ratatui::crossterm::event::read()
                .map_err(|error| InventoryError(format!("cannot read config input: {error}")))?
            {
                match key.code {
                    ratatui::crossterm::event::KeyCode::Char('q')
                    | ratatui::crossterm::event::KeyCode::Esc => break,
                    ratatui::crossterm::event::KeyCode::Down => {
                        scroll = scroll.saturating_add(1);
                    }
                    ratatui::crossterm::event::KeyCode::Up => {
                        scroll = scroll.saturating_sub(1);
                    }
                    ratatui::crossterm::event::KeyCode::Char('r') => {
                        snapshot = collect_snapshot()?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

fn draft_volume_candidates(
    snapshot: &ResourceSnapshot,
    now_unix_ms: u64,
) -> Vec<DraftVolumeCandidate> {
    match snapshot.platform {
        RuntimePlatform::NativeLinux => {
            let mut candidates = Vec::new();
            let mut seen = HashSet::new();
            for device in &snapshot.block_devices {
                let (Some(filesystem_uuid), Some(device_identity)) =
                    (device.uuid.as_deref(), backing_device_identity(device))
                else {
                    continue;
                };
                let identity = StorageVolumeIdentity::Linux {
                    filesystem_uuid: filesystem_uuid.into(),
                    device_identity: device_identity.into(),
                };
                let Ok(observation) = linux_target_free_bytes(
                    snapshot,
                    filesystem_uuid,
                    device_identity,
                    now_unix_ms,
                ) else {
                    continue;
                };
                if !seen.insert(identity) {
                    continue;
                }
                let mountpoint = device
                    .mounts
                    .iter()
                    .find(|mount| Some(mount.mount_id) == observation.mount_id)
                    .map(|mount| mount.mountpoint.as_str())
                    .unwrap_or("mount unknown");
                candidates.push(DraftVolumeCandidate {
                    display: format!(
                        "{} at {} — filesystem {}, device {}",
                        device.model.as_deref().unwrap_or(&device.name),
                        mountpoint,
                        filesystem_uuid,
                        device_identity
                    ),
                    free_bytes: observation.free_bytes,
                    storage: DraftStorageIdentity::NativeLinux {
                        filesystem_uuid: filesystem_uuid.into(),
                        device_identity: device_identity.into(),
                    },
                });
            }
            candidates
        }
        RuntimePlatform::Wsl2 => {
            let Some(windows) = snapshot.windows.as_ref() else {
                return Vec::new();
            };
            if !sample_is_fresh(windows.observed_unix_ms, now_unix_ms) {
                return Vec::new();
            }
            windows
                .volumes
                .iter()
                .filter_map(|volume| {
                    let volume_id = volume.volume_id.as_deref()?;
                    let matching_id_count = windows
                        .volumes
                        .iter()
                        .filter(|candidate| {
                            candidate
                                .volume_id
                                .as_deref()
                                .is_some_and(|observed| observed.eq_ignore_ascii_case(volume_id))
                        })
                        .count();
                    let free_bytes = windows_volume_eligibility(volume, matching_id_count).ok()?;
                    if !draft_wsl_target_path_is_valid(volume_id, volume.drive_letter.as_deref()) {
                        return None;
                    }
                    Some(DraftVolumeCandidate {
                        display: format!(
                            "{} {} ({}) — volume {}",
                            volume.drive_letter.as_deref().map_or_else(
                                || "no drive letter".into(),
                                |letter| { format!("{letter}:") }
                            ),
                            volume.label.as_deref().unwrap_or("unlabeled"),
                            volume
                                .file_system
                                .as_deref()
                                .unwrap_or("filesystem unknown"),
                            volume_id
                        ),
                        free_bytes,
                        storage: DraftStorageIdentity::Wsl2 {
                            volume_id: volume_id.into(),
                            drive_letter: volume.drive_letter.clone(),
                        },
                    })
                })
                .collect()
        }
    }
}

fn draft_windows_path(
    volume_id: &str,
    drive_letter: Option<&str>,
    role: DraftTargetRole,
) -> Result<String, InventoryError> {
    let leaf = match role {
        DraftTargetRole::FallbackSwap => "fallback-swap.vhdx",
        DraftTargetRole::RamSharedOrigin => "origin.vhdx",
    };
    let path = if let Some(letter) = drive_letter {
        let letter = letter.trim_end_matches(':');
        if letter.len() != 1 || !letter.as_bytes()[0].is_ascii_alphabetic() {
            return Err(InventoryError(
                "selected Windows volume has an invalid drive-letter display value".into(),
            ));
        }
        format!("{}:\\wsl\\ramshared\\{leaf}", letter.to_ascii_uppercase())
    } else {
        format!(
            "{}\\wsl\\ramshared\\{leaf}",
            volume_id.trim_end_matches(['\\', '/'])
        )
    };
    Ok(path)
}

fn draft_wsl_target_path_is_valid(volume_id: &str, drive_letter: Option<&str>) -> bool {
    let Ok(path) = draft_windows_path(volume_id, drive_letter, DraftTargetRole::FallbackSwap)
    else {
        return false;
    };
    let profile = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![ResourceTarget::WslFallback {
            windows_volume_id: volume_id.into(),
            path,
            bytes: 1,
        }],
    };
    profile.validate_for(ResourcePlatform::Wsl2).is_ok()
}

fn materialize_draft_target(
    candidate: &DraftVolumeCandidate,
    role: DraftTargetRole,
    bytes: u64,
) -> Result<ResourceTarget, InventoryError> {
    match &candidate.storage {
        DraftStorageIdentity::NativeLinux {
            filesystem_uuid,
            device_identity,
        } => Ok(match role {
            DraftTargetRole::FallbackSwap => ResourceTarget::LinuxSwapfile {
                filesystem_uuid: filesystem_uuid.clone(),
                device_identity: device_identity.clone(),
                managed_relative_path: DRAFT_LINUX_SWAP_PATH.into(),
                bytes,
                priority: -1,
            },
            DraftTargetRole::RamSharedOrigin => ResourceTarget::LinuxFileOriginRequest {
                filesystem_uuid: filesystem_uuid.clone(),
                device_identity: device_identity.clone(),
                managed_relative_path: DRAFT_LINUX_ORIGIN_PATH.into(),
                allocated_bytes: bytes,
            },
        }),
        DraftStorageIdentity::Wsl2 {
            volume_id,
            drive_letter,
        } => {
            let path = draft_windows_path(volume_id, drive_letter.as_deref(), role)?;
            Ok(match role {
                DraftTargetRole::FallbackSwap => ResourceTarget::WslFallback {
                    windows_volume_id: volume_id.clone(),
                    path,
                    bytes,
                },
                DraftTargetRole::RamSharedOrigin => ResourceTarget::WslOrigin {
                    windows_volume_id: volume_id.clone(),
                    path,
                    allocated_bytes: bytes,
                },
            })
        }
    }
}

fn parse_draft_size_mib(value: &str) -> Result<u64, InventoryError> {
    let mib = value
        .trim()
        .parse::<u64>()
        .map_err(|_| InventoryError("size must be a positive integer MiB value".into()))?;
    if mib == 0 {
        return Err(InventoryError("size must be greater than zero".into()));
    }
    mib.checked_mul(1024 * 1024)
        .ok_or_else(|| InventoryError("size exceeds the supported byte range".into()))
}

fn read_draft_line(
    input: &mut dyn Read,
    output: &mut dyn Write,
    prompt: &str,
) -> Result<String, InventoryError> {
    write!(output, "{prompt}")
        .map_err(|error| InventoryError(format!("cannot write config prompt: {error}")))?;
    output
        .flush()
        .map_err(|error| InventoryError(format!("cannot flush config prompt: {error}")))?;
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        let read = input
            .read(&mut byte)
            .map_err(|error| InventoryError(format!("cannot read config input: {error}")))?;
        if read == 0 {
            return Err(InventoryError(
                "end of input before draft confirmation".into(),
            ));
        }
        match byte[0] {
            b'\n' => break,
            b'\r' => continue,
            value if bytes.len() < MAX_DRAFT_LINE_BYTES => bytes.push(value),
            _ => return Err(InventoryError("config input line exceeds 128 bytes".into())),
        }
    }
    String::from_utf8(bytes)
        .map_err(|error| InventoryError(format!("config input is not UTF-8: {error}")))
}

fn resolve_user_draft_path(path: &Path) -> Result<(PathBuf, PathBuf), InventoryError> {
    let file_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| InventoryError("draft output must name a new file".into()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        InventoryError(format!("cannot resolve draft parent directory: {error}"))
    })?;
    let metadata = fs::symlink_metadata(&canonical_parent).map_err(|error| {
        InventoryError(format!("cannot inspect draft parent directory: {error}"))
    })?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(InventoryError(
            "draft parent must be a directory owned by this user and not group/world writable"
                .into(),
        ));
    }
    Ok((canonical_parent.clone(), canonical_parent.join(file_name)))
}

fn remove_draft_if_same_file(path: &Path, opened: &fs::File) {
    let Ok(opened_metadata) = opened.metadata() else {
        return;
    };
    let Ok(path_metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if !path_metadata.file_type().is_symlink()
        && path_metadata.dev() == opened_metadata.dev()
        && path_metadata.ino() == opened_metadata.ino()
    {
        let _ = fs::remove_file(path);
    }
}

fn save_profile_draft(path: &Path, profile_text: &str) -> Result<PathBuf, InventoryError> {
    if profile_text.len() > MAX_RESOURCE_PROFILE_BYTES {
        return Err(InventoryError(
            "draft profile exceeds the 64 KiB size limit".into(),
        ));
    }
    let (parent, resolved_path) = resolve_user_draft_path(path)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options.open(&resolved_path).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            InventoryError("draft output already exists; it was not overwritten".into())
        } else {
            InventoryError(format!("cannot create draft output: {error}"))
        }
    })?;
    let write_result = (|| {
        fchmod(&file, Mode::RUSR | Mode::WUSR)
            .map_err(|error| io::Error::other(error.to_string()))?;
        file.write_all(profile_text.as_bytes())?;
        file.sync_all()?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o777 != 0o600
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "draft file owner, type, link count, or mode verification failed",
            ));
        }
        if metadata.len() != profile_text.len() as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "draft file length does not match the requested profile",
            ));
        }
        file.seek(SeekFrom::Start(0))?;
        let mut readback = vec![0; profile_text.len()];
        file.read_exact(&mut readback)?;
        let mut trailing = [0_u8; 1];
        if file.read(&mut trailing)? != 0 || readback != profile_text.as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "draft file content does not match the requested profile",
            ));
        }
        fs::File::open(&parent)?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        remove_draft_if_same_file(&resolved_path, &file);
        if let Ok(parent_directory) = fs::File::open(&parent) {
            let _ = parent_directory.sync_all();
        }
        return Err(InventoryError(format!(
            "could not securely write profile draft: {error}"
        )));
    }
    Ok(resolved_path)
}

fn display_draft_candidates(
    snapshot: &ResourceSnapshot,
    candidates: &[DraftVolumeCandidate],
    output: &mut dyn Write,
) -> Result<(), InventoryError> {
    write!(output, "{}", render_text(snapshot))
        .map_err(|error| InventoryError(format!("cannot display resource inventory: {error}")))?;
    if candidates.is_empty() {
        return Err(InventoryError(
            "no fresh, uniquely identified eligible storage volume is available for a draft target"
                .into(),
        ));
    }
    writeln!(output, "Eligible draft targets:")
        .map_err(|error| InventoryError(format!("cannot display draft candidates: {error}")))?;
    for (index, candidate) in candidates.iter().enumerate() {
        writeln!(
            output,
            "  {}. {} — free {}",
            index + 1,
            candidate.display,
            format_gib(Some(candidate.free_bytes))
        )
        .map_err(|error| InventoryError(format!("cannot display draft candidates: {error}")))?;
    }
    Ok(())
}

fn add_draft_target(
    snapshot: &ResourceSnapshot,
    candidates: &[DraftVolumeCandidate],
    profile: &mut ResourceProfile,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<bool, InventoryError> {
    let role_text = read_draft_line(input, output, "Add target [swap | origin | done]: ")?;
    let role = match role_text.trim().to_ascii_lowercase().as_str() {
        "swap" => DraftTargetRole::FallbackSwap,
        "origin" => DraftTargetRole::RamSharedOrigin,
        "done" => return Ok(false),
        _ => {
            return Err(InventoryError(
                "target role must be swap, origin, or done".into(),
            ));
        }
    };
    let volume_text = read_draft_line(input, output, "Eligible volume number: ")?;
    let volume_index = volume_text
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1))
        .filter(|index| *index < candidates.len())
        .ok_or_else(|| InventoryError("selected volume number is not in the list".into()))?;
    let size_text = read_draft_line(input, output, "Target size (MiB): ")?;
    let bytes = parse_draft_size_mib(&size_text)?;
    profile.targets.push(materialize_draft_target(
        &candidates[volume_index],
        role,
        bytes,
    )?);
    let profile_text = profile
        .to_toml()
        .map_err(|error| InventoryError(format!("cannot encode draft profile: {error}")))?;
    let plan = match build_resource_plan(snapshot, Some(&profile_text)) {
        Ok(plan) if plan.status == "ready_for_review" => plan,
        Ok(plan) => {
            profile.targets.pop();
            writeln!(
                output,
                "Target refused; it was not added:\n{}",
                render_plan_text(&plan)
            )
            .map_err(|error| InventoryError(format!("cannot display target refusal: {error}")))?;
            return Ok(true);
        }
        Err(error) => {
            profile.targets.pop();
            writeln!(output, "Target refused: {error}").map_err(|write_error| {
                InventoryError(format!("cannot display target refusal: {write_error}"))
            })?;
            return Ok(true);
        }
    };
    let selected = plan
        .targets
        .last()
        .map(|target| target.kind)
        .unwrap_or("target");
    writeln!(
        output,
        "Added {selected}; current plan remains read-only and apply-disabled."
    )
    .map_err(|error| InventoryError(format!("cannot display draft result: {error}")))?;
    Ok(true)
}

fn collect_draft_profile(
    snapshot: &ResourceSnapshot,
    candidates: &[DraftVolumeCandidate],
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<ResourceProfile, InventoryError> {
    let mut profile = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: Vec::new(),
    };
    loop {
        if profile.targets.len() == MAX_DRAFT_TARGETS {
            writeln!(output, "Maximum of {MAX_DRAFT_TARGETS} targets reached.")
                .map_err(|error| InventoryError(format!("cannot write draft result: {error}")))?;
            break;
        }
        if !add_draft_target(snapshot, candidates, &mut profile, input, output)? {
            break;
        }
    }
    if profile.targets.is_empty() {
        return Err(InventoryError("no draft targets were selected".into()));
    }
    profile
        .validate_for(profile_platform(snapshot.platform))
        .map_err(|error| InventoryError(format!("draft profile is invalid: {error}")))?;
    Ok(profile)
}

fn save_reviewed_draft(
    snapshot: &ResourceSnapshot,
    profile: &ResourceProfile,
    output_path: &Path,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<(), InventoryError> {
    let profile_text = profile
        .to_toml()
        .map_err(|error| InventoryError(format!("cannot encode draft profile: {error}")))?;
    let plan = build_resource_plan(snapshot, Some(&profile_text))?;
    if plan.status != "ready_for_review" || plan.apply_enabled || plan.writes_performed {
        return Err(InventoryError(
            "final draft plan is blocked or unexpectedly permits mutation".into(),
        ));
    }
    writeln!(output, "{}", render_plan_text(&plan))
        .map_err(|error| InventoryError(format!("cannot display final draft plan: {error}")))?;
    writeln!(
        output,
        "Draft only: no swap, origin, GPU, .wslconfig, or running tier was changed."
    )
    .map_err(|error| InventoryError(format!("cannot display draft boundary: {error}")))?;
    let confirmation = read_draft_line(
        input,
        output,
        &format!("Type SAVE to create new draft {}: ", output_path.display()),
    )?;
    if confirmation != "SAVE" {
        writeln!(output, "Draft canceled; no file was written.")
            .map_err(|error| InventoryError(format!("cannot display cancellation: {error}")))?;
        return Ok(());
    }
    let saved_path = save_profile_draft(output_path, &profile_text)?;
    writeln!(output, "Validated draft saved at {}.", saved_path.display())
        .map_err(|error| InventoryError(format!("cannot report saved draft: {error}")))
}

fn run_draft_wizard_with_io(
    snapshot: &ResourceSnapshot,
    output_path: &Path,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<(), InventoryError> {
    let candidates = draft_volume_candidates(snapshot, unix_millis());
    display_draft_candidates(snapshot, &candidates, output)?;
    let profile = collect_draft_profile(snapshot, &candidates, input, output)?;
    save_reviewed_draft(snapshot, &profile, output_path, input, output)
}

fn run_draft_wizard(output_path: &str, output: &mut dyn Write) -> Result<(), InventoryError> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(InventoryError(
            "config draft needs a terminal on stdin and stdout; no profile was written".into(),
        ));
    }
    let snapshot = collect_snapshot()?;
    let mut input = io::stdin().lock();
    run_draft_wizard_with_io(&snapshot, Path::new(output_path), &mut input, output)
}

pub(crate) fn run(mode: ConfigMode, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
    let result = match mode {
        ConfigMode::Interactive => run_interactive(),
        ConfigMode::Draft { output_path } => run_draft_wizard(&output_path, stdout),
        ConfigMode::Show { json } => collect_snapshot().and_then(|snapshot| {
            let output = if json {
                render_json(&snapshot)?
            } else {
                render_text(&snapshot)
            };
            writeln!(stdout, "{output}")
                .map_err(|error| InventoryError(format!("cannot write config output: {error}")))
        }),
        ConfigMode::Plan { json, profile_path } => (|| {
            let snapshot = collect_snapshot()?;
            let require_root_owned = profile_path.is_none();
            let path = profile_path.map_or_else(
                || PathBuf::from(DEFAULT_RESOURCE_PROFILE_PATH),
                PathBuf::from,
            );
            let profile_text = load_profile_text(&path, require_root_owned)?;
            if !require_root_owned && profile_text.is_none() {
                return Err(InventoryError(
                    "requested profile file does not exist".into(),
                ));
            }
            let plan = build_resource_plan(&snapshot, profile_text.as_deref())?;
            let output = if json {
                render_plan_json(&plan)?
            } else {
                render_plan_text(&plan)
            };
            writeln!(stdout, "{output}")
                .map_err(|error| InventoryError(format!("cannot write resource plan: {error}")))
        })(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(stderr, "resource config failed: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn platform_detection_distinguishes_native_linux_from_wsl2() {
        assert_eq!(
            classify_platform("6.18.40.1-microsoft-standard-WSL2", ""),
            RuntimePlatform::Wsl2
        );
        assert_eq!(
            classify_platform("6.8.0-45-generic", "Linux version 6.8.0-45-generic"),
            RuntimePlatform::NativeLinux
        );
    }

    #[test]
    fn wsl2_guest_storage_requires_host_volume_identity_and_capacity_binding() {
        let mut candidate = fixture_block_device();
        candidate.eligible_for_file_storage = true;
        candidate.eligibility_reason = "mount is otherwise eligible".into();

        apply_platform_storage_policy(std::slice::from_mut(&mut candidate), RuntimePlatform::Wsl2);
        assert!(!candidate.eligible_for_file_storage);
        assert!(candidate.eligibility_reason.contains("host-volume"));

        candidate.eligible_for_file_storage = true;
        candidate.eligibility_reason = "native filesystem candidate".into();
        apply_platform_storage_policy(
            std::slice::from_mut(&mut candidate),
            RuntimePlatform::NativeLinux,
        );
        assert!(candidate.eligible_for_file_storage);
        assert_eq!(candidate.eligibility_reason, "native filesystem candidate");
    }

    #[test]
    fn meminfo_accepts_user_sized_ram_and_swap_without_product_minima() {
        // Deliberately varied parser fixtures; these are not product defaults.
        let small = parse_meminfo(
            "MemTotal: 262144 kB\nMemAvailable: 196608 kB\nSwapTotal: 131072 kB\nSwapFree: 65536 kB\n",
        );
        let large = parse_meminfo(
            "MemTotal: 50331648 kB\nMemAvailable: 40265318 kB\nSwapTotal: 20971520 kB\nSwapFree: 15728640 kB\n",
        );

        assert!(small.required_counters_available);
        assert!(large.required_counters_available);
        assert_eq!(small.total_bytes, Some(262_144 * 1024));
        assert_eq!(large.swap_total_bytes, Some(20_971_520 * 1024));
    }

    #[test]
    fn meminfo_missing_or_inconsistent_counters_are_unavailable() {
        let missing = parse_meminfo("MemTotal: 4096 kB\n");
        let inconsistent = parse_meminfo(
            "MemTotal: 4096 kB\nMemAvailable: 8192 kB\nSwapTotal: 4096 kB\nSwapFree: 8192 kB\n",
        );

        assert!(!missing.required_counters_available);
        assert!(!inconsistent.required_counters_available);
        assert_eq!(inconsistent.total_bytes, None);
        assert_eq!(inconsistent.swap_total_bytes, None);
    }

    #[test]
    fn swap_parser_keeps_all_devices_and_variable_capacities() {
        let swaps = parse_swap_table(
            "Filename Type Size Used Priority\n/dev/zram0 partition 524288 32768 100\n/mnt/c/swap\\040file file 12582912 0 -2\n",
        )
        .expect("valid swap inventory");

        assert_eq!(swaps.len(), 2);
        assert_eq!(swaps[0].size_kib, 524_288);
        assert_eq!(swaps[1].filename, "/mnt/c/swap\\040file");
        assert_eq!(swaps[1].size_kib, 12_582_912);
    }

    #[test]
    fn linux_block_inventory_preserves_mounted_and_unmounted_devices() {
        let devices = parse_lsblk_json(
            r#"{"blockdevices":[{"name":"nvme0n1","path":"/dev/nvme0n1","type":"disk","size":2000000000000,"maj:min":"259:0","wwn":"wwn-123","ro":false,"rm":false,"tran":"nvme","mountpoints":[null],"children":[{"name":"nvme0n1p1","path":"/dev/nvme0n1p1","type":"part","size":1000000000000,"maj:min":"259:1","partuuid":"part-uuid","ro":false,"rm":false,"fstype":"ext4","uuid":"fs-uuid","mountpoints":["/data"],"pkname":"nvme0n1"}]},{"name":"sda","path":"/dev/sda","type":"disk","size":500000000000,"maj:min":"8:0","ro":false,"mountpoints":[null]}]}"#,
        )
        .expect("valid lsblk response");

        assert_eq!(devices.len(), 3);
        assert!(devices.iter().any(|device| device.name == "nvme0n1"));
        let unmounted = devices
            .iter()
            .find(|device| device.name == "sda")
            .expect("unmounted disk is visible");
        assert!(unmounted.mountpoints.is_empty());
        let mounted = devices
            .iter()
            .find(|device| device.name == "nvme0n1p1")
            .expect("mounted filesystem is visible");
        assert_eq!(mounted.filesystem.as_deref(), Some("ext4"));
        assert_eq!(mounted.mountpoints, vec!["/data"]);
        assert!(!mounted.eligible_for_file_storage);
        assert_eq!(mounted.major_minor.as_deref(), Some("259:1"));
        assert_eq!(mounted.partition_uuid.as_deref(), Some("part-uuid"));
        assert_eq!(
            mounted.parent_hardware_identity.as_deref(),
            Some("wwn:wwn-123")
        );
        assert_eq!(mounted.removable, Some(false));
        assert_eq!(mounted.transport.as_deref(), Some("nvme"));
    }

    #[test]
    fn mountinfo_parser_decodes_paths_and_records_mount_identity_and_access() {
        let mounts = parse_mountinfo(
            "41 32 259:1 / /mnt/data\\040disk rw,nosuid shared:7 - ext4 /dev/nvme0n1p1 rw,errors=remount-ro\n42 32 8:1 / /mnt/readonly ro - xfs /dev/sda1 ro\n",
        )
        .expect("valid mountinfo");

        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].mount_id, 41);
        assert_eq!(mounts[0].parent_mount_id, 32);
        assert_eq!(mounts[0].major_minor, "259:1");
        assert_eq!(mounts[0].mountpoint, "/mnt/data disk");
        assert_eq!(mounts[0].filesystem, "ext4");
        assert!(!mounts[0].read_only);
        assert!(mounts[1].read_only);
    }

    #[test]
    fn storage_candidate_requires_current_writable_mount_capacity_and_stable_identity() {
        let writable = MountInfo {
            mount_id: 41,
            parent_mount_id: 32,
            major_minor: "259:1".into(),
            root: "/".into(),
            mountpoint: "/data".into(),
            mount_options: vec!["rw".into()],
            filesystem: "ext4".into(),
            source: "/dev/nvme0n1p1".into(),
            super_options: vec!["rw".into()],
            read_only: false,
            total_bytes: Some(300 * 1024 * 1024 * 1024),
            available_bytes: Some(200 * 1024 * 1024 * 1024),
            capacity_observed_unix_ms: Some(unix_millis()),
        };
        let candidate = BlockDevice {
            name: "nvme0n1p1".into(),
            path: "/dev/nvme0n1p1".into(),
            kind: "part".into(),
            size_bytes: Some(300 * 1024 * 1024 * 1024),
            filesystem: Some("ext4".into()),
            uuid: Some("fs-uuid".into()),
            mountpoints: vec!["/data".into()],
            parent: Some("nvme0n1".into()),
            major_minor: Some("259:1".into()),
            partition_uuid: Some("part-uuid".into()),
            hardware_identity: None,
            parent_hardware_identity: Some("wwn:wwn-123".into()),
            mounts: vec![writable.clone()],
            read_only: Some(false),
            removable: Some(false),
            rotational: Some(false),
            transport: Some("nvme".into()),
            model: Some("Test NVMe".into()),
            eligible_for_file_storage: false,
            eligibility_reason: String::new(),
        };

        assert!(storage_eligibility(&candidate, Some(&writable)).0);
        assert!(!storage_eligibility(&candidate, None).0);
        let mut missing_uuid = candidate.clone();
        missing_uuid.uuid = None;
        assert!(!storage_eligibility(&missing_uuid, Some(&writable)).0);
        let mut missing_parent_identity = candidate.clone();
        missing_parent_identity.parent_hardware_identity = None;
        assert!(!storage_eligibility(&missing_parent_identity, Some(&writable)).0);
        let mut unknown_removable = candidate.clone();
        unknown_removable.removable = None;
        assert!(!storage_eligibility(&unknown_removable, Some(&writable)).0);
        let mut removable = candidate.clone();
        removable.removable = Some(true);
        assert!(!storage_eligibility(&removable, Some(&writable)).0);
        let mut usb = candidate.clone();
        usb.transport = Some("usb".into());
        assert!(!storage_eligibility(&usb, Some(&writable)).0);

        let mut read_only = writable.clone();
        read_only.read_only = true;
        assert!(!storage_eligibility(&candidate, Some(&read_only)).0);
        let mut unknown_capacity = writable.clone();
        unknown_capacity.available_bytes = None;
        assert!(!storage_eligibility(&candidate, Some(&unknown_capacity)).0);
        let mut unsupported = writable;
        unsupported.filesystem = "btrfs".into();
        assert!(!storage_eligibility(&candidate, Some(&unsupported)).0);

        let mut wrong_device = fixture_mount_info();
        wrong_device.major_minor = "8:1".into();
        assert!(!storage_eligibility(&candidate, Some(&wrong_device)).0);
        let mut missing_total = fixture_mount_info();
        missing_total.total_bytes = None;
        assert!(!storage_eligibility(&candidate, Some(&missing_total)).0);
        let mut unmounted_device = candidate.clone();
        unmounted_device.kind = "disk".into();
        assert!(!storage_eligibility(&unmounted_device, Some(&fixture_mount_info())).0);
        let mut read_only_device = candidate.clone();
        read_only_device.read_only = Some(true);
        assert!(!storage_eligibility(&read_only_device, Some(&fixture_mount_info())).0);
        let mut xfs_mount = fixture_mount_info();
        xfs_mount.filesystem = "xfs".into();
        assert!(storage_eligibility(&candidate, Some(&xfs_mount)).0);
    }

    #[test]
    fn storage_candidate_rejects_filesystem_subtree_mounts() {
        let device = fixture_block_device();
        let mut mount = fixture_mount_info();
        mount.root = "/mounted-subtree".into();

        let (eligible, reason) = storage_eligibility(&device, Some(&mount));

        assert!(!eligible);
        assert!(reason.contains("filesystem root"));
    }

    #[test]
    fn network_backed_block_devices_are_ineligible_and_multiple_local_disks_remain_eligible() {
        let nvme = fixture_block_device();
        let nvme_mount = fixture_mount_info();

        let mut sata = fixture_block_device();
        sata.name = "sdb1".into();
        sata.path = "/dev/sdb1".into();
        sata.major_minor = Some("8:17".into());
        sata.parent = Some("sdb".into());
        sata.parent_hardware_identity = Some("serial:local-sata-2".into());
        sata.transport = Some("sata".into());
        let sata_mount = MountInfo {
            major_minor: "8:17".into(),
            mountpoint: "/mnt/second".into(),
            source: "/dev/sdb1".into(),
            filesystem: "xfs".into(),
            ..fixture_mount_info()
        };

        assert!(storage_eligibility(&nvme, Some(&nvme_mount)).0);
        assert!(storage_eligibility(&sata, Some(&sata_mount)).0);

        for transport in ["iscsi", "nbd", "rbd", "fcoe", "nvme-of"] {
            let mut remote = nvme.clone();
            remote.transport = Some(transport.into());
            let (eligible, reason) = storage_eligibility(&remote, Some(&nvme_mount));
            assert!(!eligible, "{transport} must not be a writable local target");
            assert!(reason.contains("network"), "unexpected reason: {reason}");
        }

        for transport in [None, Some("unclassified".into())] {
            let mut ambiguous = nvme.clone();
            ambiguous.transport = transport;
            let (eligible, reason) = storage_eligibility(&ambiguous, Some(&nvme_mount));
            assert!(!eligible, "unproven local transport must be refused");
            assert!(reason.contains("transport"), "unexpected reason: {reason}");
        }
    }

    #[test]
    fn mounted_whole_disk_filesystem_uses_its_own_stable_identity() {
        let mut disk = fixture_block_device();
        disk.name = "sdc".into();
        disk.path = "/dev/sdc".into();
        disk.kind = "disk".into();
        disk.size_bytes = Some(1024 * 1024 * 1024 * 1024);
        disk.uuid = Some("root-fs-uuid".into());
        disk.parent = None;
        disk.major_minor = Some("8:32".into());
        disk.partition_uuid = None;
        disk.hardware_identity = Some("wwn:virtual-disk-123".into());
        disk.parent_hardware_identity = None;

        let mount = MountInfo {
            major_minor: "8:32".into(),
            source: "/dev/sdc".into(),
            ..fixture_mount_info()
        };
        assert!(storage_eligibility(&disk, Some(&mount)).0);

        disk.hardware_identity = None;
        let (eligible, reason) = storage_eligibility(&disk, Some(&mount));
        assert!(!eligible);
        assert!(reason.contains("stable"));
    }

    #[test]
    fn mount_capacity_uses_live_available_blocks_without_writing() {
        let (total, available) = mount_capacity_bytes("/").expect("root filesystem capacity");
        assert!(available > 0);
        assert!(total >= available);
    }

    #[test]
    fn windows_snapshot_lists_multiple_host_volumes_and_live_memory() {
        let snapshot = parse_windows_snapshot(
            r#"{"observed_utc":"2026-09-27T22:00:00Z","host_memory":{"total_bytes":34359738368,"free_bytes":17179869184,"committed_bytes":25769803776,"commit_limit_bytes":60129542144},"volumes":[{"drive_letter":"C","label":"System","file_system":"NTFS","drive_type":"Fixed","size_bytes":500000000000,"free_bytes":200000000000,"volume_id":"vol-c"},{"drive_letter":"I","label":"Data","file_system":"NTFS","drive_type":"Fixed","size_bytes":1000000000000,"free_bytes":600000000000,"volume_id":"vol-i"}]}"#,
        )
        .expect("valid Windows snapshot");

        assert_eq!(snapshot.host_memory.free_bytes, Some(17_179_869_184));
        assert_eq!(snapshot.volumes.len(), 2);
        assert_eq!(snapshot.volumes[0].drive_letter.as_deref(), Some("C"));
        assert_eq!(snapshot.volumes[1].drive_letter.as_deref(), Some("I"));
    }

    #[test]
    fn windows_inventory_lists_every_volume_and_explains_ineligible_targets() {
        let mut snapshot = fixture_snapshot();
        let windows = snapshot.windows.as_mut().expect("Windows snapshot fixture");
        windows.volumes.extend([
            WindowsVolume {
                drive_letter: Some("F".into()),
                label: Some("Removable".into()),
                file_system: Some("exFAT".into()),
                drive_type: "Removable".into(),
                size_bytes: Some(64 * 1024 * 1024 * 1024),
                free_bytes: Some(32 * 1024 * 1024 * 1024),
                volume_id: Some("volume-f".into()),
            },
            WindowsVolume {
                drive_letter: Some("G".into()),
                label: Some("Unsupported".into()),
                file_system: Some("FAT32".into()),
                drive_type: "Fixed".into(),
                size_bytes: Some(64 * 1024 * 1024 * 1024),
                free_bytes: Some(32 * 1024 * 1024 * 1024),
                volume_id: Some("volume-g".into()),
            },
            WindowsVolume {
                drive_letter: Some("H".into()),
                label: Some("Unidentified".into()),
                file_system: Some("NTFS".into()),
                drive_type: "Fixed".into(),
                size_bytes: Some(64 * 1024 * 1024 * 1024),
                free_bytes: Some(32 * 1024 * 1024 * 1024),
                volume_id: None,
            },
            WindowsVolume {
                drive_letter: None,
                label: Some("Malformed path".into()),
                file_system: Some("NTFS".into()),
                drive_type: "Fixed".into(),
                size_bytes: Some(64 * 1024 * 1024 * 1024),
                free_bytes: Some(32 * 1024 * 1024 * 1024),
                volume_id: Some("not-a-volume-guid".into()),
            },
        ]);

        let output = render_text(&snapshot);

        assert!(output.contains("Windows volumes:"));
        assert!(output.contains("C: System NTFS (Fixed) — ID vol-c — eligible"));
        assert!(output.contains(
            "F: Removable exFAT (Removable) — ID volume-f — ineligible: volume is not fixed"
        ));
        assert!(output.contains(
            "G: Unsupported FAT32 (Fixed) — ID volume-g — ineligible: filesystem is not NTFS/ReFS"
        ));
        assert!(output.contains("H: Unidentified NTFS (Fixed) — identity unknown — ineligible: stable volume identity is unavailable"));
        assert!(output.contains("(no drive letter) Malformed path NTFS (Fixed) — ID not-a-volume-guid — ineligible: no safe canonical target path"));
    }

    #[test]
    fn windows_inventory_probe_does_not_filter_volumes_by_drive_type() {
        let script = windows_inventory_script();
        let normalized = script
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();

        assert!(normalized.contains("get-volume-erroractionstop"));
        assert!(!normalized.contains("where-object{$_"));
        assert!(normalized.contains("size_bytes=if($null-ne$_.size)"));
        assert!(normalized.contains("free_bytes=if($null-ne$_.sizeremaining)"));
    }

    #[test]
    fn resource_view_labels_guest_and_windows_host_memory_separately() {
        let snapshot = fixture_snapshot();
        let output = render_text(&snapshot);

        assert!(output.contains("WSL guest RAM"));
        assert!(output.contains("Windows host RAM"));
        assert!(output.contains("Windows commit headroom"));
        assert!(output.contains("C:"));
        assert!(output.contains("I:"));
    }

    #[test]
    fn resource_view_keeps_native_linux_memory_out_of_windows_scope() {
        let mut snapshot = fixture_snapshot();
        snapshot.platform = RuntimePlatform::NativeLinux;
        snapshot.windows = None;
        let mut device = fixture_block_device();
        device.mounts = vec![fixture_mount_info()];
        device.eligible_for_file_storage = true;
        snapshot.block_devices.push(device);
        let output = render_text(&snapshot);

        assert!(output.contains("System RAM"));
        assert!(!output.contains("Windows host RAM"));
        assert!(output.contains("Test NVMe"));
        assert!(output.contains("/data [ext4 mount 41"));
        assert!(output.contains("free 200.00 GiB of 300.00 GiB"));
        assert!(output.contains("Storage speed comparison: not measured"));
    }

    #[test]
    fn malformed_device_and_windows_payloads_fail_closed() {
        assert!(parse_lsblk_json("not json").is_err());
        assert!(parse_lsblk_json("{}").is_err());
        assert!(parse_mountinfo("malformed mount record").is_err());
        assert!(parse_mountinfo("1 0 8:1 / /bad\\777 rw - ext4 /dev/sda1 rw").is_err());
        assert!(
            parse_lsblk_json(r#"{"blockdevices":[{"path":"/dev/sda","type":"disk"}]}"#).is_err()
        );
        assert!(parse_swap_table("Filename Type Size Used Priority\nbad row\n").is_err());
        assert!(parse_windows_snapshot("[]").is_err());
        assert!(parse_windows_snapshot("{}").is_err());
        assert!(
            parse_windows_snapshot(r#"{"host_memory":{},"volumes":[null],"observed_utc":"now"}"#)
                .is_err()
        );
        assert!(parse_windows_snapshot(r#"{"host_memory":{},"volumes":[]}"#).is_err());
    }

    #[test]
    fn interactive_config_requires_a_tty_and_tui_draws_both_memory_scopes() {
        if !io::stdin().is_terminal() {
            let error = run_interactive().expect_err("non-terminal interactive mode refuses");
            assert!(error.to_string().contains("needs a terminal"));
        }

        let backend = ratatui::backend::TestBackend::new(160, 48);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| draw_config_frame(frame, &fixture_snapshot(), 0))
            .expect("draw resource configuration screen");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("WSL guest RAM"));
        assert!(rendered.contains("Windows host RAM"));
        assert!(rendered.contains("C:"));
        assert!(rendered.contains("I:"));
    }

    #[test]
    fn config_draft_builds_native_targets_from_eligible_mounts() {
        let mut snapshot = fixture_snapshot();
        snapshot.platform = RuntimePlatform::NativeLinux;
        snapshot.windows = None;
        snapshot.block_devices = vec![fixture_block_device()];

        let candidates = draft_volume_candidates(&snapshot, unix_millis());
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].display.contains("/data"));
        assert_eq!(candidates[0].free_bytes, 200 * 1024 * 1024 * 1024);

        let swap = materialize_draft_target(
            &candidates[0],
            DraftTargetRole::FallbackSwap,
            2 * 1024 * 1024 * 1024,
        )
        .expect("native swap request materializes");
        let origin = materialize_draft_target(
            &candidates[0],
            DraftTargetRole::RamSharedOrigin,
            4 * 1024 * 1024 * 1024,
        )
        .expect("native origin request materializes");
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps::default(),
            targets: vec![swap, origin],
        };
        let profile_text = profile.to_toml().expect("draft profile encodes");
        let plan = build_resource_plan(&snapshot, Some(&profile_text))
            .expect("native selected targets plan read-only");

        assert_eq!(plan.status, "ready_for_review");
        assert!(!plan.apply_enabled);
        assert!(!plan.writes_performed);
        assert_eq!(
            plan.targets[0].volume_identity,
            "filesystem:fs-uuid;device:wwn:wwn-123"
        );
        assert_eq!(plan.targets[0].requested_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(plan.targets[1].requested_bytes, 4 * 1024 * 1024 * 1024);
        assert!(profile_text.contains("linux_file_origin_request"));
    }

    #[test]
    fn config_draft_builds_wsl_targets_from_unique_eligible_volumes() {
        let snapshot = fixture_snapshot();
        let candidates = draft_volume_candidates(&snapshot, unix_millis());
        assert_eq!(candidates.len(), 2);
        let selected = candidates
            .iter()
            .find(|candidate| {
                matches!(
                    &candidate.storage,
                    DraftStorageIdentity::Wsl2 { volume_id, .. } if volume_id == "vol-i"
                )
            })
            .expect("I volume remains selectable");
        assert!(selected.display.contains("I:"));

        let swap = materialize_draft_target(
            selected,
            DraftTargetRole::FallbackSwap,
            2 * 1024 * 1024 * 1024,
        )
        .expect("WSL fallback selection materializes");
        let origin = materialize_draft_target(
            selected,
            DraftTargetRole::RamSharedOrigin,
            4 * 1024 * 1024 * 1024,
        )
        .expect("WSL origin selection materializes");
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps::default(),
            targets: vec![swap, origin],
        };
        let profile_text = profile.to_toml().expect("WSL draft profile encodes");
        let plan = build_resource_plan(&snapshot, Some(&profile_text))
            .expect("selected WSL targets plan read-only");

        assert_eq!(plan.status, "ready_for_review");
        assert_eq!(
            plan.targets[0].path,
            "I:\\wsl\\ramshared\\fallback-swap.vhdx"
        );
        assert_eq!(plan.targets[1].path, "I:\\wsl\\ramshared\\origin.vhdx");
        assert!(
            plan.targets
                .iter()
                .all(|target| target.volume_identity == "vol-i")
        );
        assert!(!plan.apply_enabled);
        assert!(!plan.writes_performed);
    }

    #[test]
    fn config_draft_builds_wsl_volume_guid_target_without_drive_letter() {
        let mut snapshot = fixture_snapshot();
        let volume_guid = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\";
        snapshot
            .windows
            .as_mut()
            .expect("Windows inventory fixture")
            .volumes
            .push(WindowsVolume {
                drive_letter: None,
                label: Some("Unlettered data".into()),
                file_system: Some("NTFS".into()),
                drive_type: "Fixed".into(),
                size_bytes: Some(300 * 1024 * 1024 * 1024),
                free_bytes: Some(200 * 1024 * 1024 * 1024),
                volume_id: Some(volume_guid.into()),
            });

        let candidates = draft_volume_candidates(&snapshot, unix_millis());
        let selected = candidates
            .iter()
            .find(|candidate| {
                matches!(
                    &candidate.storage,
                    DraftStorageIdentity::Wsl2 { volume_id, drive_letter: None }
                        if volume_id == volume_guid
                )
            })
            .expect("canonical volume-GUID target is selectable without a drive letter");
        let target = materialize_draft_target(
            selected,
            DraftTargetRole::RamSharedOrigin,
            1024 * 1024 * 1024,
        )
        .expect("volume-GUID origin request materializes");
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps::default(),
            targets: vec![target],
        };
        let profile_text = profile.to_toml().expect("volume-GUID profile encodes");
        let plan = build_resource_plan(&snapshot, Some(&profile_text))
            .expect("volume-GUID target plans against its unique volume");

        assert_eq!(plan.status, "ready_for_review");
        assert_eq!(
            plan.targets[0].path,
            r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\wsl\ramshared\origin.vhdx"
        );
        assert_eq!(plan.targets[0].volume_identity, volume_guid.to_lowercase());
    }

    #[test]
    fn config_draft_refuses_ineligible_ambiguous_stale_and_overflowed_targets() {
        let now = unix_millis();
        let mut ambiguous_linux = fixture_snapshot();
        ambiguous_linux.platform = RuntimePlatform::NativeLinux;
        ambiguous_linux.windows = None;
        ambiguous_linux.block_devices = vec![fixture_block_device(), fixture_block_device()];
        assert!(draft_volume_candidates(&ambiguous_linux, now).is_empty());

        let mut stale_windows = fixture_snapshot();
        stale_windows
            .windows
            .as_mut()
            .expect("Windows inventory fixture")
            .observed_unix_ms = Some(now.saturating_sub(STORAGE_SAMPLE_MAX_AGE_MS + 1));
        assert!(draft_volume_candidates(&stale_windows, now).is_empty());

        let mut unsupported_windows = fixture_snapshot();
        unsupported_windows
            .windows
            .as_mut()
            .expect("Windows inventory fixture")
            .volumes[1]
            .drive_type = "Removable".into();
        assert_eq!(draft_volume_candidates(&unsupported_windows, now).len(), 1);

        let mut duplicate_volume_id = fixture_snapshot();
        let duplicate = duplicate_volume_id
            .windows
            .as_mut()
            .expect("Windows inventory fixture")
            .volumes[1]
            .clone();
        duplicate_volume_id
            .windows
            .as_mut()
            .expect("Windows inventory fixture")
            .volumes
            .push(duplicate);
        assert_eq!(draft_volume_candidates(&duplicate_volume_id, now).len(), 1);

        let mut malformed_unlettered = fixture_snapshot();
        malformed_unlettered
            .windows
            .as_mut()
            .expect("Windows inventory fixture")
            .volumes
            .push(WindowsVolume {
                drive_letter: None,
                label: Some("Malformed identity".into()),
                file_system: Some("NTFS".into()),
                drive_type: "Fixed".into(),
                size_bytes: Some(64 * 1024 * 1024 * 1024),
                free_bytes: Some(32 * 1024 * 1024 * 1024),
                volume_id: Some("not-a-volume-guid".into()),
            });
        assert_eq!(draft_volume_candidates(&malformed_unlettered, now).len(), 2);

        assert!(parse_draft_size_mib("0").is_err());
        assert!(parse_draft_size_mib("18446744073709551615").is_err());
        assert!(parse_draft_size_mib("-1").is_err());
        assert_eq!(parse_draft_size_mib("4096").unwrap(), 4096 * 1024 * 1024);
    }

    #[test]
    fn resource_plan_aggregates_case_aliases_before_capacity_check() {
        let mut snapshot = fixture_snapshot();
        snapshot
            .windows
            .as_mut()
            .expect("Windows inventory fixture")
            .volumes[0]
            .free_bytes = Some(65 * 1024 * 1024 * 1024);
        let profile = format!(
            r#"
schema_version = 1

[[targets]]
kind = "wsl_fallback"
windows_volume_id = "vol-c"
path = "C:\\wsl\\ramshared\\fallback.vhdx"
bytes = {}

[[targets]]
kind = "wsl_origin"
windows_volume_id = "VOL-C"
path = "C:\\wsl\\ramshared\\origin.vhdx"
allocated_bytes = {}
"#,
            30_u64 * 1024 * 1024 * 1024,
            30_u64 * 1024 * 1024 * 1024
        );

        let plan = build_resource_plan(&snapshot, Some(&profile))
            .expect("case variants resolve to the same current volume");

        assert_eq!(plan.status, "blocked");
        assert_eq!(plan.targets[0].status, "insufficient_space");
        assert_eq!(plan.targets[1].status, "insufficient_space");
        assert_eq!(plan.targets[0].required_free_bytes, 70 * 1024 * 1024 * 1024);
        assert_eq!(plan.targets[0].volume_identity, "vol-c");
        assert!(!plan.apply_enabled);
    }

    #[test]
    fn config_draft_save_requires_owned_parent_uses_mode_0600_and_never_overwrites() {
        let directory = draft_test_directory("save");
        let path = directory.join("profile.toml");
        let first = "schema_version = 1\n";
        let saved = save_profile_draft(&path, first).expect("draft saves securely");
        assert_eq!(saved, path);
        assert_eq!(
            fs::read_to_string(&path).expect("draft content reads back"),
            first
        );
        let metadata = fs::metadata(&path).expect("draft metadata");
        assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert!(save_profile_draft(&path, "changed\n").is_err());
        assert_eq!(fs::read_to_string(&path).expect("original remains"), first);

        let unsafe_directory = directory.join("unsafe");
        fs::create_dir(&unsafe_directory).expect("unsafe fixture directory creates");
        let mut permissions = fs::metadata(&unsafe_directory)
            .expect("unsafe directory metadata")
            .permissions();
        permissions.set_mode(0o777);
        fs::set_permissions(&unsafe_directory, permissions).expect("unsafe mode applies");
        assert!(save_profile_draft(&unsafe_directory.join("blocked.toml"), first).is_err());

        fs::remove_dir_all(directory).expect("draft fixtures removed");
    }

    #[test]
    fn config_draft_wizard_saves_only_after_review_and_explicit_confirmation() {
        let snapshot = fixture_snapshot();
        let directory = draft_test_directory("wizard");
        let output_path = directory.join("profile.toml");
        let mut input = io::Cursor::new("origin\n2\n4096\nswap\n1\n1024\ndone\nSAVE\n");
        let mut output = Vec::new();

        run_draft_wizard_with_io(&snapshot, &output_path, &mut input, &mut output)
            .expect("confirmed draft wizard completes");

        let output = String::from_utf8(output).expect("wizard output is UTF-8");
        assert!(output.contains("Windows host RAM"));
        assert!(output.contains("I:\\wsl\\ramshared\\origin.vhdx"));
        assert!(output.contains("Draft only: no swap, origin, GPU, .wslconfig"));
        assert!(output.contains("Validated draft saved"));
        let profile_text = fs::read_to_string(&output_path).expect("saved draft reads");
        let profile = ResourceProfile::parse(&profile_text).expect("saved profile parses");
        assert_eq!(profile.targets.len(), 2);
        assert!(profile.targets.iter().any(|target| matches!(
            target,
            ResourceTarget::WslOrigin { windows_volume_id, .. } if windows_volume_id == "vol-i"
        )));
        assert!(profile.targets.iter().any(|target| matches!(
            target,
            ResourceTarget::WslFallback { windows_volume_id, .. } if windows_volume_id == "vol-c"
        )));

        let canceled_path = directory.join("canceled.toml");
        let mut input = io::Cursor::new("swap\n1\n1024\ndone\nNO\n");
        let mut output = Vec::new();
        run_draft_wizard_with_io(&snapshot, &canceled_path, &mut input, &mut output)
            .expect("declined draft is a successful cancellation");
        assert!(!canceled_path.exists());

        fs::remove_dir_all(directory).expect("wizard fixtures removed");
    }

    fn draft_test_directory(label: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time follows Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ramshared-config-draft-{label}-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("temporary draft directory creates");
        path
    }

    #[test]
    fn current_guest_config_show_is_read_only_before_and_after() {
        fn topology(text: &str) -> Vec<(String, String, u64, i32)> {
            parse_swap_table(text)
                .expect("guest swap table parses")
                .into_iter()
                .map(|swap| (swap.filename, swap.kind, swap.size_kib, swap.priority))
                .collect()
        }

        let swaps_before = std::fs::read_to_string("/proc/swaps").expect("guest swap table");
        let topology_before = topology(&swaps_before);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run(ConfigMode::Show { json: true }, &mut stdout, &mut stderr);

        let swaps_after = std::fs::read_to_string("/proc/swaps").expect("guest swap table");
        assert_eq!(exit, std::process::ExitCode::SUCCESS);
        assert_eq!(topology(&swaps_after), topology_before);
        let json: serde_json::Value =
            serde_json::from_slice(&stdout).expect("resource snapshot JSON");
        assert!(json.get("platform").is_some());
        assert!(json.get("guest_memory").is_some());
        if json["platform"] == "wsl2" {
            let windows = json
                .get("windows")
                .and_then(serde_json::Value::as_object)
                .unwrap_or_else(|| {
                    panic!(
                        "WSL2 inventory omitted the optional Windows provider; warnings={}",
                        json.get("warnings").unwrap_or(&serde_json::Value::Null)
                    )
                });
            assert!(windows["host_memory"]["free_bytes"].is_number());
            assert!(windows["host_memory"]["commit_limit_bytes"].is_number());
            assert!(!windows["volumes"].as_array().unwrap().is_empty());
        }
    }

    #[test]
    fn config_plan_never_mutates_host_or_guest() {
        let snapshot = fixture_snapshot();
        let before = render_json(&snapshot).expect("snapshot serializes before planning");
        let profile = r#"
schema_version = 1

[[targets]]
kind = "wsl_fallback"
windows_volume_id = "vol-i"
path = "I:\\wsl\\swap.vhdx"
bytes = 4294967296
"#;

        let plan = build_resource_plan(&snapshot, Some(profile)).expect("profile plans");

        assert_eq!(plan.status, "ready_for_review");
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].status, "storage_ready");
        assert_eq!(plan.targets[0].required_free_bytes, 14 * 1024 * 1024 * 1024);
        assert!(!plan.apply_enabled);
        assert_eq!(
            render_json(&snapshot).expect("snapshot serializes after planning"),
            before,
            "planning must leave the observed host/guest snapshot unchanged"
        );
    }

    #[test]
    fn native_linux_plan_resolves_current_mount_from_stable_filesystem_identity() {
        let mut snapshot = fixture_snapshot();
        snapshot.platform = RuntimePlatform::NativeLinux;
        snapshot.windows = None;
        let mut device = fixture_block_device();
        device.eligible_for_file_storage = true;
        snapshot.block_devices = vec![device];
        let profile = r#"
schema_version = 1

[[targets]]
kind = "linux_swapfile"
filesystem_uuid = "fs-uuid"
device_identity = "wwn:wwn-123"
managed_relative_path = "swap/ramshared.swap"
bytes = 1073741824
priority = -1
"#;

        let plan = build_resource_plan(&snapshot, Some(profile)).expect("native plan builds");

        assert_eq!(plan.status, "ready_for_review");
        assert_eq!(plan.targets[0].status, "storage_ready");
        assert_eq!(plan.targets[0].observed_mount_id, Some(41));
        assert_eq!(plan.targets[0].required_free_bytes, 11 * 1024 * 1024 * 1024);
        assert_eq!(
            plan.targets[0].observed_free_bytes,
            Some(200 * 1024 * 1024 * 1024)
        );
        assert!(render_plan_text(&plan).contains("current mount ID (ephemeral): 41"));

        let mut stale_snapshot = snapshot.clone();
        stale_snapshot.block_devices[0].mounts[0].capacity_observed_unix_ms = Some(1);
        let stale_plan = build_resource_plan(&stale_snapshot, Some(profile))
            .expect("stale native telemetry is represented as a refusal");
        assert_eq!(stale_plan.targets[0].status, "stale_sample");
    }

    #[test]
    fn native_linux_origin_request_plan_binds_volume_without_claiming_creation() {
        let mut snapshot = fixture_snapshot();
        snapshot.platform = RuntimePlatform::NativeLinux;
        snapshot.windows = None;
        let mut device = fixture_block_device();
        device.eligible_for_file_storage = true;
        snapshot.block_devices = vec![device];
        let profile = r#"
schema_version = 1

[[targets]]
kind = "linux_file_origin_request"
filesystem_uuid = "fs-uuid"
device_identity = "wwn:wwn-123"
managed_relative_path = "origin/ramshared.img"
allocated_bytes = 4294967296
"#;

        let plan = build_resource_plan(&snapshot, Some(profile)).expect("origin request plans");

        assert_eq!(plan.status, "ready_for_review");
        assert_eq!(plan.targets[0].kind, "linux_file_origin_request");
        assert_eq!(plan.targets[0].status, "storage_ready");
        assert_eq!(plan.targets[0].required_free_bytes, 14 * 1024 * 1024 * 1024);
        assert_eq!(plan.targets[0].observed_mount_id, Some(41));
        assert!(!plan.writes_performed);
        assert!(!plan.apply_enabled);
        assert!(render_plan_text(&plan).contains("Read-only plan"));
    }

    #[test]
    fn native_linux_profile_survives_a_new_mount_namespace_id() {
        let mut snapshot = fixture_snapshot();
        snapshot.platform = RuntimePlatform::NativeLinux;
        snapshot.windows = None;
        let mut device = fixture_block_device();
        device.eligible_for_file_storage = true;
        device.mounts[0].mount_id = 990;
        snapshot.block_devices = vec![device];
        let profile = r#"
schema_version = 1

[[targets]]
kind = "linux_swapfile"
filesystem_uuid = "fs-uuid"
device_identity = "wwn:wwn-123"
managed_relative_path = "swap/ramshared.swap"
bytes = 1073741824
priority = -1
"#;

        let plan = build_resource_plan(&snapshot, Some(profile)).expect("profile plans");

        assert_eq!(plan.targets[0].status, "storage_ready");
        assert_eq!(plan.targets[0].observed_mount_id, Some(990));
    }

    #[test]
    fn native_linux_plan_refuses_multiple_current_mounts_for_one_profile_identity() {
        let mut snapshot = fixture_snapshot();
        snapshot.platform = RuntimePlatform::NativeLinux;
        snapshot.windows = None;
        let mut device = fixture_block_device();
        device.eligible_for_file_storage = true;
        let mut second_mount = device.mounts[0].clone();
        second_mount.mount_id = 42;
        second_mount.mountpoint = "/data-alias".into();
        device.mounts.push(second_mount);
        snapshot.block_devices = vec![device];
        let profile = r#"
schema_version = 1

[[targets]]
kind = "linux_swapfile"
filesystem_uuid = "fs-uuid"
device_identity = "wwn:wwn-123"
managed_relative_path = "swap/ramshared.swap"
bytes = 1073741824
priority = -1
"#;

        let plan = build_resource_plan(&snapshot, Some(profile)).expect("profile plans");

        assert_eq!(plan.targets[0].status, "identity_ambiguous");
        assert_eq!(plan.targets[0].observed_mount_id, None);
    }

    #[test]
    fn resource_plan_without_profile_reports_not_configured_and_read_only() {
        let plan = build_resource_plan(&fixture_snapshot(), None).expect("empty plan builds");

        assert_eq!(plan.profile_state, "not_configured");
        assert_eq!(plan.status, "not_configured");
        assert!(plan.targets.is_empty());
        assert!(!plan.writes_performed);
        assert!(!plan.apply_enabled);
    }

    #[test]
    fn resource_policy_rejects_unknown_stale_and_inconsistent_samples() {
        let unknown_volume = r#"
schema_version = 1

[[targets]]
kind = "wsl_fallback"
windows_volume_id = "volume-not-in-inventory"
path = "C:\\wsl\\swap.vhdx"
bytes = 1048576
"#;
        let unknown_plan = build_resource_plan(&fixture_snapshot(), Some(unknown_volume))
            .expect("unbound target remains a visible refusal");
        assert_eq!(unknown_plan.targets[0].status, "identity_unavailable");

        let mismatched_path = r#"
schema_version = 1

[[targets]]
kind = "wsl_fallback"
windows_volume_id = "vol-i"
path = "C:\\wsl\\swap.vhdx"
bytes = 1048576
"#;
        let mismatched_plan = build_resource_plan(&fixture_snapshot(), Some(mismatched_path))
            .expect("path-volume mismatch remains a visible refusal");
        assert_eq!(mismatched_plan.targets[0].status, "identity_unavailable");

        let valid_volume = r#"
schema_version = 1

[[targets]]
kind = "wsl_fallback"
windows_volume_id = "vol-i"
path = "I:\\wsl\\swap.vhdx"
bytes = 1048576
"#;
        let mut stale_snapshot = fixture_snapshot();
        stale_snapshot
            .windows
            .as_mut()
            .expect("fixture includes host inventory")
            .observed_unix_ms = Some(1);
        let stale_plan = build_resource_plan(&stale_snapshot, Some(valid_volume))
            .expect("stale telemetry is represented as a refusal");
        assert_eq!(stale_plan.targets[0].status, "stale_sample");

        let mut inconsistent_snapshot = fixture_snapshot();
        let volume = &mut inconsistent_snapshot
            .windows
            .as_mut()
            .expect("fixture includes host inventory")
            .volumes[1];
        volume.size_bytes = Some(1);
        volume.free_bytes = Some(2);
        let inconsistent_plan = build_resource_plan(&inconsistent_snapshot, Some(valid_volume))
            .expect("inconsistent capacity is represented as a refusal");
        assert_eq!(inconsistent_plan.targets[0].status, "inconsistent_sample");
    }

    #[test]
    fn resource_plan_rejects_drive_and_volume_guid_aliases_for_same_target() {
        let mut snapshot = fixture_snapshot();
        let volume_guid = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\";
        snapshot
            .windows
            .as_mut()
            .expect("fixture includes host inventory")
            .volumes[1]
            .volume_id = Some(volume_guid.into());
        let profile = format!(
            r#"
schema_version = 1

[[targets]]
kind = "wsl_fallback"
windows_volume_id = {volume_guid:?}
path = "I:\\wsl\\swap.vhdx"
bytes = 1048576

[[targets]]
kind = "wsl_origin"
windows_volume_id = {volume_guid:?}
path = "\\\\?\\Volume{{01234567-89ab-cdef-0123-456789abcdef}}\\wsl\\swap.vhdx"
allocated_bytes = 1048576
"#
        );

        let plan = build_resource_plan(&snapshot, Some(&profile)).expect("profile plans");

        assert_eq!(plan.targets[0].status, "storage_ready");
        assert_eq!(plan.targets[1].status, "duplicate_target_path");
        assert!(!plan.apply_enabled);
    }

    #[test]
    fn profile_loader_rejects_symlinks_oversized_files_and_untrusted_system_profiles() {
        use std::os::unix::fs::symlink;

        let directory = std::env::temp_dir().join(format!(
            "ramshared-profile-loader-{}-{}",
            std::process::id(),
            unix_millis()
        ));
        fs::create_dir(&directory).expect("temporary test directory is created");
        let profile_path = directory.join("profile.toml");
        let profile_content = "schema_version = 1\n";
        fs::write(&profile_path, profile_content).expect("temporary profile is written");

        assert_eq!(
            load_profile_text(&profile_path, false).expect("explicit profile reads"),
            Some(profile_content.into())
        );
        assert!(load_profile_text(&profile_path, true).is_err());

        let symlink_path = directory.join("profile-link.toml");
        symlink(&profile_path, &symlink_path).expect("profile symlink is created");
        assert!(load_profile_text(&symlink_path, false).is_err());
        assert!(load_profile_text(&directory, false).is_err());

        let oversized_path = directory.join("oversized.toml");
        fs::write(&oversized_path, vec![b'x'; MAX_RESOURCE_PROFILE_BYTES + 1])
            .expect("oversized test profile is written");
        assert!(load_profile_text(&oversized_path, false).is_err());

        fs::remove_dir_all(directory).expect("temporary profile fixtures are removed");
    }

    fn fixture_mount_info() -> MountInfo {
        MountInfo {
            mount_id: 41,
            parent_mount_id: 32,
            major_minor: "259:1".into(),
            root: "/".into(),
            mountpoint: "/data".into(),
            mount_options: vec!["rw".into()],
            filesystem: "ext4".into(),
            source: "/dev/nvme0n1p1".into(),
            super_options: vec!["rw".into()],
            read_only: false,
            total_bytes: Some(300 * 1024 * 1024 * 1024),
            available_bytes: Some(200 * 1024 * 1024 * 1024),
            capacity_observed_unix_ms: Some(unix_millis()),
        }
    }

    fn fixture_block_device() -> BlockDevice {
        BlockDevice {
            name: "nvme0n1p1".into(),
            path: "/dev/nvme0n1p1".into(),
            kind: "part".into(),
            size_bytes: Some(300 * 1024 * 1024 * 1024),
            filesystem: Some("ext4".into()),
            uuid: Some("fs-uuid".into()),
            mountpoints: vec!["/data".into()],
            parent: Some("nvme0n1".into()),
            major_minor: Some("259:1".into()),
            partition_uuid: Some("part-uuid".into()),
            hardware_identity: None,
            parent_hardware_identity: Some("wwn:wwn-123".into()),
            mounts: vec![fixture_mount_info()],
            read_only: Some(false),
            removable: Some(false),
            rotational: Some(false),
            transport: Some("nvme".into()),
            model: Some("Test NVMe".into()),
            eligible_for_file_storage: false,
            eligibility_reason: String::new(),
        }
    }

    fn fixture_snapshot() -> ResourceSnapshot {
        ResourceSnapshot {
            platform: RuntimePlatform::Wsl2,
            observed_unix_ms: unix_millis(),
            guest_memory: MemorySnapshot {
                total_bytes: Some(16 * 1024 * 1024 * 1024),
                available_bytes: Some(8 * 1024 * 1024 * 1024),
                swap_total_bytes: Some(4 * 1024 * 1024 * 1024),
                swap_free_bytes: Some(2 * 1024 * 1024 * 1024),
                required_counters_available: true,
            },
            swaps: Vec::new(),
            windows: Some(WindowsSnapshot {
                observed_utc: "2026-09-27T22:00:00Z".into(),
                observed_unix_ms: Some(unix_millis()),
                host_memory: HostMemorySnapshot {
                    total_bytes: Some(32 * 1024 * 1024 * 1024),
                    free_bytes: Some(16 * 1024 * 1024 * 1024),
                    committed_bytes: Some(24 * 1024 * 1024 * 1024),
                    commit_limit_bytes: Some(56 * 1024 * 1024 * 1024),
                },
                volumes: vec![
                    WindowsVolume {
                        drive_letter: Some("C".into()),
                        label: Some("System".into()),
                        file_system: Some("NTFS".into()),
                        drive_type: "Fixed".into(),
                        size_bytes: Some(500_000_000_000),
                        free_bytes: Some(200_000_000_000),
                        volume_id: Some("vol-c".into()),
                    },
                    WindowsVolume {
                        drive_letter: Some("I".into()),
                        label: Some("Data".into()),
                        file_system: Some("NTFS".into()),
                        drive_type: "Fixed".into(),
                        size_bytes: Some(1_000_000_000_000),
                        free_bytes: Some(600_000_000_000),
                        volume_id: Some("vol-i".into()),
                    },
                ],
            }),
            block_devices: Vec::new(),
            gpu_budget_status: "not sampled by storage inventory".into(),
            warnings: Vec::new(),
        }
    }
}
