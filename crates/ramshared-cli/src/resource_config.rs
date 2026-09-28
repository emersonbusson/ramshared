//! Read-only discovery and display for the cross-platform resource settings UI.

use std::collections::HashMap;
use std::fmt::{self, Write as FmtWrite};
use std::io::{self, IsTerminal, Write};
use std::process::{Command, ExitCode};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use serde::Serialize;
use serde_json::Value;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const LINUX_INVENTORY_OUTPUT_LIMIT: usize = 1024 * 1024;
const WINDOWS_INVENTORY_OUTPUT_LIMIT: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigMode {
    Interactive,
    Show { json: bool },
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
    host_memory: HostMemorySnapshot,
    volumes: Vec<WindowsVolume>,
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

fn collect_windows_snapshot() -> Result<WindowsSnapshot, InventoryError> {
    let script = r#"
$ErrorActionPreference = 'Stop'
$os = Get-CimInstance Win32_OperatingSystem
$memory = Get-CimInstance Win32_PerfRawData_PerfOS_Memory
$volumes = @(Get-Volume -ErrorAction Stop | Where-Object { $_.DriveType -eq 'Fixed' } | ForEach-Object {
    [pscustomobject]@{
        drive_letter = if ($null -ne $_.DriveLetter) { [string]$_.DriveLetter } else { $null }
        label = [string]$_.FileSystemLabel
        file_system = [string]$_.FileSystem
        drive_type = [string]$_.DriveType
        size_bytes = [uint64]$_.Size
        free_bytes = [uint64]$_.SizeRemaining
        volume_id = [string]$_.UniqueId
    }
})
$result = [pscustomobject]@{
    observed_utc = [DateTime]::UtcNow.ToString('o')
    host_memory = [pscustomobject]@{
        total_bytes = [uint64]$os.TotalVisibleMemorySize * [uint64]1024
        free_bytes = [uint64]$os.FreePhysicalMemory * [uint64]1024
        committed_bytes = [uint64]$memory.CommittedBytes
        commit_limit_bytes = [uint64]$memory.CommitLimit
    }
    volumes = @($volumes)
}
ConvertTo-Json -InputObject $result -Depth 5 -Compress
"#;
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
    let observed_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;

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
        let _ = writeln!(output, "Windows fixed volumes:");
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
                    let _ = writeln!(
                        output,
                        "  {drive} {} {} — free {} of {}",
                        volume.label.as_deref().unwrap_or("unlabeled"),
                        volume
                            .file_system
                            .as_deref()
                            .unwrap_or("filesystem unknown"),
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

pub(crate) fn run(mode: ConfigMode, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
    let result = match mode {
        ConfigMode::Interactive => run_interactive(),
        ConfigMode::Show { json } => collect_snapshot().and_then(|snapshot| {
            let output = if json {
                render_json(&snapshot)?
            } else {
                render_text(&snapshot)
            };
            writeln!(stdout, "{output}")
                .map_err(|error| InventoryError(format!("cannot write config output: {error}")))
        }),
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
                .expect("WSL2 inventory includes a Windows provider result");
            assert!(windows["host_memory"]["free_bytes"].is_number());
            assert!(windows["host_memory"]["commit_limit_bytes"].is_number());
            assert!(!windows["volumes"].as_array().unwrap().is_empty());
        }
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
            observed_unix_ms: 0,
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
