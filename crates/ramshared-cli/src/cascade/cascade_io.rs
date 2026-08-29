//! I/O orchestration for cascade `up`/`down` (shell, zram, daemon spawn).
//! Hang policy (parse, ghost, orphan plan, swapoff allowlist) stays in parent `cascade`.
//! E2E: `scripts/safety/cascade-health.sh` + BINARY_MATCH — not thrash unit tests.

use super::*;
use crate::bounded_process;
use ramshared_tier::{TierPriorities, validate_order, vram_safety_net};
use rustix::fd::OwnedFd;
use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
#[cfg(not(test))]
use std::os::fd::AsRawFd;
#[cfg(not(test))]
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const SHORT_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_OUTPUT_LIMIT: usize = 64 * 1024;
const LIFECYCLE_BINDING_SCHEMA: u32 = 1;
const LIFECYCLE_BINDING_MAX_BYTES: u64 = 64 * 1024;

#[cfg(test)]
thread_local! {
    static TEST_LIVE_MANAGED_DEVICES:
        std::cell::RefCell<std::collections::VecDeque<Result<Vec<BoundDeviceIdentity>, String>>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

fn command_label(command: &str, args: &[&str]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{command} {}", args.join(" "))
    }
}

/// Runs a direct child with a bounded wait and captures its bounded output.
///
/// The child leads a private process group. Timeout and inherited-pipe cleanup
/// target exactly that group and prove the direct child reaped before return.
fn run_command_bounded_for(
    command: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, CascadeError> {
    run_command_bounded_for_with_spawn(command, args, timeout, |_| {})
}

fn run_command_bounded_for_with_spawn<F>(
    command: &str,
    args: &[&str],
    timeout: Duration,
    on_spawn: F,
) -> Result<String, CascadeError>
where
    F: FnOnce(u32),
{
    let label = command_label(command, args);
    let mut command = Command::new(command);
    command.args(args);
    let output = bounded_process::run_capture_command(
        &mut command,
        &label,
        timeout,
        COMMAND_OUTPUT_LIMIT,
        on_spawn,
    )
    .map_err(|error| CascadeError::Shell {
        cmd: label.clone(),
        msg: error.to_string(),
    })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let status = output
        .status
        .code()
        .map(|code| format!("exit {code}"))
        .unwrap_or_else(|| "terminated by signal".into());
    Err(CascadeError::Shell {
        cmd: label,
        msg: if stderr.is_empty() {
            status
        } else {
            format!("{status}: {stderr}")
        },
    })
}

fn run_command_bounded(command: &str, args: &[&str]) -> Result<String, CascadeError> {
    run_command_bounded_for(command, args, SHORT_COMMAND_TIMEOUT)
}

trait CommandRunner {
    fn run(&self, command: &str, args: &[&str]) -> Result<String, CascadeError>;
}

struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, command: &str, args: &[&str]) -> Result<String, CascadeError> {
        run_command_bounded(command, args)
    }
}

#[derive(Clone, Debug)]
struct RuntimePaths {
    runtime_dir: PathBuf,
    socket: PathBuf,
    zram_dev_file: PathBuf,
    swap_dev_file: PathBuf,
    lifecycle_binding_file: PathBuf,
    pid_file: PathBuf,
    capacity_status_file: PathBuf,
    cache_status_file: PathBuf,
    supervisor_status_file: PathBuf,
    cache_target_request_file: PathBuf,
    reclaim_request_file: PathBuf,
    forensics_markers: Vec<PathBuf>,
    #[cfg(test)]
    zram_sysfs: PathBuf,
}

impl RuntimePaths {
    fn system() -> Self {
        Self {
            runtime_dir: PathBuf::from("/run/ramshared"),
            socket: PathBuf::from(SOCK),
            zram_dev_file: PathBuf::from(ZRAM_DEV_FILE),
            swap_dev_file: PathBuf::from(SWAP_DEV_FILE),
            lifecycle_binding_file: PathBuf::from("/run/ramshared/lifecycle-binding.json"),
            pid_file: PathBuf::from(PID_FILE),
            capacity_status_file: PathBuf::from(CAPACITY_STATUS_FILE),
            cache_status_file: PathBuf::from(CACHE_STATUS_FILE),
            supervisor_status_file: PathBuf::from(SUPERVISOR_STATUS_FILE),
            cache_target_request_file: PathBuf::from("/run/ramshared/cache-target.json"),
            reclaim_request_file: PathBuf::from("/run/ramshared/reclaim-request.json"),
            forensics_markers: ARMED_MARKER_CANDIDATES.iter().map(PathBuf::from).collect(),
            #[cfg(test)]
            zram_sysfs: PathBuf::from("/sys/block/zram0"),
        }
    }

    #[cfg(test)]
    fn under(root: &Path) -> Self {
        let runtime_dir = root.join("runtime");
        let forensics = root.join("forensics");
        Self {
            socket: runtime_dir.join("wsl2d.sock"),
            zram_dev_file: runtime_dir.join("zram-dev"),
            swap_dev_file: runtime_dir.join("swap-dev"),
            lifecycle_binding_file: runtime_dir.join("lifecycle-binding.json"),
            pid_file: runtime_dir.join("ramsharedd.pid"),
            capacity_status_file: runtime_dir.join("capacity-guaranteed"),
            cache_status_file: runtime_dir.join("cache-status.json"),
            supervisor_status_file: runtime_dir.join("supervisor-state.json"),
            cache_target_request_file: runtime_dir.join("cache-target.json"),
            reclaim_request_file: runtime_dir.join("reclaim-request.json"),
            runtime_dir,
            forensics_markers: vec![forensics.join(".armed")],
            zram_sysfs: root.join("sys/block/zram0"),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ManagedDeviceKind {
    Nbd,
    Ublk,
    Zram,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct BoundDeviceIdentity {
    kind: ManagedDeviceKind,
    path: String,
    dev_t: String,
    sysfs_path: String,
    sysfs_dev_t: String,
    kernel_owner_instance_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BoundSocketIdentity {
    path: String,
    filesystem_dev: u64,
    inode: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BoundOriginIdentity {
    partuuid: String,
    ptuuid: String,
    partition_dev_t: String,
    parent_dev_t: String,
    expected_swap_uuid: String,
    host_manifest_sha256: String,
    configuration_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LifecycleBinding {
    schema_version: u32,
    boot_id: String,
    daemon_invocation_id: String,
    daemon_pid: u32,
    daemon_start_ticks: u64,
    daemon_instance_id: String,
    export_socket: BoundSocketIdentity,
    origin: BoundOriginIdentity,
    devices: Vec<BoundDeviceIdentity>,
}

fn canonical_boot_id(value: &str) -> bool {
    canonical_origin_uuid(value)
}

fn canonical_invocation_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(not(test))]
fn linux_device_number(dev: u64) -> String {
    let major = ((dev & 0x0000_0000_000f_ff00) >> 8) | ((dev & 0xffff_f000_0000_0000) >> 32);
    let minor = (dev & 0xff) | ((dev & 0x0000_0fff_fff0_0000) >> 12);
    format!("{major}:{minor}")
}

fn current_boot_id() -> Result<String, CascadeError> {
    #[cfg(test)]
    {
        Ok("11111111-2222-4333-8444-555555555555".into())
    }
    #[cfg(not(test))]
    {
        let value = fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map_err(|error| CascadeError::Io(format!("read boot identity: {error}")))?;
        let value = value.trim().to_ascii_lowercase();
        if !canonical_boot_id(&value) {
            return Err(CascadeError::Precondition(
                "kernel boot identity is not canonical".into(),
            ));
        }
        Ok(value)
    }
}

fn daemon_invocation_id(pid: u32) -> Result<String, CascadeError> {
    #[cfg(test)]
    {
        let _ = pid;
        Ok("0123456789abcdef0123456789abcdef".into())
    }
    #[cfg(not(test))]
    {
        let bytes = fs::read(format!("/proc/{pid}/environ")).map_err(|error| {
            CascadeError::Precondition(format!(
                "cannot read exact daemon systemd invocation identity: {error}"
            ))
        })?;
        let mut matches = bytes.split(|byte| *byte == 0).filter_map(|entry| {
            entry
                .strip_prefix(b"INVOCATION_ID=")
                .and_then(|value| std::str::from_utf8(value).ok())
        });
        let value = matches
            .next()
            .filter(|_| matches.next().is_none())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| {
                CascadeError::Precondition(
                    "daemon has no unique systemd InvocationID; direct unmanaged activation is refused"
                        .into(),
                )
            })?;
        if !canonical_invocation_id(&value) {
            return Err(CascadeError::Precondition(
                "daemon systemd InvocationID is not canonical".into(),
            ));
        }
        Ok(value)
    }
}

fn socket_identity(path: &Path) -> Result<BoundSocketIdentity, CascadeError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| CascadeError::Io(format!("stat export socket: {error}")))?;
    #[cfg(not(test))]
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(CascadeError::Precondition(
            "daemon export path is not an exact Unix socket".into(),
        ));
    }
    #[cfg(test)]
    if metadata.file_type().is_symlink() {
        return Err(CascadeError::Precondition(
            "daemon export path is a symlink".into(),
        ));
    }
    Ok(BoundSocketIdentity {
        path: path.to_string_lossy().into_owned(),
        filesystem_dev: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(test))]
fn parse_sysfs_device_number(text: &str) -> Result<String, CascadeError> {
    let value = text.trim();
    if !canonical_device_number(value) {
        return Err(CascadeError::Precondition(
            "managed device sysfs dev_t is invalid".into(),
        ));
    }
    Ok(value.to_string())
}

fn device_kind_for_path(path: &str) -> Option<ManagedDeviceKind> {
    if is_nbd_device_path(path) {
        Some(ManagedDeviceKind::Nbd)
    } else if is_ublk_device_path(path) {
        Some(ManagedDeviceKind::Ublk)
    } else if is_zram_device_path(path) {
        Some(ManagedDeviceKind::Zram)
    } else {
        None
    }
}

fn observe_bound_device(
    path: &str,
    expected_kind: ManagedDeviceKind,
) -> Result<BoundDeviceIdentity, CascadeError> {
    let path = canonicalize_swap_path(path);
    if device_kind_for_path(&path) != Some(expected_kind) {
        return Err(CascadeError::Precondition(format!(
            "managed device kind and path disagree: {path}"
        )));
    }
    #[cfg(test)]
    {
        let index = path
            .bytes()
            .rev()
            .take_while(u8::is_ascii_digit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(char::from)
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0);
        let (major, owner) = match expected_kind {
            ManagedDeviceKind::Nbd => (43, Some("4242-100".into())),
            ManagedDeviceKind::Ublk => (259, None),
            ManagedDeviceKind::Zram => (252, None),
        };
        Ok(BoundDeviceIdentity {
            kind: expected_kind,
            path: path.clone(),
            dev_t: format!("{major}:{index}"),
            sysfs_path: format!(
                "/sys/devices/virtual/block/{}",
                path.rsplit('/').next().unwrap_or_default()
            ),
            sysfs_dev_t: format!("{major}:{index}"),
            kernel_owner_instance_id: owner,
        })
    }
    #[cfg(not(test))]
    {
        let named = fs::symlink_metadata(&path).map_err(|error| {
            CascadeError::Precondition(format!("managed device {path} is unavailable: {error}"))
        })?;
        if named.file_type().is_symlink() || !named.file_type().is_block_device() {
            return Err(CascadeError::Precondition(format!(
                "managed device {path} is not an exact block-device node"
            )));
        }
        let dev_t = linux_device_number(named.rdev());
        let basename = Path::new(&path)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| CascadeError::Precondition("managed device name is invalid".into()))?;
        let sysfs =
            fs::canonicalize(Path::new("/sys/class/block").join(basename)).map_err(|error| {
                CascadeError::Precondition(format!("resolve managed sysfs identity: {error}"))
            })?;
        let sysfs_dev_t =
            parse_sysfs_device_number(&fs::read_to_string(sysfs.join("dev")).map_err(
                |error| CascadeError::Precondition(format!("read managed sysfs dev_t: {error}")),
            )?)?;
        if sysfs_dev_t != dev_t {
            return Err(CascadeError::Precondition(
                "managed device node and sysfs dev_t disagree".into(),
            ));
        }
        let kernel_owner_instance_id = if expected_kind == ManagedDeviceKind::Nbd {
            let pid = fs::read_to_string(sysfs.join("pid"))
                .map_err(|error| {
                    CascadeError::Precondition(format!("read NBD kernel owner PID: {error}"))
                })?
                .trim()
                .parse::<u32>()
                .map_err(|_| {
                    CascadeError::Precondition("NBD kernel owner PID is invalid".into())
                })?;
            if pid == 0 {
                return Err(CascadeError::Precondition(
                    "NBD kernel owner is absent".into(),
                ));
            }
            Some(daemon_instance_id_from_pid(pid).ok_or_else(|| {
                CascadeError::Precondition("NBD kernel owner start identity is unavailable".into())
            })?)
        } else {
            None
        };
        Ok(BoundDeviceIdentity {
            kind: expected_kind,
            path,
            dev_t,
            sysfs_path: sysfs.to_string_lossy().into_owned(),
            sysfs_dev_t,
            kernel_owner_instance_id,
        })
    }
}

fn detect_live_managed_devices() -> Result<Vec<BoundDeviceIdentity>, CascadeError> {
    #[cfg(test)]
    {
        if let Some(snapshot) =
            TEST_LIVE_MANAGED_DEVICES.with(|queue| queue.borrow_mut().pop_front())
        {
            return snapshot.map_err(CascadeError::Io);
        }
        Ok(Vec::new())
    }
    #[cfg(not(test))]
    {
        let mut devices = Vec::new();
        let entries = fs::read_dir("/sys/class/block")
            .map_err(|error| CascadeError::Io(format!("enumerate managed devices: {error}")))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                CascadeError::Io(format!("enumerate managed device entry: {error}"))
            })?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| CascadeError::Precondition("non-UTF-8 block-device name".into()))?;
            let path = format!("/dev/{name}");
            let Some(kind) = device_kind_for_path(&path) else {
                continue;
            };
            let live = match kind {
                ManagedDeviceKind::Nbd => match fs::read_to_string(entry.path().join("pid")) {
                    Ok(value) if value.trim().is_empty() => false,
                    Ok(value) => {
                        value.trim().parse::<u32>().map_err(|_| {
                            CascadeError::Precondition(format!("{name} owner PID is malformed"))
                        })? > 0
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => {
                        return Err(CascadeError::Precondition(format!(
                            "read {name} owner PID: {error}"
                        )));
                    }
                },
                ManagedDeviceKind::Zram => {
                    fs::read_to_string(entry.path().join("disksize"))
                        .map_err(|error| {
                            CascadeError::Precondition(format!("read {name} disksize: {error}"))
                        })?
                        .trim()
                        .parse::<u64>()
                        .map_err(|_| {
                            CascadeError::Precondition(format!("{name} disksize is malformed"))
                        })?
                        > 0
                }
                ManagedDeviceKind::Ublk => true,
            };
            if live {
                devices.push(observe_bound_device(&path, kind)?);
            }
        }
        devices.sort();
        Ok(devices)
    }
}

/// Captures the device set immediately before an ownership-changing command.
///
/// Unit tests inject only post-effect snapshots through the established seam,
/// so their pre-effect world is the empty set. Production always enumerates
/// the real kernel state here.
fn detect_live_managed_devices_before_effect() -> Result<Vec<BoundDeviceIdentity>, CascadeError> {
    #[cfg(test)]
    {
        Ok(Vec::new())
    }
    #[cfg(not(test))]
    {
        detect_live_managed_devices()
    }
}

fn sorted_device_set(devices: &[BoundDeviceIdentity]) -> Vec<BoundDeviceIdentity> {
    let mut devices = devices.to_vec();
    devices.sort();
    devices
}

fn exact_single_device_delta(
    before: &[BoundDeviceIdentity],
    after: &[BoundDeviceIdentity],
    kind: ManagedDeviceKind,
) -> Option<BoundDeviceIdentity> {
    let before = sorted_device_set(before);
    let after = sorted_device_set(after);
    if before.windows(2).any(|pair| pair[0] == pair[1])
        || after.windows(2).any(|pair| pair[0] == pair[1])
        || after.len() != before.len().checked_add(1)?
        || before.iter().any(|device| !after.contains(device))
    {
        return None;
    }
    let mut added = after
        .iter()
        .filter(|device| !before.contains(device))
        .cloned();
    let device = added.next()?;
    (added.next().is_none() && device.kind == kind).then_some(device)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DetachedNbdObservation {
    path: String,
    dev_t: String,
    sysfs_path: String,
    sysfs_dev_t: String,
}

/// Proves that one exact NBD target is detached at the kernel boundary.
///
/// A missing entry in the live-device enumeration is not enough: a timeout can
/// race an attach effect. The target node, sysfs dev_t, owner PID, exported
/// size, holders, and a fresh strict `/proc/swaps` snapshot must all agree.
fn observe_exact_detached_nbd(path: &str) -> Result<DetachedNbdObservation, CascadeError> {
    let path = canonicalize_swap_path(path);
    validate_nbd_swap_device(&path)?;
    let swaps = read_swaps()?;
    if swaps.iter().any(|entry| entry.canonical_path() == path) {
        return Err(CascadeError::UnsafeContainment(format!(
            "NBD target {path} is still active in a fresh strict /proc/swaps snapshot"
        )));
    }
    #[cfg(test)]
    {
        let observed = observe_bound_device(&path, ManagedDeviceKind::Nbd)?;
        Ok(DetachedNbdObservation {
            path,
            dev_t: observed.dev_t,
            sysfs_path: observed.sysfs_path,
            sysfs_dev_t: observed.sysfs_dev_t,
        })
    }
    #[cfg(not(test))]
    {
        let named = fs::symlink_metadata(&path).map_err(|error| {
            CascadeError::Precondition(format!("stat exact detached NBD target: {error}"))
        })?;
        if named.file_type().is_symlink() || !named.file_type().is_block_device() {
            return Err(CascadeError::Precondition(
                "detached NBD target is not an exact block-device node".into(),
            ));
        }
        let dev_t = linux_device_number(named.rdev());
        let basename = Path::new(&path)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| CascadeError::Precondition("detached NBD name is invalid".into()))?;
        let sysfs =
            fs::canonicalize(Path::new("/sys/class/block").join(basename)).map_err(|error| {
                CascadeError::Precondition(format!("resolve detached NBD sysfs identity: {error}"))
            })?;
        let sysfs_dev_t = parse_sysfs_device_number(
            &fs::read_to_string(sysfs.join("dev")).map_err(|error| {
                CascadeError::Precondition(format!("read detached NBD sysfs dev_t: {error}"))
            })?,
        )?;
        if sysfs_dev_t != dev_t {
            return Err(CascadeError::Precondition(
                "detached NBD node and sysfs dev_t disagree".into(),
            ));
        }
        match fs::read_to_string(sysfs.join("pid")) {
            Ok(value) => {
                let value = value.trim();
                if !value.is_empty()
                    && value.parse::<u32>().map_err(|_| {
                        CascadeError::Precondition("detached NBD owner PID is malformed".into())
                    })? != 0
                {
                    return Err(CascadeError::UnsafeContainment(format!(
                        "NBD target {path} still has a kernel owner PID"
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CascadeError::Precondition(format!(
                    "read detached NBD owner PID: {error}"
                )));
            }
        }
        let size = fs::read_to_string(sysfs.join("size"))
            .map_err(|error| {
                CascadeError::Precondition(format!("read detached NBD size: {error}"))
            })?
            .trim()
            .parse::<u64>()
            .map_err(|_| CascadeError::Precondition("detached NBD size is malformed".into()))?;
        if size != 0 {
            return Err(CascadeError::UnsafeContainment(format!(
                "NBD target {path} still exports {size} sectors"
            )));
        }
        let mut holders = fs::read_dir(sysfs.join("holders")).map_err(|error| {
            CascadeError::Precondition(format!("enumerate detached NBD holders: {error}"))
        })?;
        if holders
            .next()
            .transpose()
            .map_err(|error| {
                CascadeError::Precondition(format!("read detached NBD holder: {error}"))
            })?
            .is_some()
        {
            return Err(CascadeError::UnsafeContainment(format!(
                "NBD target {path} still has a kernel holder"
            )));
        }
        let named_after = fs::symlink_metadata(&path).map_err(|error| {
            CascadeError::Precondition(format!("restat exact detached NBD target: {error}"))
        })?;
        if named_after.file_type().is_symlink()
            || !named_after.file_type().is_block_device()
            || linux_device_number(named_after.rdev()) != dev_t
        {
            return Err(CascadeError::Precondition(
                "detached NBD target identity changed during absence proof".into(),
            ));
        }
        Ok(DetachedNbdObservation {
            path,
            dev_t,
            sysfs_path: sysfs.to_string_lossy().into_owned(),
            sysfs_dev_t,
        })
    }
}

/// Keeps an already-verified block-device file descriptor open across one
/// external effect. Tools that accept an arbitrary block path receive the
/// `/proc/<pid>/fd/<fd>` path, binding their open to this exact device object.
/// Tools whose own ABI derives sysfs names from `/dev/<name>` still receive the
/// canonical name, while this pin plus pre/post dev_t checks narrows (but cannot
/// eliminate) that external-tool/kernel boundary.
struct EffectBoundDevice {
    identity: BoundDeviceIdentity,
    effect_path: String,
    #[cfg(not(test))]
    file: fs::File,
}

impl EffectBoundDevice {
    fn path(&self) -> &str {
        &self.effect_path
    }

    fn revalidate_named_identity(&self) -> Result<(), CascadeError> {
        let observed = observe_bound_device(&self.identity.path, self.identity.kind)?;
        if observed != self.identity {
            return Err(CascadeError::Precondition(format!(
                "managed device identity changed across an external effect: {}",
                self.identity.path
            )));
        }
        #[cfg(not(test))]
        {
            let metadata = self.file.metadata().map_err(|error| {
                CascadeError::Precondition(format!(
                    "stat pinned managed device after external effect: {error}"
                ))
            })?;
            if linux_device_number(metadata.rdev()) != self.identity.dev_t {
                return Err(CascadeError::Precondition(
                    "pinned managed-device dev_t changed across an external effect".into(),
                ));
            }
        }
        Ok(())
    }
}

fn bind_device_for_effect(device: &BoundDeviceIdentity) -> Result<EffectBoundDevice, CascadeError> {
    revalidate_bound_device(device)?;
    #[cfg(test)]
    {
        Ok(EffectBoundDevice {
            identity: device.clone(),
            effect_path: device.path.clone(),
        })
    }
    #[cfg(not(test))]
    {
        let file = fs::File::open(&device.path).map_err(|error| {
            CascadeError::Precondition(format!(
                "open exact managed device for effect binding: {error}"
            ))
        })?;
        let metadata = file.metadata().map_err(|error| {
            CascadeError::Precondition(format!("stat exact managed device effect binding: {error}"))
        })?;
        if !metadata.file_type().is_block_device()
            || linux_device_number(metadata.rdev()) != device.dev_t
        {
            return Err(CascadeError::Precondition(
                "opened managed-device fd does not match the sealed dev_t".into(),
            ));
        }
        revalidate_bound_device(device)?;
        let effect_path = format!("/proc/{}/fd/{}", std::process::id(), file.as_raw_fd());
        let opened = fs::metadata(&effect_path).map_err(|error| {
            CascadeError::Precondition(format!("stat managed-device proc-fd effect path: {error}"))
        })?;
        if linux_device_number(opened.rdev()) != device.dev_t {
            return Err(CascadeError::Precondition(
                "managed-device proc-fd path does not preserve the sealed dev_t".into(),
            ));
        }
        Ok(EffectBoundDevice {
            identity: device.clone(),
            effect_path,
            file,
        })
    }
}

fn validate_lifecycle_binding(binding: &LifecycleBinding) -> Result<(), CascadeError> {
    if binding.schema_version != LIFECYCLE_BINDING_SCHEMA
        || !canonical_boot_id(&binding.boot_id)
        || !canonical_invocation_id(&binding.daemon_invocation_id)
        || binding.daemon_pid == 0
        || binding.daemon_start_ticks == 0
        || binding.daemon_instance_id
            != format!("{}-{}", binding.daemon_pid, binding.daemon_start_ticks)
        || binding.export_socket.path.is_empty()
        || binding.export_socket.inode == 0
        || !canonical_origin_uuid(&binding.origin.partuuid)
        || !canonical_origin_uuid(&binding.origin.ptuuid)
        || !canonical_origin_uuid(&binding.origin.expected_swap_uuid)
        || !canonical_device_number(&binding.origin.partition_dev_t)
        || !canonical_device_number(&binding.origin.parent_dev_t)
        || !canonical_sha256(&binding.origin.host_manifest_sha256)
        || !canonical_sha256(&binding.origin.configuration_sha256)
    {
        return Err(CascadeError::Precondition(
            "lifecycle binding has an invalid schema or identity".into(),
        ));
    }
    let nbd_count = binding
        .devices
        .iter()
        .filter(|device| device.kind == ManagedDeviceKind::Nbd)
        .count();
    let ublk_count = binding
        .devices
        .iter()
        .filter(|device| device.kind == ManagedDeviceKind::Ublk)
        .count();
    let zram_count = binding
        .devices
        .iter()
        .filter(|device| device.kind == ManagedDeviceKind::Zram)
        .count();
    let unique = binding
        .devices
        .iter()
        .map(|device| device.path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if nbd_count != 1
        || ublk_count != 0
        || zram_count > 1
        || unique.len() != binding.devices.len()
        || binding.devices.iter().any(|device| {
            device_kind_for_path(&device.path) != Some(device.kind)
                || device.dev_t != device.sysfs_dev_t
                || !canonical_device_number(&device.dev_t)
                || device.sysfs_path.is_empty()
        })
    {
        return Err(CascadeError::Precondition(
            "lifecycle binding device cardinality or identity is invalid".into(),
        ));
    }
    Ok(())
}

fn write_lifecycle_binding(
    paths: &RuntimePaths,
    binding: &LifecycleBinding,
) -> Result<(), CascadeError> {
    validate_lifecycle_binding(binding)?;
    let payload = serde_json::to_vec(binding)
        .map_err(|error| CascadeError::Io(format!("serialize lifecycle binding: {error}")))?;
    if payload.is_empty() || payload.len() as u64 > LIFECYCLE_BINDING_MAX_BYTES {
        return Err(CascadeError::Precondition(
            "lifecycle binding exceeds its bounded schema".into(),
        ));
    }
    let temporary = paths
        .runtime_dir
        .join(format!(".lifecycle-binding.{}.tmp", std::process::id()));
    remove_runtime_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| CascadeError::Io(format!("create lifecycle binding: {error}")))?;
    file.write_all(&payload)
        .and_then(|()| file.sync_all())
        .map_err(|error| CascadeError::Io(format!("persist lifecycle binding: {error}")))?;
    drop(file);
    fs::rename(&temporary, &paths.lifecycle_binding_file)
        .map_err(|error| CascadeError::Io(format!("publish lifecycle binding: {error}")))?;
    fs::File::open(&paths.runtime_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| CascadeError::Io(format!("sync lifecycle binding directory: {error}")))?;
    Ok(())
}

fn read_lifecycle_binding(paths: &RuntimePaths) -> Result<LifecycleBinding, CascadeError> {
    let named = fs::symlink_metadata(&paths.lifecycle_binding_file)
        .map_err(|error| CascadeError::Io(format!("stat lifecycle binding: {error}")))?;
    if named.file_type().is_symlink()
        || !named.file_type().is_file()
        || named.len() == 0
        || named.len() > LIFECYCLE_BINDING_MAX_BYTES
        || named.permissions().mode() & 0o077 != 0
    {
        return Err(CascadeError::Precondition(
            "lifecycle binding is not a sealed regular file".into(),
        ));
    }
    #[cfg(not(test))]
    if named.uid() != 0 {
        return Err(CascadeError::Precondition(
            "lifecycle binding is not root-owned".into(),
        ));
    }
    let mut file = fs::File::open(&paths.lifecycle_binding_file)
        .map_err(|error| CascadeError::Io(format!("open lifecycle binding: {error}")))?;
    let opened = file
        .metadata()
        .map_err(|error| CascadeError::Io(format!("stat opened lifecycle binding: {error}")))?;
    if opened.dev() != named.dev() || opened.ino() != named.ino() {
        return Err(CascadeError::Precondition(
            "lifecycle binding changed while opening".into(),
        ));
    }
    let mut payload = Vec::new();
    file.read_to_end(&mut payload)
        .map_err(|error| CascadeError::Io(format!("read lifecycle binding: {error}")))?;
    let binding = serde_json::from_slice::<LifecycleBinding>(&payload).map_err(|error| {
        CascadeError::Precondition(format!("invalid lifecycle binding: {error}"))
    })?;
    validate_lifecycle_binding(&binding)?;
    Ok(binding)
}

fn binding_origin_matches_current(binding: &LifecycleBinding) -> Result<(), CascadeError> {
    let current = read_sealed_origin_config()?;
    let expected = BoundOriginIdentity {
        partuuid: current.partuuid,
        ptuuid: current.ptuuid,
        partition_dev_t: current.partition_dev_t,
        parent_dev_t: current.parent_dev_t,
        expected_swap_uuid: current.expected_swap_uuid,
        host_manifest_sha256: current.host_manifest_sha256,
        configuration_sha256: current.configuration_sha256,
    };
    if binding.origin != expected {
        return Err(CascadeError::Precondition(
            "lifecycle binding no longer matches the sealed origin manifest".into(),
        ));
    }
    Ok(())
}

fn revalidate_daemon_and_socket(
    binding: &LifecycleBinding,
    paths: &RuntimePaths,
) -> Result<(), CascadeError> {
    if current_boot_id()? != binding.boot_id {
        return Err(CascadeError::Precondition(
            "kernel boot identity differs from the lifecycle binding".into(),
        ));
    }
    if daemon_instance_id_from_pid(binding.daemon_pid).as_deref()
        != Some(binding.daemon_instance_id.as_str())
    {
        return Err(CascadeError::Precondition(
            "daemon start identity differs from the lifecycle binding".into(),
        ));
    }
    if daemon_invocation_id(binding.daemon_pid)? != binding.daemon_invocation_id {
        return Err(CascadeError::Precondition(
            "daemon InvocationID differs from the lifecycle binding".into(),
        ));
    }
    if !cache_status_has_current_daemon_identity(paths, binding.daemon_pid) {
        return Err(CascadeError::Precondition(
            "daemon cache status differs from the lifecycle binding".into(),
        ));
    }
    if socket_identity(&paths.socket)? != binding.export_socket {
        return Err(CascadeError::Precondition(
            "daemon export socket differs from the lifecycle binding".into(),
        ));
    }
    binding_origin_matches_current(binding)
}

fn prove_exact_live_device_set(
    binding: &LifecycleBinding,
    live_devices: &[BoundDeviceIdentity],
    expected_live_devices: &[BoundDeviceIdentity],
) -> Result<(), CascadeError> {
    let mut expected = expected_live_devices.to_vec();
    expected.sort();
    if expected.windows(2).any(|pair| pair[0] == pair[1])
        || expected
            .iter()
            .any(|device| !binding.devices.contains(device))
    {
        return Err(CascadeError::Precondition(
            "expected live-device set is not an exact subset of the lifecycle binding".into(),
        ));
    }

    let mut observed = live_devices.to_vec();
    observed.sort();
    if observed != expected {
        let expected_paths = expected
            .iter()
            .map(|device| device.path.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let observed_paths = observed
            .iter()
            .map(|device| device.path.as_str())
            .collect::<Vec<_>>()
            .join(",");
        return Err(CascadeError::Precondition(format!(
            "live managed-device cardinality differs from the exact lifecycle stage (expected=[{expected_paths}], observed=[{observed_paths}]); no device was mutated"
        )));
    }
    Ok(())
}

fn authorize_bound_environment(
    binding: &LifecycleBinding,
    paths: &RuntimePaths,
    swaps: &[SwapEntry],
    live_devices: &[BoundDeviceIdentity],
    expected_live_devices: &[BoundDeviceIdentity],
) -> Result<(), CascadeError> {
    validate_lifecycle_binding(binding)?;
    revalidate_daemon_and_socket(binding, paths)?;
    let bound_by_path = binding
        .devices
        .iter()
        .map(|device| (device.path.as_str(), device))
        .collect::<std::collections::BTreeMap<_, _>>();
    if paths.swap_dev_file.exists() {
        let recorded = read_nbd_path_record(&paths.swap_dev_file)?;
        let expected = binding
            .devices
            .iter()
            .find(|device| device.kind == ManagedDeviceKind::Nbd)
            .ok_or_else(|| {
                CascadeError::Precondition(
                    "validated lifecycle binding omitted its exact NBD device".into(),
                )
            })?;
        if recorded != expected.path {
            return Err(CascadeError::Precondition(
                "NBD runtime record differs from the exact lifecycle binding; no device was mutated"
                    .into(),
            ));
        }
    }
    if paths.zram_dev_file.exists() {
        let recorded = read_bound_device_record(&paths.zram_dev_file)?;
        let Some(expected) = binding
            .devices
            .iter()
            .find(|device| device.kind == ManagedDeviceKind::Zram)
        else {
            return Err(CascadeError::Precondition(
                "unbound zram runtime record detected; no device was mutated".into(),
            ));
        };
        if &recorded != expected {
            return Err(CascadeError::Precondition(
                "zram runtime record differs from the exact lifecycle binding; no device was mutated"
                    .into(),
            ));
        }
    }
    for entry in swaps
        .iter()
        .filter(|entry| entry.is_managed_or_orphan_vram_tier())
    {
        let path = entry.canonical_path();
        if !bound_by_path.contains_key(path.as_str()) {
            return Err(CascadeError::Precondition(format!(
                "foreign or ambiguous managed swap detected at {path}; no device was mutated"
            )));
        }
    }
    let mut seen_live = std::collections::BTreeSet::new();
    for observed in live_devices {
        let Some(expected) = bound_by_path.get(observed.path.as_str()) else {
            return Err(CascadeError::Precondition(format!(
                "foreign live {:?} device detected at {}; no device was mutated",
                observed.kind, observed.path
            )));
        };
        if observed != *expected || !seen_live.insert(observed.path.as_str()) {
            return Err(CascadeError::Precondition(format!(
                "managed device identity is ambiguous at {}; no device was mutated",
                observed.path
            )));
        }
    }
    prove_exact_live_device_set(binding, live_devices, expected_live_devices)
}

fn prove_exact_swap_absent(device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
    let entries = read_swaps()?;
    if entries
        .iter()
        .any(|entry| entry.canonical_path() == device.path)
    {
        return Err(CascadeError::UnsafeContainment(format!(
            "{} remains active in a fresh strict /proc/swaps snapshot; backend preserved",
            device.path
        )));
    }
    Ok(())
}

fn revalidate_bound_device(device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
    let observed = observe_bound_device(&device.path, device.kind)?;
    if &observed != device {
        return Err(CascadeError::Precondition(format!(
            "managed device identity changed before mutation: {}",
            device.path
        )));
    }
    Ok(())
}

fn prove_only_exact_provisional_device_live(
    device: &BoundDeviceIdentity,
) -> Result<(), CascadeError> {
    let mut observed = detect_live_managed_devices()?;
    observed.sort();
    if observed != [device.clone()] {
        return Err(CascadeError::Precondition(format!(
            "provisional ownership for {} is not the only exact live managed device; no mutation was attempted",
            device.path
        )));
    }
    Ok(())
}

fn write_bound_device_record(
    path: &Path,
    device: &BoundDeviceIdentity,
) -> Result<(), CascadeError> {
    let parent = path
        .parent()
        .ok_or_else(|| CascadeError::Precondition("device record has no parent".into()))?;
    let payload = serde_json::to_vec(device)
        .map_err(|error| CascadeError::Io(format!("serialize device record: {error}")))?;
    let temporary = parent.join(format!(".device-record.{}.tmp", std::process::id()));
    remove_runtime_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| CascadeError::Io(format!("create device record: {error}")))?;
    file.write_all(&payload)
        .and_then(|()| file.sync_all())
        .map_err(|error| CascadeError::Io(format!("persist device record: {error}")))?;
    drop(file);
    fs::rename(&temporary, path)
        .map_err(|error| CascadeError::Io(format!("publish device record: {error}")))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| CascadeError::Io(format!("sync device record directory: {error}")))?;
    Ok(())
}

fn read_bound_device_record(path: &Path) -> Result<BoundDeviceIdentity, CascadeError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| CascadeError::Io(format!("stat device record: {error}")))?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_file()
        || metadata.len() == 0
        || metadata.len() > LIFECYCLE_BINDING_MAX_BYTES
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(CascadeError::Precondition(
            "managed device record is not a sealed regular file".into(),
        ));
    }
    #[cfg(not(test))]
    if metadata.uid() != 0 {
        return Err(CascadeError::Precondition(
            "managed device record is not root-owned".into(),
        ));
    }
    let mut file = fs::File::open(path)
        .map_err(|error| CascadeError::Io(format!("open managed device record: {error}")))?;
    let opened = file
        .metadata()
        .map_err(|error| CascadeError::Io(format!("stat opened device record: {error}")))?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(CascadeError::Precondition(
            "managed device record changed while opening".into(),
        ));
    }
    let mut payload = Vec::new();
    file.read_to_end(&mut payload)
        .map_err(|error| CascadeError::Io(format!("read managed device record: {error}")))?;
    let device = serde_json::from_slice::<BoundDeviceIdentity>(&payload).map_err(|error| {
        CascadeError::Precondition(format!("managed device record is invalid: {error}"))
    })?;
    if device_kind_for_path(&device.path) != Some(device.kind)
        || device.dev_t != device.sysfs_dev_t
        || !canonical_device_number(&device.dev_t)
    {
        return Err(CascadeError::Precondition(
            "managed device record identity is invalid".into(),
        ));
    }
    Ok(device)
}

fn write_nbd_path_record(path: &Path, device: &str) -> Result<(), CascadeError> {
    let canonical = canonicalize_swap_path(device);
    if canonical != device || device_kind_for_path(device) != Some(ManagedDeviceKind::Nbd) {
        return Err(CascadeError::Precondition(
            "NBD path record is not an exact managed device".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| CascadeError::Precondition("NBD path record has no parent".into()))?;
    let temporary = parent.join(format!(".nbd-path-record.{}.tmp", std::process::id()));
    remove_runtime_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| CascadeError::Io(format!("create NBD path record: {error}")))?;
    file.write_all(device.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| CascadeError::Io(format!("persist NBD path record: {error}")))?;
    drop(file);
    fs::rename(&temporary, path)
        .map_err(|error| CascadeError::Io(format!("publish NBD path record: {error}")))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| CascadeError::Io(format!("sync NBD path record directory: {error}")))?;
    Ok(())
}

fn read_nbd_path_record(path: &Path) -> Result<String, CascadeError> {
    let named = fs::symlink_metadata(path)
        .map_err(|error| CascadeError::Io(format!("stat NBD path record: {error}")))?;
    if named.file_type().is_symlink()
        || !named.file_type().is_file()
        || named.len() == 0
        || named.len() > 4096
    {
        return Err(CascadeError::Precondition(
            "NBD path record is not a bounded regular file".into(),
        ));
    }
    #[cfg(not(test))]
    if named.uid() != 0 || named.permissions().mode() & 0o077 != 0 {
        return Err(CascadeError::Precondition(
            "NBD path record is not sealed and root-owned".into(),
        ));
    }
    let mut file = fs::File::open(path)
        .map_err(|error| CascadeError::Io(format!("open NBD path record: {error}")))?;
    let opened = file
        .metadata()
        .map_err(|error| CascadeError::Io(format!("stat opened NBD path record: {error}")))?;
    if opened.dev() != named.dev() || opened.ino() != named.ino() {
        return Err(CascadeError::Precondition(
            "NBD path record changed while opening".into(),
        ));
    }
    let mut value = String::new();
    file.read_to_string(&mut value)
        .map_err(|error| CascadeError::Io(format!("read NBD path record: {error}")))?;
    if canonicalize_swap_path(&value) != value
        || device_kind_for_path(&value) != Some(ManagedDeviceKind::Nbd)
    {
        return Err(CascadeError::Precondition(
            "NBD path record is not an exact managed device".into(),
        ));
    }
    Ok(value)
}

fn parse_zram_device(output: &str) -> Option<String> {
    let device = output.trim();
    let suffix = device.strip_prefix("/dev/zram")?;
    (!suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| device.to_string())
}

fn arm_forensics_at(paths: &RuntimePaths) {
    let payload = format!(
        "armed_at={}\npid={}\nreason=ramshared-up\n",
        chrono_like_now(),
        std::process::id()
    );
    for path in &paths.forensics_markers {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                use std::io::Write;
                if file.write_all(payload.as_bytes()).is_ok() {
                    eprintln!("[up] forensics armed: {}", path.display());
                    return;
                }
                eprintln!(
                    "[warn] failed to write forensics marker: {}",
                    path.display()
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                match fs::symlink_metadata(path) {
                    Ok(metadata)
                        if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
                    {
                        eprintln!("[up] forensics marker already exists: {}", path.display());
                        return;
                    }
                    _ => eprintln!(
                        "[warn] refusing non-regular forensics marker: {}",
                        path.display()
                    ),
                }
            }
            Err(_) => {}
        }
    }
}

pub(crate) fn remove_runtime_file(path: impl AsRef<Path>) {
    let path = path.as_ref();
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!(
            "[warn] failed to remove runtime file {}: {e}",
            path.display()
        ),
    }
}

fn disarm_forensics_at(paths: &RuntimePaths) {
    for path in &paths.forensics_markers {
        remove_runtime_file(path);
    }
}

fn process_start_ticks(stat: &str) -> Option<&str> {
    stat.rsplit_once(") ")?.1.split_whitespace().nth(19)
}

fn daemon_instance_id_from_pid(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let start_ticks = process_start_ticks(&stat)?;
    (pid > 0 && !start_ticks.is_empty() && start_ticks.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| format!("{pid}-{start_ticks}"))
}

fn cache_status_has_current_daemon_identity_at(
    paths: &RuntimePaths,
    pid: u32,
    now_unix_ms: u64,
) -> bool {
    let Some(expected) = daemon_instance_id_from_pid(pid) else {
        return false;
    };
    let Ok(text) = fs::read_to_string(&paths.cache_status_file) else {
        return false;
    };
    let Ok(status) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    cache_status_matches_current_daemon(&status, &expected, now_unix_ms)
        && status.get("ok").and_then(serde_json::Value::as_bool) == Some(true)
        && status
            .get("origin_state")
            .and_then(serde_json::Value::as_str)
            == Some("READY")
        && matches!(
            status
                .get("cache_state")
                .and_then(serde_json::Value::as_str),
            Some("ACTIVE") | Some("RESTRICTED") | Some("UNAVAILABLE") | Some("OFF")
        )
        && [
            "logical_capacity_kib",
            "vram_cached_kib",
            "ssd_origin_written_kib",
            "cache_fallback_reads",
            "cache_invalidations",
            "cache_releases",
            "cache_target_kib",
        ]
        .iter()
        .all(|field| {
            status
                .get(*field)
                .and_then(serde_json::Value::as_u64)
                .is_some()
        })
        && status
            .get("gpu_headroom_kib")
            .is_some_and(|value| value.is_null() || value.as_u64().is_some())
}

fn cache_status_has_current_daemon_identity(paths: &RuntimePaths, pid: u32) -> bool {
    unix_time_ms().is_some_and(|now_unix_ms| {
        cache_status_has_current_daemon_identity_at(paths, pid, now_unix_ms)
    })
}

fn verified_daemon_pid(paths: &RuntimePaths) -> Option<u32> {
    let pid = fs::read_to_string(&paths.pid_file)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    daemon_pid_matches(pid, &comm).then_some(pid)
}

fn open_verified_daemon_pidfd(
    paths: &RuntimePaths,
    pid: u32,
    expected_instance_id: &str,
) -> Result<OwnedFd, CascadeError> {
    if daemon_instance_id_from_pid(pid).as_deref() != Some(expected_instance_id) {
        return Err(CascadeError::Precondition(
            "daemon instance changed before pidfd acquisition; refusing ambiguous signal".into(),
        ));
    }
    if !cache_status_has_current_daemon_identity(paths, pid) {
        return Err(CascadeError::Precondition(
            "daemon cache status changed before pidfd acquisition; refusing ambiguous signal"
                .into(),
        ));
    }
    let raw_pid = i32::try_from(pid).map_err(|_| {
        CascadeError::Precondition("daemon PID is outside the kernel pidfd range".into())
    })?;
    let pid = Pid::from_raw(raw_pid).ok_or_else(|| {
        CascadeError::Precondition("daemon PID cannot be bound to a pidfd".into())
    })?;
    let pidfd = pidfd_open(pid, PidfdFlags::empty()).map_err(|error| {
        CascadeError::Precondition(format!("open kernel-bound daemon pidfd: {error}"))
    })?;
    // Bind first, then prove the record still names that same bound process.
    // If it exited or the numeric PID was reused before this revalidation, the
    // evidence no longer authorizes a signal and the pidfd is dropped.
    if daemon_instance_id_from_pid(raw_pid as u32).as_deref() != Some(expected_instance_id)
        || !cache_status_has_current_daemon_identity(paths, raw_pid as u32)
    {
        return Err(CascadeError::Precondition(
            "daemon instance changed while acquiring pidfd; refusing ambiguous signal".into(),
        ));
    }
    Ok(pidfd)
}

fn signal_daemon_pidfd(pidfd: &OwnedFd) -> Result<(), CascadeError> {
    pidfd_send_signal(pidfd, Signal::TERM).map_err(|error| {
        CascadeError::Precondition(format!("kernel-bound pidfd TERM failed: {error}"))
    })
}

fn signal_verified_daemon(
    paths: &RuntimePaths,
    pid: u32,
    expected_instance_id: &str,
) -> Result<(), CascadeError> {
    let pidfd = open_verified_daemon_pidfd(paths, pid, expected_instance_id)?;
    signal_daemon_pidfd(&pidfd)
}

fn process_is_gone_or_zombie(pid: u32) -> bool {
    let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return true;
    };
    let Some(after_comm) = stat.rsplit_once(") ") else {
        return false;
    };
    after_comm.1.starts_with('Z')
}

trait SpawnedChildContainment {
    fn terminate_group_and_reap(&mut self) -> Result<(), CascadeError>;
}

impl SpawnedChildContainment for std::process::Child {
    fn terminate_group_and_reap(&mut self) -> Result<(), CascadeError> {
        bounded_process::terminate_group_and_reap(self, "spawned daemon child")
            .map_err(|error| CascadeError::UnsafeContainment(error.to_string()))
    }
}

fn terminate_spawned_child_with(
    child: &mut dyn SpawnedChildContainment,
) -> Result<(), CascadeError> {
    child.terminate_group_and_reap()
}

fn terminate_spawned_child(child: &mut std::process::Child) -> Result<(), CascadeError> {
    terminate_spawned_child_with(child)
}

fn stop_daemon_gracefully_at(paths: &RuntimePaths, timeout: Duration) -> Result<(), CascadeError> {
    if !paths.pid_file.exists() {
        return Ok(());
    }
    let Some(pid) = verified_daemon_pid(paths) else {
        // Never select a replacement process by name and never erase the
        // evidence when a claimed daemon identity cannot be verified.
        return Err(CascadeError::Precondition(
            "daemon stop refused: PID record does not identify the exact ramsharedd process; runtime evidence retained".into(),
        ));
    };
    if !cache_status_has_current_daemon_identity(paths, pid) {
        return Err(CascadeError::Precondition(
            "daemon stop refused: cache status does not identify the exact current ramsharedd process; runtime evidence retained".into(),
        ));
    }
    if !daemon_kill_allowed(&read_swaps()?) {
        return Err(CascadeError::Precondition(
            "daemon stop refused: nbd/ublk remains in /proc/swaps; runtime evidence retained"
                .into(),
        ));
    }
    let expected_instance_id = daemon_instance_id_from_pid(pid).ok_or_else(|| {
        CascadeError::Precondition(
            "daemon stop refused: PID identity changed before TERM; runtime evidence retained"
                .into(),
        )
    })?;
    signal_verified_daemon(paths, pid, &expected_instance_id)?;
    let deadline = Instant::now() + timeout;
    while !process_is_gone_or_zombie(pid) && Instant::now() < deadline {
        sleep(Duration::from_millis(100));
    }
    if !process_is_gone_or_zombie(pid) {
        return Err(CascadeError::Precondition(format!(
            "daemon {pid} did not exit within {} ms; no broad signal was sent and runtime evidence was retained",
            timeout.as_millis()
        )));
    }
    remove_runtime_file(&paths.pid_file);
    Ok(())
}

fn daemon_pid_matches(pid: u32, comm: &str) -> bool {
    pid > 0 && comm.trim() == "ramsharedd"
}

#[cfg(test)]
pub(crate) fn setup_zram(mb: u64, prio: i32) -> Result<String, CascadeError> {
    setup_zram_with(&SystemCommandRunner, &RuntimePaths::system(), mb, prio)
}

fn rollback_zram_tier<R: CommandRunner>(
    runner: &R,
    paths: &RuntimePaths,
    zram_device: &str,
    active: bool,
) -> bool {
    if zram_device.is_empty() {
        return true;
    }
    let identity = read_bound_device_record(&paths.zram_dev_file);
    let Ok(identity) = identity else {
        return false;
    };
    if identity.path != canonicalize_swap_path(zram_device) {
        return false;
    }
    if prove_only_exact_provisional_device_live(&identity).is_err()
        || revalidate_bound_device(&identity).is_err()
    {
        return false;
    }
    let pinned = match bind_device_for_effect(&identity) {
        Ok(pinned) => pinned,
        Err(_) => return false,
    };
    if active {
        let Ok(swaps) = read_swaps() else {
            return false;
        };
        if swaps
            .iter()
            .any(|entry| entry.canonical_path() == identity.path)
            && runner.run("swapoff", &["--", pinned.path()]).is_err()
        {
            return false;
        }
    }
    if prove_exact_swap_absent(&identity).is_err()
        || pinned.revalidate_named_identity().is_err()
        // zramctl requires the canonical device name to derive its sysfs node;
        // the exact fd remains open across this unavoidable tool boundary.
        || runner.run("zramctl", &["-r", zram_device]).is_err()
    {
        return false;
    }
    let Ok(live_devices) = detect_live_managed_devices() else {
        return false;
    };
    if live_devices
        .iter()
        .any(|observed| observed.path == identity.path)
    {
        return false;
    }
    remove_runtime_file(&paths.zram_dev_file);
    true
}

fn error_after_zram_rollback<R: CommandRunner>(
    runner: &R,
    paths: &RuntimePaths,
    zram_device: &str,
    may_be_active: bool,
    primary: CascadeError,
) -> CascadeError {
    if rollback_zram_tier(runner, paths, zram_device, may_be_active) {
        primary
    } else {
        CascadeError::UnsafeContainment(format!(
            "zram setup failed and exact rollback could not be proven ({primary}); device and ownership evidence were preserved for attended recovery"
        ))
    }
}

fn reconcile_malformed_zram_allocation<R: CommandRunner>(
    runner: &R,
    paths: &RuntimePaths,
    before: &[BoundDeviceIdentity],
    malformed_output: &str,
) -> Result<bool, CascadeError> {
    let after = detect_live_managed_devices().map_err(|error| {
        CascadeError::UnsafeContainment(format!(
            "zramctl returned malformed success output and post-allocation state is unreadable ({error}); no reset was attempted"
        ))
    })?;
    if sorted_device_set(&after) == sorted_device_set(before) {
        return Ok(false);
    }
    let Some(device) = exact_single_device_delta(before, &after, ManagedDeviceKind::Zram) else {
        return Err(CascadeError::UnsafeContainment(format!(
            "zramctl returned malformed success output ({malformed_output:?}) and the allocation delta is ambiguous; no device was reset"
        )));
    };
    if let Err(error) = write_bound_device_record(&paths.zram_dev_file, &device) {
        return Err(CascadeError::UnsafeContainment(format!(
            "new exact zram device {} was reconciled after malformed output but its ownership record could not be sealed ({error}); device preserved",
            device.path
        )));
    }
    let pinned = bind_device_for_effect(&device).map_err(|error| {
        CascadeError::UnsafeContainment(format!(
            "new exact zram device {} could not be fd/dev_t-bound before reset ({error}); ownership evidence preserved",
            device.path
        ))
    })?;
    prove_exact_swap_absent(&device).map_err(|error| {
        CascadeError::UnsafeContainment(format!(
            "new exact zram device {} is not proven inactive ({error}); ownership evidence preserved",
            device.path
        ))
    })?;
    // zramctl derives the sysfs target from the canonical device name and does
    // not accept the proc-fd path portably. Keep the exact fd open, revalidate
    // immediately before the call, and require disappearance afterward.
    pinned.revalidate_named_identity().map_err(|error| {
        CascadeError::UnsafeContainment(format!(
            "new exact zram device {} changed identity before reset ({error}); ownership evidence preserved",
            device.path
        ))
    })?;
    runner
        .run("zramctl", &["-r", &device.path])
        .map_err(|error| {
            CascadeError::UnsafeContainment(format!(
                "reset of reconciled zram device {} is unproven ({error}); ownership evidence preserved",
                device.path
            ))
        })?;
    let final_devices = detect_live_managed_devices().map_err(|error| {
        CascadeError::UnsafeContainment(format!(
            "reset completion for reconciled zram device {} is unreadable ({error}); ownership evidence preserved",
            device.path
        ))
    })?;
    if sorted_device_set(&final_devices) != sorted_device_set(before) {
        return Err(CascadeError::UnsafeContainment(format!(
            "managed-device cardinality did not return to the exact pre-allocation snapshot after resetting {}; ownership evidence preserved",
            device.path
        )));
    }
    remove_runtime_file(&paths.zram_dev_file);
    Ok(true)
}

fn setup_zram_with<R: CommandRunner>(
    runner: &R,
    paths: &RuntimePaths,
    mb: u64,
    prio: i32,
) -> Result<String, CascadeError> {
    if mb == 0 {
        eprintln!("[up] zram skipped (--zram 0)");
        return Ok(String::new());
    }
    let _ = runner.run("modprobe", &["zram"]);
    // Prefer free device via zramctl with algorithm fallbacks.
    let size = format!("{mb}M");
    let mut last_err = String::new();
    for algo in ZRAM_ALGOS {
        let before = detect_live_managed_devices_before_effect().map_err(|error| {
            CascadeError::Precondition(format!(
                "cannot enumerate managed devices before zram allocation: {error}"
            ))
        })?;
        match runner.run("zramctl", &["--find", "--size", &size, "--algorithm", algo]) {
            Ok(output) => {
                let Some(zdev) = parse_zram_device(&output) else {
                    last_err = format!("zramctl returned an unexpected device: {output}");
                    if reconcile_malformed_zram_allocation(runner, paths, &before, &output)? {
                        eprintln!(
                            "[up] reset the exact zram allocation reconciled from malformed allocator output"
                        );
                        return Err(CascadeError::Precondition(
                            "zram allocator protocol was malformed; the exact newly allocated device was reset, and setup stopped before another allocation"
                                .into(),
                        ));
                    }
                    continue;
                };
                let priority = prio.to_string();
                let after = detect_live_managed_devices().map_err(|error| {
                    CascadeError::UnsafeContainment(format!(
                        "zram allocator returned {zdev} but post-allocation state is unreadable ({error}); no formatting or activation was attempted"
                    ))
                })?;
                let Some(identity) =
                    exact_single_device_delta(&before, &after, ManagedDeviceKind::Zram)
                else {
                    return Err(CascadeError::UnsafeContainment(format!(
                        "zram allocator returned {zdev} but exact allocation cardinality could not be proven; no formatting or activation was attempted"
                    )));
                };
                if identity.path != canonicalize_swap_path(&zdev) {
                    return Err(CascadeError::UnsafeContainment(format!(
                        "zram allocator output {zdev} differs from the exact newly allocated device {}; no formatting or activation was attempted",
                        identity.path
                    )));
                }
                if let Err(error) = write_bound_device_record(&paths.zram_dev_file, &identity) {
                    return Err(CascadeError::UnsafeContainment(format!(
                        "zram identity could not be recorded before formatting ({error}); {zdev} was preserved without mkswap, swapon, or reset"
                    )));
                }
                let pinned = match bind_device_for_effect(&identity) {
                    Ok(pinned) => pinned,
                    Err(error) => {
                        return Err(error_after_zram_rollback(
                            runner, paths, &zdev, false, error,
                        ));
                    }
                };
                if let Err(error) = pinned.revalidate_named_identity() {
                    return Err(error_after_zram_rollback(
                        runner, paths, &zdev, false, error,
                    ));
                }
                if let Err(error) = runner.run("mkswap", &[pinned.path()]) {
                    return Err(error_after_zram_rollback(
                        runner, paths, &zdev, false, error,
                    ));
                }
                if let Err(error) = pinned.revalidate_named_identity() {
                    return Err(error_after_zram_rollback(
                        runner, paths, &zdev, false, error,
                    ));
                }
                if let Err(error) = runner.run("swapon", &["-p", &priority, pinned.path()]) {
                    return Err(CascadeError::UnsafeContainment(format!(
                        "swapon outcome for {zdev} is uncertain ({error}); exact zram device and ownership evidence were preserved"
                    )));
                }
                if let Err(error) = pinned.revalidate_named_identity() {
                    return Err(CascadeError::UnsafeContainment(format!(
                        "zram identity changed after swapon ({error}); exact device and ownership evidence were preserved"
                    )));
                }
                eprintln!("[up] zram {zdev} algo={algo} prio={prio}");
                return Ok(zdev);
            }
            Err(e) => {
                last_err = e.to_string();
                eprintln!("[up] zram algorithm {algo} failed: {last_err}");
            }
        }
    }
    Err(CascadeError::Precondition(format!(
        "zram is unavailable through ownership-creating zramctl --find ({last_err}); the unbound zram0 sysfs fallback is disabled to avoid mutating a foreign device. Try --zram 0 or repair zramctl/modprobe."
    )))
}

fn check_transport(transport: Transport) -> Result<(), CascadeError> {
    check_transport_for(transport, is_wsl2())
}

fn check_transport_for(transport: Transport, running_on_wsl2: bool) -> Result<(), CascadeError> {
    // cascade-transport-policy ITEM-3: ublk fail-closed before idempotent return (#16).
    // Auto already resolved to Nbd on WSL2; explicit ublk or Auto→Ublk (non-WSL2) still blocked
    // until full up wire-up exists (SPEC future + dedicated AUDIT-2.5 for teardown).
    if transport == Transport::Ublk {
        let msg = if running_on_wsl2 {
            "transport ublk refused on WSL2 (freeze risk 2026-06-09; Day-1 is nbd). \
             There is no safety override. The kernel may provide ublk_drv; the cascade product \
             does not use it."
        } else {
            "transport ublk in `ramshared up` is not implemented yet (future SPEC). \
             Use --transport nbd or auto. A manual ublk daemon is lab-only."
        };
        return Err(CascadeError::Precondition(msg.into()));
    }
    Ok(())
}

fn check_safety_net(vram_mb: u64, force: bool, prios: &TierPriorities) -> Result<(), CascadeError> {
    // A1 — DEMOTE safety net (requires a tier below VRAM).
    let vram_bytes = vram_mb
        .checked_mul(1024 * 1024)
        .ok_or_else(|| CascadeError::Arg("--vram: overflow (MiB value is too large)".into()))?;
    let net = vram_safety_net(lower_tier_present()?, mem_available_bytes(), vram_bytes);
    if !net.is_safe() && !force {
        return Err(CascadeError::Precondition(
            "no DEMOTE safety net (no VHDX and insufficient RAM); \
             use --force-no-safety-net only when intentional"
                .into(),
        ));
    }
    eprintln!("[up] A1 safety net: {net:?}");
    // Product order: zram, then the origin-backed logical device, then WSL fallback swap.
    eprintln!(
        "[up] priority: zram({}) > RamShared origin/cache({}) > WSL fallback swap; SSD origin is authoritative",
        prios.zram, prios.vram
    );
    Ok(())
}

fn build_daemon_command(
    daemon_path: &str,
    vram_mb: u64,
    cache_cap_mib: u64,
    socket: &str,
    swap_dev: &str,
    _origin_path: &str,
) -> Command {
    // Test fixtures are writable shell source files. Execute them through the
    // immutable system interpreter rather than directly: a parallel test can
    // otherwise race the kernel's writable-text exclusion (ETXTBSY) before
    // this production command builder is reached. Production always executes
    // the caller-selected daemon binary directly.
    #[cfg(test)]
    let mut command = {
        let mut command = Command::new("/bin/sh");
        command.arg(daemon_path);
        command
    };
    #[cfg(not(test))]
    let mut command = Command::new(daemon_path);
    command
        .args([
            "--size",
            &vram_mb.to_string(),
            "--sock",
            socket,
            "--nbd",
            swap_dev,
            "--origin-manifest",
            ORIGIN_CONFIG_FILE,
        ])
        .env("RAMSHARED_VRAM_CACHE_CAP_MIB", cache_cap_mib.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    bounded_process::configure_process_group(&mut command);
    command
}

fn spawn_daemon_with_deadline(
    daemon_path: &str,
    vram_mb: u64,
    cache_cap_mib: u64,
    swap_dev: &str,
    origin_path: &str,
    paths: &RuntimePaths,
    readiness_timeout: Duration,
) -> Result<std::process::Child, CascadeError> {
    fs::create_dir_all(&paths.runtime_dir).map_err(|error| CascadeError::Io(error.to_string()))?;
    remove_runtime_file(&paths.socket);
    remove_runtime_file(&paths.cache_status_file);
    remove_runtime_file(&paths.supervisor_status_file);
    remove_runtime_file(&paths.cache_target_request_file);
    remove_runtime_file(&paths.reclaim_request_file);
    let socket = paths.socket.to_string_lossy().into_owned();
    let mut child = build_daemon_command(
        daemon_path,
        vram_mb,
        cache_cap_mib,
        &socket,
        swap_dev,
        origin_path,
    )
    .spawn()
    .map_err(|error| CascadeError::Shell {
        cmd: daemon_path.to_string(),
        msg: error.to_string(),
    })?;
    if let Err(error) = fs::write(&paths.pid_file, child.id().to_string()) {
        terminate_spawned_child(&mut child)?;
        return Err(CascadeError::Io(error.to_string()));
    }
    let deadline = Instant::now() + readiness_timeout;
    while (!paths.socket.exists() || !cache_status_has_current_daemon_identity(paths, child.id()))
        && Instant::now() < deadline
    {
        sleep(Duration::from_millis(50));
    }
    if !paths.socket.exists() {
        // No NBD attach exists yet. Cleanup is limited to our direct child.
        terminate_spawned_child(&mut child)?;
        remove_runtime_file(&paths.pid_file);
        disarm_forensics_at(paths);
        return Err(CascadeError::Precondition(
            "daemon did not start (socket missing)".into(),
        ));
    }
    if !cache_status_has_current_daemon_identity(paths, child.id()) {
        // No NBD attach exists yet. A daemon without an exact current identity
        // cannot safely consume control-plane zero-cache requests.
        terminate_spawned_child(&mut child)?;
        remove_runtime_file(&paths.cache_status_file);
        remove_runtime_file(&paths.pid_file);
        disarm_forensics_at(paths);
        return Err(CascadeError::Precondition(
            "daemon did not publish a valid current cache identity".into(),
        ));
    }
    Ok(child)
}

fn validate_nbd_swap_device(swap_dev: &str) -> Result<(), CascadeError> {
    if is_nbd_device_path(swap_dev) && is_allowlisted_managed_path(swap_dev) {
        return Ok(());
    }
    Err(CascadeError::Precondition(format!(
        "NBD attach refused for non-managed device: {swap_dev}"
    )))
}

fn rollback_nbd_attach_with<R: CommandRunner, C: SpawnedChildContainment>(
    runner: &R,
    attached: bool,
    record_written: bool,
    swap_dev: &str,
    daemon: &mut C,
    paths: &RuntimePaths,
) -> Result<(), CascadeError> {
    if attached {
        let binding = read_lifecycle_binding(paths).map_err(|error| {
            CascadeError::UnsafeContainment(format!(
                "NBD detach refused because the exact lifecycle binding is unavailable ({error}); backend preserved"
            ))
        })?;
        let device = binding
            .devices
            .iter()
            .find(|device| {
                device.kind == ManagedDeviceKind::Nbd
                    && device.path == canonicalize_swap_path(swap_dev)
            })
            .ok_or_else(|| {
                CascadeError::UnsafeContainment(
                    "NBD detach refused because the attached device is not uniquely bound".into(),
                )
            })?;
        let swaps = read_swaps().map_err(|error| {
            CascadeError::UnsafeContainment(format!(
                "NBD detach refused because swap absence is unreadable ({error})"
            ))
        })?;
        authorize_bound_environment(
            &binding,
            paths,
            &swaps,
            &detect_live_managed_devices()?,
            &binding.devices,
        )?;
        prove_exact_swap_absent(device)?;
        let pinned = bind_device_for_effect(device)?;
        pinned.revalidate_named_identity()?;
        if let Err(error) = runner.run("nbd-client", &["-d", swap_dev]) {
            return Err(CascadeError::UnsafeContainment(format!(
                "NBD detach for {swap_dev} is unproven after startup failure ({error}); exact daemon, runtime records, and forensics retained"
            )));
        }
        let expected_after = binding
            .devices
            .iter()
            .filter(|expected| expected.kind != ManagedDeviceKind::Nbd)
            .cloned()
            .collect::<Vec<_>>();
        authorize_bound_environment(
            &binding,
            paths,
            &read_swaps()?,
            &detect_live_managed_devices()?,
            &expected_after,
        )
        .map_err(|error| {
            CascadeError::UnsafeContainment(format!(
                "NBD detach completion for {swap_dev} is unproven after startup failure ({error}); exact daemon, runtime records, and forensics retained"
            ))
        })?;
    }
    terminate_spawned_child_with(daemon)?;
    if record_written {
        remove_runtime_file(&paths.swap_dev_file);
    }
    remove_runtime_file(&paths.capacity_status_file);
    remove_runtime_file(&paths.cache_status_file);
    remove_runtime_file(&paths.supervisor_status_file);
    remove_runtime_file(&paths.cache_target_request_file);
    remove_runtime_file(&paths.reclaim_request_file);
    remove_runtime_file(&paths.pid_file);
    disarm_forensics_at(paths);
    Ok(())
}

fn rollback_nbd_attach<R: CommandRunner>(
    runner: &R,
    attached: bool,
    record_written: bool,
    swap_dev: &str,
    daemon: &mut std::process::Child,
    paths: &RuntimePaths,
) -> Result<(), CascadeError> {
    rollback_nbd_attach_with(runner, attached, record_written, swap_dev, daemon, paths)
}

fn error_after_nbd_rollback<R: CommandRunner>(
    runner: &R,
    attached: bool,
    record_written: bool,
    swap_dev: &str,
    daemon: &mut std::process::Child,
    paths: &RuntimePaths,
    primary: CascadeError,
) -> CascadeError {
    match rollback_nbd_attach(runner, attached, record_written, swap_dev, daemon, paths) {
        Ok(()) => primary,
        Err(containment) => containment,
    }
}

struct NbdAttachConfig<'a> {
    connections: u32,
    swap_dev: &'a str,
    vram_prio: i32,
    disk_baseline_kib: u64,
    logical_capacity_kib: u64,
    zram_device: &'a str,
    origin_partuuid: &'a str,
    origin_ptuuid: &'a str,
    origin_partition_dev_t: &'a str,
    origin_parent_dev_t: &'a str,
    expected_swap_uuid: &'a str,
    host_manifest_sha256: &'a str,
    configuration_sha256: &'a str,
}

fn capture_lifecycle_binding(
    paths: &RuntimePaths,
    daemon_pid: u32,
    swap_dev: &str,
    config: &NbdAttachConfig<'_>,
) -> Result<LifecycleBinding, CascadeError> {
    let daemon_instance_id = daemon_instance_id_from_pid(daemon_pid).ok_or_else(|| {
        CascadeError::Precondition("daemon PID/start identity is unavailable".into())
    })?;
    let daemon_start_ticks = daemon_instance_id
        .rsplit_once('-')
        .and_then(|(_, value)| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| CascadeError::Precondition("daemon start identity is invalid".into()))?;
    let mut devices = vec![observe_bound_device(swap_dev, ManagedDeviceKind::Nbd)?];
    if !config.zram_device.is_empty() {
        let zram = read_bound_device_record(&paths.zram_dev_file)?;
        if zram.path != canonicalize_swap_path(config.zram_device)
            || zram.kind != ManagedDeviceKind::Zram
        {
            return Err(CascadeError::Precondition(
                "zram record differs from the device created by this lifecycle".into(),
            ));
        }
        revalidate_bound_device(&zram)?;
        devices.push(zram);
    }
    devices.sort();
    let binding = LifecycleBinding {
        schema_version: LIFECYCLE_BINDING_SCHEMA,
        boot_id: current_boot_id()?,
        daemon_invocation_id: daemon_invocation_id(daemon_pid)?,
        daemon_pid,
        daemon_start_ticks,
        daemon_instance_id,
        export_socket: socket_identity(&paths.socket)?,
        origin: BoundOriginIdentity {
            partuuid: config.origin_partuuid.to_ascii_lowercase(),
            ptuuid: config.origin_ptuuid.to_ascii_lowercase(),
            partition_dev_t: config.origin_partition_dev_t.to_string(),
            parent_dev_t: config.origin_parent_dev_t.to_string(),
            expected_swap_uuid: config.expected_swap_uuid.to_ascii_lowercase(),
            host_manifest_sha256: config.host_manifest_sha256.to_ascii_lowercase(),
            configuration_sha256: config.configuration_sha256.to_ascii_lowercase(),
        },
        devices,
    };
    validate_lifecycle_binding(&binding)?;
    #[cfg(not(test))]
    authorize_bound_environment(
        &binding,
        paths,
        &read_swaps()?,
        &detect_live_managed_devices()?,
        &binding.devices,
    )?;
    Ok(binding)
}

fn verify_preprovisioned_swap<R: CommandRunner>(
    runner: &R,
    device: &BoundDeviceIdentity,
    expected_swap_uuid: &str,
) -> Result<EffectBoundDevice, CascadeError> {
    let pinned = bind_device_for_effect(device)?;
    let swap_type = runner.run("blkid", &["-s", "TYPE", "-o", "value", pinned.path()])?;
    pinned.revalidate_named_identity()?;
    let swap_uuid = runner.run("blkid", &["-s", "UUID", "-o", "value", pinned.path()])?;
    pinned.revalidate_named_identity()?;
    if swap_type.trim() != "swap"
        || !canonical_origin_uuid(swap_uuid.trim())
        || !swap_uuid.trim().eq_ignore_ascii_case(expected_swap_uuid)
    {
        return Err(CascadeError::Precondition(
            "NBD swap signature differs from the sealed provisioned UUID; normal up never formats it"
                .into(),
        ));
    }
    Ok(pinned)
}

fn reconcile_uncertain_nbd_attach<R: CommandRunner>(
    runner: &R,
    daemon: &mut std::process::Child,
    paths: &RuntimePaths,
    before: &[BoundDeviceIdentity],
    config: &NbdAttachConfig<'_>,
    primary: CascadeError,
) -> CascadeError {
    let first = match detect_live_managed_devices() {
        Ok(devices) => devices,
        Err(error) => {
            return CascadeError::UnsafeContainment(format!(
                "NBD attach outcome is uncertain ({primary}) and the first kernel-state reconciliation failed ({error}); exact backend and daemon preserved"
            ));
        }
    };
    #[cfg(not(test))]
    sleep(Duration::from_millis(50));
    let second = match detect_live_managed_devices() {
        Ok(devices) => devices,
        Err(error) => {
            return CascadeError::UnsafeContainment(format!(
                "NBD attach outcome is uncertain ({primary}) and the confirming kernel-state reconciliation failed ({error}); exact backend and daemon preserved"
            ));
        }
    };
    let before = sorted_device_set(before);
    let first_sorted = sorted_device_set(&first);
    let second_sorted = sorted_device_set(&second);
    if first_sorted == before && second_sorted == before {
        let first_absence = match observe_exact_detached_nbd(config.swap_dev) {
            Ok(observation) => observation,
            Err(error) => {
                return CascadeError::UnsafeContainment(format!(
                    "NBD attach outcome is uncertain ({primary}); live-device cardinality returned to baseline but exact target absence is unproven ({error}), so the backend and daemon were preserved"
                ));
            }
        };
        #[cfg(not(test))]
        sleep(Duration::from_millis(50));
        let second_absence = match observe_exact_detached_nbd(config.swap_dev) {
            Ok(observation) => observation,
            Err(error) => {
                return CascadeError::UnsafeContainment(format!(
                    "NBD attach outcome is uncertain ({primary}); confirming exact target absence failed ({error}), so the backend and daemon were preserved"
                ));
            }
        };
        if first_absence != second_absence {
            return CascadeError::UnsafeContainment(format!(
                "NBD attach outcome is uncertain ({primary}); detached target identity changed between absence proofs, so the backend and daemon were preserved"
            ));
        }
        return error_after_nbd_rollback(
            runner,
            false,
            false,
            config.swap_dev,
            daemon,
            paths,
            primary,
        );
    }
    let first_delta = exact_single_device_delta(&before, &first_sorted, ManagedDeviceKind::Nbd);
    let second_delta = exact_single_device_delta(&before, &second_sorted, ManagedDeviceKind::Nbd);
    if first_sorted == second_sorted
        && let (Some(first_device), Some(second_device)) = (first_delta, second_delta)
        && first_device == second_device
        && first_device.path == canonicalize_swap_path(config.swap_dev)
        && first_device.kernel_owner_instance_id.is_some()
    {
        let binding = match capture_lifecycle_binding(paths, daemon.id(), config.swap_dev, config) {
            Ok(binding) => binding,
            Err(error) => {
                return CascadeError::UnsafeContainment(format!(
                    "NBD attach took effect before command failure ({primary}), but the exact lifecycle binding could not be captured ({error}); backend and daemon preserved"
                ));
            }
        };
        if let Err(error) = write_lifecycle_binding(paths, &binding) {
            return CascadeError::UnsafeContainment(format!(
                "NBD attach took effect before command failure ({primary}), but its lifecycle binding could not be persisted ({error}); backend and daemon preserved"
            ));
        }
        if let Err(error) = write_nbd_path_record(&paths.swap_dev_file, config.swap_dev) {
            return CascadeError::UnsafeContainment(format!(
                "NBD attach took effect before command failure ({primary}), but its path record could not be persisted ({error}); lifecycle binding, backend, and daemon preserved"
            ));
        }
        return CascadeError::UnsafeContainment(format!(
            "NBD attach took effect before command failure ({primary}); exact path/dev_t/owner/cardinality evidence was sealed and the backend and daemon were intentionally preserved"
        ));
    }
    CascadeError::UnsafeContainment(format!(
        "NBD attach outcome is ambiguous after command failure ({primary}); two fresh kernel snapshots did not prove stable absence or one exact owned device, so the backend and daemon were preserved"
    ))
}

fn connect_nbd_with<R: CommandRunner>(
    runner: &R,
    daemon: &mut std::process::Child,
    paths: &RuntimePaths,
    config: NbdAttachConfig<'_>,
) -> Result<(), CascadeError> {
    let NbdAttachConfig {
        connections,
        swap_dev,
        vram_prio,
        disk_baseline_kib,
        logical_capacity_kib,
        zram_device,
        origin_partuuid,
        origin_ptuuid,
        origin_partition_dev_t,
        origin_parent_dev_t,
        expected_swap_uuid,
        host_manifest_sha256,
        configuration_sha256,
    } = config;
    if let Err(error) = validate_nbd_swap_device(swap_dev) {
        return Err(error_after_nbd_rollback(
            runner, false, false, swap_dev, daemon, paths, error,
        ));
    }
    if connections == 0 {
        let error = CascadeError::Arg("--connections must be at least 1".into());
        return Err(error_after_nbd_rollback(
            runner, false, false, swap_dev, daemon, paths, error,
        ));
    }
    let conns = connections.to_string();
    let socket = paths.socket.to_string_lossy().into_owned();
    let before_attach = match detect_live_managed_devices_before_effect() {
        Ok(devices) => devices,
        Err(error) => {
            return Err(error_after_nbd_rollback(
                runner, false, false, swap_dev, daemon, paths, error,
            ));
        }
    };
    let mut nbd_args: Vec<&str> = Vec::new();
    if connections > 1 {
        nbd_args.extend(["-C", conns.as_str()]);
    }
    nbd_args.extend(["-unix", socket.as_str(), swap_dev]);
    let binding_config = NbdAttachConfig {
        connections,
        swap_dev,
        vram_prio,
        disk_baseline_kib,
        logical_capacity_kib,
        zram_device,
        origin_partuuid,
        origin_ptuuid,
        origin_partition_dev_t,
        origin_parent_dev_t,
        expected_swap_uuid,
        host_manifest_sha256,
        configuration_sha256,
    };
    if let Err(error) = runner.run("nbd-client", &nbd_args) {
        return Err(reconcile_uncertain_nbd_attach(
            runner,
            daemon,
            paths,
            &before_attach,
            &binding_config,
            error,
        ));
    }
    let binding = capture_lifecycle_binding(paths, daemon.id(), swap_dev, &binding_config)
        .map_err(|error| {
            CascadeError::UnsafeContainment(format!(
                "NBD attached but exact lifecycle binding could not be sealed ({error}); backend and daemon preserved"
            ))
        })?;
    write_lifecycle_binding(paths, &binding).map_err(|error| {
        CascadeError::UnsafeContainment(format!(
            "NBD attached but lifecycle binding could not be persisted ({error}); backend and daemon preserved"
        ))
    })?;
    let nbd_identity = binding
        .devices
        .iter()
        .find(|device| device.kind == ManagedDeviceKind::Nbd && device.path == swap_dev)
        .cloned()
        .ok_or_else(|| {
            CascadeError::UnsafeContainment(
                "sealed lifecycle does not contain the exact attached NBD device; backend and daemon preserved"
                    .into(),
            )
        })?;
    let pinned = match verify_preprovisioned_swap(runner, &nbd_identity, expected_swap_uuid) {
        Ok(pinned) => pinned,
        Err(error) => {
            return Err(error_after_nbd_rollback(
                runner, true, false, swap_dev, daemon, paths, error,
            ));
        }
    };
    if let Err(error) = fs::write(
        &paths.capacity_status_file,
        format!(
            "schema_version=4\nmode=origin-cache\nlogical_capacity_kib={}\n\
             vram_cached_kib=0\nssd_origin_written_kib=0\nfallback_swap_used_kib=0\n\
             origin_partuuid={}\ndisk_baseline_kib={}\n",
            logical_capacity_kib, origin_partuuid, disk_baseline_kib
        ),
    ) {
        return Err(error_after_nbd_rollback(
            runner,
            true,
            false,
            swap_dev,
            daemon,
            paths,
            CascadeError::Io(error.to_string()),
        ));
    }
    if let Err(error) = write_nbd_path_record(&paths.swap_dev_file, swap_dev) {
        return Err(error_after_nbd_rollback(
            runner, true, true, swap_dev, daemon, paths, error,
        ));
    }
    let priority = vram_prio.to_string();
    if let Err(error) = pinned.revalidate_named_identity() {
        return Err(CascadeError::UnsafeContainment(format!(
            "NBD identity changed before swapon ({error}); exact backend, daemon, lifecycle binding, and runtime records were preserved"
        )));
    }
    if let Err(error) = runner.run("swapon", &["-p", &priority, pinned.path()]) {
        return Err(CascadeError::UnsafeContainment(format!(
            "swapon outcome for {swap_dev} is uncertain ({error}); exact NBD backend, daemon, lifecycle binding, and runtime records were preserved"
        )));
    }
    if let Err(error) = pinned.revalidate_named_identity() {
        return Err(CascadeError::UnsafeContainment(format!(
            "NBD identity changed after swapon ({error}); exact backend, daemon, lifecycle binding, and runtime records were preserved"
        )));
    }
    Ok(())
}

/// Starts the cascade from argv already separated by the top-level CLI dispatcher.
/// Argument parsing is completed before this function can touch swap, the daemon,
/// or `/run/ramshared` (DT-6).
pub fn up_with_args(args: &[String]) -> Result<(), CascadeError> {
    up_with_config(parse_up_args_from(args, default_daemon())?)
}

fn setup_new_cascade<R: CommandRunner>(
    runner: &R,
    paths: &RuntimePaths,
    args: &UpArgs,
    prios: &TierPriorities,
) -> Result<std::process::Child, CascadeError> {
    let partuuid = origin_partuuid(&args.origin_path)?;
    if !partuuid.eq_ignore_ascii_case(&args.origin_partuuid) {
        return Err(CascadeError::Precondition(
            "sealed origin path and PARTUUID identity disagree".into(),
        ));
    }
    fs::create_dir_all(&paths.runtime_dir).map_err(|error| CascadeError::Io(error.to_string()))?;
    arm_forensics_at(paths);

    // zram tier (HOT). --zram 0 skips.
    let zram_device = match setup_zram_with(runner, paths, args.zram_mb, prios.zram) {
        Ok(device) => device,
        Err(error) => {
            disarm_forensics_at(paths);
            return Err(error);
        }
    };

    // Origin-backed tier with a clean, revocable VRAM cache: daemon + NBD.
    let daemon = (|| {
        validate_nbd_swap_device(&args.swap_dev)?;
        runner.run("modprobe", &["nbd", "nbds_max=1", "max_part=0"])?;
        let mut daemon = spawn_daemon_with_deadline(
            &args.daemon,
            args.vram_mb,
            args.cache_cap_mib,
            &args.swap_dev,
            &args.origin_path,
            paths,
            Duration::from_secs(6),
        )?;
        connect_nbd_with(
            runner,
            &mut daemon,
            paths,
            NbdAttachConfig {
                connections: args.connections,
                swap_dev: &args.swap_dev,
                vram_prio: prios.vram,
                disk_baseline_kib: args.disk_baseline_kib,
                logical_capacity_kib: args.vram_mb.saturating_mul(1024),
                zram_device: &zram_device,
                origin_partuuid: partuuid,
                origin_ptuuid: &args.origin_ptuuid,
                origin_partition_dev_t: &args.origin_partition_dev_t,
                origin_parent_dev_t: &args.origin_parent_dev_t,
                expected_swap_uuid: &args.expected_swap_uuid,
                host_manifest_sha256: &args.host_manifest_sha256,
                configuration_sha256: &args.configuration_sha256,
            },
        )?;
        Ok(daemon)
    })();
    let daemon = match daemon {
        Ok(daemon) => daemon,
        Err(error) => {
            if matches!(error, CascadeError::UnsafeContainment(_)) {
                return Err(error);
            }
            let zram_rolled_back =
                rollback_zram_tier(runner, paths, &zram_device, !zram_device.is_empty());
            remove_runtime_file(&paths.socket);
            remove_runtime_file(&paths.pid_file);
            if zram_rolled_back {
                remove_runtime_file(&paths.lifecycle_binding_file);
                disarm_forensics_at(paths);
            } else {
                arm_forensics_at(paths);
                eprintln!(
                    "[warn] zram rollback is incomplete; retained its runtime record and marker for manual recovery"
                );
            }
            return Err(error);
        }
    };
    eprintln!(
        "[up] RamShared logical device {} (prio {}, {} MiB, {} connection(s))",
        args.swap_dev, prios.vram, args.vram_mb, args.connections
    );
    eprintln!(
        "[up] cascade active: zram({}) > SSD-authoritative origin with VRAM cache({}) > WSL fallback | down always swapoff before daemon stop",
        prios.zram, prios.vram
    );
    Ok(daemon)
}

fn up_with_config(mut a: UpArgs) -> Result<(), CascadeError> {
    let prios = TierPriorities::default();
    let runner = SystemCommandRunner;
    let paths = RuntimePaths::system();
    validate_order(prios).map_err(|e| CascadeError::Precondition(e.to_string()))?;

    check_transport(a.transport)?;
    origin_partuuid(&a.origin_path)?;
    validate_origin_logical_capacity(a.vram_mb)?;

    // Ghosts: detecção apenas; nunca autorizam mutation (#16).
    refuse_ghost_swap_state()?;

    // SPEC wsl2-cascade-boot ITEM-5: idempotent if already healthy.
    let entries_now = read_swaps()?;
    if cascade_already_healthy(&entries_now) {
        eprintln!("[up] cascade already active — no action (idempotent)");
        return status(false);
    }

    // Unbound enumeration is detection-only, including active swap with used_kb == 0.
    refuse_unbound_managed_devices()?;

    let entries_after = read_swaps()?;
    if cascade_already_healthy(&entries_after) {
        eprintln!("[up] cascade already active after fresh snapshot — no-op");
        return status(false);
    }
    refuse_half_cascade(&entries_after)?;

    check_safety_net(a.vram_mb, a.force, &prios)?;
    a.disk_baseline_kib = disk_swap_used_kib(&entries_after);
    let _daemon = setup_new_cascade(&runner, &paths, &a, &prios)?;
    status(false)
}

/// One local cascade-down step. The plan is pure: it contains no process,
/// filesystem, daemon, or device operation by itself.
#[derive(Clone, Debug, Eq, PartialEq)]
enum NbdLifecycleAction {
    Swapoff(BoundDeviceIdentity),
    ResetZram(BoundDeviceIdentity),
    DisconnectNbd(BoundDeviceIdentity),
    StopDaemon,
}

/// Ordered, injected teardown contract for managed swap and NBD ownership.
///
/// Every swapoff action precedes every NBD disconnect and daemon stop. The
/// executor stops at the first swapoff error, so later ownership-changing
/// actions cannot run after a failed swapoff.
#[derive(Clone, Debug, Eq, PartialEq)]
struct NbdLifecyclePlan {
    actions: Vec<NbdLifecycleAction>,
}

/// Constructs the local teardown sequence from already-authorized paths.
///
/// Callers provide the swap, zram-reset, and NBD targets separately so the
/// ordering contract is testable without querying a live host.
fn plan_nbd_lifecycle(
    binding: &LifecycleBinding,
    swaps: &[SwapEntry],
    live_devices: &[BoundDeviceIdentity],
) -> Result<NbdLifecyclePlan, CascadeError> {
    validate_lifecycle_binding(binding)?;
    prove_exact_live_device_set(binding, live_devices, &binding.devices)?;
    let mut actions = Vec::new();
    for device in &binding.devices {
        if swaps
            .iter()
            .any(|entry| entry.canonical_path() == device.path)
        {
            actions.push(NbdLifecycleAction::Swapoff(device.clone()));
        }
    }
    for kind in [ManagedDeviceKind::Zram, ManagedDeviceKind::Nbd] {
        for device in binding.devices.iter().filter(|device| device.kind == kind) {
            match kind {
                ManagedDeviceKind::Zram => {
                    actions.push(NbdLifecycleAction::ResetZram(device.clone()))
                }
                ManagedDeviceKind::Nbd => {
                    actions.push(NbdLifecycleAction::DisconnectNbd(device.clone()))
                }
                ManagedDeviceKind::Ublk => unreachable!("validated NBD binding excludes ublk"),
            }
        }
    }
    actions.push(NbdLifecycleAction::StopDaemon);
    Ok(NbdLifecyclePlan { actions })
}

/// Narrow side-effect boundary for the pure NBD lifecycle plan.
trait NbdLifecycleExecutor {
    fn swapoff(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError>;
    fn reset_zram(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError>;
    fn disconnect_nbd(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError>;
    fn stop_daemon(&self) -> Result<(), CascadeError>;
}

/// Runs an injected plan. A swapoff failure is terminal for this invocation:
/// it cannot fall through to NBD disconnect or daemon stop.
fn execute_nbd_lifecycle_plan<E: NbdLifecycleExecutor>(
    plan: &NbdLifecyclePlan,
    executor: &E,
) -> Result<(), CascadeError> {
    for action in &plan.actions {
        match action {
            NbdLifecycleAction::Swapoff(device) => executor.swapoff(device)?,
            NbdLifecycleAction::ResetZram(device) => executor.reset_zram(device)?,
            NbdLifecycleAction::DisconnectNbd(device) => executor.disconnect_nbd(device)?,
            NbdLifecycleAction::StopDaemon => executor.stop_daemon()?,
        }
    }
    Ok(())
}

struct RuntimeNbdLifecycleExecutor<'a, R> {
    runner: &'a R,
    paths: &'a RuntimePaths,
    binding: &'a LifecycleBinding,
}

impl<R: CommandRunner> NbdLifecycleExecutor for RuntimeNbdLifecycleExecutor<'_, R> {
    fn swapoff(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
        let swaps = read_swaps()?;
        authorize_bound_environment(
            self.binding,
            self.paths,
            &swaps,
            &detect_live_managed_devices()?,
            &self.binding.devices,
        )?;
        let pinned = bind_device_for_effect(device)?;
        let result = self.runner.run("swapoff", &["--", pinned.path()]);
        match result {
            Ok(_) => {
                prove_exact_swap_absent(device)?;
                pinned.revalidate_named_identity()?;
                eprintln!("[down] swapoff ok: {}", device.path);
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                let absent =
                    message.contains("No such file") || message.contains("Invalid argument");
                if absent && prove_exact_swap_absent(device).is_ok() {
                    eprintln!(
                        "[down] swapoff skip (freshly proven absent): {}",
                        device.path
                    );
                    Ok(())
                } else {
                    Err(CascadeError::UnsafeContainment(format!(
                        "swapoff for {} is uncertain or still active ({error}); backend preserved",
                        device.path
                    )))
                }
            }
        }
    }

    fn reset_zram(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
        let swaps = read_swaps()?;
        authorize_bound_environment(
            self.binding,
            self.paths,
            &swaps,
            &detect_live_managed_devices()?,
            &self.binding.devices,
        )?;
        prove_exact_swap_absent(device)?;
        let pinned = bind_device_for_effect(device)?;
        pinned.revalidate_named_identity()?;
        // zramctl does not portably accept a proc-fd target because it derives
        // the sysfs name from /dev/<name>. The fd pin stays live across the
        // call and exact disappearance is mandatory afterward.
        self.runner.run("zramctl", &["-r", &device.path])?;
        let expected_after = self
            .binding
            .devices
            .iter()
            .filter(|expected| expected.kind != ManagedDeviceKind::Zram)
            .cloned()
            .collect::<Vec<_>>();
        authorize_bound_environment(
            self.binding,
            self.paths,
            &read_swaps()?,
            &detect_live_managed_devices()?,
            &expected_after,
        )
        .map_err(|error| {
            CascadeError::UnsafeContainment(format!(
                "zram reset completion is unproven for {}; lifecycle evidence retained ({error})",
                device.path
            ))
        })?;
        Ok(())
    }

    fn disconnect_nbd(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
        let swaps = read_swaps()?;
        let expected_before = self
            .binding
            .devices
            .iter()
            .filter(|expected| expected.kind == ManagedDeviceKind::Nbd)
            .cloned()
            .collect::<Vec<_>>();
        authorize_bound_environment(
            self.binding,
            self.paths,
            &swaps,
            &detect_live_managed_devices()?,
            &expected_before,
        )?;
        prove_exact_swap_absent(device)?;
        let pinned = bind_device_for_effect(device)?;
        pinned.revalidate_named_identity()?;
        // nbd-client likewise derives NBD sysfs state from the canonical name;
        // retain the fd/dev_t pin and prove exact disappearance after the call.
        self.runner.run("nbd-client", &["-d", &device.path])?;
        authorize_bound_environment(
            self.binding,
            self.paths,
            &read_swaps()?,
            &detect_live_managed_devices()?,
            &[],
        )
        .map_err(|error| {
            CascadeError::UnsafeContainment(format!(
                "NBD disconnect completion is unproven for {}; lifecycle evidence retained ({error})",
                device.path
            ))
        })?;
        Ok(())
    }

    fn stop_daemon(&self) -> Result<(), CascadeError> {
        let swaps = read_swaps()?;
        authorize_bound_environment(
            self.binding,
            self.paths,
            &swaps,
            &detect_live_managed_devices()?,
            &[],
        )?;
        stop_daemon_gracefully_at(self.paths, Duration::from_secs(10))
    }
}

fn down_with_runtime<R: CommandRunner>(
    runner: &R,
    paths: &RuntimePaths,
) -> Result<(), CascadeError> {
    let current_entries = read_swaps()?;
    let has_runtime_evidence = paths.pid_file.exists()
        || paths.swap_dev_file.exists()
        || paths.zram_dev_file.exists()
        || paths.socket.exists();
    let has_managed_swap = current_entries
        .iter()
        .any(SwapEntry::is_managed_or_orphan_vram_tier);
    if !paths.lifecycle_binding_file.exists() {
        if !has_runtime_evidence && !has_managed_swap {
            eprintln!("[down] no sealed lifecycle is present — no action");
            return Ok(());
        }
        return Err(CascadeError::Precondition(
            "runtime or managed-device evidence exists without an exact lifecycle binding; all devices are preserved"
                .into(),
        ));
    }
    let binding = read_lifecycle_binding(paths)?;
    let live_devices = detect_live_managed_devices()?;
    authorize_bound_environment(
        &binding,
        paths,
        &current_entries,
        &live_devices,
        &binding.devices,
    )?;
    let plan = plan_nbd_lifecycle(&binding, &current_entries, &live_devices)?;
    let executor = RuntimeNbdLifecycleExecutor {
        runner,
        paths,
        binding: &binding,
    };

    // ALWAYS swapoff before NBD disconnect/daemon stop. A swapoff error returns
    // here, leaving runtime records, daemon, and NBD device untouched.
    execute_nbd_lifecycle_plan(&plan, &executor)?;

    remove_runtime_file(&paths.socket);
    remove_runtime_file(&paths.zram_dev_file);
    remove_runtime_file(&paths.swap_dev_file);
    remove_runtime_file(&paths.capacity_status_file);
    remove_runtime_file(&paths.cache_status_file);
    remove_runtime_file(&paths.supervisor_status_file);
    remove_runtime_file(&paths.cache_target_request_file);
    remove_runtime_file(&paths.reclaim_request_file);
    remove_runtime_file(&paths.pid_file);
    remove_runtime_file(&paths.lifecycle_binding_file);
    disarm_forensics_at(paths);
    eprintln!("[down] cascade unmounted (swapoff-first, no broad kill)");
    Ok(())
}

pub fn down() -> Result<(), CascadeError> {
    let runner = SystemCommandRunner;
    let paths = RuntimePaths::system();
    down_with_runtime(&runner, &paths)?;
    status(false)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn nbd_attach_fixture() -> NbdAttachConfig<'static> {
        NbdAttachConfig {
            connections: 1,
            swap_dev: "/dev/nbd0",
            vram_prio: 100,
            disk_baseline_kib: 0,
            logical_capacity_kib: 64 * 1024,
            zram_device: "",
            origin_partuuid: "11111111-2222-4333-8444-555555555555",
            origin_ptuuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
            origin_partition_dev_t: "8:33",
            origin_parent_dev_t: "8:32",
            expected_swap_uuid: "99999999-8888-4777-8666-555555555555",
            host_manifest_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            configuration_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        }
    }

    fn up_args_fixture() -> UpArgs {
        UpArgs {
            vram_mb: 64,
            zram_mb: 64,
            daemon: "ramsharedd".into(),
            force: false,
            connections: 1,
            transport: Transport::Nbd,
            swap_dev: "/dev/nbd0".into(),
            origin_path: "/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555".into(),
            origin_partuuid: "11111111-2222-4333-8444-555555555555".into(),
            origin_ptuuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
            origin_partition_dev_t: "8:33".into(),
            origin_parent_dev_t: "8:32".into(),
            expected_swap_uuid: "99999999-8888-4777-8666-555555555555".into(),
            host_manifest_sha256:
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            configuration_sha256:
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            cache_cap_mib: 64,
            disk_baseline_kib: 0,
        }
    }

    fn bound_device_fixture(path: &str, kind: ManagedDeviceKind) -> BoundDeviceIdentity {
        observe_bound_device(path, kind).expect("deterministic device identity fixture")
    }

    fn lifecycle_binding_fixture(devices: Vec<BoundDeviceIdentity>) -> LifecycleBinding {
        LifecycleBinding {
            schema_version: LIFECYCLE_BINDING_SCHEMA,
            boot_id: "11111111-2222-4333-8444-555555555555".into(),
            daemon_invocation_id: "0123456789abcdef0123456789abcdef".into(),
            daemon_pid: 4242,
            daemon_start_ticks: 100,
            daemon_instance_id: "4242-100".into(),
            export_socket: BoundSocketIdentity {
                path: "/run/ramshared/wsl2d.sock".into(),
                filesystem_dev: 1,
                inode: 1,
            },
            origin: BoundOriginIdentity {
                partuuid: "11111111-2222-4333-8444-555555555555".into(),
                ptuuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
                partition_dev_t: "8:33".into(),
                parent_dev_t: "8:32".into(),
                expected_swap_uuid: "99999999-8888-4777-8666-555555555555".into(),
                host_manifest_sha256:
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                configuration_sha256:
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            },
            devices,
        }
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let parent = std::env::temp_dir();
            for _ in 0..1024 {
                let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
                let path = parent.join(format!(
                    "ramshared-cascade-io-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self { path },
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        panic!("create isolated cascade test directory {path:?}: {error}")
                    }
                }
            }
            panic!("exhausted isolated cascade fixture names");
        }

        fn program(&self, name: &str, source: &str) -> PathBuf {
            let path = self.path.join(name);
            let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let staged = self.path.join(format!(".{name}.{sequence}.staged"));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged)
                .unwrap_or_else(|error| panic!("create staged test program {staged:?}: {error}"));
            use std::io::Write;
            file.write_all(source.as_bytes())
                .unwrap_or_else(|error| panic!("write staged test program {staged:?}: {error}"));
            file.sync_all()
                .unwrap_or_else(|error| panic!("sync staged test program {staged:?}: {error}"));
            drop(file);
            let mut permissions = fs::metadata(&staged)
                .unwrap_or_else(|error| panic!("stat staged test program {staged:?}: {error}"))
                .permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&staged, permissions)
                .unwrap_or_else(|error| panic!("chmod staged test program {staged:?}: {error}"));
            fs::rename(&staged, &path)
                .unwrap_or_else(|error| panic!("publish test program {path:?}: {error}"));
            path
        }

        fn hold_writable_program(&self, name: &str, source: &str) -> fs::File {
            let path = self.program(name, source);
            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap_or_else(|error| panic!("open prior test program {path:?}: {error}"))
        }

        fn daemon_program(&self) -> PathBuf {
            let path = self.path.join("ramsharedd");
            let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let staged = self.path.join(format!(".ramsharedd.{sequence}.staged"));
            std::os::unix::fs::symlink("/bin/sleep", &staged)
                .unwrap_or_else(|error| panic!("stage native daemon fixture {staged:?}: {error}"));
            fs::rename(&staged, &path)
                .unwrap_or_else(|error| panic!("publish native daemon fixture {path:?}: {error}"));
            path
        }
    }

    #[test]
    // TestName: cascade_fixture_roots_are_unique_and_avoid_shared_disk_sync
    fn cascade_fixture_roots_are_unique_and_avoid_shared_disk_sync() {
        let first = TestDir::new();
        let second = TestDir::new();
        assert_ne!(first.path, second.path);
        let expected_parent = std::env::temp_dir();
        assert_eq!(first.path.parent(), Some(expected_parent.as_path()));
        assert_eq!(second.path.parent(), Some(expected_parent.as_path()));
    }

    #[test]
    fn command_and_identity_adapters_cover_legitimate_and_refusal_paths() {
        assert_eq!(command_label("/bin/true", &[]), "/bin/true");
        assert_eq!(
            run_command_bounded("/bin/true", &[])
                .expect("the bounded empty-argument child must run"),
            ""
        );
        assert_eq!(
            SystemCommandRunner
                .run("/bin/sh", &["-c", "printf adapter-ok"])
                .expect("the system adapter must preserve legitimate output"),
            "adapter-ok"
        );

        assert_eq!(
            device_kind_for_path("/dev/ublkb7"),
            Some(ManagedDeviceKind::Ublk)
        );
        assert_eq!(device_kind_for_path("/dev/not-managed"), None);
        let ublk = observe_bound_device("/dev/ublkb7", ManagedDeviceKind::Ublk)
            .expect("the deterministic ublk identity fixture must be accepted");
        assert_eq!(ublk.dev_t, "259:7");
        let error = observe_bound_device("/dev/nbd0", ManagedDeviceKind::Ublk)
            .expect_err("a path/kind mismatch must fail before observation");
        assert!(error.to_string().contains("kind and path disagree"));

        let fixture = TestDir::new();
        let target = fixture.path.join("socket-target");
        let link = fixture.path.join("socket-link");
        fs::write(&target, "not a socket").expect("write symlink target fixture");
        std::os::unix::fs::symlink(&target, &link).expect("create socket symlink fixture");
        let error = socket_identity(&link).expect_err("an export symlink must be refused");
        assert!(error.to_string().contains("symlink"));
    }

    #[test]
    fn zram_rollback_wrapper_preserves_primary_only_after_exact_cleanup() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir).expect("create rollback fixture runtime");
        let identity = bound_device_fixture("/dev/zram7", ManagedDeviceKind::Zram);
        write_bound_device_record(&paths.zram_dev_file, &identity)
            .expect("seal rollback fixture identity");
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_live_device_snapshots([Ok(vec![identity]), Ok(Vec::new())]);
        let runner = ScriptedRunner::new(vec![("zramctl -r /dev/zram7".into(), Ok(String::new()))]);

        let error = error_after_zram_rollback(
            &runner,
            &paths,
            "/dev/zram7",
            false,
            CascadeError::Precondition("primary fixture failure".into()),
        );
        assert!(error.to_string().contains("primary fixture failure"));
        assert!(!paths.zram_dev_file.exists());
        assert_eq!(runner.calls(), vec!["zramctl -r /dev/zram7"]);

        assert!(rollback_zram_tier(&runner, &paths, "", false));
    }

    fn run_isolated_zram_setup_fixture() -> Result<(), String> {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .map_err(|error| format!("create isolated zram runtime: {error}"))?;
        let runner = ScriptedRunner::new(vec![
            ("modprobe zram".into(), Ok(String::new())),
            (
                "zramctl --find --size 64M --algorithm lzo-rle".into(),
                Ok("/dev/zram7".into()),
            ),
            ("mkswap /dev/zram7".into(), Ok(String::new())),
            ("swapon -p 200 /dev/zram7".into(), Ok(String::new())),
        ]);
        set_live_device_snapshots([Ok(vec![bound_device_fixture(
            "/dev/zram7",
            ManagedDeviceKind::Zram,
        )])]);
        let device = setup_zram_with(&runner, &paths, 64, 200)
            .map_err(|error| format!("set up isolated zram fixture: {error}"))?;
        if device != "/dev/zram7" {
            return Err(format!("isolated zram fixture selected {device}"));
        }
        let recorded = read_bound_device_record(&paths.zram_dev_file)
            .map_err(|error| format!("read isolated zram record: {error}"))?;
        if recorded.path != "/dev/zram7" || recorded.kind != ManagedDeviceKind::Zram {
            return Err(format!("isolated zram record was {recorded:?}"));
        }
        let expected_calls = vec![
            "modprobe zram".to_string(),
            "zramctl --find --size 64M --algorithm lzo-rle".to_string(),
            "mkswap /dev/zram7".to_string(),
            "swapon -p 200 /dev/zram7".to_string(),
        ];
        if runner.calls() != expected_calls {
            return Err("isolated zram fixture invoked the wrong command order".to_string());
        }
        Ok(())
    }

    #[test]
    // TestName: cascade_parallel_zram_fixtures_stay_adapter_scoped_and_isolated
    fn cascade_parallel_zram_fixtures_stay_adapter_scoped_and_isolated() {
        let results = std::thread::scope(|scope| {
            let workers = (0..8)
                .map(|_| scope.spawn(run_isolated_zram_setup_fixture))
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| match worker.join() {
                    Ok(result) => result,
                    Err(_) => Err("isolated zram fixture worker panicked".to_string()),
                })
                .collect::<Vec<_>>()
        });
        for (index, result) in results.into_iter().enumerate() {
            if let Err(error) = result {
                panic!("isolated zram fixture {index} failed: {error}");
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn as_program(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn run_fixture_script_bounded(
        script: &Path,
        timeout: Duration,
    ) -> Result<String, CascadeError> {
        let script = as_program(script);
        run_command_bounded_for("/bin/sh", &[script.as_str()], timeout)
    }

    fn error_from<T>(result: Result<T, CascadeError>, expectation: &str) -> CascadeError {
        match result {
            Ok(_) => panic!("{expectation}"),
            Err(error) => error,
        }
    }

    fn fixture_process_is_stopped(pid: u32) -> bool {
        fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .is_some_and(|stat| {
                stat.rsplit_once(") ")
                    .is_some_and(|(_, tail)| tail.starts_with('T') || tail.starts_with('t'))
            })
    }

    struct FixtureDaemon(std::process::Child);

    impl FixtureDaemon {
        fn id(&self) -> u32 {
            self.0.id()
        }
    }

    impl Drop for FixtureDaemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn spawn_fixture_daemon(fixture: &TestDir, term_handler: &str) -> FixtureDaemon {
        let daemon_path = fixture.daemon_program();
        let mut child = Command::new(&daemon_path)
            .arg("60")
            .spawn()
            .unwrap_or_else(|error| panic!("start fixture daemon: {error}"));
        let deadline = Instant::now() + Duration::from_millis(250);
        while child
            .try_wait()
            .unwrap_or_else(|error| panic!("inspect fixture daemon startup: {error}"))
            .is_none()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        if child
            .try_wait()
            .unwrap_or_else(|error| panic!("inspect fixture daemon readiness: {error}"))
            .is_some()
        {
            let _ = child.kill();
            let _ = child.wait();
            panic!("fixture daemon exited before its startup deadline");
        }
        match term_handler {
            "exit 0" => {}
            "" => {
                let raw_pid = i32::try_from(child.id())
                    .unwrap_or_else(|_| panic!("fixture daemon PID is outside pidfd range"));
                let pid = Pid::from_raw(raw_pid)
                    .unwrap_or_else(|| panic!("fixture daemon PID cannot bind a pidfd"));
                let pidfd = pidfd_open(pid, PidfdFlags::empty())
                    .unwrap_or_else(|error| panic!("open fixture daemon pidfd: {error}"));
                pidfd_send_signal(&pidfd, Signal::STOP).unwrap_or_else(|error| {
                    panic!("pause exact fixture daemon with pidfd: {error}")
                });
                let stopped_deadline = Instant::now() + Duration::from_millis(250);
                while !fixture_process_is_stopped(child.id()) && Instant::now() < stopped_deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                if !fixture_process_is_stopped(child.id()) {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("fixture daemon did not enter its pidfd-bound stopped state");
                }
            }
            handler => panic!("unsupported fixture TERM handler {handler:?}"),
        }
        FixtureDaemon(child)
    }

    fn write_daemon_identity_status(paths: &RuntimePaths, pid: u32, instance_id: &str) {
        fs::write(
            &paths.cache_status_file,
            format!(
                r#"{{"schema_version":1,"daemon_instance_id":"{instance_id}","pid":{pid},"written_at_unix_ms":{},"ok":true,"origin_state":"READY","cache_state":"ACTIVE","logical_capacity_kib":1024,"vram_cached_kib":0,"gpu_headroom_kib":null,"ssd_origin_written_kib":1024,"cache_fallback_reads":0,"cache_invalidations":0,"cache_releases":0,"cache_target_kib":0}}"#,
                unix_time_ms().unwrap_or_default()
            ),
        )
        .unwrap_or_else(|error| panic!("write cache identity status: {error}"));
    }

    fn seal_runtime_lifecycle(
        paths: &RuntimePaths,
        daemon_pid: u32,
        include_zram: bool,
    ) -> Vec<BoundDeviceIdentity> {
        fs::create_dir_all(&paths.runtime_dir).expect("create lifecycle fixture runtime");
        fs::write(&paths.socket, "fixture export socket").expect("write export socket fixture");
        fs::write(&paths.pid_file, daemon_pid.to_string()).expect("write daemon PID fixture");
        let daemon_instance_id = daemon_instance_id_from_pid(daemon_pid)
            .expect("derive lifecycle fixture daemon identity");
        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        write_nbd_path_record(&paths.swap_dev_file, "/dev/nbd0").expect("write NBD path record");

        let mut devices = vec![nbd];
        if include_zram {
            let zram = bound_device_fixture("/dev/zram0", ManagedDeviceKind::Zram);
            write_bound_device_record(&paths.zram_dev_file, &zram)
                .expect("write zram identity record");
            devices.push(zram);
        } else {
            remove_runtime_file(&paths.zram_dev_file);
        }
        devices.sort();

        let mut binding = lifecycle_binding_fixture(devices.clone());
        let start_ticks = daemon_instance_id
            .rsplit_once('-')
            .and_then(|(_, value)| value.parse::<u64>().ok())
            .expect("parse lifecycle fixture daemon start time");
        binding.daemon_pid = daemon_pid;
        binding.daemon_start_ticks = start_ticks;
        binding.daemon_instance_id = daemon_instance_id;
        binding.export_socket =
            socket_identity(&paths.socket).expect("capture export socket fixture");
        write_lifecycle_binding(paths, &binding).expect("seal lifecycle fixture");
        // Production refreshes this status continuously. Publish the one-shot
        // fixture only after all potentially slow durable setup so a loaded
        // coverage run cannot manufacture stale evidence before validation.
        write_daemon_identity_status(paths, daemon_pid, &binding.daemon_instance_id);
        devices
    }

    fn prepare_connect_identity(paths: &RuntimePaths, daemon_pid: u32) {
        fs::create_dir_all(&paths.runtime_dir).expect("create connect fixture runtime");
        fs::write(&paths.socket, "fixture export socket").expect("write connect socket fixture");
        let instance_id = daemon_instance_id_from_pid(daemon_pid)
            .expect("derive connect fixture daemon identity");
        write_daemon_identity_status(paths, daemon_pid, &instance_id);
    }

    fn set_swap_snapshots(snapshots: impl IntoIterator<Item = Result<&'static str, &'static str>>) {
        let snapshots = snapshots
            .into_iter()
            .map(|snapshot| snapshot.map(str::to_string).map_err(str::to_string))
            .collect::<VecDeque<_>>();
        if let Some(Ok(last)) = snapshots.back() {
            TEST_SWAPS.with(|cell| *cell.borrow_mut() = Some(last.clone()));
        }
        TEST_SWAPS_SEQUENCE.with(|queue| {
            *queue.borrow_mut() = snapshots;
        });
    }

    fn set_live_device_snapshots(
        snapshots: impl IntoIterator<Item = Result<Vec<BoundDeviceIdentity>, &'static str>>,
    ) {
        TEST_LIVE_MANAGED_DEVICES.with(|queue| {
            *queue.borrow_mut() = snapshots
                .into_iter()
                .map(|snapshot| snapshot.map_err(str::to_string))
                .collect();
        });
    }

    fn install_two_tier_down_snapshots(
        devices: &[BoundDeviceIdentity],
        active_nbd: bool,
        active_zram: bool,
    ) {
        const EMPTY: &str = "Filename Type Size Used Priority\n";
        const NBD: &str = "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n";
        const ZRAM: &str = "Filename Type Size Used Priority\n/dev/zram0 partition 1024 0 200\n";
        const BOTH: &str = "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n/dev/zram0 partition 1024 0 200\n";

        let initial = match (active_nbd, active_zram) {
            (true, true) => BOTH,
            (true, false) => NBD,
            (false, true) => ZRAM,
            (false, false) => EMPTY,
        };
        let mut swaps = vec![Ok(initial)];
        if active_nbd {
            swaps.push(Ok(initial));
            swaps.push(Ok(if active_zram { ZRAM } else { EMPTY }));
        }
        if active_zram {
            swaps.push(Ok(ZRAM));
            swaps.push(Ok(EMPTY));
        }
        swaps.extend(std::iter::repeat_n(Ok(EMPTY), 8));
        set_swap_snapshots(swaps);

        let all = devices.to_vec();
        let nbd = devices
            .iter()
            .filter(|device| device.kind == ManagedDeviceKind::Nbd)
            .cloned()
            .collect::<Vec<_>>();
        let mut live = vec![Ok(all.clone())];
        if active_nbd {
            live.push(Ok(all.clone()));
        }
        if active_zram {
            live.push(Ok(all.clone()));
        }
        live.extend([
            Ok(all),
            Ok(nbd.clone()),
            Ok(nbd),
            Ok(Vec::new()),
            Ok(Vec::new()),
        ]);
        set_live_device_snapshots(live);
    }

    struct ScriptedRunner {
        responses: RefCell<VecDeque<(String, Result<String, CascadeError>)>>,
        calls: RefCell<Vec<String>>,
    }

    impl ScriptedRunner {
        fn new(responses: Vec<(String, Result<String, CascadeError>)>) -> Self {
            Self {
                responses: RefCell::new(responses.into()),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for ScriptedRunner {
        fn run(&self, command: &str, args: &[&str]) -> Result<String, CascadeError> {
            let label = command_label(command, args);
            self.calls.borrow_mut().push(label.clone());
            let Some((expected, response)) = self.responses.borrow_mut().pop_front() else {
                return Err(CascadeError::Shell {
                    cmd: label,
                    msg: "unexpected test command".into(),
                });
            };
            assert_eq!(expected, label, "test command order");
            response
        }
    }

    struct ParentSeams;

    impl ParentSeams {
        fn install(swaps: &str, shell_responses: usize) -> Self {
            TEST_SWAPS.with(|cell| *cell.borrow_mut() = Some(swaps.to_string()));
            TEST_SWAPS_SEQUENCE.with(|queue| queue.borrow_mut().clear());
            TEST_SWAPS_ERROR.with(|cell| *cell.borrow_mut() = None);
            TEST_LIVE_MANAGED_DEVICES.with(|queue| queue.borrow_mut().clear());
            SH_SCRIPT.with(|queue| {
                let mut queue = queue.borrow_mut();
                queue.clear();
                for _ in 0..shell_responses {
                    queue.push_back(("*".into(), Ok(String::new())));
                }
            });
            Self
        }
    }

    impl Drop for ParentSeams {
        fn drop(&mut self) {
            TEST_SWAPS.with(|cell| *cell.borrow_mut() = None);
            TEST_SWAPS_SEQUENCE.with(|queue| queue.borrow_mut().clear());
            TEST_SWAPS_ERROR.with(|cell| *cell.borrow_mut() = None);
            TEST_MEM_AVAILABLE.with(|cell| *cell.borrow_mut() = None);
            SH_SCRIPT.with(|queue| queue.borrow_mut().clear());
            TEST_LIVE_MANAGED_DEVICES.with(|queue| queue.borrow_mut().clear());
        }
    }

    fn run_timeout_teardown_fixture_with_held_predecessor() -> Result<(), String> {
        let fixture = TestDir::new();
        let _held_predecessor = fixture.hold_writable_program("ramsharedd", "#!/bin/sh\nexit 99\n");
        let daemon = spawn_fixture_daemon(&fixture, "");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .map_err(|error| format!("create isolated runtime: {error}"))?;
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), true);
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        install_two_tier_down_snapshots(&devices, false, false);
        let runner = ScriptedRunner::new(vec![
            ("zramctl -r /dev/zram0".into(), Ok(String::new())),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
        ]);

        let error = down_with_runtime(&runner, &paths)
            .err()
            .ok_or_else(|| "timeout fixture unexpectedly reported cleanup success".to_string())?;
        if !error.to_string().contains("did not exit") {
            return Err(format!("timeout fixture returned the wrong error: {error}"));
        }
        if !paths.pid_file.exists() || !paths.cache_status_file.exists() {
            return Err("timeout fixture removed forensic runtime evidence".to_string());
        }
        let expected_calls = vec![
            "zramctl -r /dev/zram0".to_string(),
            "nbd-client -d /dev/nbd0".to_string(),
        ];
        if runner.calls() != expected_calls {
            return Err(
                "timeout fixture did not retain the swapoff-first teardown order".to_string(),
            );
        }
        Ok(())
    }

    #[test]
    // TestName: cascade_parallel_timeout_teardown_publishes_immutable_fixture_and_retains_evidence
    fn cascade_parallel_timeout_teardown_publishes_immutable_fixture_and_retains_evidence() {
        let results = std::thread::scope(|scope| {
            let workers = (0..2)
                .map(|_| scope.spawn(run_timeout_teardown_fixture_with_held_predecessor))
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| match worker.join() {
                    Ok(result) => result,
                    Err(_) => Err("timeout fixture worker panicked".to_string()),
                })
                .collect::<Vec<_>>()
        });
        for (index, result) in results.into_iter().enumerate() {
            if let Err(error) = result {
                panic!("parallel timeout fixture {index} failed: {error}");
            }
        }
    }

    fn controlled_child() -> std::process::Child {
        let mut command = Command::new("/bin/sleep");
        command.arg("10");
        bounded_process::configure_process_group(&mut command)
            .spawn()
            .unwrap_or_else(|error| panic!("spawn controlled child: {error}"))
    }

    struct UnprovenChild {
        termination_requests: usize,
    }

    impl SpawnedChildContainment for UnprovenChild {
        fn terminate_group_and_reap(&mut self) -> Result<(), CascadeError> {
            self.termination_requests += 1;
            Err(CascadeError::UnsafeContainment(
                "fixture could not prove direct-child reap".into(),
            ))
        }
    }

    #[test]
    fn bounded_command_captures_stdout_and_rejects_nonzero() {
        let fixture = TestDir::new();
        let success = fixture.program("success", "#!/bin/sh\nprintf ' /dev/zram7\\n'\n");
        let output = run_fixture_script_bounded(&success, Duration::from_millis(250))
            .unwrap_or_else(|error| panic!("bounded success command: {error}"));
        assert_eq!(output, "/dev/zram7");

        let failure = fixture.program(
            "failure",
            "#!/bin/sh\nprintf 'fixture failure' >&2\nexit 12\n",
        );
        let error = error_from(
            run_fixture_script_bounded(&failure, Duration::from_millis(250)),
            "non-zero child must fail",
        );
        let message = error.to_string();
        assert!(message.contains("exit 12"), "{message}");
        assert!(message.contains("fixture failure"), "{message}");

        let signal = fixture.program("signal", "#!/bin/sh\nkill -TERM $$\n");
        let error = error_from(
            run_fixture_script_bounded(&signal, Duration::from_millis(250)),
            "signal-terminated child must fail",
        );
        assert!(
            error.to_string().contains("terminated by signal"),
            "{error}"
        );

        let oversized = fixture.program("oversized", "#!/bin/sh\nhead -c 65537 /dev/zero\n");
        let error = error_from(
            run_fixture_script_bounded(&oversized, Duration::from_millis(250)),
            "bounded output must reject an oversized stream",
        );
        assert!(error.to_string().contains("output exceeded"), "{error}");
    }

    #[test]
    fn bounded_command_times_out_and_reaps_its_direct_child() {
        let observed_pid = Cell::new(None);
        let started = Instant::now();
        let error = error_from(
            run_command_bounded_for_with_spawn(
                "/bin/sleep",
                &["10"],
                Duration::from_millis(40),
                |pid| observed_pid.set(Some(pid)),
            ),
            "sleeping direct child must time out",
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.to_string().contains("timed out"), "{error}");

        let pid = observed_pid
            .get()
            .unwrap_or_else(|| panic!("bounded command did not report its direct child PID"));
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "timed-out direct child {pid} was not reaped"
        );
    }

    #[test]
    // TestName: bounded_command_contains_descendant_that_inherits_output
    fn bounded_command_contains_descendant_that_inherits_output() {
        let fixture = TestDir::new();
        let descendant_pid_file = fixture.path.join("inherited-output-descendant.pid");
        let program = fixture.program(
            "inherited-output",
            &format!(
                "#!/bin/sh\n(sleep 10) &\nprintf '%s' \"$!\" > '{}'\nprintf 'ready\\n'\nexit 0\n",
                descendant_pid_file.display()
            ),
        );

        let started = Instant::now();
        let error = error_from(
            run_fixture_script_bounded(&program, Duration::from_millis(100)),
            "an inherited output pipe must not be accepted as command success",
        );
        let descendant_pid = fs::read_to_string(&descendant_pid_file)
            .unwrap_or_else(|read_error| panic!("read descendant PID: {read_error}"))
            .parse::<u32>()
            .unwrap_or_else(|parse_error| panic!("parse descendant PID: {parse_error}"));
        let descendant_path = PathBuf::from(format!("/proc/{descendant_pid}"));
        let deadline = Instant::now() + Duration::from_millis(250);
        while descendant_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let contained = !descendant_path.exists();
        if !contained {
            let raw_pid = i32::try_from(descendant_pid)
                .unwrap_or_else(|_| panic!("descendant PID is outside pidfd range"));
            if let Some(pid) = Pid::from_raw(raw_pid) {
                let _ = rustix::process::kill_process(pid, Signal::KILL);
            }
        }

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            error.to_string().contains("output"),
            "unexpected inherited-pipe error: {error}"
        );
        assert!(contained, "owned inherited-output descendant survived");
    }

    #[test]
    fn zram_output_requires_exact_block_identity() {
        assert_eq!(
            parse_zram_device(" /dev/zram7\n"),
            Some("/dev/zram7".to_string())
        );
        for invalid in [
            "",
            "/dev/zram",
            "/dev/zram7-extra",
            "/tmp/zram7",
            "/dev/zram-7",
            "/dev/zram7 /dev/zram8",
        ] {
            assert_eq!(parse_zram_device(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn transport_refusal_is_fail_closed_before_command() {
        let error = error_from(
            check_transport_for(Transport::Ublk, true),
            "WSL2 ublk must refuse before a command is started",
        );
        assert!(error.to_string().contains("refused"), "{error}");
        assert!(check_transport_for(Transport::Nbd, true).is_ok());
    }

    #[test]
    fn product_daemon_command_requires_origin_cache() {
        let command = build_daemon_command(
            "/opt/ramshared/current/bin/ramsharedd",
            4096,
            1024,
            "/run/ramshared/wsl2d.sock",
            "/dev/nbd0",
            "/dev/disk/by-partuuid/11111111-2222-3333-4444-555555555555",
        );
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let environment = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|item| item.to_string_lossy().into_owned()),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            arguments
                .windows(2)
                .any(|pair| { pair == ["--origin-manifest", ORIGIN_CONFIG_FILE,] })
        );
        assert!(environment.iter().any(|(key, value)| {
            key == "RAMSHARED_VRAM_CACHE_CAP_MIB" && value.as_deref() == Some("1024")
        }));
    }

    #[test]
    fn provisioning_is_explicit_and_separate_from_normal_up() {
        let normal_source = include_str!("cascade_io.rs");
        let connect_start = normal_source
            .find("fn connect_nbd_with")
            .unwrap_or_else(|| panic!("connect_nbd_with source is missing"));
        let connect_end = normal_source[connect_start..]
            .find("/// Starts the cascade")
            .map(|offset| connect_start + offset)
            .unwrap_or_else(|| panic!("connect_nbd_with boundary is missing"));
        assert!(!normal_source[connect_start..connect_end].contains("mkswap"));

        let provisioner = include_str!("../../../../scripts/safety/provision-origin-swap.sh");
        assert!(provisioner.contains("RAMSHARED_ORIGIN_PROVISION_APPROVAL"));
        assert!(provisioner.contains("/sbin/mkswap -L RAMSHARED -U"));
        assert!(provisioner.contains("FOREIGN_SIGNATURE_PRESENT"));
    }

    #[test]
    fn failed_readiness_terminates_only_spawned_child() {
        let fixture = TestDir::new();
        let daemon = fixture.program(
            "ramsharedd",
            "#!/bin/sh\nprintf '%s' \"$$\" > \"$0.pid\"\nexec sleep 10\n",
        );
        let paths = RuntimePaths::under(&fixture.path);
        let error = error_from(
            spawn_daemon_with_deadline(
                &as_program(&daemon),
                64,
                64,
                "/dev/nbd0",
                "/dev/disk/by-partuuid/11111111-2222-3333-4444-555555555555",
                &paths,
                Duration::from_millis(40),
            ),
            "a daemon without the configured socket must fail readiness",
        );
        assert!(error.to_string().contains("socket"), "{error}");

        let pid_path = fixture.path.join("ramsharedd.pid");
        let pid = fs::read_to_string(&pid_path)
            .unwrap_or_else(|read_error| panic!("read fixture daemon pid: {read_error}"))
            .trim()
            .parse::<u32>()
            .unwrap_or_else(|parse_error| panic!("parse fixture daemon pid: {parse_error}"));
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "readiness cleanup left direct child {pid} alive"
        );
        assert!(!paths.pid_file.exists(), "stale runtime pid file");
        assert!(!paths.socket.exists(), "unexpected socket path");
    }

    #[test]
    fn origin_mode_refuses_missing_daemon_cache_identity_before_nbd_attach() {
        let fixture = TestDir::new();
        let daemon = fixture.program(
            "ramsharedd",
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--sock\" ]; then touch \"$2\"; fi\n  shift\ndone\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
        );
        let paths = RuntimePaths::under(&fixture.path);
        match spawn_daemon_with_deadline(
            &as_program(&daemon),
            1024,
            1024,
            "/dev/nbd0",
            "/dev/disk/by-partuuid/11111111-2222-3333-4444-555555555555",
            &paths,
            Duration::from_millis(80),
        ) {
            Ok(mut child) => {
                let _ = terminate_spawned_child(&mut child);
                panic!("origin mode accepted a daemon without a current cache identity");
            }
            Err(error) => {
                assert!(error.to_string().contains("identity"), "{error}");
            }
        }
        assert!(
            !paths.pid_file.exists(),
            "failed origin startup retained a PID record"
        );
    }

    #[test]
    fn connect_nbd_preserves_primary_error_and_rolls_back_once() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        let marker_parent = paths.forensics_markers[0]
            .parent()
            .unwrap_or_else(|| panic!("test marker has no parent"));
        fs::create_dir_all(marker_parent)
            .unwrap_or_else(|error| panic!("create test marker parent: {error}"));
        fs::write(&paths.forensics_markers[0], "armed")
            .unwrap_or_else(|error| panic!("write test marker: {error}"));
        let mut daemon = controlled_child();
        prepare_connect_identity(&paths, daemon.id());
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        set_live_device_snapshots([Ok(vec![nbd]), Ok(Vec::new())]);
        let socket = paths.socket.to_string_lossy().into_owned();
        let attach = format!("nbd-client -unix {socket} /dev/nbd0");
        let runner = ScriptedRunner::new(vec![
            (attach.clone(), Ok(String::new())),
            (
                "blkid -s TYPE -o value /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "blkid -s TYPE -o value /dev/nbd0".into(),
                    msg: "fixture signature failure".into(),
                }),
            ),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
        ]);

        let error = error_from(
            connect_nbd_with(
                &runner,
                &mut daemon,
                &paths,
                NbdAttachConfig {
                    connections: 1,
                    swap_dev: "/dev/nbd0",
                    vram_prio: 100,
                    disk_baseline_kib: 0,
                    logical_capacity_kib: 64 * 1024,
                    origin_partuuid: "11111111-2222-4333-8444-555555555555",
                    expected_swap_uuid: "99999999-8888-4777-8666-555555555555",
                    ..nbd_attach_fixture()
                },
            ),
            "signature error must be returned after rollback",
        );
        assert!(
            error.to_string().contains("fixture signature failure"),
            "primary error was replaced: {error}"
        );
        assert_eq!(
            runner.calls(),
            vec![
                attach,
                "blkid -s TYPE -o value /dev/nbd0".into(),
                "nbd-client -d /dev/nbd0".into(),
            ]
        );
        assert!(
            daemon
                .try_wait()
                .unwrap_or_else(|wait_error| panic!("inspect controlled child: {wait_error}"))
                .is_some(),
            "rollback did not reap the exact spawned child"
        );
        assert!(
            !paths.forensics_markers[0].exists(),
            "marker was not disarmed"
        );
    }

    #[test]
    // TestName: nbd_rollback_unproven_child_termination_retains_forensics
    fn nbd_rollback_unproven_child_termination_retains_forensics() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        fs::create_dir_all(
            paths.forensics_markers[0]
                .parent()
                .unwrap_or_else(|| panic!("test marker has no parent")),
        )
        .unwrap_or_else(|error| panic!("create test forensics directory: {error}"));
        fs::write(&paths.swap_dev_file, "/dev/nbd0")
            .unwrap_or_else(|error| panic!("write test swap record: {error}"));
        fs::write(&paths.pid_file, "fixture-pid")
            .unwrap_or_else(|error| panic!("write test PID record: {error}"));
        fs::write(&paths.cache_status_file, "cache-evidence")
            .unwrap_or_else(|error| panic!("write test cache evidence: {error}"));
        fs::write(&paths.capacity_status_file, "capacity-evidence")
            .unwrap_or_else(|error| panic!("write test capacity evidence: {error}"));
        fs::write(&paths.forensics_markers[0], "armed")
            .unwrap_or_else(|error| panic!("arm test forensics: {error}"));
        let runner = ScriptedRunner::new(Vec::new());
        let mut child = UnprovenChild {
            termination_requests: 0,
        };

        let error = error_from(
            rollback_nbd_attach_with(&runner, false, true, "/dev/nbd0", &mut child, &paths),
            "unproven exact child termination must stop cleanup",
        );

        assert!(matches!(error, CascadeError::UnsafeContainment(_)));
        assert_eq!(child.termination_requests, 1);
        assert!(paths.swap_dev_file.exists());
        assert!(paths.pid_file.exists());
        assert!(paths.cache_status_file.exists());
        assert!(paths.capacity_status_file.exists());
        assert!(paths.forensics_markers[0].exists());
    }

    #[test]
    // TestName: nbd_startup_disconnect_refusal_preserves_containment_evidence
    fn nbd_startup_disconnect_refusal_preserves_containment_evidence() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        fs::create_dir_all(
            paths.forensics_markers[0]
                .parent()
                .unwrap_or_else(|| panic!("test marker has no parent")),
        )
        .unwrap_or_else(|error| panic!("create marker parent: {error}"));
        fs::write(&paths.forensics_markers[0], "armed")
            .unwrap_or_else(|error| panic!("write marker: {error}"));
        fs::write(&paths.pid_file, "fixture-pid")
            .unwrap_or_else(|error| panic!("write PID evidence: {error}"));
        let mut daemon = controlled_child();
        prepare_connect_identity(&paths, daemon.id());
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        set_live_device_snapshots([Ok(vec![nbd])]);
        let socket = paths.socket.to_string_lossy().into_owned();
        let attach = format!("nbd-client -unix {socket} /dev/nbd0");
        let runner = ScriptedRunner::new(vec![
            (attach.clone(), Ok(String::new())),
            (
                "blkid -s TYPE -o value /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "blkid -s TYPE -o value /dev/nbd0".into(),
                    msg: "fixture signature failure".into(),
                }),
            ),
            (
                "nbd-client -d /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "nbd-client -d /dev/nbd0".into(),
                    msg: "fixture detach failure".into(),
                }),
            ),
        ]);

        let error = error_from(
            connect_nbd_with(
                &runner,
                &mut daemon,
                &paths,
                NbdAttachConfig {
                    connections: 1,
                    swap_dev: "/dev/nbd0",
                    vram_prio: 100,
                    disk_baseline_kib: 0,
                    logical_capacity_kib: 64 * 1024,
                    origin_partuuid: "11111111-2222-4333-8444-555555555555",
                    expected_swap_uuid: "99999999-8888-4777-8666-555555555555",
                    ..nbd_attach_fixture()
                },
            ),
            "an unproven NBD detach must be terminal",
        );
        assert!(error.to_string().contains("unsafe containment"), "{error}");
        assert!(
            daemon
                .try_wait()
                .unwrap_or_else(|wait_error| panic!("inspect child: {wait_error}"))
                .is_none(),
            "daemon was killed despite an unproven NBD detach"
        );
        assert!(paths.pid_file.exists(), "PID evidence was deleted");
        assert!(
            paths.forensics_markers[0].exists(),
            "forensics were disarmed"
        );
        assert_eq!(
            runner.calls(),
            vec![
                attach,
                "blkid -s TYPE -o value /dev/nbd0".into(),
                "nbd-client -d /dev/nbd0".into(),
            ]
        );
    }

    #[test]
    fn nbd_startup_disconnect_postcheck_preserves_daemon_and_evidence() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        let mut daemon = controlled_child();
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), false);
        arm_forensics_at(&paths);
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_swap_snapshots([
            Ok("Filename Type Size Used Priority\n"),
            Ok("Filename Type Size Used Priority\n"),
            Ok("Filename Type Size Used Priority\n"),
        ]);
        set_live_device_snapshots([Ok(devices.clone()), Ok(devices)]);
        let runner =
            ScriptedRunner::new(vec![("nbd-client -d /dev/nbd0".into(), Ok(String::new()))]);

        let error = error_from(
            rollback_nbd_attach_with(&runner, true, true, "/dev/nbd0", &mut daemon, &paths),
            "a successful command without exact disappearance proof must preserve containment",
        );

        assert!(error.to_string().contains("completion"), "{error}");
        assert_eq!(runner.calls(), vec!["nbd-client -d /dev/nbd0"]);
        assert!(
            daemon
                .try_wait()
                .unwrap_or_else(|wait_error| panic!("inspect controlled child: {wait_error}"))
                .is_none(),
            "unproven detach terminated the exact daemon"
        );
        assert!(paths.lifecycle_binding_file.exists());
        assert!(paths.swap_dev_file.exists());
        assert!(paths.pid_file.exists());
        assert!(paths.forensics_markers[0].exists());
    }

    #[test]
    fn connect_nbd_refusal_terminates_exact_daemon_without_detach() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        let marker_parent = paths.forensics_markers[0]
            .parent()
            .unwrap_or_else(|| panic!("test marker has no parent"));
        fs::create_dir_all(marker_parent)
            .unwrap_or_else(|error| panic!("create test marker parent: {error}"));
        fs::write(&paths.forensics_markers[0], "armed")
            .unwrap_or_else(|error| panic!("write test marker: {error}"));
        let runner = ScriptedRunner::new(Vec::new());
        let mut daemon = controlled_child();

        let error = error_from(
            connect_nbd_with(
                &runner,
                &mut daemon,
                &paths,
                NbdAttachConfig {
                    connections: 0,
                    swap_dev: "/dev/nbd0",
                    vram_prio: 100,
                    disk_baseline_kib: 0,
                    logical_capacity_kib: 64 * 1024,
                    origin_partuuid: "11111111-2222-4333-8444-555555555555",
                    expected_swap_uuid: "99999999-8888-4777-8666-555555555555",
                    ..nbd_attach_fixture()
                },
            ),
            "invalid connection count must stop the exact spawned daemon",
        );
        assert!(error.to_string().contains("connections"));
        assert!(
            runner.calls().is_empty(),
            "pre-attach refusal ran a command"
        );
        assert!(
            daemon
                .try_wait()
                .unwrap_or_else(|wait_error| panic!("inspect controlled child: {wait_error}"))
                .is_some(),
            "pre-attach refusal did not reap the exact spawned child"
        );
        assert!(!paths.forensics_markers[0].exists());
    }

    #[test]
    fn connect_nbd_uncertain_swapon_preserves_backend_and_daemon() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        let mut daemon = controlled_child();
        prepare_connect_identity(&paths, daemon.id());
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        let socket = paths.socket.to_string_lossy().into_owned();
        let attach = format!("nbd-client -unix {socket} /dev/nbd0");
        let runner = ScriptedRunner::new(vec![
            (attach.clone(), Ok(String::new())),
            ("blkid -s TYPE -o value /dev/nbd0".into(), Ok("swap".into())),
            (
                "blkid -s UUID -o value /dev/nbd0".into(),
                Ok("99999999-8888-4777-8666-555555555555".into()),
            ),
            (
                "swapon -p 100 /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "swapon -p 100 /dev/nbd0".into(),
                    msg: "fixture activate failure".into(),
                }),
            ),
        ]);

        let error = error_from(
            connect_nbd_with(
                &runner,
                &mut daemon,
                &paths,
                NbdAttachConfig {
                    connections: 1,
                    swap_dev: "/dev/nbd0",
                    vram_prio: 100,
                    disk_baseline_kib: 0,
                    logical_capacity_kib: 64 * 1024,
                    origin_partuuid: "11111111-2222-4333-8444-555555555555",
                    expected_swap_uuid: "99999999-8888-4777-8666-555555555555",
                    ..nbd_attach_fixture()
                },
            ),
            "uncertain swapon must preserve the exact backend and daemon",
        );
        assert!(error.to_string().contains("swapon outcome"), "{error}");
        assert_eq!(
            runner.calls(),
            vec![
                attach,
                "blkid -s TYPE -o value /dev/nbd0".into(),
                "blkid -s UUID -o value /dev/nbd0".into(),
                "swapon -p 100 /dev/nbd0".into(),
            ]
        );
        assert!(
            daemon
                .try_wait()
                .unwrap_or_else(|wait_error| panic!("inspect controlled child: {wait_error}"))
                .is_none(),
            "uncertain swapon terminated the exact daemon"
        );
        assert!(paths.swap_dev_file.exists());
        assert!(paths.lifecycle_binding_file.exists());
    }

    #[test]
    // TestName: nbd_effect_before_timeout_preserves_backend_and_seals_exact_evidence
    fn nbd_effect_before_timeout_preserves_backend_and_seals_exact_evidence() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        let mut daemon = controlled_child();
        prepare_connect_identity(&paths, daemon.id());
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        set_live_device_snapshots([Ok(vec![nbd.clone()]), Ok(vec![nbd])]);
        let socket = paths.socket.to_string_lossy().into_owned();
        let attach = format!("nbd-client -unix {socket} /dev/nbd0");
        let runner = ScriptedRunner::new(vec![(
            attach.clone(),
            Err(CascadeError::Shell {
                cmd: attach.clone(),
                msg: "timed out after effect fixture".into(),
            }),
        )]);

        let error = error_from(
            connect_nbd_with(&runner, &mut daemon, &paths, nbd_attach_fixture()),
            "effect-before-timeout must return unsafe containment",
        );

        assert!(matches!(error, CascadeError::UnsafeContainment(_)));
        assert!(error.to_string().contains("took effect"), "{error}");
        assert_eq!(runner.calls(), vec![attach]);
        assert!(
            daemon
                .try_wait()
                .unwrap_or_else(|wait_error| panic!("inspect preserved daemon: {wait_error}"))
                .is_none(),
            "uncertain attach terminated the backend daemon"
        );
        let binding = read_lifecycle_binding(&paths).unwrap_or_else(|read_error| {
            panic!("read provisional lifecycle binding: {read_error}")
        });
        assert_eq!(binding.devices.len(), 1);
        assert_eq!(binding.devices[0].path, "/dev/nbd0");
        assert_eq!(
            read_nbd_path_record(&paths.swap_dev_file)
                .unwrap_or_else(|read_error| panic!("read provisional NBD path: {read_error}")),
            "/dev/nbd0"
        );
        terminate_spawned_child(&mut daemon)
            .unwrap_or_else(|stop_error| panic!("contain preserved fixture daemon: {stop_error}"));
    }

    #[test]
    // TestName: nbd_failed_attach_terminates_backend_only_after_two_absence_snapshots
    fn nbd_failed_attach_terminates_backend_only_after_two_absence_snapshots() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        let mut daemon = controlled_child();
        prepare_connect_identity(&paths, daemon.id());
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_live_device_snapshots([Ok(Vec::new()), Ok(Vec::new())]);
        let socket = paths.socket.to_string_lossy().into_owned();
        let attach = format!("nbd-client -unix {socket} /dev/nbd0");
        let runner = ScriptedRunner::new(vec![(
            attach.clone(),
            Err(CascadeError::Shell {
                cmd: attach.clone(),
                msg: "fixture attach failure".into(),
            }),
        )]);

        let error = error_from(
            connect_nbd_with(&runner, &mut daemon, &paths, nbd_attach_fixture()),
            "proven absent attach must return the primary error",
        );

        assert!(
            error.to_string().contains("fixture attach failure"),
            "{error}"
        );
        assert_eq!(runner.calls(), vec![attach]);
        assert!(
            daemon
                .try_wait()
                .unwrap_or_else(|wait_error| panic!("inspect contained daemon: {wait_error}"))
                .is_some(),
            "backend was not terminated after two exact absence snapshots"
        );
        assert!(!paths.lifecycle_binding_file.exists());
        assert!(!paths.swap_dev_file.exists());
    }

    #[test]
    // TestName: nbd_failed_attach_preserves_backend_when_target_status_is_not_absent
    fn nbd_failed_attach_preserves_backend_when_target_status_is_not_absent() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        let mut daemon = controlled_child();
        prepare_connect_identity(&paths, daemon.id());
        let _seams = ParentSeams::install(
            "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n",
            0,
        );
        set_live_device_snapshots([Ok(Vec::new()), Ok(Vec::new())]);
        let socket = paths.socket.to_string_lossy().into_owned();
        let attach = format!("nbd-client -unix {socket} /dev/nbd0");
        let runner = ScriptedRunner::new(vec![(
            attach.clone(),
            Err(CascadeError::Shell {
                cmd: attach.clone(),
                msg: "fixture attach failure with active target status".into(),
            }),
        )]);

        let error = error_from(
            connect_nbd_with(&runner, &mut daemon, &paths, nbd_attach_fixture()),
            "ambiguous target status must preserve the backend",
        );

        assert!(matches!(error, CascadeError::UnsafeContainment(_)));
        assert!(error.to_string().contains("absence is unproven"), "{error}");
        assert_eq!(runner.calls(), vec![attach]);
        assert!(
            daemon
                .try_wait()
                .unwrap_or_else(|wait_error| panic!("inspect preserved daemon: {wait_error}"))
                .is_none(),
            "ambiguous target status terminated the backend daemon"
        );
        terminate_spawned_child(&mut daemon)
            .unwrap_or_else(|stop_error| panic!("contain preserved fixture daemon: {stop_error}"));
    }

    #[test]
    fn zram_fallback_refuses_unexpected_device_without_swapon() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        let runner = ScriptedRunner::new(vec![
            ("modprobe zram".into(), Ok(String::new())),
            (
                "zramctl --find --size 64M --algorithm lzo-rle".into(),
                Ok("/tmp/not-zram".into()),
            ),
            (
                "zramctl --find --size 64M --algorithm lzo".into(),
                Ok("/tmp/not-zram".into()),
            ),
            (
                "zramctl --find --size 64M --algorithm zstd".into(),
                Ok("/tmp/not-zram".into()),
            ),
            (
                "zramctl --find --size 64M --algorithm lz4".into(),
                Ok("/tmp/not-zram".into()),
            ),
            (
                "zramctl --find --size 64M --algorithm deflate".into(),
                Ok("/tmp/not-zram".into()),
            ),
        ]);

        let error = error_from(
            setup_zram_with(&runner, &paths, 64, 200),
            "non-zram output must fail before mkswap or swapon",
        );
        assert!(error.to_string().contains("unexpected device"), "{error}");
        assert_eq!(runner.calls().len(), 6);
        assert!(!paths.zram_dev_file.exists());
    }

    #[test]
    // TestName: malformed_zram_success_resets_exact_new_device_without_leak
    fn malformed_zram_success_resets_exact_new_device_without_leak() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create zram reconciliation runtime: {error}"));
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        let zram = bound_device_fixture("/dev/zram7", ManagedDeviceKind::Zram);
        set_live_device_snapshots([Ok(vec![zram]), Ok(Vec::new())]);
        let runner = ScriptedRunner::new(vec![
            ("modprobe zram".into(), Ok(String::new())),
            (
                "zramctl --find --size 64M --algorithm lzo-rle".into(),
                Ok("allocator-success-without-device".into()),
            ),
            ("zramctl -r /dev/zram7".into(), Ok(String::new())),
        ]);

        let error = error_from(
            setup_zram_with(&runner, &paths, 64, 200),
            "malformed successful allocation must stop after exact reset",
        );

        assert!(
            error
                .to_string()
                .contains("exact newly allocated device was reset")
        );
        assert_eq!(
            runner.calls(),
            vec![
                "modprobe zram".to_string(),
                "zramctl --find --size 64M --algorithm lzo-rle".to_string(),
                "zramctl -r /dev/zram7".to_string(),
            ]
        );
        assert!(!paths.zram_dev_file.exists());
    }

    #[test]
    fn zram_uncertain_swapon_preserves_runtime_record_and_device() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_live_device_snapshots([Ok(vec![bound_device_fixture(
            "/dev/zram7",
            ManagedDeviceKind::Zram,
        )])]);
        let runner = ScriptedRunner::new(vec![
            ("modprobe zram".into(), Ok(String::new())),
            (
                "zramctl --find --size 64M --algorithm lzo-rle".into(),
                Ok("/dev/zram7".into()),
            ),
            ("mkswap /dev/zram7".into(), Ok(String::new())),
            (
                "swapon -p 200 /dev/zram7".into(),
                Err(CascadeError::Shell {
                    cmd: "swapon -p 200 /dev/zram7".into(),
                    msg: "fixture zram activation failure".into(),
                }),
            ),
        ]);

        let error = error_from(
            setup_zram_with(&runner, &paths, 64, 200),
            "uncertain zram activation must preserve the exact backend",
        );
        assert!(error.to_string().contains("swapon outcome"));
        assert_eq!(
            runner.calls(),
            vec![
                "modprobe zram".to_string(),
                "zramctl --find --size 64M --algorithm lzo-rle".to_string(),
                "mkswap /dev/zram7".to_string(),
                "swapon -p 200 /dev/zram7".to_string(),
            ]
        );
        assert!(paths.zram_dev_file.exists());
    }

    #[test]
    fn zram_setup_never_mutates_unbound_sysfs_fallback() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.zram_sysfs)
            .unwrap_or_else(|error| panic!("create test sysfs root: {error}"));
        fs::write(paths.zram_sysfs.join("reset"), "foreign-sentinel")
            .unwrap_or_else(|error| panic!("create test reset: {error}"));
        let runner = ScriptedRunner::new(vec![
            ("modprobe zram".into(), Ok(String::new())),
            (
                "zramctl --find --size 1M --algorithm lzo-rle".into(),
                Err(CascadeError::Shell {
                    cmd: "zramctl --find --size 1M --algorithm lzo-rle".into(),
                    msg: "no free zram".into(),
                }),
            ),
            (
                "zramctl --find --size 1M --algorithm lzo".into(),
                Err(CascadeError::Shell {
                    cmd: "zramctl --find --size 1M --algorithm lzo".into(),
                    msg: "no free zram".into(),
                }),
            ),
            (
                "zramctl --find --size 1M --algorithm zstd".into(),
                Err(CascadeError::Shell {
                    cmd: "zramctl --find --size 1M --algorithm zstd".into(),
                    msg: "no free zram".into(),
                }),
            ),
            (
                "zramctl --find --size 1M --algorithm lz4".into(),
                Err(CascadeError::Shell {
                    cmd: "zramctl --find --size 1M --algorithm lz4".into(),
                    msg: "no free zram".into(),
                }),
            ),
            (
                "zramctl --find --size 1M --algorithm deflate".into(),
                Err(CascadeError::Shell {
                    cmd: "zramctl --find --size 1M --algorithm deflate".into(),
                    msg: "no free zram".into(),
                }),
            ),
        ]);
        let error = error_from(
            setup_zram_with(&runner, &paths, 1, 200),
            "an unavailable ownership-creating allocator must not fall back to zram0 sysfs",
        );
        assert!(
            error.to_string().contains("sysfs fallback is disabled"),
            "{error}"
        );
        assert_eq!(
            fs::read_to_string(paths.zram_sysfs.join("reset"))
                .expect("read untouched reset fixture"),
            "foreign-sentinel"
        );
        assert!(!paths.zram_sysfs.join("disksize").exists());

        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        TEST_MEM_AVAILABLE.with(|cell| *cell.borrow_mut() = Some(0));
        assert!(check_safety_net(1, false, &TierPriorities::default()).is_err());
        assert!(check_safety_net(1, true, &TierPriorities::default()).is_ok());
        assert!(check_safety_net(u64::MAX, true, &TierPriorities::default()).is_err());
    }

    #[test]
    fn runtime_marker_and_pid_record_refuse_unsafe_identity() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        arm_forensics_at(&paths);
        assert!(paths.forensics_markers[0].exists());
        arm_forensics_at(&paths);
        disarm_forensics_at(&paths);
        assert!(!paths.forensics_markers[0].exists());

        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        fs::write(&paths.pid_file, "not-a-pid")
            .unwrap_or_else(|error| panic!("write malformed PID: {error}"));
        let _ = stop_daemon_gracefully_at(&paths, Duration::from_millis(10));
        assert!(
            paths.pid_file.exists(),
            "an unverifiable daemon PID must retain runtime evidence"
        );
        assert!(process_is_gone_or_zombie(u32::MAX));

        let system = RuntimePaths::system();
        assert_eq!(system.socket, PathBuf::from(SOCK));
        assert_eq!(system.pid_file, PathBuf::from(PID_FILE));
    }

    #[test]
    fn setup_new_cascade_uses_only_temp_runtime_and_direct_child_fixture() {
        let fixture = TestDir::new();
        let daemon = fixture.program(
            "ramsharedd",
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--sock\" ]; then\n    runtime=${2%/*}\n    start=$(awk '{print $22}' /proc/$$/stat)\n    now=$(date +%s%3N)\n    printf '{\"schema_version\":1,\"daemon_instance_id\":\"%s-%s\",\"written_at_unix_ms\":%s,\"ok\":true,\"origin_state\":\"READY\",\"cache_state\":\"ACTIVE\",\"logical_capacity_kib\":1024,\"vram_cached_kib\":0,\"gpu_headroom_kib\":null,\"ssd_origin_written_kib\":1024,\"cache_fallback_reads\":0,\"cache_invalidations\":0,\"cache_releases\":0,\"cache_target_kib\":0}' \"$$\" \"$start\" \"$now\" > \"$runtime/cache-status.json\"\n    touch \"$2\"\n  fi\n  shift\ndone\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
        );
        let paths = RuntimePaths::under(&fixture.path);
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_live_device_snapshots([Ok(vec![bound_device_fixture(
            "/dev/zram7",
            ManagedDeviceKind::Zram,
        )])]);
        let socket = paths.socket.to_string_lossy().into_owned();
        let runner = ScriptedRunner::new(vec![
            ("modprobe zram".into(), Ok(String::new())),
            (
                "zramctl --find --size 64M --algorithm lzo-rle".into(),
                Ok("/dev/zram7".into()),
            ),
            ("mkswap /dev/zram7".into(), Ok(String::new())),
            ("swapon -p 200 /dev/zram7".into(), Ok(String::new())),
            (
                "modprobe nbd nbds_max=1 max_part=0".into(),
                Ok(String::new()),
            ),
            (
                format!("nbd-client -unix {socket} /dev/nbd0"),
                Ok(String::new()),
            ),
            ("blkid -s TYPE -o value /dev/nbd0".into(), Ok("swap".into())),
            (
                "blkid -s UUID -o value /dev/nbd0".into(),
                Ok("99999999-8888-4777-8666-555555555555".into()),
            ),
            ("swapon -p 100 /dev/nbd0".into(), Ok(String::new())),
        ]);
        let args = UpArgs {
            vram_mb: 64,
            zram_mb: 64,
            daemon: as_program(&daemon),
            force: false,
            connections: 1,
            transport: Transport::Nbd,
            swap_dev: "/dev/nbd0".into(),
            origin_path: "/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555".into(),
            expected_swap_uuid: "99999999-8888-4777-8666-555555555555".into(),
            cache_cap_mib: 64,
            disk_baseline_kib: 0,
            ..up_args_fixture()
        };

        let mut daemon = setup_new_cascade(&runner, &paths, &args, &TierPriorities::default())
            .unwrap_or_else(|error| panic!("isolated cascade setup: {error}"));
        let zram_record = read_bound_device_record(&paths.zram_dev_file)
            .unwrap_or_else(|error| panic!("read zram identity record: {error}"));
        assert_eq!(zram_record.path, "/dev/zram7");
        assert_eq!(zram_record.kind, ManagedDeviceKind::Zram);
        let nbd_path_record = read_nbd_path_record(&paths.swap_dev_file)
            .unwrap_or_else(|error| panic!("read NBD path record: {error}"));
        assert_eq!(nbd_path_record, "/dev/nbd0");
        assert_eq!(
            fs::read_to_string(&paths.capacity_status_file)
                .ok()
                .as_deref(),
            Some(
                "schema_version=4\nmode=origin-cache\nlogical_capacity_kib=65536\n\
                 vram_cached_kib=0\nssd_origin_written_kib=0\nfallback_swap_used_kib=0\n\
                 origin_partuuid=11111111-2222-4333-8444-555555555555\n\
                 disk_baseline_kib=0\n"
            )
        );
        assert!(
            paths.socket.exists(),
            "fixture daemon did not create its socket path"
        );
        assert_eq!(runner.calls().len(), 9);

        // This setup assertion deliberately injects its writable shell
        // fixture through /bin/sh to avoid ETXTBSY under parallel execution.
        // It therefore cannot satisfy the production executable-name proof
        // used by the graceful-stop tests. Contain only the exact child
        // returned by setup; the native ramsharedd fixture tests cover the
        // production pidfd graceful-stop path separately.
        terminate_spawned_child(&mut daemon)
            .unwrap_or_else(|error| panic!("contain direct fixture daemon: {error}"));
        remove_runtime_file(&paths.pid_file);
        assert!(
            !paths.pid_file.exists(),
            "fixture daemon PID was not cleaned up"
        );
    }

    #[test]
    fn setup_new_cascade_rolls_back_zram_after_nbd_failure() {
        let fixture = TestDir::new();
        let daemon = fixture.program(
            "ramsharedd",
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--sock\" ]; then\n    runtime=${2%/*}\n    start=$(awk '{print $22}' /proc/$$/stat)\n    now=$(date +%s%3N)\n    printf '{\"schema_version\":1,\"daemon_instance_id\":\"%s-%s\",\"written_at_unix_ms\":%s,\"ok\":true,\"origin_state\":\"READY\",\"cache_state\":\"ACTIVE\",\"logical_capacity_kib\":1024,\"vram_cached_kib\":0,\"gpu_headroom_kib\":null,\"ssd_origin_written_kib\":1024,\"cache_fallback_reads\":0,\"cache_invalidations\":0,\"cache_releases\":0,\"cache_target_kib\":0}' \"$$\" \"$start\" \"$now\" > \"$runtime/cache-status.json\"\n    touch \"$2\"\n  fi\n  shift\ndone\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
        );
        let paths = RuntimePaths::under(&fixture.path);
        const ZRAM_ACTIVE: &str =
            "Filename Type Size Used Priority\n/dev/zram7 partition 65536 0 200\n";
        const EMPTY: &str = "Filename Type Size Used Priority\n";
        let _seams = ParentSeams::install(ZRAM_ACTIVE, 0);
        set_swap_snapshots([
            Ok(ZRAM_ACTIVE),
            Ok(ZRAM_ACTIVE),
            Ok(ZRAM_ACTIVE),
            Ok(ZRAM_ACTIVE),
            Ok(EMPTY),
        ]);
        let zram = bound_device_fixture("/dev/zram7", ManagedDeviceKind::Zram);
        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        set_live_device_snapshots([
            Ok(vec![zram.clone()]),
            Ok(vec![nbd, zram.clone()]),
            Ok(vec![zram.clone()]),
            Ok(vec![zram]),
            Ok(Vec::new()),
        ]);
        let socket = paths.socket.to_string_lossy().into_owned();
        let runner = ScriptedRunner::new(vec![
            ("modprobe zram".into(), Ok(String::new())),
            (
                "zramctl --find --size 64M --algorithm lzo-rle".into(),
                Ok("/dev/zram7".into()),
            ),
            ("mkswap /dev/zram7".into(), Ok(String::new())),
            ("swapon -p 200 /dev/zram7".into(), Ok(String::new())),
            (
                "modprobe nbd nbds_max=1 max_part=0".into(),
                Ok(String::new()),
            ),
            (
                format!("nbd-client -unix {socket} /dev/nbd0"),
                Ok(String::new()),
            ),
            (
                "blkid -s TYPE -o value /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "blkid -s TYPE -o value /dev/nbd0".into(),
                    msg: "fixture nbd signature failure".into(),
                }),
            ),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
            ("swapoff -- /dev/zram7".into(), Ok(String::new())),
            ("zramctl -r /dev/zram7".into(), Ok(String::new())),
        ]);
        let args = UpArgs {
            vram_mb: 64,
            zram_mb: 64,
            daemon: as_program(&daemon),
            force: false,
            connections: 1,
            transport: Transport::Nbd,
            swap_dev: "/dev/nbd0".into(),
            origin_path: "/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555".into(),
            expected_swap_uuid: "99999999-8888-4777-8666-555555555555".into(),
            cache_cap_mib: 64,
            disk_baseline_kib: 0,
            ..up_args_fixture()
        };

        let error = error_from(
            setup_new_cascade(&runner, &paths, &args, &TierPriorities::default()),
            "NBD failure must roll back the just-created zram tier",
        );
        assert!(error.to_string().contains("fixture nbd signature failure"));
        assert_eq!(
            runner.calls(),
            vec![
                "modprobe zram".to_string(),
                "zramctl --find --size 64M --algorithm lzo-rle".to_string(),
                "mkswap /dev/zram7".to_string(),
                "swapon -p 200 /dev/zram7".to_string(),
                "modprobe nbd nbds_max=1 max_part=0".to_string(),
                format!("nbd-client -unix {socket} /dev/nbd0"),
                "blkid -s TYPE -o value /dev/nbd0".to_string(),
                "nbd-client -d /dev/nbd0".to_string(),
                "swapoff -- /dev/zram7".to_string(),
                "zramctl -r /dev/zram7".to_string(),
            ]
        );
        assert!(!paths.zram_dev_file.exists());
        assert!(!paths.swap_dev_file.exists());
        assert!(!paths.pid_file.exists());
        assert!(!paths.socket.exists());
        assert!(!paths.forensics_markers[0].exists());
    }

    #[test]
    fn setup_new_cascade_keeps_zram_record_on_swapoff_refusal() {
        let fixture = TestDir::new();
        let daemon = fixture.program(
            "ramsharedd",
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--sock\" ]; then\n    runtime=${2%/*}\n    start=$(awk '{print $22}' /proc/$$/stat)\n    now=$(date +%s%3N)\n    printf '{\"schema_version\":1,\"daemon_instance_id\":\"%s-%s\",\"written_at_unix_ms\":%s,\"ok\":true,\"origin_state\":\"READY\",\"cache_state\":\"ACTIVE\",\"logical_capacity_kib\":1024,\"vram_cached_kib\":0,\"gpu_headroom_kib\":null,\"ssd_origin_written_kib\":1024,\"cache_fallback_reads\":0,\"cache_invalidations\":0,\"cache_releases\":0,\"cache_target_kib\":0}' \"$$\" \"$start\" \"$now\" > \"$runtime/cache-status.json\"\n    touch \"$2\"\n  fi\n  shift\ndone\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
        );
        let paths = RuntimePaths::under(&fixture.path);
        const ZRAM_ACTIVE: &str =
            "Filename Type Size Used Priority\n/dev/zram7 partition 65536 0 200\n";
        let _seams = ParentSeams::install(ZRAM_ACTIVE, 0);
        set_swap_snapshots([Ok(ZRAM_ACTIVE), Ok(ZRAM_ACTIVE), Ok(ZRAM_ACTIVE)]);
        let zram = bound_device_fixture("/dev/zram7", ManagedDeviceKind::Zram);
        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        set_live_device_snapshots([
            Ok(vec![zram.clone()]),
            Ok(vec![nbd, zram.clone()]),
            Ok(vec![zram.clone()]),
            Ok(vec![zram]),
        ]);
        let socket = paths.socket.to_string_lossy().into_owned();
        let runner = ScriptedRunner::new(vec![
            ("modprobe zram".into(), Ok(String::new())),
            (
                "zramctl --find --size 64M --algorithm lzo-rle".into(),
                Ok("/dev/zram7".into()),
            ),
            ("mkswap /dev/zram7".into(), Ok(String::new())),
            ("swapon -p 200 /dev/zram7".into(), Ok(String::new())),
            (
                "modprobe nbd nbds_max=1 max_part=0".into(),
                Ok(String::new()),
            ),
            (
                format!("nbd-client -unix {socket} /dev/nbd0"),
                Ok(String::new()),
            ),
            (
                "blkid -s TYPE -o value /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "blkid -s TYPE -o value /dev/nbd0".into(),
                    msg: "fixture nbd signature failure".into(),
                }),
            ),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
            (
                "swapoff -- /dev/zram7".into(),
                Err(CascadeError::Shell {
                    cmd: "swapoff -- /dev/zram7".into(),
                    msg: "fixture zram busy".into(),
                }),
            ),
        ]);
        let args = UpArgs {
            vram_mb: 64,
            zram_mb: 64,
            daemon: as_program(&daemon),
            force: false,
            connections: 1,
            transport: Transport::Nbd,
            swap_dev: "/dev/nbd0".into(),
            origin_path: "/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555".into(),
            expected_swap_uuid: "99999999-8888-4777-8666-555555555555".into(),
            cache_cap_mib: 64,
            disk_baseline_kib: 0,
            ..up_args_fixture()
        };

        let error = error_from(
            setup_new_cascade(&runner, &paths, &args, &TierPriorities::default()),
            "swapoff refusal must preserve the original NBD error and zram record",
        );
        assert!(error.to_string().contains("fixture nbd signature failure"));
        assert_eq!(
            runner.calls(),
            vec![
                "modprobe zram".to_string(),
                "zramctl --find --size 64M --algorithm lzo-rle".to_string(),
                "mkswap /dev/zram7".to_string(),
                "swapon -p 200 /dev/zram7".to_string(),
                "modprobe nbd nbds_max=1 max_part=0".to_string(),
                format!("nbd-client -unix {socket} /dev/nbd0"),
                "blkid -s TYPE -o value /dev/nbd0".to_string(),
                "nbd-client -d /dev/nbd0".to_string(),
                "swapoff -- /dev/zram7".to_string(),
            ]
        );
        let zram_record = read_bound_device_record(&paths.zram_dev_file)
            .unwrap_or_else(|error| panic!("read preserved zram identity record: {error}"));
        assert_eq!(zram_record.path, "/dev/zram7");
        assert!(!paths.swap_dev_file.exists());
        assert!(!paths.pid_file.exists());
        assert!(!paths.socket.exists());
        assert!(paths.forensics_markers[0].exists());
    }

    struct RecordingNbdLifecycleExecutor {
        calls: RefCell<Vec<String>>,
        fail_swapoff: bool,
    }

    impl RecordingNbdLifecycleExecutor {
        fn new(fail_swapoff: bool) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                fail_swapoff,
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl NbdLifecycleExecutor for RecordingNbdLifecycleExecutor {
        fn swapoff(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
            self.calls
                .borrow_mut()
                .push(format!("swapoff {}", device.path));
            if self.fail_swapoff {
                return Err(CascadeError::Shell {
                    cmd: format!("swapoff {}", device.path),
                    msg: "manufactured swapoff refusal".into(),
                });
            }
            Ok(())
        }

        fn reset_zram(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
            self.calls
                .borrow_mut()
                .push(format!("reset-zram {}", device.path));
            Ok(())
        }

        fn disconnect_nbd(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
            self.calls
                .borrow_mut()
                .push(format!("disconnect-nbd {}", device.path));
            Ok(())
        }

        fn stop_daemon(&self) -> Result<(), CascadeError> {
            self.calls.borrow_mut().push("stop-daemon".into());
            Ok(())
        }
    }

    #[test]
    // TestName: swapoff_completes_before_nbd_disconnect
    fn swapoff_completes_before_nbd_disconnect() {
        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        let zram = bound_device_fixture("/dev/zram0", ManagedDeviceKind::Zram);
        let binding = lifecycle_binding_fixture(vec![nbd.clone(), zram.clone()]);
        let swaps = parse_proc_swaps(
            "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n/dev/zram0 partition 512 0 200\n",
        )
        .expect("strict swap fixture");
        let plan =
            plan_nbd_lifecycle(&binding, &swaps, &[nbd, zram]).expect("authorized lifecycle plan");
        let executor = RecordingNbdLifecycleExecutor::new(false);

        execute_nbd_lifecycle_plan(&plan, &executor)
            .unwrap_or_else(|error| panic!("local injected lifecycle: {error}"));

        assert_eq!(
            executor.calls(),
            vec![
                "swapoff /dev/nbd0",
                "swapoff /dev/zram0",
                "reset-zram /dev/zram0",
                "disconnect-nbd /dev/nbd0",
                "stop-daemon",
            ]
        );
    }

    #[test]
    // TestName: failed_swapoff_keeps_daemon_and_device_alive
    fn failed_swapoff_keeps_daemon_and_device_alive() {
        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        let binding = lifecycle_binding_fixture(vec![nbd.clone()]);
        let swaps =
            parse_proc_swaps("Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n")
                .expect("strict swap fixture");
        let plan = plan_nbd_lifecycle(&binding, &swaps, &[nbd]).expect("authorized lifecycle plan");
        let executor = RecordingNbdLifecycleExecutor::new(true);

        let error = error_from(
            execute_nbd_lifecycle_plan(&plan, &executor),
            "a refused swapoff must stop the injected plan",
        );
        assert!(error.to_string().contains("manufactured swapoff refusal"));
        assert_eq!(executor.calls(), vec!["swapoff /dev/nbd0"]);
    }

    #[test]
    fn lifecycle_binding_rejects_ambiguous_device_cardinality() {
        let first = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        let second = bound_device_fixture("/dev/nbd1", ManagedDeviceKind::Nbd);
        let binding = lifecycle_binding_fixture(vec![first, second]);

        let error = error_from(
            validate_lifecycle_binding(&binding),
            "two NBD devices must invalidate an exact lifecycle binding",
        );

        assert!(error.to_string().contains("cardinality"), "{error}");
    }

    #[test]
    fn down_refuses_foreign_live_device_without_running_a_command() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), false);
        let mut observed = devices.clone();
        observed.push(bound_device_fixture("/dev/nbd1", ManagedDeviceKind::Nbd));
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_swap_snapshots([Ok("Filename Type Size Used Priority\n")]);
        set_live_device_snapshots([Ok(observed)]);
        let runner = ScriptedRunner::new(Vec::new());

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "foreign device discovery must refuse the whole teardown",
        );

        assert!(error.to_string().contains("foreign live"), "{error}");
        assert!(runner.calls().is_empty());
        assert!(paths.lifecycle_binding_file.exists());
        assert!(!process_is_gone_or_zombie(daemon.id()));
    }

    #[test]
    fn down_refuses_missing_bound_live_device_without_running_a_command() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        let _devices = seal_runtime_lifecycle(&paths, daemon.id(), false);
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_swap_snapshots([Ok("Filename Type Size Used Priority\n")]);
        set_live_device_snapshots([Ok(Vec::new())]);
        let runner = ScriptedRunner::new(Vec::new());

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "a missing lifecycle-bound device must refuse the whole teardown",
        );

        assert!(error.to_string().contains("cardinality differs"), "{error}");
        assert!(runner.calls().is_empty());
        assert!(paths.lifecycle_binding_file.exists());
        assert!(paths.swap_dev_file.exists());
        assert!(!process_is_gone_or_zombie(daemon.id()));
    }

    #[test]
    fn zram_reset_stage_mismatch_stops_before_nbd_disconnect() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), true);
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        let full = devices.clone();
        set_live_device_snapshots([Ok(full.clone()), Ok(full.clone()), Ok(full)]);
        let runner = ScriptedRunner::new(vec![("zramctl -r /dev/zram0".into(), Ok(String::new()))]);

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "zram reset without exact stage transition must stop the teardown",
        );

        assert!(error.to_string().contains("reset completion"), "{error}");
        assert_eq!(runner.calls(), vec!["zramctl -r /dev/zram0"]);
        assert!(paths.lifecycle_binding_file.exists());
        assert!(paths.swap_dev_file.exists());
        assert!(paths.zram_dev_file.exists());
        assert!(!process_is_gone_or_zombie(daemon.id()));
    }

    #[test]
    fn zram_rollback_requires_recorded_identity_before_any_command() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        let runner = ScriptedRunner::new(Vec::new());

        assert!(!rollback_zram_tier(&runner, &paths, "/dev/zram7", true));
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn down_refuses_ambiguous_live_identity_without_running_a_command() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), false);
        let mut retargeted = devices[0].clone();
        retargeted.dev_t = "43:9".into();
        retargeted.sysfs_dev_t = "43:9".into();
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_swap_snapshots([Ok("Filename Type Size Used Priority\n")]);
        set_live_device_snapshots([Ok(vec![devices[0].clone(), retargeted])]);
        let runner = ScriptedRunner::new(Vec::new());

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "ambiguous device identity must refuse the whole teardown",
        );

        assert!(error.to_string().contains("ambiguous"), "{error}");
        assert!(runner.calls().is_empty());
        assert!(paths.lifecycle_binding_file.exists());
        assert!(!process_is_gone_or_zombie(daemon.id()));
    }

    #[test]
    fn down_refuses_foreign_managed_swap_without_running_a_command() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), false);
        const FOREIGN_SWAP: &str =
            "Filename Type Size Used Priority\n/dev/nbd1 partition 1024 0 100\n";
        let _seams = ParentSeams::install(FOREIGN_SWAP, 0);
        set_swap_snapshots([Ok(FOREIGN_SWAP)]);
        set_live_device_snapshots([Ok(devices)]);
        let runner = ScriptedRunner::new(Vec::new());

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "foreign active swap must refuse the whole teardown",
        );

        assert!(
            error.to_string().contains("foreign or ambiguous"),
            "{error}"
        );
        assert!(runner.calls().is_empty());
        assert!(paths.lifecycle_binding_file.exists());
        assert!(!process_is_gone_or_zombie(daemon.id()));
    }

    #[test]
    fn down_refuses_mismatched_runtime_record_without_running_a_command() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), false);
        remove_runtime_file(&paths.swap_dev_file);
        write_nbd_path_record(&paths.swap_dev_file, "/dev/nbd9")
            .expect("write retargeted NBD path record fixture");
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        set_swap_snapshots([Ok("Filename Type Size Used Priority\n")]);
        set_live_device_snapshots([Ok(devices)]);
        let runner = ScriptedRunner::new(Vec::new());

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "a mismatched runtime record must refuse the whole teardown",
        );

        assert!(
            error.to_string().contains("runtime record differs"),
            "{error}"
        );
        assert!(runner.calls().is_empty());
        assert!(paths.lifecycle_binding_file.exists());
        assert!(!process_is_gone_or_zombie(daemon.id()));
    }

    #[test]
    fn down_refuses_unreadable_or_malformed_swap_snapshot_before_mutation() {
        for snapshot in [
            Err("manufactured unreadable /proc/swaps"),
            Ok("Filename Type Size Used Priority\n/dev/nbd0 partition broken 0 100\n"),
        ] {
            let fixture = TestDir::new();
            let paths = RuntimePaths::under(&fixture.path);
            fs::create_dir_all(&paths.runtime_dir)
                .unwrap_or_else(|error| panic!("create evidence runtime: {error}"));
            fs::write(&paths.pid_file, "preserve-me")
                .unwrap_or_else(|error| panic!("write evidence fixture: {error}"));
            let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
            set_swap_snapshots([snapshot]);
            let runner = ScriptedRunner::new(Vec::new());

            assert!(down_with_runtime(&runner, &paths).is_err());
            assert!(runner.calls().is_empty());
            assert!(paths.pid_file.exists());
        }
    }

    #[test]
    fn active_zero_use_swapoff_failure_preserves_backend_and_evidence() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), false);
        const ACTIVE_ZERO: &str =
            "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n";
        let _seams = ParentSeams::install(ACTIVE_ZERO, 0);
        set_swap_snapshots([Ok(ACTIVE_ZERO), Ok(ACTIVE_ZERO)]);
        set_live_device_snapshots([Ok(devices.clone()), Ok(devices)]);
        let runner = ScriptedRunner::new(vec![(
            "swapoff -- /dev/nbd0".into(),
            Err(CascadeError::Shell {
                cmd: "swapoff -- /dev/nbd0".into(),
                msg: "manufactured swapoff refusal".into(),
            }),
        )]);

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "zero-use active swap is still active and must preserve its backend on failure",
        );

        assert!(error.to_string().contains("backend preserved"), "{error}");
        assert_eq!(runner.calls(), vec!["swapoff -- /dev/nbd0"]);
        assert!(paths.lifecycle_binding_file.exists());
        assert!(paths.swap_dev_file.exists());
        assert!(!process_is_gone_or_zombie(daemon.id()));
    }

    #[test]
    fn uncertain_swapoff_absence_proof_preserves_backend_and_evidence() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), false);
        const ACTIVE_ZERO: &str =
            "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n";
        let _seams = ParentSeams::install(ACTIVE_ZERO, 0);
        set_swap_snapshots([
            Ok(ACTIVE_ZERO),
            Ok(ACTIVE_ZERO),
            Err("manufactured unreadable fresh absence proof"),
        ]);
        set_live_device_snapshots([Ok(devices.clone()), Ok(devices)]);
        let runner = ScriptedRunner::new(vec![(
            "swapoff -- /dev/nbd0".into(),
            Err(CascadeError::Shell {
                cmd: "swapoff -- /dev/nbd0".into(),
                msg: "No such file or directory".into(),
            }),
        )]);

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "an unreadable fresh snapshot must not convert swapoff uncertainty into success",
        );

        assert!(error.to_string().contains("backend preserved"), "{error}");
        assert_eq!(runner.calls(), vec!["swapoff -- /dev/nbd0"]);
        assert!(paths.lifecycle_binding_file.exists());
        assert!(paths.swap_dev_file.exists());
        assert!(!process_is_gone_or_zombie(daemon.id()));
    }

    #[test]
    // TestName: nbd_disconnect_refusal_stops_before_daemon_stop
    fn nbd_disconnect_refusal_stops_before_daemon_stop() {
        struct DisconnectRefusingExecutor {
            calls: RefCell<Vec<String>>,
        }

        impl NbdLifecycleExecutor for DisconnectRefusingExecutor {
            fn swapoff(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
                self.calls
                    .borrow_mut()
                    .push(format!("swapoff {}", device.path));
                Ok(())
            }

            fn reset_zram(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
                self.calls
                    .borrow_mut()
                    .push(format!("reset-zram {}", device.path));
                Ok(())
            }

            fn disconnect_nbd(&self, device: &BoundDeviceIdentity) -> Result<(), CascadeError> {
                self.calls
                    .borrow_mut()
                    .push(format!("disconnect-nbd {}", device.path));
                Err(CascadeError::Shell {
                    cmd: format!("nbd-client -d {}", device.path),
                    msg: "manufactured NBD disconnect refusal".into(),
                })
            }

            fn stop_daemon(&self) -> Result<(), CascadeError> {
                self.calls.borrow_mut().push("stop-daemon".into());
                Ok(())
            }
        }

        let nbd = bound_device_fixture("/dev/nbd0", ManagedDeviceKind::Nbd);
        let binding = lifecycle_binding_fixture(vec![nbd.clone()]);
        let swaps =
            parse_proc_swaps("Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n")
                .expect("strict swap fixture");
        let plan = plan_nbd_lifecycle(&binding, &swaps, &[nbd]).expect("authorized lifecycle plan");
        let executor = DisconnectRefusingExecutor {
            calls: RefCell::new(Vec::new()),
        };

        let error = error_from(
            execute_nbd_lifecycle_plan(&plan, &executor),
            "a refused NBD disconnect must stop the injected plan",
        );
        assert!(
            error
                .to_string()
                .contains("manufactured NBD disconnect refusal")
        );
        assert_eq!(
            executor.calls.into_inner(),
            vec!["swapoff /dev/nbd0", "disconnect-nbd /dev/nbd0"]
        );
    }

    #[test]
    fn down_with_runtime_preserves_swapoff_first_and_cleans_temp_state() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        fs::create_dir_all(
            paths.forensics_markers[0]
                .parent()
                .unwrap_or_else(|| panic!("test marker has no parent")),
        )
        .unwrap_or_else(|error| panic!("create test forensics: {error}"));
        fs::write(&paths.forensics_markers[0], "armed")
            .unwrap_or_else(|error| panic!("write test marker: {error}"));
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), true);
        let _seams = ParentSeams::install(
            "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n/dev/zram0 partition 1024 0 200\n",
            0,
        );
        install_two_tier_down_snapshots(&devices, true, true);
        let runner = ScriptedRunner::new(vec![
            ("swapoff -- /dev/nbd0".into(), Ok(String::new())),
            ("swapoff -- /dev/zram0".into(), Ok(String::new())),
            ("zramctl -r /dev/zram0".into(), Ok(String::new())),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
        ]);

        down_with_runtime(&runner, &paths)
            .unwrap_or_else(|error| panic!("isolated cascade down: {error}"));
        assert_eq!(
            runner.calls(),
            vec![
                "swapoff -- /dev/nbd0",
                "swapoff -- /dev/zram0",
                "zramctl -r /dev/zram0",
                "nbd-client -d /dev/nbd0",
            ]
        );
        assert!(!paths.swap_dev_file.exists());
        assert!(!paths.zram_dev_file.exists());
        assert!(!paths.forensics_markers[0].exists());
    }

    #[test]
    fn down_removes_status_that_a_subsequent_cli_could_treat_as_current() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), true);
        let cache_status = paths.cache_status_file.clone();
        let supervisor_status = paths.supervisor_status_file.clone();
        fs::write(&supervisor_status, r#"{"control_state":"HEALTHY"}"#)
            .unwrap_or_else(|error| panic!("write supervisor status: {error}"));
        let _seams = ParentSeams::install(
            "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n/dev/zram0 partition 1024 0 200\n",
            0,
        );
        install_two_tier_down_snapshots(&devices, true, true);
        let runner = ScriptedRunner::new(vec![
            ("swapoff -- /dev/nbd0".into(), Ok(String::new())),
            ("swapoff -- /dev/zram0".into(), Ok(String::new())),
            ("zramctl -r /dev/zram0".into(), Ok(String::new())),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
        ]);

        down_with_runtime(&runner, &paths)
            .unwrap_or_else(|error| panic!("isolated cascade down: {error}"));

        assert!(
            !cache_status.exists(),
            "down left current-looking cache status"
        );
        assert!(
            !supervisor_status.exists(),
            "down left current-looking supervisor status"
        );
    }

    #[test]
    fn down_refuses_unverifiable_daemon_and_preserves_runtime_evidence() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        fs::write(&paths.swap_dev_file, "/dev/nbd0")
            .unwrap_or_else(|error| panic!("write test swap record: {error}"));
        fs::write(&paths.zram_dev_file, "/dev/zram0")
            .unwrap_or_else(|error| panic!("write test zram record: {error}"));
        fs::write(&paths.pid_file, "not-a-pid")
            .unwrap_or_else(|error| panic!("write malformed PID: {error}"));
        fs::write(&paths.cache_status_file, r#"{"cache_state":"ACTIVE"}"#)
            .unwrap_or_else(|error| panic!("write cache status: {error}"));
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        let runner = ScriptedRunner::new(vec![
            ("swapoff -- /dev/nbd0".into(), Ok(String::new())),
            ("swapoff -- /dev/zram0".into(), Ok(String::new())),
            ("zramctl -r /dev/zram0".into(), Ok(String::new())),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
        ]);

        assert!(down_with_runtime(&runner, &paths).is_err());
        assert!(
            paths.pid_file.exists(),
            "PID evidence was deleted after refusal"
        );
        assert!(
            paths.cache_status_file.exists(),
            "cache evidence was deleted after daemon-stop refusal"
        );
    }

    #[test]
    // TestName: nbd_disconnect_refusal_preserves_daemon_and_runtime_evidence
    fn nbd_disconnect_refusal_preserves_daemon_and_runtime_evidence() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), true);
        arm_forensics_at(&paths);
        let _seams = ParentSeams::install(
            "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n",
            0,
        );
        install_two_tier_down_snapshots(&devices, true, false);
        let runner = ScriptedRunner::new(vec![
            ("swapoff -- /dev/nbd0".into(), Ok(String::new())),
            ("zramctl -r /dev/zram0".into(), Ok(String::new())),
            (
                "nbd-client -d /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "nbd-client -d /dev/nbd0".into(),
                    msg: "manufactured NBD disconnect refusal".into(),
                }),
            ),
        ]);

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "an NBD disconnect refusal must refuse down",
        );
        assert_eq!(
            error.to_string(),
            "command `nbd-client -d /dev/nbd0` failed: manufactured NBD disconnect refusal"
        );
        assert!(
            !process_is_gone_or_zombie(daemon.id()),
            "daemon was stopped"
        );
        assert!(paths.swap_dev_file.exists(), "swap record was deleted");
        assert!(paths.zram_dev_file.exists(), "zram record was deleted");
        assert!(paths.pid_file.exists(), "PID evidence was deleted");
        assert!(
            paths.cache_status_file.exists(),
            "cache evidence was deleted"
        );
        assert!(
            paths.forensics_markers[0].exists(),
            "forensics marker was deleted"
        );
        assert_eq!(
            runner.calls(),
            vec![
                "swapoff -- /dev/nbd0",
                "zramctl -r /dev/zram0",
                "nbd-client -d /dev/nbd0",
            ]
        );
    }

    #[test]
    // TestName: daemon_stop_refuses_mismatched_instance_identity
    fn daemon_stop_refuses_mismatched_instance_identity() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        fs::write(&paths.pid_file, daemon.id().to_string())
            .unwrap_or_else(|error| panic!("write fixture daemon PID: {error}"));
        write_daemon_identity_status(&paths, daemon.id(), "999999-999999");
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);

        let error = error_from(
            stop_daemon_gracefully_at(&paths, Duration::from_millis(100)),
            "a stale daemon instance identity must refuse TERM",
        );
        assert_eq!(
            error.to_string(),
            "daemon stop refused: cache status does not identify the exact current ramsharedd process; runtime evidence retained"
        );
        assert!(
            !process_is_gone_or_zombie(daemon.id()),
            "stale PID received TERM"
        );
        assert!(
            paths.pid_file.exists(),
            "PID evidence was deleted after refusal"
        );
        assert!(
            paths.cache_status_file.exists(),
            "cache identity evidence was deleted after refusal"
        );
    }

    #[test]
    // TestName: daemon_stop_refuses_stale_incomplete_or_unhealthy_cache_status
    fn daemon_stop_refuses_stale_incomplete_or_unhealthy_cache_status() {
        for cache_status in [
            r#"{"schema_version":1,"daemon_instance_id":"placeholder"}"#.to_string(),
            r#"{"schema_version":1,"daemon_instance_id":"placeholder","written_at_unix_ms":0,"ok":true,"origin_state":"READY","cache_state":"ACTIVE","logical_capacity_kib":1,"vram_cached_kib":0,"gpu_headroom_kib":null,"ssd_origin_written_kib":1,"cache_fallback_reads":0,"cache_invalidations":0,"cache_releases":0,"cache_target_kib":0}"#.to_string(),
            format!(
                r#"{{"schema_version":1,"daemon_instance_id":"placeholder","written_at_unix_ms":{},"ok":false,"origin_state":"FAILED","cache_state":"STUCK","logical_capacity_kib":1,"vram_cached_kib":0,"gpu_headroom_kib":null,"ssd_origin_written_kib":1,"cache_fallback_reads":0,"cache_invalidations":0,"cache_releases":0,"cache_target_kib":0}}"#,
                unix_time_ms().unwrap_or_default()
            ),
        ] {
            let fixture = TestDir::new();
            let daemon = spawn_fixture_daemon(&fixture, "exit 0");
            let paths = RuntimePaths::under(&fixture.path);
            fs::create_dir_all(&paths.runtime_dir)
                .unwrap_or_else(|error| panic!("create test runtime: {error}"));
            fs::write(&paths.pid_file, daemon.id().to_string())
                .unwrap_or_else(|error| panic!("write fixture daemon PID: {error}"));
            let daemon_instance_id = daemon_instance_id_from_pid(daemon.id())
                .unwrap_or_else(|| panic!("derive fixture daemon identity"));
            fs::write(
                &paths.cache_status_file,
                cache_status.replace("placeholder", &daemon_instance_id),
            )
            .unwrap_or_else(|error| panic!("write cache status: {error}"));
            let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);

            assert!(stop_daemon_gracefully_at(&paths, Duration::from_millis(100)).is_err());
            assert!(
                !process_is_gone_or_zombie(daemon.id()),
                "unhealthy cache status authorized TERM"
            );
            assert!(paths.pid_file.exists(), "PID evidence was deleted");
            assert!(paths.cache_status_file.exists(), "cache evidence was deleted");
        }
    }

    #[test]
    // TestName: daemon_signal_revalidates_instance_immediately_before_term
    fn daemon_signal_revalidates_instance_immediately_before_term() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        write_daemon_identity_status(
            &paths,
            daemon.id(),
            &daemon_instance_id_from_pid(daemon.id())
                .unwrap_or_else(|| panic!("derive fixture daemon identity")),
        );
        let expected = "replacement-instance".to_string();

        let error = error_from(
            signal_verified_daemon(&paths, daemon.id(), &expected),
            "a replacement daemon instance must not receive TERM",
        );
        assert!(
            error
                .to_string()
                .contains("changed before pidfd acquisition"),
            "{error}"
        );
        assert!(
            !process_is_gone_or_zombie(daemon.id()),
            "replacement process received TERM"
        );
    }

    #[test]
    // TestName: pidfd_signal_remains_bound_after_validated_target_is_replaced
    fn pidfd_signal_remains_bound_after_validated_target_is_replaced() {
        let fixture = TestDir::new();
        let mut daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        let expected = daemon_instance_id_from_pid(daemon.id())
            .unwrap_or_else(|| panic!("derive fixture daemon identity"));
        fs::write(&paths.pid_file, daemon.id().to_string())
            .unwrap_or_else(|error| panic!("write test daemon PID: {error}"));
        write_daemon_identity_status(&paths, daemon.id(), &expected);

        let target = open_verified_daemon_pidfd(&paths, daemon.id(), &expected)
            .unwrap_or_else(|error| panic!("validated daemon must produce pidfd: {error}"));
        daemon
            .0
            .kill()
            .unwrap_or_else(|error| panic!("stop validated fixture daemon: {error}"));
        daemon
            .0
            .wait()
            .unwrap_or_else(|error| panic!("reap validated fixture daemon: {error}"));
        let mut foreign = Command::new("/bin/sleep")
            .arg("10")
            .spawn()
            .unwrap_or_else(|error| panic!("spawn foreign replacement fixture: {error}"));
        let foreign_instance = daemon_instance_id_from_pid(foreign.id())
            .unwrap_or_else(|| panic!("derive foreign process identity"));
        fs::write(&paths.pid_file, foreign.id().to_string())
            .unwrap_or_else(|error| panic!("write foreign replacement PID: {error}"));
        write_daemon_identity_status(&paths, foreign.id(), &foreign_instance);

        let error = error_from(
            signal_daemon_pidfd(&target),
            "a pidfd for the exited validated target must not signal a replacement",
        );
        assert!(error.to_string().contains("pidfd TERM"), "{error}");
        assert!(
            foreign
                .try_wait()
                .unwrap_or_else(|error| panic!("inspect foreign replacement: {error}"))
                .is_none(),
            "foreign process received TERM"
        );
        foreign
            .kill()
            .unwrap_or_else(|error| panic!("stop foreign replacement: {error}"));
        foreign
            .wait()
            .unwrap_or_else(|error| panic!("reap foreign replacement: {error}"));
    }

    #[test]
    // TestName: daemon_stop_allows_matching_instance_identity
    fn daemon_stop_allows_matching_instance_identity() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "exit 0");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        fs::write(&paths.pid_file, daemon.id().to_string())
            .unwrap_or_else(|error| panic!("write fixture daemon PID: {error}"));
        let daemon_instance_id = daemon_instance_id_from_pid(daemon.id())
            .unwrap_or_else(|| panic!("derive fixture daemon identity"));
        write_daemon_identity_status(&paths, daemon.id(), &daemon_instance_id);
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);

        stop_daemon_gracefully_at(&paths, Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("matching daemon identity must stop: {error}"));
        assert!(
            process_is_gone_or_zombie(daemon.id()),
            "matching daemon did not stop"
        );
        assert!(
            !paths.pid_file.exists(),
            "stopped daemon retained PID record"
        );
    }

    #[test]
    fn down_timeout_preserves_runtime_evidence_and_refuses_success() {
        let fixture = TestDir::new();
        let daemon = spawn_fixture_daemon(&fixture, "");
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        let devices = seal_runtime_lifecycle(&paths, daemon.id(), true);
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", 0);
        install_two_tier_down_snapshots(&devices, false, false);
        assert!(
            read_swaps().expect("strict injected swaps").is_empty(),
            "timeout fixture must start swap-free"
        );
        struct RecordingRunner(RefCell<Vec<String>>);

        impl CommandRunner for RecordingRunner {
            fn run(&self, command: &str, args: &[&str]) -> Result<String, CascadeError> {
                self.0.borrow_mut().push(command_label(command, args));
                Ok(String::new())
            }
        }

        let runner = RecordingRunner(RefCell::new(Vec::new()));

        let error = error_from(
            down_with_runtime(&runner, &paths),
            "a daemon that misses the stop deadline must fail down",
        );
        assert!(error.to_string().contains("did not exit"), "{error}");
        assert!(
            paths.pid_file.exists(),
            "PID evidence was deleted after timeout"
        );
        assert!(
            paths.cache_status_file.exists(),
            "cache evidence was deleted after daemon-stop timeout"
        );
        assert_eq!(
            runner.0.borrow().as_slice(),
            ["zramctl -r /dev/zram0", "nbd-client -d /dev/nbd0"]
        );
    }

    #[test]
    fn daemon_pid_requires_positive_pid_and_exact_identity() {
        assert!(daemon_pid_matches(42, "ramsharedd\n"));
        assert!(!daemon_pid_matches(0, "ramsharedd\n"));
        assert!(!daemon_pid_matches(42, "ramsharedd-helper\n"));
        assert!(!daemon_pid_matches(42, "other\n"));
    }

    #[test]
    fn parent_seams_install_reserves_capacity() {
        let shell_responses = 100;
        let _seams = ParentSeams::install("Filename Type Size Used Priority\n", shell_responses);
        SH_SCRIPT.with(|queue| {
            let q = queue.borrow();
            assert!(
                q.capacity() >= shell_responses,
                "SH_SCRIPT queue capacity {} is less than reserved {}",
                q.capacity(),
                shell_responses
            );
        });
    }
}
