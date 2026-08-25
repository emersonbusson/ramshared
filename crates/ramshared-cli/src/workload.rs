use rustix::fs::{
    AtFlags, FileType, FlockOperation, Mode, OFlags, flock, fstat, fsync, mkdirat, open, openat,
    renameat, statat,
};
use rustix::process::geteuid;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::bounded_process;
use crate::supervisor::{SupervisorActionResult, SupervisorActionStatus};

const GIB_BYTES: u64 = 1024 * 1024 * 1024;
const MIB_BYTES: u64 = 1024 * 1024;
const DEFAULT_LEDGER_DIR: &str = "/run/ramshared/admission";
const SAFE_MODE_FILE: &str = "/var/lib/ramshared/safe-mode.json";
const RESUME_LEASE_FILE: &str = "/run/ramshared/host-resume-lease.json";
const RECOVERY_PENDING_FILE: &str = "/var/lib/ramshared/recovery-pending.json";
const SUPERVISOR_STATUS_SCHEMA_VERSION: u32 = 3;
const SAFE_MODE_GATE_SCHEMA_VERSION: u32 = 1;
const RESUME_LEASE_SCHEMA_VERSION: u32 = 2;
const ADMISSION_STATE_SCHEMA_VERSION: u32 = 1;
pub(crate) const RESERVATION_LEDGER_SCHEMA_VERSION: u32 = 3;
const MAX_LEDGER_BYTES: u64 = 1024 * 1024;
const MAX_CONTROL_EVIDENCE_AGE_MS: u64 = 15_000;
const SCOPE_START_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const SCOPE_STATUS_QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const SCOPE_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(100);
static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_atomic_durable(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("atomic file parent missing")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".ramshared-{}.tmp", unique_nonce()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(contents)
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkloadBudget {
    pub control_plane_reserve_bytes: u64,
    pub memory_high_bytes: u64,
    pub memory_max_bytes: u64,
}

pub fn calculate_budget(
    memory_total_bytes: u64,
    _memory_available_bytes: u64,
) -> Result<WorkloadBudget, String> {
    if memory_total_bytes == 0 {
        return Err("invalid memory snapshot".into());
    }
    let reserve = (memory_total_bytes.div_ceil(4)).max(4 * GIB_BYTES);
    let memory_max = memory_total_bytes.saturating_sub(reserve);
    if memory_max < GIB_BYTES {
        return Err(format!(
            "less than 1 GiB remains after the {} MiB control-plane reserve",
            reserve >> 20
        ));
    }
    let high_gap = (memory_total_bytes.div_ceil(10)).max(GIB_BYTES);
    Ok(WorkloadBudget {
        control_plane_reserve_bytes: reserve,
        memory_high_bytes: memory_max.saturating_sub(high_gap),
        memory_max_bytes: memory_max,
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkloadClass {
    Interactive,
    Build,
    BrowserTest,
    Batch,
}

impl WorkloadClass {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "interactive" => Some(Self::Interactive),
            "build" => Some(Self::Build),
            "browser-test" => Some(Self::BrowserTest),
            "batch" => Some(Self::Batch),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Build => "build",
            Self::BrowserTest => "browser-test",
            Self::Batch => "batch",
        }
    }

    fn default_memory_mib(self) -> u64 {
        match self {
            Self::Interactive => 2 * 1024,
            Self::Build => 6 * 1024,
            Self::BrowserTest => 4 * 1024,
            Self::Batch => 8 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkloadRequest {
    pub class: WorkloadClass,
    pub memory_max_bytes: u64,
    pub command: Vec<String>,
}

pub fn parse_run_args(args: &[String]) -> Result<WorkloadRequest, String> {
    let mut class = None;
    let mut requested_mib = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--class" => {
                index += 1;
                class = Some(
                    args.get(index)
                        .and_then(|value| WorkloadClass::parse(value))
                        .ok_or("--class must be interactive|build|browser-test|batch")?,
                );
            }
            "--memory-max" => {
                index += 1;
                requested_mib = Some(
                    args.get(index)
                        .ok_or("--memory-max requires MiB")?
                        .parse::<u64>()
                        .ok()
                        .filter(|value| *value > 0)
                        .ok_or("--memory-max must be a positive MiB value")?,
                );
            }
            "--" => {
                let class = class.ok_or("--class is required")?;
                let command = args[index + 1..].to_vec();
                if command.is_empty() {
                    return Err("a command is required after --".into());
                }
                let memory_mib = requested_mib.unwrap_or_else(|| class.default_memory_mib());
                return Ok(WorkloadRequest {
                    class,
                    memory_max_bytes: memory_mib
                        .checked_mul(MIB_BYTES)
                        .ok_or("--memory-max overflow")?,
                    command,
                });
            }
            _ => {
                return Err(
                    "usage: ramshared run --class <class> [--memory-max MiB] -- <command>".into(),
                );
            }
        }
        index += 1;
    }
    Err("usage: ramshared run --class <class> [--memory-max MiB] -- <command>".into())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OwnerIdentity {
    pub boot_id: String,
    pub pid: u32,
    pub start_time: u64,
    pub nonce: String,
}

impl OwnerIdentity {
    pub(crate) fn current() -> Result<Self, String> {
        let pid = std::process::id();
        Ok(Self {
            boot_id: fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .map_err(|error| format!("read boot id: {error}"))?
                .trim()
                .to_string(),
            pid,
            start_time: process_start_time(pid).ok_or("read process start time")?,
            nonce: unique_nonce(),
        })
    }
}

fn process_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

fn unique_nonce() -> String {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}-{:x}", std::process::id(), time, sequence)
}

trait OwnerProbe {
    fn is_alive(&self, owner: &OwnerIdentity) -> bool;
}

struct SystemOwnerProbe;

impl OwnerProbe for SystemOwnerProbe {
    fn is_alive(&self, owner: &OwnerIdentity) -> bool {
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").ok();
        boot.as_deref().map(str::trim) == Some(owner.boot_id.as_str())
            && process_start_time(owner.pid) == Some(owner.start_time)
    }
}

fn unix_time_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))
}

fn path_is_present(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn valid_incident_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn valid_systemd_invocation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_distro(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn identity_is_current(identity: &OwnerIdentity, boot_id: &str, probe: &dyn OwnerProbe) -> bool {
    identity.boot_id == boot_id
        && identity.pid != 0
        && identity.start_time != 0
        && !identity.nonce.is_empty()
        && identity
            .nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        && probe.is_alive(identity)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct SupervisorStatus {
    schema_version: u32,
    control_state: String,
    healthy_samples: u64,
    action_results: Vec<SupervisorActionResult>,
    supervisor_identity: OwnerIdentity,
    written_at_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct AdmissionState {
    schema_version: u32,
    admission_open: bool,
    control_state: String,
    supervisor_identity: OwnerIdentity,
    written_at_unix_ms: u64,
}

fn read_current_supervisor_status_at(
    path: &Path,
    current: &OwnerIdentity,
    now_ms: u64,
    minimum_healthy_samples: u64,
    probe: &dyn OwnerProbe,
) -> Result<SupervisorStatus, String> {
    let text = fs::read_to_string(path)
        .map_err(|_| "supervisor health evidence is unavailable".to_string())?;
    let status: SupervisorStatus = serde_json::from_str(&text)
        .map_err(|_| "supervisor health evidence is malformed".to_string())?;
    if status.schema_version != SUPERVISOR_STATUS_SCHEMA_VERSION
        || status.written_at_unix_ms > now_ms
        || now_ms.saturating_sub(status.written_at_unix_ms) > MAX_CONTROL_EVIDENCE_AGE_MS
        || !identity_is_current(&status.supervisor_identity, &current.boot_id, probe)
        || status
            .action_results
            .iter()
            .any(|result| match result.status {
                SupervisorActionStatus::Succeeded => result.error.is_some(),
                SupervisorActionStatus::Failed => result.error.is_none(),
            })
    {
        return Err("supervisor health evidence is stale, foreign, or invalid".into());
    }
    if status.control_state != "HEALTHY" {
        return Err(format!(
            "workload admission is closed by control state {}",
            status.control_state
        ));
    }
    if status.healthy_samples < minimum_healthy_samples {
        return Err("supervisor health evidence has too few healthy samples".into());
    }
    Ok(status)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ResumeLease {
    schema_version: u32,
    boot_id: String,
    incident_id: String,
    distro: String,
    supervisor_identity: OwnerIdentity,
    issued_at_epoch_ms: u64,
    expires_at_epoch_ms: u64,
}

fn validate_resume_lease(
    lease: &ResumeLease,
    current: &OwnerIdentity,
    supervisor: &SupervisorStatus,
    now_ms: u64,
    probe: &dyn OwnerProbe,
) -> Result<(), String> {
    if lease.schema_version != RESUME_LEASE_SCHEMA_VERSION
        || lease.boot_id != current.boot_id
        || !valid_incident_id(&lease.incident_id)
        || !valid_distro(&lease.distro)
        || lease.supervisor_identity != supervisor.supervisor_identity
        || !identity_is_current(&lease.supervisor_identity, &current.boot_id, probe)
        || lease.issued_at_epoch_ms > now_ms
        || lease.expires_at_epoch_ms < now_ms
        || lease
            .expires_at_epoch_ms
            .saturating_sub(lease.issued_at_epoch_ms)
            > MAX_CONTROL_EVIDENCE_AGE_MS
        || now_ms.saturating_sub(lease.issued_at_epoch_ms) > MAX_CONTROL_EVIDENCE_AGE_MS
    {
        return Err("resume lease is stale, foreign, or invalid".into());
    }
    Ok(())
}

fn read_resume_lease_if_present(
    path: &Path,
    current: &OwnerIdentity,
    supervisor: &SupervisorStatus,
    now_ms: u64,
    probe: &dyn OwnerProbe,
) -> Result<Option<ResumeLease>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("resume lease is unavailable".into()),
    };
    let lease: ResumeLease =
        serde_json::from_str(&text).map_err(|_| "resume lease is malformed".to_string())?;
    validate_resume_lease(&lease, current, supervisor, now_ms, probe)?;
    Ok(Some(lease))
}

fn admission_state_allows_at(
    path: &Path,
    current: &OwnerIdentity,
    supervisor: &SupervisorStatus,
    now_ms: u64,
    probe: &dyn OwnerProbe,
) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|_| "admission state evidence is unavailable".to_string())?;
    let state: AdmissionState = serde_json::from_str(&text)
        .map_err(|_| "admission state evidence is malformed".to_string())?;
    if state.schema_version != ADMISSION_STATE_SCHEMA_VERSION
        || state.written_at_unix_ms > now_ms
        || now_ms.saturating_sub(state.written_at_unix_ms) > MAX_CONTROL_EVIDENCE_AGE_MS
        || state.supervisor_identity != supervisor.supervisor_identity
        || !identity_is_current(&state.supervisor_identity, &current.boot_id, probe)
    {
        return Err("admission state evidence is stale, foreign, or invalid".into());
    }
    if !state.admission_open || state.control_state != "HEALTHY" {
        return Err(format!(
            "admission state closes new workloads for control state {}",
            state.control_state
        ));
    }
    Ok(())
}

pub(crate) fn publish_admission_state_at(
    root: &Path,
    supervisor_identity: &OwnerIdentity,
    control_state: &str,
    written_at_unix_ms: u64,
) -> Result<(), String> {
    let paths = LedgerPaths::new(root);
    let lock = acquire_lock(&paths, supervisor_identity, &SystemOwnerProbe)?;
    let state = AdmissionState {
        schema_version: ADMISSION_STATE_SCHEMA_VERSION,
        admission_open: control_state == "HEALTHY",
        control_state: control_state.to_string(),
        supervisor_identity: supervisor_identity.clone(),
        written_at_unix_ms,
    };
    let contents = format!(
        "{}\n",
        serde_json::to_string(&state).map_err(|error| error.to_string())?
    );
    store_named_locked(&lock, "admission-state.json", contents.as_bytes())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Reservation {
    pub id: String,
    pub owner: OwnerIdentity,
    pub class: WorkloadClass,
    pub memory_bytes: u64,
    pub unit: String,
    pub invocation_id: Option<String>,
    pub issued_ordinal: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ReservationLedger {
    schema_version: u32,
    next_ordinal: u64,
    reservations: Vec<Reservation>,
}

struct LedgerPaths {
    root: PathBuf,
    #[cfg(test)]
    ledger: PathBuf,
    #[cfg(test)]
    transition: PathBuf,
    admission_state: PathBuf,
    #[cfg(test)]
    quarantine: PathBuf,
}

impl LedgerPaths {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            #[cfg(test)]
            ledger: root.join("reservations.json"),
            #[cfg(test)]
            transition: root.join("ledger-transition.lock"),
            admission_state: root.join("admission-state.json"),
            #[cfg(test)]
            quarantine: root.join("quarantine"),
        }
    }
}

struct LedgerLock {
    paths: LedgerPaths,
    root_file: File,
    root_identity: FileIdentity,
    transition_file: File,
    transition_identity: FileIdentity,
    quarantine_file: File,
    quarantine_identity: FileIdentity,
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        let _ = flock(&self.root_file, FlockOperation::Unlock);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl FileIdentity {
    fn from_stat(stat: &rustix::fs::Stat) -> Self {
        Self {
            device: stat.st_dev,
            inode: stat.st_ino,
        }
    }
}

fn open_directory_path(path: &Path, create_missing: bool) -> Result<File, String> {
    if path.as_os_str().is_empty() {
        return Err("ledger authority path is empty".into());
    }
    let start = if path.is_absolute() { "/" } else { "." };
    let mut directory = File::from(
        open(
            start,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| format!("open ledger authority anchor: {error}"))?,
    );
    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::ParentDir | Component::Prefix(_) => {
                return Err("ledger authority path contains an unsafe component".into());
            }
        };
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let opened = match openat(&directory, name, flags, Mode::empty()) {
            Ok(opened) => opened,
            Err(rustix::io::Errno::NOENT) if create_missing => {
                mkdirat(&directory, name, Mode::RWXU)
                    .map_err(|error| format!("create ledger authority component: {error}"))?;
                fsync(&directory).map_err(|error| {
                    format!("sync ledger authority parent after creation: {error}")
                })?;
                openat(&directory, name, flags, Mode::empty())
                    .map_err(|error| format!("open created ledger authority component: {error}"))?
            }
            Err(error) => {
                return Err(format!(
                    "open ledger authority component without following links: {error}"
                ));
            }
        };
        directory = File::from(opened);
    }
    Ok(directory)
}

fn validate_directory(file: &File, label: &str) -> Result<FileIdentity, String> {
    let stat = fstat(file).map_err(|error| format!("inspect {label}: {error}"))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != geteuid().as_raw()
        || stat.st_mode & 0o022 != 0
    {
        return Err(format!(
            "{label} must be a directory owned by the effective user without group/world write"
        ));
    }
    Ok(FileIdentity::from_stat(&stat))
}

fn validate_regular(file: &File, label: &str) -> Result<FileIdentity, String> {
    let stat = fstat(file).map_err(|error| format!("inspect {label}: {error}"))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != geteuid().as_raw()
        || stat.st_mode & 0o022 != 0
        || stat.st_nlink != 1
    {
        return Err(format!(
            "{label} must be a singly linked regular file owned by the effective user without group/world write"
        ));
    }
    Ok(FileIdentity::from_stat(&stat))
}

fn named_identity(
    directory: &File,
    name: &OsStr,
    expected_type: FileType,
    label: &str,
) -> Result<FileIdentity, String> {
    let stat = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| format!("inspect named {label}: {error}"))?;
    if FileType::from_raw_mode(stat.st_mode) != expected_type {
        return Err(format!("named {label} has an unsafe filesystem type"));
    }
    Ok(FileIdentity::from_stat(&stat))
}

fn open_or_create_regular(
    directory: &File,
    name: &OsStr,
    label: &str,
) -> Result<(File, bool), String> {
    let existing_flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let (opened, created) = match openat(directory, name, existing_flags, Mode::empty()) {
        Ok(opened) => (opened, false),
        Err(rustix::io::Errno::NOENT) => (
            openat(
                directory,
                name,
                existing_flags | OFlags::CREATE | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|error| format!("create {label}: {error}"))?,
            true,
        ),
        Err(error) => return Err(format!("open {label} without following links: {error}")),
    };
    let file = File::from(opened);
    validate_regular(&file, label)?;
    if created {
        fsync(directory).map_err(|error| format!("sync parent after creating {label}: {error}"))?;
    }
    Ok((file, created))
}

fn open_or_create_directory(
    directory: &File,
    name: &OsStr,
    label: &str,
) -> Result<(File, bool), String> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let (opened, created) = match openat(directory, name, flags, Mode::empty()) {
        Ok(opened) => (opened, false),
        Err(rustix::io::Errno::NOENT) => {
            mkdirat(directory, name, Mode::RWXU)
                .map_err(|error| format!("create {label}: {error}"))?;
            let opened = openat(directory, name, flags, Mode::empty())
                .map_err(|error| format!("open created {label}: {error}"))?;
            (opened, true)
        }
        Err(error) => return Err(format!("open {label} without following links: {error}")),
    };
    let file = File::from(opened);
    validate_directory(&file, label)?;
    if created {
        fsync(directory).map_err(|error| format!("sync parent after creating {label}: {error}"))?;
    }
    Ok((file, created))
}

impl LedgerLock {
    fn revalidate(&self) -> Result<(), String> {
        if validate_directory(&self.root_file, "ledger authority")? != self.root_identity {
            return Err("opened ledger directory authority identity changed".into());
        }
        let named_root = open_directory_path(&self.paths.root, false)?;
        if validate_directory(&named_root, "named ledger authority")? != self.root_identity {
            return Err("named ledger directory authority was replaced".into());
        }
        if validate_regular(&self.transition_file, "transition owner")? != self.transition_identity
            || named_identity(
                &self.root_file,
                OsStr::new("ledger-transition.lock"),
                FileType::RegularFile,
                "transition owner",
            )? != self.transition_identity
        {
            return Err("transition owner identity changed".into());
        }
        if validate_directory(&self.quarantine_file, "quarantine authority")?
            != self.quarantine_identity
            || named_identity(
                &self.root_file,
                OsStr::new("quarantine"),
                FileType::Directory,
                "quarantine authority",
            )? != self.quarantine_identity
        {
            return Err("quarantine authority identity changed".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TransitionOwnerRecord {
    schema_version: u32,
    owner: OwnerIdentity,
}

const TRANSITION_OWNER_SCHEMA_VERSION: u32 = 1;

fn transition_record(lock: &mut LedgerLock) -> Result<Option<TransitionOwnerRecord>, String> {
    lock.revalidate()?;
    lock.transition_file
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    (&mut lock.transition_file)
        .take(MAX_LEDGER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_LEDGER_BYTES {
        return Err("reservation ledger transition owner exceeds its bounded size".into());
    }
    lock.revalidate()?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let record: TransitionOwnerRecord = serde_json::from_slice(&bytes).map_err(|_| {
        "reservation ledger transition owner is malformed or from an unsupported protocol"
            .to_string()
    })?;
    if record.schema_version != TRANSITION_OWNER_SCHEMA_VERSION
        || record.owner.boot_id.is_empty()
        || record.owner.pid == 0
        || record.owner.start_time == 0
        || record.owner.nonce.is_empty()
        || !record
            .owner
            .nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(
            "reservation ledger transition owner is malformed or from an unsupported protocol"
                .into(),
        );
    }
    Ok(Some(record))
}

#[cfg(test)]
type TransitionRecoveryHook = Box<dyn Fn(&LedgerPaths)>;

#[cfg(test)]
thread_local! {
    static TRANSITION_RECOVERY_HOOK: std::cell::RefCell<Option<TransitionRecoveryHook>> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
fn invoke_transition_recovery_hook(paths: &LedgerPaths) {
    TRANSITION_RECOVERY_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook(paths);
        }
    });
}

#[cfg(not(test))]
fn invoke_transition_recovery_hook(_paths: &LedgerPaths) {}

fn write_transition_record(lock: &mut LedgerLock, owner: &OwnerIdentity) -> Result<(), String> {
    lock.revalidate()?;
    let record = TransitionOwnerRecord {
        schema_version: TRANSITION_OWNER_SCHEMA_VERSION,
        owner: owner.clone(),
    };
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    let result = (|| {
        lock.transition_file
            .set_len(0)
            .map_err(|error| error.to_string())?;
        lock.transition_file
            .seek(SeekFrom::Start(0))
            .map_err(|error| error.to_string())?;
        lock.transition_file
            .write_all(&bytes)
            .map_err(|error| error.to_string())?;
        lock.transition_file
            .sync_all()
            .map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = lock.transition_file.set_len(0);
        let _ = lock.transition_file.sync_all();
    }
    result?;
    lock.revalidate()
}

fn quarantine_transition_snapshot(lock: &mut LedgerLock) -> Result<(), String> {
    lock.revalidate().map_err(|_| {
        "transition owner changed during recovery; containment retained".to_string()
    })?;
    lock.transition_file
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("read transition owner for recovery: {error}"))?;
    let mut contents = Vec::new();
    (&mut lock.transition_file)
        .take(MAX_LEDGER_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|error| format!("read transition owner for recovery: {error}"))?;
    if contents.len() as u64 > MAX_LEDGER_BYTES {
        return Err("transition owner recovery evidence exceeds its bounded size".into());
    }
    let snapshot_name = format!("transition-owner-{}.json", unique_nonce());
    let snapshot = openat(
        &lock.quarantine_file,
        snapshot_name.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| format!("preserve transition owner before recovery: {error}"))?;
    let mut snapshot = File::from(snapshot);
    validate_regular(&snapshot, "transition quarantine snapshot")?;
    snapshot
        .write_all(&contents)
        .map_err(|error| format!("preserve transition owner before recovery: {error}"))?;
    snapshot
        .sync_all()
        .map_err(|error| format!("sync transition owner recovery evidence: {error}"))?;
    fsync(&lock.quarantine_file).map_err(|error| format!("sync quarantine directory: {error}"))?;
    lock.revalidate()
        .map_err(|_| "transition owner changed during recovery; containment retained".to_string())
}

fn acquire_lock(
    paths: &LedgerPaths,
    owner: &OwnerIdentity,
    probe: &dyn OwnerProbe,
) -> Result<LedgerLock, String> {
    let root_file = open_directory_path(&paths.root, true)?;
    let root_identity = validate_directory(&root_file, "ledger authority")?;
    match flock(&root_file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => {}
        Err(rustix::io::Errno::WOULDBLOCK) => {
            return Err("reservation ledger transition is busy or ambiguous".into());
        }
        Err(error) => {
            return Err(format!("lock reservation ledger transition: {error}"));
        }
    }
    let (transition_file, _) = open_or_create_regular(
        &root_file,
        OsStr::new("ledger-transition.lock"),
        "reservation ledger transition owner",
    )?;
    let transition_identity = validate_regular(&transition_file, "transition owner")?;
    let (quarantine_file, _) = open_or_create_directory(
        &root_file,
        OsStr::new("quarantine"),
        "reservation ledger quarantine",
    )?;
    let quarantine_identity = validate_directory(&quarantine_file, "quarantine authority")?;
    let mut lock = LedgerLock {
        paths: LedgerPaths::new(&paths.root),
        root_file,
        root_identity,
        transition_file,
        transition_identity,
        quarantine_file,
        quarantine_identity,
    };
    lock.revalidate()?;
    let existing = transition_record(&mut lock)?;
    if let Some(existing) = existing {
        // A successful advisory acquisition is the liveness authority for a
        // compliant owner: it cannot still be inside a transition.  A dead
        // or prior-boot record is crash evidence, so copy it before reuse.
        // Never rename or unlink the canonical path: the opened inode must
        // remain the inode named by the path throughout recovery.
        let stale_owner =
            existing.owner.boot_id != owner.boot_id || !probe.is_alive(&existing.owner);
        if stale_owner {
            quarantine_transition_snapshot(&mut lock)?;
        }
        invoke_transition_recovery_hook(paths);
        lock.revalidate().map_err(|_| {
            "transition owner changed during recovery; containment retained".to_string()
        })?;
    }
    write_transition_record(&mut lock, owner)?;
    lock.revalidate().map_err(|_| {
        "transition owner changed while claiming lock; containment retained".to_string()
    })?;
    Ok(lock)
}

fn validate_ledger(ledger: &ReservationLedger) -> Result<(), String> {
    if ledger.schema_version != RESERVATION_LEDGER_SCHEMA_VERSION {
        return Err(format!(
            "unsupported reservation ledger schema: {}",
            ledger.schema_version
        ));
    }
    if ledger.next_ordinal == 0
        || ledger.reservations.iter().any(|reservation| {
            reservation.issued_ordinal == 0 || reservation.issued_ordinal >= ledger.next_ordinal
        })
    {
        return Err("reservation ledger ordinal sequence is invalid".into());
    }
    let mut ordinals = ledger
        .reservations
        .iter()
        .map(|reservation| reservation.issued_ordinal)
        .collect::<Vec<_>>();
    ordinals.sort_unstable();
    if ordinals.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("reservation ledger contains duplicate issued ordinals".into());
    }
    Ok(())
}

fn load_ledger_locked(lock: &LedgerLock) -> Result<ReservationLedger, String> {
    lock.revalidate()?;
    let opened = openat(
        &lock.root_file,
        OsStr::new("reservations.json"),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::NOENT {
            "reservation ledger is unavailable".to_string()
        } else {
            format!("read reservation ledger: {error}")
        }
    })?;
    let mut file = File::from(opened);
    let identity = validate_regular(&file, "reservation ledger")?;
    if named_identity(
        &lock.root_file,
        OsStr::new("reservations.json"),
        FileType::RegularFile,
        "reservation ledger",
    )? != identity
    {
        return Err("reservation ledger identity changed before read".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_LEDGER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read reservation ledger: {error}"))?;
    if bytes.len() as u64 > MAX_LEDGER_BYTES {
        return Err("reservation ledger exceeds its bounded size".into());
    }
    if validate_regular(&file, "reservation ledger")? != identity
        || named_identity(
            &lock.root_file,
            OsStr::new("reservations.json"),
            FileType::RegularFile,
            "reservation ledger",
        )? != identity
    {
        return Err("reservation ledger identity changed during read".into());
    }
    lock.revalidate()?;
    let ledger: ReservationLedger = serde_json::from_slice(&bytes)
        .map_err(|_| "reservation ledger is malformed".to_string())?;
    validate_ledger(&ledger)?;
    Ok(ledger)
}

pub(crate) fn read_reservation_ledger(path: &Path) -> Result<Vec<Reservation>, String> {
    if path.file_name() != Some(OsStr::new("reservations.json")) {
        return Err("reservation ledger path is not canonical".into());
    }
    let root = path
        .parent()
        .ok_or("reservation ledger parent is missing")?;
    let paths = LedgerPaths::new(root);
    let owner = OwnerIdentity::current()?;
    let lock = acquire_lock(&paths, &owner, &SystemOwnerProbe)?;
    Ok(load_ledger_locked(&lock)?.reservations)
}

fn store_named_locked(lock: &LedgerLock, name: &str, contents: &[u8]) -> Result<(), String> {
    lock.revalidate()?;
    let temporary = format!(".{name}-{}.tmp", unique_nonce());
    let opened = openat(
        &lock.root_file,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| format!("create durable {name} temporary: {error}"))?;
    let mut file = File::from(opened);
    validate_regular(&file, "durable temporary file")?;
    let result = (|| {
        file.write_all(contents)
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        lock.revalidate()?;
        renameat(&lock.root_file, temporary.as_str(), &lock.root_file, name)
            .map_err(|error| error.to_string())?;
        fsync(&lock.root_file).map_err(|error| error.to_string())?;
        let named = named_identity(
            &lock.root_file,
            OsStr::new(name),
            FileType::RegularFile,
            "durable publication",
        )?;
        if validate_regular(&file, "durable publication")? != named {
            return Err("durable publication identity changed after rename".into());
        }
        lock.revalidate()
    })();
    if result.is_err() {
        let _ = rustix::fs::unlinkat(&lock.root_file, temporary.as_str(), AtFlags::empty());
    }
    result
}

fn store_ledger(lock: &LedgerLock, ledger: &ReservationLedger) -> Result<(), String> {
    validate_ledger(ledger)?;
    let mut bytes = serde_json::to_vec(ledger).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_LEDGER_BYTES {
        return Err("reservation ledger exceeds its bounded size".into());
    }
    store_named_locked(lock, "reservations.json", &bytes)
}

#[cfg(test)]
fn load_ledger(path: &Path) -> Result<ReservationLedger, String> {
    if path.file_name() != Some(OsStr::new("reservations.json")) {
        return Err("reservation ledger path is not canonical".into());
    }
    let root = path
        .parent()
        .ok_or("reservation ledger parent is missing")?;
    let paths = LedgerPaths::new(root);
    let owner = OwnerIdentity::current()?;
    let lock = acquire_lock(&paths, &owner, &SystemOwnerProbe)?;
    load_ledger_locked(&lock)
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn acquire_reservation_at(
    root: &Path,
    safe_mode: &Path,
    resume_lease: &Path,
    owner: OwnerIdentity,
    class: WorkloadClass,
    memory_bytes: u64,
    aggregate_max: u64,
    probe: &dyn OwnerProbe,
) -> Result<Reservation, String> {
    let _ = resume_lease;
    if path_is_present(safe_mode)? {
        return Err("safe mode closes new workload admission until recover --resume".into());
    }
    let paths = LedgerPaths::new(root);
    let lock = acquire_lock(&paths, &owner, probe)?;
    let mut ledger = load_ledger_locked(&lock)?;
    ledger
        .reservations
        .retain(|item| probe.is_alive(&item.owner));
    let reserved = ledger
        .reservations
        .iter()
        .try_fold(0_u64, |sum, item| sum.checked_add(item.memory_bytes))
        .ok_or("reservation sum overflow")?;
    if memory_bytes == 0 || reserved.saturating_add(memory_bytes) > aggregate_max {
        return Err(format!(
            "aggregate workload budget exceeded: reserved={} requested={} max={aggregate_max}",
            reserved, memory_bytes
        ));
    }
    let id = unique_nonce();
    let issued_ordinal = ledger.next_ordinal;
    ledger.next_ordinal = ledger
        .next_ordinal
        .checked_add(1)
        .ok_or("reservation ordinal sequence exhausted")?;
    let reservation = Reservation {
        id: id.clone(),
        owner,
        class,
        memory_bytes,
        unit: format!("ramshared-{}-{}.scope", class.as_str(), id),
        invocation_id: None,
        issued_ordinal,
    };
    ledger.reservations.push(reservation.clone());
    store_ledger(&lock, &ledger)?;
    Ok(reservation)
}

#[allow(clippy::too_many_arguments)]
fn admit_reservation_at(
    root: &Path,
    safe_mode: &Path,
    resume_lease: &Path,
    supervisor_state: &Path,
    owner: OwnerIdentity,
    class: WorkloadClass,
    memory_bytes: u64,
    aggregate_max: u64,
    probe: &dyn OwnerProbe,
) -> Result<(Reservation, LedgerLock), String> {
    let paths = LedgerPaths::new(root);
    let lock = acquire_lock(&paths, &owner, probe)?;
    if path_is_present(safe_mode)? {
        return Err("safe mode closes new workload admission until recover --resume".into());
    }
    let now_ms = unix_time_ms()?;
    let supervisor = read_current_supervisor_status_at(supervisor_state, &owner, now_ms, 0, probe)?;
    admission_state_allows_at(&paths.admission_state, &owner, &supervisor, now_ms, probe)?;
    read_resume_lease_if_present(resume_lease, &owner, &supervisor, now_ms, probe)?
        .ok_or("boot-ephemeral resume lease is required for workload admission")?;
    let mut ledger = load_ledger_locked(&lock)?;
    ledger
        .reservations
        .retain(|item| probe.is_alive(&item.owner));
    let reserved = ledger
        .reservations
        .iter()
        .try_fold(0_u64, |sum, item| sum.checked_add(item.memory_bytes))
        .ok_or("reservation sum overflow")?;
    if memory_bytes == 0 || reserved.saturating_add(memory_bytes) > aggregate_max {
        return Err(format!(
            "aggregate workload budget exceeded: reserved={} requested={} max={aggregate_max}",
            reserved, memory_bytes
        ));
    }
    let id = unique_nonce();
    let issued_ordinal = ledger.next_ordinal;
    ledger.next_ordinal = ledger
        .next_ordinal
        .checked_add(1)
        .ok_or("reservation ordinal sequence exhausted")?;
    let reservation = Reservation {
        id: id.clone(),
        owner,
        class,
        memory_bytes,
        unit: format!("ramshared-{}-{}.scope", class.as_str(), id),
        invocation_id: None,
        issued_ordinal,
    };
    ledger.reservations.push(reservation.clone());
    store_ledger(&lock, &ledger)?;
    Ok((reservation, lock))
}

fn release_reservation_at(
    root: &Path,
    reservation: &Reservation,
    probe: &dyn OwnerProbe,
) -> Result<bool, String> {
    let paths = LedgerPaths::new(root);
    let lock = acquire_lock(&paths, &reservation.owner, probe)?;
    let mut ledger = load_ledger_locked(&lock)?;
    let Some(index) = ledger
        .reservations
        .iter()
        .position(|item| item.id == reservation.id)
    else {
        return Ok(false);
    };
    if ledger.reservations[index].owner != reservation.owner {
        return Err("reservation owner identity mismatch".into());
    }
    ledger.reservations.remove(index);
    store_ledger(&lock, &ledger)?;
    Ok(true)
}

fn bind_reservation_invocation_while_locked(
    lock: &LedgerLock,
    reservation: &Reservation,
    invocation_id: &str,
) -> Result<Reservation, String> {
    if !valid_systemd_invocation_id(invocation_id) {
        return Err("managed scope InvocationID is not canonical".into());
    }
    let mut ledger = load_ledger_locked(lock)?;
    let entry = ledger
        .reservations
        .iter_mut()
        .find(|item| item.id == reservation.id)
        .ok_or("managed scope reservation disappeared before identity binding")?;
    if entry.owner != reservation.owner || entry.unit != reservation.unit {
        return Err("managed scope reservation identity changed before binding".into());
    }
    match entry.invocation_id.as_deref() {
        Some(existing) if existing != invocation_id => {
            return Err("managed scope reservation already has another InvocationID".into());
        }
        Some(_) => {}
        None => entry.invocation_id = Some(invocation_id.to_string()),
    }
    let bound = entry.clone();
    store_ledger(lock, &ledger)?;
    Ok(bound)
}

fn parse_meminfo(text: &str) -> Result<u64, String> {
    let total_kib = text.lines().find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        (name == "MemTotal")
            .then(|| rest.split_whitespace().next()?.parse::<u64>().ok())
            .flatten()
    });
    total_kib
        .map(|value| value.saturating_mul(1024))
        .ok_or_else(|| "MemTotal missing from /proc/meminfo".into())
}

fn build_runner_args(unit: &str, request: &WorkloadRequest, budget: WorkloadBudget) -> Vec<String> {
    vec![
        "--scope".into(),
        "--quiet".into(),
        "--unit".into(),
        unit.trim_end_matches(".scope").into(),
        "--property".into(),
        "Slice=ramshared-workloads.slice".into(),
        "--property".into(),
        format!(
            "MemoryHigh={}",
            request.memory_max_bytes.min(budget.memory_high_bytes)
        ),
        "--property".into(),
        format!("MemoryMax={}", request.memory_max_bytes),
        "--property".into(),
        "TasksMax=8192".into(),
        "--setenv".into(),
        format!("RAMSHARED_WORKLOAD_CLASS={}", request.class.as_str()),
        "--".into(),
    ]
}

enum ScopeCompletion {
    /// The exact scope reached a terminal state. The inner result preserves
    /// the workload's exit/error semantics and is safe to release from the
    /// admission ledger.
    Terminal(Result<Option<i32>, String>),
    /// The scope may still exist but its exact terminal state could not be
    /// proven. Keep the reservation and its forensic evidence fail-closed.
    UnsafeContainment(String),
}

enum ScopeStart {
    Acknowledged(String),
    /// A control acknowledgement or its identity snapshot is inconclusive.
    /// The unit may exist, so ledger evidence must survive for containment.
    UnsafeContainment(String),
}

#[derive(Debug)]
struct ScopeStatus {
    id: String,
    invocation_id: String,
    load_state: String,
    active_state: String,
    exec_main_code: String,
    exec_main_status: i32,
}

impl ScopeStatus {
    #[cfg(test)]
    fn terminal_success(unit: &str) -> Self {
        Self {
            id: unit.to_string(),
            invocation_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            load_state: "loaded".into(),
            active_state: "inactive".into(),
            exec_main_code: "exited".into(),
            exec_main_status: 0,
        }
    }

    #[cfg(test)]
    fn active(unit: &str) -> Self {
        Self {
            id: unit.to_string(),
            invocation_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            load_state: "loaded".into(),
            active_state: "active".into(),
            exec_main_code: "exited".into(),
            exec_main_status: 0,
        }
    }

    fn terminal_exit_status(&self) -> Result<Option<Option<i32>>, String> {
        if self.load_state != "loaded" {
            return Err(format!("scope load state is {}", self.load_state));
        }
        match self.active_state.as_str() {
            "active" | "activating" | "reloading" | "deactivating" => Ok(None),
            "inactive" | "failed" => match self.exec_main_code.as_str() {
                "exited" => Ok(Some(Some(self.exec_main_status))),
                "killed" | "dumped" => Ok(Some(None)),
                code => Err(format!("scope terminal execution code is {code}")),
            },
            state => Err(format!("scope active state is {state}")),
        }
    }
}

trait ScopeStatusAdapter: Send + Sync {
    fn query(&self, unit: &str) -> Result<ScopeStatus, String>;
}

struct SystemScopeStatusAdapter;

impl ScopeStatusAdapter for SystemScopeStatusAdapter {
    fn query(&self, unit: &str) -> Result<ScopeStatus, String> {
        let output = run_scope_status_command(unit)?;
        parse_scope_status(unit, &output)
    }
}

fn run_scope_status_command(unit: &str) -> Result<String, String> {
    let mut status = Command::new("systemctl");
    status
        .arg("show")
        .arg("--property=Id")
        .arg("--property=InvocationID")
        .arg("--property=LoadState")
        .arg("--property=ActiveState")
        .arg("--property=ExecMainCode")
        .arg("--property=ExecMainStatus")
        .arg(unit);
    let output = bounded_process::run_capture_command(
        &mut status,
        "exact scope status query",
        SCOPE_STATUS_QUERY_TIMEOUT,
        bounded_process::DEFAULT_OUTPUT_LIMIT,
        |_| {},
    )
    .map_err(|error| format!("exact scope status query custody failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "exact scope status query failed: {}",
            output
                .status
                .code()
                .map_or_else(|| "signal".into(), |code| code.to_string())
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("exact scope status query returned non-UTF-8 output: {error}"))
}

fn parse_scope_status(unit: &str, output: &str) -> Result<ScopeStatus, String> {
    let mut id = None;
    let mut invocation_id = None;
    let mut load_state = None;
    let mut active_state = None;
    let mut exec_main_code = None;
    let mut exec_main_status = None;
    for line in output.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or("malformed exact scope status response")?;
        let slot = match name {
            "Id" => &mut id,
            "InvocationID" => &mut invocation_id,
            "LoadState" => &mut load_state,
            "ActiveState" => &mut active_state,
            "ExecMainCode" => &mut exec_main_code,
            "ExecMainStatus" => &mut exec_main_status,
            _ => return Err(format!("unexpected exact scope status field {name}")),
        };
        if slot.replace(value.to_string()).is_some() {
            return Err(format!("duplicate exact scope status field {name}"));
        }
    }
    let id = id.ok_or("exact scope status omitted Id")?;
    if id != unit {
        return Err(format!(
            "exact scope status referred to {id}, expected {unit}"
        ));
    }
    let invocation_id = invocation_id
        .filter(|value| valid_systemd_invocation_id(value))
        .ok_or("exact scope status omitted InvocationID")?;
    Ok(ScopeStatus {
        id,
        invocation_id,
        load_state: load_state.ok_or("exact scope status omitted LoadState")?,
        active_state: active_state.ok_or("exact scope status omitted ActiveState")?,
        exec_main_code: exec_main_code.ok_or("exact scope status omitted ExecMainCode")?,
        exec_main_status: exec_main_status
            .ok_or("exact scope status omitted ExecMainStatus")?
            .parse()
            .map_err(|error| format!("invalid exact scope ExecMainStatus: {error}"))?,
    })
}

fn wait_for_scope_start_acknowledgement(
    acknowledgement: &mut Child,
) -> Result<std::process::ExitStatus, String> {
    bounded_process::wait_grouped_child(
        acknowledgement,
        "managed scope acknowledgement",
        SCOPE_START_ACK_TIMEOUT,
    )
    .map_err(|error| format!("managed scope acknowledgement bounded timeout: {error}"))
}

trait ScopeExecution {
    /// Prove `systemd-run` accepted the scope while admission remains closed
    /// to concurrent transitions. This is deliberately not the workload
    /// lifetime: systemd owns that lifetime after the acknowledgement.
    fn acknowledge_start(&mut self) -> ScopeStart;

    fn wait_for_completion(&mut self) -> ScopeCompletion;
}

trait ScopeRunner {
    fn launch(
        &self,
        unit: &str,
        runner_args: &[String],
        command: &[String],
    ) -> Result<Box<dyn ScopeExecution>, String>;
}

struct SystemScopeRunner;

struct SystemScopeExecution {
    acknowledgement: Child,
    unit: String,
    status: Arc<dyn ScopeStatusAdapter>,
    invocation_id: Option<String>,
}

impl SystemScopeExecution {
    fn with_status_adapter(
        acknowledgement: Child,
        unit: String,
        status: Arc<dyn ScopeStatusAdapter>,
    ) -> Self {
        Self {
            acknowledgement,
            unit,
            status,
            invocation_id: None,
        }
    }
}

impl ScopeExecution for SystemScopeExecution {
    fn acknowledge_start(&mut self) -> ScopeStart {
        let status = match wait_for_scope_start_acknowledgement(&mut self.acknowledgement) {
            Ok(status) => status,
            Err(error) => {
                return ScopeStart::UnsafeContainment(format!(
                    "managed scope acknowledgement is unknown; reservation retained: {error}"
                ));
            }
        };
        if !status.success() {
            return ScopeStart::UnsafeContainment(format!(
                "managed scope acknowledgement failed; reservation retained: {}",
                status
                    .code()
                    .map_or_else(|| "signal".into(), |code| code.to_string())
            ));
        }
        let scope_status = match self.status.query(&self.unit) {
            Ok(scope_status) => scope_status,
            Err(error) => {
                return ScopeStart::UnsafeContainment(format!(
                    "managed scope identity is unknown; reservation retained: {error}"
                ));
            }
        };
        if scope_status.id != self.unit || scope_status.invocation_id.is_empty() {
            return ScopeStart::UnsafeContainment(format!(
                "managed scope identity is not bound to {}; reservation retained",
                self.unit
            ));
        }
        self.invocation_id = Some(scope_status.invocation_id.clone());
        ScopeStart::Acknowledged(scope_status.invocation_id)
    }

    fn wait_for_completion(&mut self) -> ScopeCompletion {
        loop {
            let status = match self.status.query(&self.unit) {
                Ok(status) => status,
                Err(error) => {
                    return ScopeCompletion::UnsafeContainment(format!(
                        "exact terminal state for {} is unknown; reservation retained: {error}",
                        self.unit
                    ));
                }
            };
            if status.id != self.unit {
                return ScopeCompletion::UnsafeContainment(format!(
                    "exact terminal state changed from {} to {}; reservation retained",
                    self.unit, status.id
                ));
            }
            match &self.invocation_id {
                Some(expected) if expected != &status.invocation_id => {
                    return ScopeCompletion::UnsafeContainment(format!(
                        "scope {} invocation identity changed; reservation retained",
                        self.unit
                    ));
                }
                Some(_) => {}
                None => self.invocation_id = Some(status.invocation_id.clone()),
            }
            match status.terminal_exit_status() {
                Ok(Some(exit_status)) => return ScopeCompletion::Terminal(Ok(exit_status)),
                Ok(None) => std::thread::sleep(SCOPE_STATUS_POLL_INTERVAL),
                Err(error) => {
                    return ScopeCompletion::UnsafeContainment(format!(
                        "exact terminal state for {} is invalid; reservation retained: {error}",
                        self.unit
                    ));
                }
            }
        }
    }
}

impl ScopeRunner for SystemScopeRunner {
    fn launch(
        &self,
        unit: &str,
        runner_args: &[String],
        command: &[String],
    ) -> Result<Box<dyn ScopeExecution>, String> {
        let terminator = runner_args
            .iter()
            .position(|argument| argument == "--")
            .ok_or("managed scope arguments omit the command terminator")?;
        let mut systemd_run = Command::new("systemd-run");
        systemd_run
            .args(&runner_args[..terminator])
            .arg("--no-block")
            .arg("--")
            .args(command);
        let acknowledgement = bounded_process::configure_process_group(&mut systemd_run)
            .spawn()
            .map_err(|error| format!("failed to start managed systemd scope: {error}"))?;
        Ok(Box::new(SystemScopeExecution::with_status_adapter(
            acknowledgement,
            unit.to_string(),
            Arc::new(SystemScopeStatusAdapter),
        )))
    }
}

#[allow(clippy::too_many_arguments)]
fn run_with(
    request: WorkloadRequest,
    meminfo: &str,
    runner: &dyn ScopeRunner,
    ledger_root: &Path,
    safe_mode: &Path,
    resume_lease: &Path,
    supervisor_state: &Path,
) -> Result<(), String> {
    let owner = OwnerIdentity::current()?;
    let budget = calculate_budget(parse_meminfo(meminfo)?, 0)?;
    let (mut reservation, admission_lock) = admit_reservation_at(
        ledger_root,
        safe_mode,
        resume_lease,
        supervisor_state,
        owner,
        request.class,
        request.memory_max_bytes,
        budget.memory_max_bytes,
        &SystemOwnerProbe,
    )?;
    let runner_args = build_runner_args(&reservation.unit, &request, budget);
    let mut admission_lock = Some(admission_lock);
    let (result, release_reservation) = match runner.launch(
        &reservation.unit,
        &runner_args,
        &request.command,
    ) {
        Ok(mut execution) => match execution.acknowledge_start() {
            ScopeStart::Acknowledged(invocation_id) => {
                let Some(lock) = admission_lock.as_ref() else {
                    return Err(
                        "managed scope identity lock disappeared before binding; reservation retained"
                            .into(),
                    );
                };
                reservation = match bind_reservation_invocation_while_locked(
                    lock,
                    &reservation,
                    &invocation_id,
                ) {
                    Ok(reservation) => reservation,
                    Err(error) => {
                        return Err(format!(
                            "managed scope identity was not persisted; reservation retained: {error}"
                        ));
                    }
                };
                drop(admission_lock.take());
                match execution.wait_for_completion() {
                    ScopeCompletion::Terminal(result) => (result, true),
                    ScopeCompletion::UnsafeContainment(error) => (Err(error), false),
                }
            }
            ScopeStart::UnsafeContainment(error) => (Err(error), false),
        },
        Err(error) => (Err(error), true),
    };
    let result = match result {
        Ok(Some(0)) => Ok(()),
        Ok(Some(code)) => Err(format!("managed workload exited with status {code}")),
        Ok(None) => Err("managed workload exited with a signal".into()),
        Err(error) => Err(error),
    };
    drop(admission_lock);
    if release_reservation {
        let release = release_reservation_at(ledger_root, &reservation, &SystemOwnerProbe);
        result.and(release.map(|_| ()))
    } else {
        result
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let request = parse_run_args(args)?;
    let meminfo = fs::read_to_string("/proc/meminfo").map_err(|error| error.to_string())?;
    run_with(
        request,
        &meminfo,
        &SystemScopeRunner,
        Path::new(DEFAULT_LEDGER_DIR),
        Path::new(SAFE_MODE_FILE),
        Path::new(RESUME_LEASE_FILE),
        Path::new("/run/ramshared/supervisor-state.json"),
    )
}

pub fn session(args: &[String]) -> Result<(), String> {
    let mut session_args = args.to_vec();
    if !session_args.iter().any(|item| item == "--") {
        session_args.push("--".into());
        session_args.push(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
    }
    run(&session_args)
}

fn recover_status_at(safe_mode: &Path, resume_lease: &Path) -> Result<String, String> {
    serde_json::to_string(&serde_json::json!({
        "schema_version": 4,
        "safe_mode": safe_mode.is_file(),
        "resume_lease": resume_lease.is_file()
    }))
    .map_err(|error| error.to_string())
}

pub fn recover_status() -> Result<String, String> {
    recover_status_at(Path::new(SAFE_MODE_FILE), Path::new(RESUME_LEASE_FILE))
}

#[derive(Debug, Deserialize)]
struct GuestSafeModeGate {
    schema_version: u32,
    incident_id: String,
    distro: String,
    boot_id: String,
}

#[derive(Debug, Deserialize)]
struct HostSafeModeGate {
    schema_version: u32,
    incident_id: String,
    distro: String,
    prior_boot_id: String,
    reason: String,
    timestamp_utc: String,
}

struct RecoveryPaths {
    supervisor_state: PathBuf,
    boot_id: PathBuf,
    guest_gate: PathBuf,
    host_gate_dir: PathBuf,
    pending: PathBuf,
    resume_lease: PathBuf,
}

fn recover_resume_at(paths: &RecoveryPaths) -> Result<(), String> {
    let current = OwnerIdentity::current()?;
    let boot_id = fs::read_to_string(&paths.boot_id).map_err(|error| error.to_string())?;
    let boot_id = boot_id.trim();
    if boot_id != current.boot_id {
        return Err("recovery boot identity is not current".into());
    }
    let now_ms = unix_time_ms()?;
    let supervisor = read_current_supervisor_status_at(
        &paths.supervisor_state,
        &current,
        now_ms,
        60,
        &SystemOwnerProbe,
    )
    .map_err(|error| {
        if error == "supervisor health evidence is unavailable"
            || error == "supervisor health evidence is malformed"
        {
            error
        } else {
            "recover requires 60 consecutive current healthy supervisor samples".to_string()
        }
    })?;
    let guest_gate_text = fs::read_to_string(&paths.guest_gate)
        .map_err(|_| "guest safe-mode marker is unavailable".to_string())?;
    let guest_gate: GuestSafeModeGate = serde_json::from_str(&guest_gate_text)
        .map_err(|_| "guest safe-mode marker is malformed".to_string())?;
    if guest_gate.schema_version != SAFE_MODE_GATE_SCHEMA_VERSION
        || !valid_incident_id(&guest_gate.incident_id)
        || !valid_distro(&guest_gate.distro)
    {
        return Err("guest safe-mode identity is invalid".into());
    }
    if guest_gate.boot_id != boot_id {
        return Err("safe-mode marker does not belong to the current boot".into());
    }
    let host_gate_path = paths
        .host_gate_dir
        .join(format!("{}.json", guest_gate.distro));
    let pending_path = &paths.pending;
    let host_gate = match fs::read_to_string(&host_gate_path) {
        Ok(text) => Some(
            serde_json::from_str::<HostSafeModeGate>(&text)
                .map_err(|_| "Windows safe-mode gate is malformed".to_string())?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("Windows safe-mode gate is unavailable".into()),
    };
    let pending = match fs::read_to_string(pending_path) {
        Ok(text) => Some(
            serde_json::from_str::<ResumeLease>(&text)
                .map_err(|_| "recovery transaction is malformed".to_string())?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("recovery transaction is unavailable".into()),
    };
    let new_lease = || ResumeLease {
        schema_version: RESUME_LEASE_SCHEMA_VERSION,
        boot_id: boot_id.to_string(),
        incident_id: guest_gate.incident_id.clone(),
        distro: guest_gate.distro.clone(),
        supervisor_identity: supervisor.supervisor_identity.clone(),
        issued_at_epoch_ms: now_ms,
        expires_at_epoch_ms: now_ms.saturating_add(MAX_CONTROL_EVIDENCE_AGE_MS),
    };
    if let Some(record) = host_gate.as_ref() {
        if record.schema_version != SAFE_MODE_GATE_SCHEMA_VERSION
            || record.incident_id != guest_gate.incident_id
            || record.distro != guest_gate.distro
            || record.prior_boot_id.is_empty()
            || record.prior_boot_id == boot_id
            || record.reason.is_empty()
            || record.timestamp_utc.is_empty()
        {
            return Err("Windows and guest safe-mode identities do not match".into());
        }
        let transaction = new_lease();
        write_atomic_durable(
            pending_path,
            format!(
                "{}\n",
                serde_json::to_string(&transaction).map_err(|error| error.to_string())?
            )
            .as_bytes(),
        )?;
    } else {
        let Some(transaction) = pending.as_ref() else {
            return Err(
                "matching Windows safe-mode gate or recovery transaction is unavailable".into(),
            );
        };
        validate_resume_lease(
            transaction,
            &current,
            &supervisor,
            now_ms,
            &SystemOwnerProbe,
        )?;
        if transaction.incident_id != guest_gate.incident_id
            || transaction.distro != guest_gate.distro
            || transaction.boot_id != boot_id
        {
            return Err("recovery transaction identity is invalid".into());
        }
    }
    let lease = new_lease();
    write_atomic_durable(
        &paths.resume_lease,
        format!(
            "{}\n",
            serde_json::to_string(&lease).map_err(|error| error.to_string())?
        )
        .as_bytes(),
    )?;
    if host_gate.is_some() {
        fs::remove_file(&host_gate_path)
            .map_err(|error| format!("remove matching Windows safe-mode gate: {error}"))?;
    }
    fs::remove_file(&paths.guest_gate)
        .map_err(|error| format!("remove guest safe-mode gate: {error}"))?;
    fs::remove_file(pending_path).map_err(|error| error.to_string())
}

pub fn recover_resume() -> Result<(), String> {
    recover_resume_at(&RecoveryPaths {
        supervisor_state: PathBuf::from("/run/ramshared/supervisor-state.json"),
        boot_id: PathBuf::from("/proc/sys/kernel/random/boot_id"),
        guest_gate: PathBuf::from(SAFE_MODE_FILE),
        host_gate_dir: PathBuf::from("/mnt/c/ProgramData/RamShared/safe-mode"),
        pending: PathBuf::from(RECOVERY_PENDING_FILE),
        resume_lease: PathBuf::from(RESUME_LEASE_FILE),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::cell::RefCell;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
    };
    use std::time::Duration;

    const GIB: u64 = 1024 * 1024 * 1024;
    const FIXTURE_INVOCATION_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    static TEMP: AtomicUsize = AtomicUsize::new(0);

    struct FakeProbe {
        dead_pid: Option<u32>,
    }

    struct FakeRunner {
        result: Result<Option<i32>, String>,
        calls: RefCell<Vec<(Vec<String>, Vec<String>)>>,
    }

    struct TransitionBlockingRunner {
        ledger_root: PathBuf,
        supervisor: OwnerIdentity,
        calls: RefCell<usize>,
    }

    struct FakeExecution {
        result: Result<Option<i32>, String>,
    }

    struct TransitionBlockingExecution {
        ledger_root: PathBuf,
        supervisor: OwnerIdentity,
    }

    struct StartPublishingRunner {
        ledger_root: PathBuf,
        supervisor: OwnerIdentity,
        start_calls: RefCell<usize>,
        wait_calls: Arc<AtomicUsize>,
    }

    struct StartPublishingExecution {
        ledger_root: PathBuf,
        supervisor: OwnerIdentity,
        wait_calls: Arc<AtomicUsize>,
    }

    struct BlockingScopeStatusAdapter {
        state: Mutex<BlockingScopeStatus>,
        changed: Condvar,
    }

    struct BlockingScopeStatus {
        terminal_poll_started: bool,
        terminal: bool,
        queries: usize,
        units: Vec<String>,
    }

    struct SystemExecutionRunner {
        status: Arc<dyn ScopeStatusAdapter>,
        launches: Arc<AtomicUsize>,
    }

    struct ChangingScopeIdentityAdapter {
        queries: AtomicUsize,
    }

    struct ErroringScopeStatusAdapter {
        queries: AtomicUsize,
    }

    impl ScopeRunner for FakeRunner {
        fn launch(
            &self,
            _unit: &str,
            runner_args: &[String],
            command: &[String],
        ) -> Result<Box<dyn ScopeExecution>, String> {
            self.calls
                .borrow_mut()
                .push((runner_args.to_vec(), command.to_vec()));
            Ok(Box::new(FakeExecution {
                result: self.result.clone(),
            }))
        }
    }

    impl ScopeExecution for FakeExecution {
        fn acknowledge_start(&mut self) -> ScopeStart {
            ScopeStart::Acknowledged(FIXTURE_INVOCATION_ID.into())
        }

        fn wait_for_completion(&mut self) -> ScopeCompletion {
            ScopeCompletion::Terminal(self.result.clone())
        }
    }

    impl ScopeRunner for TransitionBlockingRunner {
        fn launch(
            &self,
            _unit: &str,
            _runner_args: &[String],
            _command: &[String],
        ) -> Result<Box<dyn ScopeExecution>, String> {
            *self.calls.borrow_mut() += 1;
            Ok(Box::new(TransitionBlockingExecution {
                ledger_root: self.ledger_root.clone(),
                supervisor: self.supervisor.clone(),
            }))
        }
    }

    impl ScopeExecution for TransitionBlockingExecution {
        fn acknowledge_start(&mut self) -> ScopeStart {
            assert!(
                LedgerPaths::new(&self.ledger_root).transition.exists(),
                "workload start must retain the admission transition guard"
            );
            let error = publish_admission_state_at(
                &self.ledger_root,
                &self.supervisor,
                "GUARDED",
                now_ms(),
            )
            .unwrap_err();
            assert!(
                error.contains("transition is busy"),
                "a close-admission transition interleaved with workload start: {error}"
            );
            ScopeStart::Acknowledged(FIXTURE_INVOCATION_ID.into())
        }

        fn wait_for_completion(&mut self) -> ScopeCompletion {
            ScopeCompletion::Terminal(Ok(Some(0)))
        }
    }

    impl ScopeRunner for StartPublishingRunner {
        fn launch(
            &self,
            _unit: &str,
            _runner_args: &[String],
            _command: &[String],
        ) -> Result<Box<dyn ScopeExecution>, String> {
            *self.start_calls.borrow_mut() += 1;
            Ok(Box::new(StartPublishingExecution {
                ledger_root: self.ledger_root.clone(),
                supervisor: self.supervisor.clone(),
                wait_calls: self.wait_calls.clone(),
            }))
        }
    }

    impl ScopeStatusAdapter for BlockingScopeStatusAdapter {
        fn query(&self, unit: &str) -> Result<ScopeStatus, String> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "fixture status lock poisoned")?;
            state.queries += 1;
            state.units.push(unit.to_string());
            self.changed.notify_all();
            if state.queries == 1 {
                return Ok(ScopeStatus::active(unit));
            }
            state.terminal_poll_started = true;
            self.changed.notify_all();
            while !state.terminal {
                state = self
                    .changed
                    .wait(state)
                    .map_err(|_| "fixture status lock poisoned")?;
            }
            Ok(ScopeStatus::terminal_success(unit))
        }
    }

    impl ScopeStatusAdapter for ChangingScopeIdentityAdapter {
        fn query(&self, unit: &str) -> Result<ScopeStatus, String> {
            if self.queries.fetch_add(1, AtomicOrdering::SeqCst) == 0 {
                return Ok(ScopeStatus::active(unit));
            }
            Ok(ScopeStatus {
                id: unit.to_string(),
                invocation_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                load_state: "loaded".into(),
                active_state: "inactive".into(),
                exec_main_code: "exited".into(),
                exec_main_status: 0,
            })
        }
    }

    impl ScopeStatusAdapter for ErroringScopeStatusAdapter {
        fn query(&self, unit: &str) -> Result<ScopeStatus, String> {
            if self.queries.fetch_add(1, AtomicOrdering::SeqCst) == 0 {
                Ok(ScopeStatus::active(unit))
            } else {
                Err("fixture bounded status query failed".into())
            }
        }
    }

    impl BlockingScopeStatusAdapter {
        fn wait_until_terminal_poll(&self) {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .changed
                .wait_timeout_while(state, Duration::from_secs(1), |state| {
                    !state.terminal_poll_started
                })
                .unwrap();
            assert!(
                state.terminal_poll_started,
                "SystemScopeExecution never started its terminal status poll"
            );
        }

        fn allow_terminal(&self) {
            let mut state = self.state.lock().unwrap();
            state.terminal = true;
            self.changed.notify_all();
        }
    }

    impl ScopeRunner for SystemExecutionRunner {
        fn launch(
            &self,
            unit: &str,
            runner_args: &[String],
            command: &[String],
        ) -> Result<Box<dyn ScopeExecution>, String> {
            self.launches.fetch_add(1, AtomicOrdering::SeqCst);
            assert!(
                !runner_args.iter().any(|argument| argument == "--collect"),
                "terminal status evidence requires systemd to retain the exact scope"
            );
            assert_eq!(command, [String::from("/bin/true")]);
            let mut command = Command::new("/bin/true");
            let acknowledgement = bounded_process::configure_process_group(&mut command)
                .spawn()
                .map_err(|error| error.to_string())?;
            Ok(Box::new(SystemScopeExecution::with_status_adapter(
                acknowledgement,
                unit.to_string(),
                self.status.clone(),
            )))
        }
    }

    impl ScopeExecution for StartPublishingExecution {
        fn acknowledge_start(&mut self) -> ScopeStart {
            assert!(
                LedgerPaths::new(&self.ledger_root).transition.exists(),
                "launch acknowledgement must remain serialized with admission"
            );
            ScopeStart::Acknowledged(FIXTURE_INVOCATION_ID.into())
        }

        fn wait_for_completion(&mut self) -> ScopeCompletion {
            self.wait_calls.fetch_add(1, AtomicOrdering::SeqCst);
            let reservations = load_ledger(&LedgerPaths::new(&self.ledger_root).ledger)
                .unwrap_or_else(|error| panic!("read identity-bound ledger: {error}"))
                .reservations;
            assert_eq!(reservations.len(), 1);
            assert_eq!(
                reservations[0].invocation_id.as_deref(),
                Some(FIXTURE_INVOCATION_ID),
                "InvocationID must be durable before the admission transition is released"
            );
            match publish_admission_state_at(
                &self.ledger_root,
                &self.supervisor,
                "GUARDED",
                now_ms(),
            ) {
                Ok(()) => ScopeCompletion::Terminal(Ok(Some(0))),
                Err(error) => ScopeCompletion::Terminal(Err(format!(
                    "supervisor transition remained blocked by workload: {error}"
                ))),
            }
        }
    }
    impl OwnerProbe for FakeProbe {
        fn is_alive(&self, owner: &OwnerIdentity) -> bool {
            self.dead_pid != Some(owner.pid)
        }
    }
    fn owner(pid: u32, start_time: u64) -> OwnerIdentity {
        OwnerIdentity {
            boot_id: "boot-a".into(),
            pid,
            start_time,
            nonce: unique_nonce(),
        }
    }
    fn fixture() -> PathBuf {
        let parent = if Path::new("/dev/shm").is_dir() {
            PathBuf::from("/dev/shm")
        } else {
            std::env::temp_dir()
        };
        for _ in 0..1024 {
            let number = TEMP.fetch_add(1, AtomicOrdering::SeqCst);
            let path = parent.join(format!("ramshared-ledger-{}-{number}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    fs::write(
                        path.join("reservations.json"),
                        serde_json::to_vec(&ReservationLedger {
                            schema_version: RESERVATION_LEDGER_SCHEMA_VERSION,
                            next_ordinal: 1,
                            reservations: Vec::new(),
                        })
                        .unwrap(),
                    )
                    .unwrap();
                    return path;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create isolated workload fixture: {error}"),
            }
        }
        panic!("exhausted isolated workload fixture names");
    }

    #[test]
    // TestName: workload_fixture_roots_are_unique_and_avoid_shared_disk_sync
    fn workload_fixture_roots_are_unique_and_avoid_shared_disk_sync() {
        let first = fixture();
        let second = fixture();
        assert_ne!(first, second);
        if Path::new("/dev/shm").is_dir() {
            assert_eq!(first.parent(), Some(Path::new("/dev/shm")));
            assert_eq!(second.parent(), Some(Path::new("/dev/shm")));
        }
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn healthy_supervisor_status(
        owner: &OwnerIdentity,
        written_at_unix_ms: u64,
    ) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 3,
            "control_state": "HEALTHY",
            "healthy_samples": 60,
            "action_results": [],
            "supervisor_identity": owner,
            "written_at_unix_ms": written_at_unix_ms,
        })
    }

    fn current_healthy_supervisor_status() -> serde_json::Value {
        let owner = OwnerIdentity::current().unwrap();
        healthy_supervisor_status(&owner, now_ms())
    }

    fn valid_resume_lease(owner: &OwnerIdentity) -> serde_json::Value {
        let now = now_ms();
        serde_json::json!({
            "schema_version": 2,
            "boot_id": owner.boot_id,
            "incident_id": "0123456789abcdef0123456789abcdef",
            "distro": "Ubuntu-24.04",
            "supervisor_identity": owner,
            "issued_at_epoch_ms": now,
            "expires_at_epoch_ms": now + 15_000,
        })
    }

    fn publish_open_admission_state(root: &Path, owner: &OwnerIdentity) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("reservations.json"),
            serde_json::to_vec(&ReservationLedger {
                schema_version: RESERVATION_LEDGER_SCHEMA_VERSION,
                next_ordinal: 1,
                reservations: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();
        publish_admission_state_at(root, owner, "HEALTHY", now_ms())
            .unwrap_or_else(|error| panic!("publish open admission state: {error}"));
    }

    fn run_isolated_admission_fixture() {
        let root = fixture();
        let owner = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&owner, now_ms()).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("lease.json"),
            valid_resume_lease(&owner).to_string(),
        )
        .unwrap();
        let ledger_root = root.join("ledger");
        publish_open_admission_state(&ledger_root, &owner);
        let runner = FakeRunner {
            result: Ok(Some(0)),
            calls: RefCell::new(Vec::new()),
        };
        run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &ledger_root,
            &root.join("safe"),
            &root.join("lease.json"),
            &root.join("supervisor.json"),
        )
        .unwrap();
        assert_eq!(runner.calls.borrow().len(), 1);
        assert!(
            load_ledger(&LedgerPaths::new(&ledger_root).ledger)
                .unwrap()
                .reservations
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: workload_parallel_admission_fixtures_stay_durable_and_isolated
    fn workload_parallel_admission_fixtures_stay_durable_and_isolated() {
        std::thread::scope(|scope| {
            let workers = (0..8)
                .map(|_| scope.spawn(run_isolated_admission_fixture))
                .collect::<Vec<_>>();
            for worker in workers {
                worker.join().unwrap();
            }
        });
    }

    #[test]
    fn aggregate_reservations_share_one_limit() {
        let root = fixture();
        let probe = FakeProbe { dead_pid: None };
        acquire_reservation_at(
            &root,
            &root.join("safe"),
            &root.join("lease"),
            owner(10, 1),
            WorkloadClass::Build,
            6,
            10,
            &probe,
        )
        .unwrap();
        assert!(
            acquire_reservation_at(
                &root,
                &root.join("safe"),
                &root.join("lease"),
                owner(11, 1),
                WorkloadClass::Batch,
                5,
                10,
                &probe,
            )
            .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reservation_release_is_idempotent() {
        let root = fixture();
        let probe = FakeProbe { dead_pid: None };
        let item = acquire_reservation_at(
            &root,
            &root.join("safe"),
            &root.join("lease"),
            owner(10, 1),
            WorkloadClass::Build,
            6,
            10,
            &probe,
        )
        .unwrap();
        assert!(release_reservation_at(&root, &item, &probe).unwrap());
        assert!(!release_reservation_at(&root, &item, &probe).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: invalid_reservation_ledger_refuses_admission_without_launch_or_rewrite
    fn invalid_reservation_ledger_refuses_admission_without_launch_or_rewrite() {
        for (name, contents) in [
            ("missing", None),
            ("malformed", Some("not-json")),
            (
                "unsupported",
                Some(r#"{"schema_version":999,"reservations":[]}"#),
            ),
        ] {
            let root = fixture();
            let owner = OwnerIdentity::current().unwrap();
            let ledger_root = root.join("ledger");
            let ledger_path = ledger_root.join("reservations.json");
            fs::create_dir_all(&ledger_root).unwrap();
            fs::write(
                root.join("supervisor.json"),
                format!("{}\n", healthy_supervisor_status(&owner, now_ms())),
            )
            .unwrap();
            fs::write(root.join("lease"), valid_resume_lease(&owner).to_string()).unwrap();
            publish_admission_state_at(&ledger_root, &owner, "HEALTHY", now_ms()).unwrap();
            if let Some(contents) = contents {
                fs::write(&ledger_path, contents).unwrap();
            }
            let before = fs::read(&ledger_path).ok();
            let runner = FakeRunner {
                result: Ok(Some(0)),
                calls: RefCell::new(Vec::new()),
            };
            let error = run_with(
                WorkloadRequest {
                    class: WorkloadClass::Interactive,
                    memory_max_bytes: GIB,
                    command: vec!["/bin/true".into()],
                },
                "MemTotal: 16777216 kB\n",
                &runner,
                &ledger_root,
                &root.join("safe"),
                &root.join("lease"),
                &root.join("supervisor.json"),
            )
            .unwrap_err();
            assert!(
                error.contains("reservation ledger"),
                "{name} ledger returned the wrong refusal: {error}"
            );
            assert!(
                runner.calls.borrow().is_empty(),
                "{name} ledger launched a workload before refusal"
            );
            assert_eq!(
                fs::read(&ledger_path).ok(),
                before,
                "{name} ledger was rewritten during refusal"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    // TestName: unsupported_reservation_ledger_refuses_release_without_rewrite
    fn unsupported_reservation_ledger_refuses_release_without_rewrite() {
        let root = fixture();
        let reservation = Reservation {
            id: "fixture-reservation".into(),
            owner: owner(10, 1),
            class: WorkloadClass::Batch,
            memory_bytes: 1,
            unit: "ramshared-batch-fixture.scope".into(),
            invocation_id: None,
            issued_ordinal: 1,
        };
        let contents = serde_json::json!({
            "schema_version": 999,
            "next_ordinal": 2,
            "reservations": [{
                "id": reservation.id.clone(),
                "owner": reservation.owner.clone(),
                "class": "batch",
                "memory_bytes": reservation.memory_bytes,
                "unit": reservation.unit.clone(),
                "invocation_id": serde_json::Value::Null,
                "issued_ordinal": reservation.issued_ordinal,
            }],
        })
        .to_string();
        let ledger_path = root.join("reservations.json");
        fs::write(&ledger_path, &contents).unwrap();
        let error =
            release_reservation_at(&root, &reservation, &FakeProbe { dead_pid: None }).unwrap_err();
        assert!(
            error.contains("unsupported reservation ledger schema"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&ledger_path).unwrap(), contents);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_dead_stale_lock_is_recovered_exactly_once() {
        let root = fixture();
        let paths = LedgerPaths::new(&root);
        fs::create_dir_all(&paths.quarantine).unwrap();
        let stale = owner(99, 1);
        fs::write(
            &paths.transition,
            serde_json::to_vec(&TransitionOwnerRecord {
                schema_version: TRANSITION_OWNER_SCHEMA_VERSION,
                owner: stale,
            })
            .unwrap(),
        )
        .unwrap();
        let probe = FakeProbe { dead_pid: Some(99) };
        let guard = acquire_lock(&paths, &owner(10, 1), &probe).unwrap();
        assert_eq!(fs::read_dir(&paths.quarantine).unwrap().count(), 1);
        assert!(acquire_lock(&paths, &owner(11, 1), &probe).is_err());
        drop(guard);
        let next = acquire_lock(&paths, &owner(12, 1), &probe).unwrap();
        assert_eq!(fs::read_dir(&paths.quarantine).unwrap().count(), 1);
        drop(next);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: stale_ledger_recovery_refuses_aba_replacement
    fn stale_ledger_recovery_refuses_aba_replacement() {
        let root = fixture();
        let paths = LedgerPaths::new(&root);
        fs::create_dir_all(&paths.quarantine).unwrap();
        let stale = owner(99, 1);
        fs::write(
            &paths.transition,
            serde_json::to_vec(&TransitionOwnerRecord {
                schema_version: TRANSITION_OWNER_SCHEMA_VERSION,
                owner: stale,
            })
            .unwrap(),
        )
        .unwrap();
        let live = owner(101, 1);
        let live_record = serde_json::to_vec(&TransitionOwnerRecord {
            schema_version: TRANSITION_OWNER_SCHEMA_VERSION,
            owner: live,
        })
        .unwrap();
        let replacement_record = live_record.clone();
        TRANSITION_RECOVERY_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |paths| {
                fs::remove_file(&paths.transition).unwrap();
                fs::write(&paths.transition, &replacement_record).unwrap();
            }));
        });

        let error = match acquire_lock(&paths, &owner(10, 1), &FakeProbe { dead_pid: Some(99) }) {
            Ok(_) => panic!("an ABA-replaced lock was quarantined"),
            Err(error) => error,
        };
        assert!(error.contains("changed during recovery"), "{error}");
        assert_eq!(
            fs::read(&paths.transition).unwrap(),
            live_record,
            "recovery moved the replacement lock"
        );
        assert_eq!(fs::read_dir(&paths.quarantine).unwrap().count(), 1);
        assert!(
            paths.transition.exists(),
            "ambiguous recovery removed the replacement transition record"
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn lock_refused(paths: &LedgerPaths) -> bool {
        match acquire_lock(paths, &owner(10, 1), &FakeProbe { dead_pid: None }) {
            Ok(guard) => {
                drop(guard);
                false
            }
            Err(_) => true,
        }
    }

    #[test]
    // TestName: ledger_authority_refuses_symlink_hardlink_unsafe_mode_and_quarantine_alias
    fn ledger_authority_refuses_symlink_hardlink_unsafe_mode_and_quarantine_alias() {
        let symlink_root = fixture();
        let symlink_paths = LedgerPaths::new(&symlink_root);
        let symlink_target = symlink_root.join("foreign-transition-owner");
        fs::write(&symlink_target, b"").unwrap();
        std::os::unix::fs::symlink(&symlink_target, &symlink_paths.transition).unwrap();
        let symlink_refused = lock_refused(&symlink_paths);
        fs::remove_dir_all(&symlink_root).unwrap();

        let hardlink_root = fixture();
        let hardlink_paths = LedgerPaths::new(&hardlink_root);
        fs::write(&hardlink_paths.transition, b"").unwrap();
        fs::hard_link(
            &hardlink_paths.transition,
            hardlink_root.join("transition-alias"),
        )
        .unwrap();
        let hardlink_refused = lock_refused(&hardlink_paths);
        fs::remove_dir_all(&hardlink_root).unwrap();

        let mode_root = fixture();
        let mode_paths = LedgerPaths::new(&mode_root);
        fs::set_permissions(&mode_root, fs::Permissions::from_mode(0o777)).unwrap();
        let unsafe_mode_refused = lock_refused(&mode_paths);
        fs::remove_dir_all(&mode_root).unwrap();

        let quarantine_root = fixture();
        let quarantine_paths = LedgerPaths::new(&quarantine_root);
        let quarantine_target = quarantine_root.join("foreign-quarantine");
        fs::create_dir(&quarantine_target).unwrap();
        std::os::unix::fs::symlink(&quarantine_target, &quarantine_paths.quarantine).unwrap();
        let quarantine_alias_refused = lock_refused(&quarantine_paths);
        fs::remove_dir_all(&quarantine_root).unwrap();

        assert!(symlink_refused, "a symlink transition owner was accepted");
        assert!(
            hardlink_refused,
            "a multiply-linked owner record was accepted"
        );
        assert!(
            unsafe_mode_refused,
            "a world-writable authority was accepted"
        );
        assert!(
            quarantine_alias_refused,
            "a symlinked quarantine authority was accepted"
        );
    }

    #[test]
    // TestName: ledger_directory_lock_prevents_replacement_owner_split_authority
    fn ledger_directory_lock_prevents_replacement_owner_split_authority() {
        let root = fixture();
        let paths = LedgerPaths::new(&root);
        let first = acquire_lock(&paths, &owner(10, 1), &FakeProbe { dead_pid: None }).unwrap();
        fs::remove_file(&paths.transition).unwrap();
        fs::write(&paths.transition, b"").unwrap();

        let challenger_refused =
            match acquire_lock(&paths, &owner(11, 1), &FakeProbe { dead_pid: None }) {
                Ok(challenger) => {
                    drop(challenger);
                    false
                }
                Err(_) => true,
            };
        drop(first);
        fs::remove_dir_all(root).unwrap();

        assert!(
            challenger_refused,
            "replacement owner record split the live ledger lock authority"
        );
    }

    #[test]
    // TestName: reservation_ledger_refuses_nofollow_violation_without_rewriting_target
    fn reservation_ledger_refuses_nofollow_violation_without_rewriting_target() {
        let root = fixture();
        let paths = LedgerPaths::new(&root);
        let target = root.join("foreign-ledger.json");
        let original = fs::read(&paths.ledger).unwrap();
        fs::write(&target, &original).unwrap();
        fs::remove_file(&paths.ledger).unwrap();
        std::os::unix::fs::symlink(&target, &paths.ledger).unwrap();

        let admitted = acquire_reservation_at(
            &root,
            &root.join("safe"),
            &root.join("lease"),
            owner(10, 1),
            WorkloadClass::Interactive,
            1,
            10,
            &FakeProbe { dead_pid: None },
        );
        let target_after = fs::read(&target).unwrap();
        let named_is_symlink = fs::symlink_metadata(&paths.ledger)
            .unwrap()
            .file_type()
            .is_symlink();
        fs::remove_dir_all(root).unwrap();

        assert!(admitted.is_err(), "symlinked ledger admitted a reservation");
        assert_eq!(
            target_after, original,
            "foreign ledger target was rewritten"
        );
        assert!(named_is_symlink, "the aliased ledger path was replaced");
    }

    #[test]
    fn malformed_ledger_lock_refuses_recovery_without_proven_owner_death() {
        let root = fixture();
        let paths = LedgerPaths::new(&root);
        fs::create_dir_all(&paths.quarantine).unwrap();
        fs::write(&paths.transition, b"not-a-ledger-owner").unwrap();

        let error = match acquire_lock(&paths, &owner(10, 1), &FakeProbe { dead_pid: None }) {
            Ok(_) => panic!("a malformed lock has no proven dead owner"),
            Err(error) => error,
        };
        assert!(error.contains("malformed"), "{error}");
        assert!(paths.transition.exists());
        assert_eq!(fs::read_dir(&paths.quarantine).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pid_reuse_cannot_own_or_release_reservation() {
        let root = fixture();
        let probe = FakeProbe { dead_pid: None };
        let item = acquire_reservation_at(
            &root,
            &root.join("safe"),
            &root.join("lease"),
            owner(10, 999),
            WorkloadClass::Interactive,
            1,
            10,
            &probe,
        )
        .unwrap();
        let mut impostor = item.clone();
        impostor.owner.start_time = 1000;
        assert!(release_reservation_at(&root, &impostor, &probe).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_scope_uses_aggregate_ceiling() {
        let budget = calculate_budget(16 * GIB, 1).unwrap();
        assert_eq!(budget.control_plane_reserve_bytes, 4 * GIB);
        assert_eq!(budget.memory_max_bytes, 12 * GIB);
        assert_eq!(budget.memory_high_bytes, 12 * GIB - (16 * GIB).div_ceil(10));
        let request = parse_run_args(&[
            "--class".into(),
            "build".into(),
            "--memory-max".into(),
            "4096".into(),
            "--".into(),
            "/bin/true".into(),
        ])
        .unwrap();
        let args = build_runner_args("test.scope", &request, budget);
        assert!(
            args.iter()
                .any(|argument| argument == "Slice=ramshared-workloads.slice")
        );
        assert!(
            args.iter()
                .any(|argument| argument == "MemoryMax=4294967296")
        );
    }

    #[test]
    fn safe_mode_marker_blocks_admission_even_when_a_lease_exists() {
        let root = fixture();
        let safe = root.join("safe");
        fs::write(&safe, "{}").unwrap();
        fs::write(root.join("lease"), "{}").unwrap();
        let probe = FakeProbe { dead_pid: None };
        assert!(
            acquire_reservation_at(
                &root,
                &safe,
                &root.join("lease"),
                owner(10, 1),
                WorkloadClass::Interactive,
                1,
                10,
                &probe,
            )
            .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parser_budget_and_owner_adapters_cover_legitimate_and_refusal_paths() {
        assert!(calculate_budget(0, 0).is_err());
        assert!(calculate_budget(4 * GIB, 0).is_err());
        assert_eq!(
            parse_meminfo("MemTotal: 16384 kB\n").unwrap(),
            16 * 1024 * 1024
        );
        assert!(parse_meminfo("MemAvailable: 1 kB\n").is_err());
        assert!(process_start_time(std::process::id()).is_some());
        let current = OwnerIdentity::current().unwrap();
        assert!(SystemOwnerProbe.is_alive(&current));

        for (class, expected_mib) in [
            ("interactive", 2048),
            ("build", 6144),
            ("browser-test", 4096),
            ("batch", 8192),
        ] {
            let request = parse_run_args(&[
                "--class".into(),
                class.into(),
                "--".into(),
                "/bin/true".into(),
            ])
            .unwrap();
            assert_eq!(request.memory_max_bytes, expected_mib * MIB_BYTES);
            assert_eq!(request.class.as_str(), class);
        }
        for args in [
            vec![],
            vec!["--class".into(), "unknown".into()],
            vec!["--memory-max".into(), "0".into()],
            vec!["--class".into(), "build".into(), "--".into()],
            vec!["--unexpected".into()],
        ] {
            assert!(parse_run_args(&args).is_err());
        }
    }

    #[test]
    // TestName: supported_reservation_ledger_allows_admission_and_release
    fn supported_reservation_ledger_allows_admission_and_release() {
        for result in [
            Ok(Some(0)),
            Ok(Some(7)),
            Ok(None),
            Err("fixture spawn refusal".into()),
        ] {
            let root = fixture();
            let runner = FakeRunner {
                result: result.clone(),
                calls: RefCell::new(Vec::new()),
            };
            let control = OwnerIdentity::current().unwrap();
            fs::write(
                root.join("supervisor.json"),
                format!("{}\n", healthy_supervisor_status(&control, now_ms())),
            )
            .unwrap();
            fs::write(root.join("lease"), valid_resume_lease(&control).to_string()).unwrap();
            publish_open_admission_state(&root.join("ledger"), &control);
            let request = WorkloadRequest {
                class: WorkloadClass::Build,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            };
            let outcome = run_with(
                request,
                "MemTotal: 16777216 kB\n",
                &runner,
                &root.join("ledger"),
                &root.join("safe"),
                &root.join("lease"),
                &root.join("supervisor.json"),
            );
            assert_eq!(outcome.is_ok(), result == Ok(Some(0)));
            assert_eq!(runner.calls.borrow().len(), 1);
            let ledger = load_ledger(&root.join("ledger/reservations.json")).unwrap();
            assert!(ledger.reservations.is_empty());
            fs::remove_dir_all(root).unwrap();
        }

        let root = fixture();
        let mut guarded = current_healthy_supervisor_status();
        guarded["control_state"] = serde_json::json!("GUARDED");
        fs::write(root.join("supervisor.json"), guarded.to_string()).unwrap();
        let runner = FakeRunner {
            result: Ok(Some(0)),
            calls: RefCell::new(Vec::new()),
        };
        let refused = run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &root.join("ledger"),
            &root.join("safe"),
            &root.join("lease"),
            &root.join("supervisor.json"),
        );
        assert!(refused.unwrap_err().contains("GUARDED"));
        assert!(runner.calls.borrow().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workload_admission_refuses_missing_malformed_stale_or_foreign_supervisor_evidence() {
        for (name, evidence) in [
            ("missing", None),
            ("malformed", Some("{".to_string())),
            (
                "stale",
                Some(healthy_supervisor_status(&OwnerIdentity::current().unwrap(), 0).to_string()),
            ),
            (
                "foreign",
                Some(
                    healthy_supervisor_status(
                        &OwnerIdentity {
                            boot_id: "foreign-boot".into(),
                            ..OwnerIdentity::current().unwrap()
                        },
                        now_ms(),
                    )
                    .to_string(),
                ),
            ),
        ] {
            let root = fixture();
            if let Some(evidence) = evidence {
                fs::write(root.join("supervisor.json"), evidence).unwrap();
            }
            let runner = FakeRunner {
                result: Ok(Some(0)),
                calls: RefCell::new(Vec::new()),
            };
            let result = run_with(
                WorkloadRequest {
                    class: WorkloadClass::Interactive,
                    memory_max_bytes: GIB,
                    command: vec!["/bin/true".into()],
                },
                "MemTotal: 16777216 kB\n",
                &runner,
                &root.join("ledger"),
                &root.join("safe"),
                &root.join("lease"),
                &root.join("supervisor.json"),
            );
            assert!(
                result.is_err(),
                "{name} supervisor evidence admitted a workload"
            );
            assert!(
                runner.calls.borrow().is_empty(),
                "{name} started a workload"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn malformed_expired_or_foreign_resume_lease_never_admits_a_workload() {
        let owner = OwnerIdentity::current().unwrap();
        for (name, lease) in [
            ("malformed", "{".to_string()),
            (
                "expired",
                serde_json::json!({
                    "schema_version": 2,
                    "boot_id": owner.boot_id,
                    "incident_id": "0123456789abcdef0123456789abcdef",
                    "distro": "Ubuntu-24.04",
                    "supervisor_identity": owner,
                    "issued_at_epoch_ms": 0,
                    "expires_at_epoch_ms": 1,
                })
                .to_string(),
            ),
            (
                "foreign",
                serde_json::json!({
                    "schema_version": 2,
                    "boot_id": "foreign-boot",
                    "incident_id": "0123456789abcdef0123456789abcdef",
                    "distro": "Ubuntu-24.04",
                    "supervisor_identity": owner,
                    "issued_at_epoch_ms": now_ms(),
                    "expires_at_epoch_ms": now_ms() + 15_000,
                })
                .to_string(),
            ),
        ] {
            let root = fixture();
            fs::write(
                root.join("supervisor.json"),
                healthy_supervisor_status(&owner, now_ms()).to_string(),
            )
            .unwrap();
            fs::write(root.join("lease"), lease).unwrap();
            publish_open_admission_state(&root.join("ledger"), &owner);
            let runner = FakeRunner {
                result: Ok(Some(0)),
                calls: RefCell::new(Vec::new()),
            };
            let result = run_with(
                WorkloadRequest {
                    class: WorkloadClass::Interactive,
                    memory_max_bytes: GIB,
                    command: vec!["/bin/true".into()],
                },
                "MemTotal: 16777216 kB\n",
                &runner,
                &root.join("ledger"),
                &root.join("safe"),
                &root.join("lease"),
                &root.join("supervisor.json"),
            );
            assert!(result.is_err(), "{name} resume lease admitted a workload");
            assert!(
                runner.calls.borrow().is_empty(),
                "{name} started a workload"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn current_supervisor_evidence_and_resume_lease_admit_one_workload() {
        let root = fixture();
        let owner = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&owner, now_ms()).to_string(),
        )
        .unwrap();
        fs::write(root.join("lease"), valid_resume_lease(&owner).to_string()).unwrap();
        publish_open_admission_state(&root.join("ledger"), &owner);
        let runner = FakeRunner {
            result: Ok(Some(0)),
            calls: RefCell::new(Vec::new()),
        };
        run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &root.join("ledger"),
            &root.join("safe"),
            &root.join("lease"),
            &root.join("supervisor.json"),
        )
        .unwrap();
        assert_eq!(runner.calls.borrow().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: workload_admission_refuses_missing_boot_ephemeral_lease
    fn workload_admission_refuses_missing_boot_ephemeral_lease() {
        let root = fixture();
        let supervisor = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&supervisor, now_ms()).to_string(),
        )
        .unwrap();
        publish_open_admission_state(&root.join("ledger"), &supervisor);
        let runner = FakeRunner {
            result: Ok(Some(0)),
            calls: RefCell::new(Vec::new()),
        };

        let error = run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &root.join("ledger"),
            &root.join("safe"),
            &root.join("missing-lease.json"),
            &root.join("supervisor.json"),
        )
        .unwrap_err();

        assert!(error.contains("resume lease"), "{error}");
        assert!(
            runner.calls.borrow().is_empty(),
            "workload started without lease"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: workload_admission_requires_serialized_open_gate
    fn workload_admission_requires_serialized_open_gate() {
        let root = fixture();
        let supervisor = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&supervisor, now_ms()).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("lease.json"),
            valid_resume_lease(&supervisor).to_string(),
        )
        .unwrap();
        let runner = FakeRunner {
            result: Ok(Some(0)),
            calls: RefCell::new(Vec::new()),
        };

        let error = run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &root.join("ledger"),
            &root.join("safe"),
            &root.join("lease.json"),
            &root.join("supervisor.json"),
        )
        .unwrap_err();

        assert!(error.contains("admission state"), "{error}");
        assert!(
            runner.calls.borrow().is_empty(),
            "closed transition admitted a scope"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: workload_start_serializes_against_close_admission_transition
    fn workload_start_serializes_against_close_admission_transition() {
        let root = fixture();
        let supervisor = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&supervisor, now_ms()).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("lease.json"),
            valid_resume_lease(&supervisor).to_string(),
        )
        .unwrap();
        let ledger_root = root.join("ledger");
        publish_open_admission_state(&ledger_root, &supervisor);
        let runner = TransitionBlockingRunner {
            ledger_root: ledger_root.clone(),
            supervisor,
            calls: RefCell::new(0),
        };

        run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &ledger_root,
            &root.join("safe"),
            &root.join("lease.json"),
            &root.join("supervisor.json"),
        )
        .unwrap();

        assert_eq!(*runner.calls.borrow(), 1);
        assert!(
            LedgerPaths::new(&ledger_root).transition.exists(),
            "workload transition protocol must retain its crash-safe owner record"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: scope_invocation_id_is_persisted
    fn scope_invocation_id_is_persisted() {
        let root = fixture();
        let supervisor = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&supervisor, now_ms()).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("lease.json"),
            valid_resume_lease(&supervisor).to_string(),
        )
        .unwrap();
        let ledger_root = root.join("ledger");
        publish_open_admission_state(&ledger_root, &supervisor);
        let runner = StartPublishingRunner {
            ledger_root: ledger_root.clone(),
            supervisor,
            start_calls: RefCell::new(0),
            wait_calls: Arc::new(AtomicUsize::new(0)),
        };

        run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &ledger_root,
            &root.join("safe"),
            &root.join("lease.json"),
            &root.join("supervisor.json"),
        )
        .unwrap();

        assert_eq!(*runner.start_calls.borrow(), 1);
        assert_eq!(runner.wait_calls.load(AtomicOrdering::SeqCst), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: system_scope_terminal_watcher_retains_full_reservation_until_exact_unit_finishes
    fn system_scope_terminal_watcher_retains_full_reservation_until_exact_unit_finishes() {
        let root = fixture();
        let supervisor = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&supervisor, now_ms()).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("lease.json"),
            valid_resume_lease(&supervisor).to_string(),
        )
        .unwrap();
        let ledger_root = root.join("ledger");
        publish_open_admission_state(&ledger_root, &supervisor);
        let status = Arc::new(BlockingScopeStatusAdapter {
            state: Mutex::new(BlockingScopeStatus {
                terminal_poll_started: false,
                terminal: false,
                queries: 0,
                units: Vec::new(),
            }),
            changed: Condvar::new(),
        });
        let runner = Arc::new(SystemExecutionRunner {
            status: status.clone(),
            launches: Arc::new(AtomicUsize::new(0)),
        });
        let worker_root = root.clone();
        let worker_ledger = ledger_root.clone();
        let worker_runner = runner.clone();
        let worker = std::thread::spawn(move || {
            run_with(
                WorkloadRequest {
                    class: WorkloadClass::Interactive,
                    memory_max_bytes: 12 * GIB,
                    command: vec!["/bin/true".into()],
                },
                "MemTotal: 16777216 kB\n",
                &*worker_runner,
                &worker_ledger,
                &worker_root.join("safe"),
                &worker_root.join("lease.json"),
                &worker_root.join("supervisor.json"),
            )
        });

        status.wait_until_terminal_poll();
        let second = match admit_reservation_at(
            &ledger_root,
            &root.join("safe"),
            &root.join("lease.json"),
            &root.join("supervisor.json"),
            OwnerIdentity::current().unwrap(),
            WorkloadClass::Interactive,
            12 * GIB,
            12 * GIB,
            &SystemOwnerProbe,
        ) {
            Ok(_) => panic!("active exact scope admitted a second full-ceiling reservation"),
            Err(error) => error,
        };
        assert!(
            second.contains("aggregate workload budget exceeded"),
            "{second}"
        );
        assert_eq!(
            load_ledger(&LedgerPaths::new(&ledger_root).ledger)
                .unwrap()
                .reservations
                .len(),
            1,
            "the acknowledged active unit was not retained in aggregate accounting"
        );

        status.allow_terminal();
        worker.join().unwrap().unwrap();
        assert_eq!(runner.launches.load(AtomicOrdering::SeqCst), 1);
        let observed_units = &status.state.lock().unwrap().units;
        assert!(observed_units.len() >= 2);
        assert!(observed_units[0].starts_with("ramshared-interactive-"));
        assert!(observed_units[0].ends_with(".scope"));
        assert!(
            load_ledger(&LedgerPaths::new(&ledger_root).ledger)
                .unwrap()
                .reservations
                .is_empty(),
            "terminal completion did not release exactly once"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: system_scope_identity_change_retains_reservation_for_containment
    fn system_scope_identity_change_retains_reservation_for_containment() {
        let root = fixture();
        let supervisor = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&supervisor, now_ms()).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("lease.json"),
            valid_resume_lease(&supervisor).to_string(),
        )
        .unwrap();
        let ledger_root = root.join("ledger");
        publish_open_admission_state(&ledger_root, &supervisor);
        let runner = SystemExecutionRunner {
            status: Arc::new(ChangingScopeIdentityAdapter {
                queries: AtomicUsize::new(0),
            }),
            launches: Arc::new(AtomicUsize::new(0)),
        };

        let error = run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &ledger_root,
            &root.join("safe"),
            &root.join("lease.json"),
            &root.join("supervisor.json"),
        )
        .unwrap_err();

        assert!(error.contains("invocation identity changed"), "{error}");
        assert_eq!(runner.launches.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            load_ledger(&LedgerPaths::new(&ledger_root).ledger)
                .unwrap()
                .reservations
                .len(),
            1,
            "replacement scope identity released live containment evidence"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: system_scope_status_error_retains_reservation_for_containment
    fn system_scope_status_error_retains_reservation_for_containment() {
        let root = fixture();
        let supervisor = OwnerIdentity::current().unwrap();
        fs::write(
            root.join("supervisor.json"),
            healthy_supervisor_status(&supervisor, now_ms()).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("lease.json"),
            valid_resume_lease(&supervisor).to_string(),
        )
        .unwrap();
        let ledger_root = root.join("ledger");
        publish_open_admission_state(&ledger_root, &supervisor);
        let runner = SystemExecutionRunner {
            status: Arc::new(ErroringScopeStatusAdapter {
                queries: AtomicUsize::new(0),
            }),
            launches: Arc::new(AtomicUsize::new(0)),
        };

        let error = run_with(
            WorkloadRequest {
                class: WorkloadClass::Interactive,
                memory_max_bytes: GIB,
                command: vec!["/bin/true".into()],
            },
            "MemTotal: 16777216 kB\n",
            &runner,
            &ledger_root,
            &root.join("safe"),
            &root.join("lease.json"),
            &root.join("supervisor.json"),
        )
        .unwrap_err();

        assert!(error.contains("terminal state"), "{error}");
        assert_eq!(runner.launches.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            load_ledger(&LedgerPaths::new(&ledger_root).ledger)
                .unwrap()
                .reservations
                .len(),
            1,
            "status-query containment failure released live scope evidence"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    // TestName: scope_ack_timeout_reaps_group_and_owned_descendant
    fn scope_ack_timeout_reaps_group_and_owned_descendant() {
        let root = fixture();
        let descendant_pid_file = root.join("ack-descendant.pid");
        let mut acknowledgement = Command::new("/bin/sh");
        acknowledgement
            .arg("-c")
            .arg("sleep 10 & printf '%s' \"$!\" > \"$1\"; wait")
            .arg("ramshared-ack-fixture")
            .arg(&descendant_pid_file);
        bounded_process::configure_process_group(&mut acknowledgement);
        let mut acknowledgement = acknowledgement.spawn().unwrap();
        let direct_pid = acknowledgement.id();
        let pid_deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !descendant_pid_file.exists() && std::time::Instant::now() < pid_deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let descendant_pid = fs::read_to_string(&descendant_pid_file)
            .unwrap()
            .parse::<u32>()
            .unwrap();

        let error = wait_for_scope_start_acknowledgement(&mut acknowledgement).unwrap_err();
        std::thread::sleep(Duration::from_millis(50));
        let direct_gone = !Path::new(&format!("/proc/{direct_pid}")).exists();
        let descendant_gone = !Path::new(&format!("/proc/{descendant_pid}")).exists();

        if !direct_gone || !descendant_gone {
            if let Some(group) = rustix::process::Pid::from_raw(direct_pid as i32) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
            let _ = acknowledgement.wait();
        }
        fs::remove_dir_all(root).unwrap();

        assert!(error.contains("bounded timeout"), "{error}");
        assert!(direct_gone, "timed-out acknowledgement was not reaped");
        assert!(
            descendant_gone,
            "owned acknowledgement descendant survived its process group"
        );
    }

    #[test]
    // TestName: transition_guard_recovers_verified_dead_owner_record
    fn transition_guard_recovers_verified_dead_owner_record() {
        let root = fixture();
        let paths = LedgerPaths::new(&root);
        fs::create_dir_all(&paths.root).unwrap();
        let stale = owner(99, 1);
        fs::write(
            &paths.transition,
            serde_json::to_vec(&TransitionOwnerRecord {
                schema_version: TRANSITION_OWNER_SCHEMA_VERSION,
                owner: stale,
            })
            .unwrap(),
        )
        .unwrap();

        let guard = acquire_lock(&paths, &owner(10, 1), &FakeProbe { dead_pid: Some(99) })
            .unwrap_or_else(|error| panic!("verified-dead transition must recover: {error}"));
        let recovered: TransitionOwnerRecord =
            serde_json::from_slice(&fs::read(&paths.transition).unwrap()).unwrap();
        assert_eq!(recovered.owner.pid, 10);
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    fn recovery_fixture(root: &Path) -> RecoveryPaths {
        RecoveryPaths {
            supervisor_state: root.join("supervisor.json"),
            boot_id: root.join("boot-id"),
            guest_gate: root.join("guest-safe.json"),
            host_gate_dir: root.join("host-safe"),
            pending: root.join("recovery-pending.json"),
            resume_lease: root.join("resume-lease.json"),
        }
    }

    fn seed_recovery(paths: &RecoveryPaths, with_host_gate: bool) {
        let owner = OwnerIdentity::current().unwrap();
        fs::write(
            &paths.supervisor_state,
            format!("{}\n", healthy_supervisor_status(&owner, now_ms())),
        )
        .unwrap();
        fs::write(&paths.boot_id, format!("{}\n", owner.boot_id)).unwrap();
        let guest_gate = serde_json::json!({
            "schema_version": 1,
            "incident_id": "0123456789abcdef0123456789abcdef",
            "distro": "Ubuntu-24.04",
            "boot_id": owner.boot_id,
        });
        fs::write(&paths.guest_gate, format!("{guest_gate}\n")).unwrap();
        if with_host_gate {
            let host_gate = serde_json::json!({
                "schema_version": 1,
                "incident_id": "0123456789abcdef0123456789abcdef",
                "distro": "Ubuntu-24.04",
                "prior_boot_id": "prior-boot",
                "reason": "guardian_proven_guest_inaccessible",
                "timestamp_utc": "2026-08-22T00:00:00Z",
            });
            fs::create_dir_all(&paths.host_gate_dir).unwrap();
            fs::write(
                paths.host_gate_dir.join("Ubuntu-24.04.json"),
                format!("{host_gate}\n"),
            )
            .unwrap();
        } else {
            let now = now_ms();
            let pending = serde_json::json!({
                "schema_version": 2,
                "incident_id": "0123456789abcdef0123456789abcdef",
                "distro": "Ubuntu-24.04",
                "boot_id": owner.boot_id,
                "supervisor_identity": owner,
                "issued_at_epoch_ms": now,
                "expires_at_epoch_ms": now + 15_000,
            });
            fs::write(&paths.pending, format!("{pending}\n")).unwrap();
        }
    }

    #[test]
    fn recovery_releases_gates_only_with_a_current_resume_lease() {
        for with_host_gate in [true, false] {
            let root = fixture();
            let paths = recovery_fixture(&root);
            seed_recovery(&paths, with_host_gate);
            recover_resume_at(&paths).unwrap();
            assert!(!paths.guest_gate.exists());
            assert!(!paths.pending.exists());
            assert!(paths.resume_lease.is_file());
            assert!(!paths.host_gate_dir.join("Ubuntu-24.04.json").exists());
            let current = OwnerIdentity::current().unwrap();
            let supervisor = read_current_supervisor_status_at(
                &paths.supervisor_state,
                &current,
                now_ms(),
                60,
                &SystemOwnerProbe,
            )
            .unwrap();
            assert!(
                read_resume_lease_if_present(
                    &paths.resume_lease,
                    &current,
                    &supervisor,
                    now_ms(),
                    &SystemOwnerProbe,
                )
                .unwrap()
                .is_some()
            );
            let status = recover_status_at(&paths.guest_gate, &paths.resume_lease).unwrap();
            assert!(status.contains(r#""safe_mode":false"#));
            assert!(status.contains(r#""resume_lease":true"#));
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn recovery_refuses_unhealthy_or_mismatched_evidence() {
        let root = fixture();
        let paths = recovery_fixture(&root);
        seed_recovery(&paths, true);
        let mut guarded: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&paths.supervisor_state).unwrap()).unwrap();
        guarded["control_state"] = serde_json::json!("GUARDED");
        guarded["healthy_samples"] = serde_json::json!(59);
        fs::write(&paths.supervisor_state, guarded.to_string()).unwrap();
        assert!(
            recover_resume_at(&paths)
                .unwrap_err()
                .contains("60 consecutive")
        );

        seed_recovery(&paths, true);
        let mismatched_gate = serde_json::json!({
            "schema_version": 1,
            "incident_id": "f".repeat(32),
            "distro": "Ubuntu-24.04",
            "prior_boot_id": "prior-boot",
            "reason": "guardian_proven_guest_inaccessible",
            "timestamp_utc": "2026-08-22T00:00:00Z",
        });
        fs::write(
            paths.host_gate_dir.join("Ubuntu-24.04.json"),
            format!("{mismatched_gate}\n"),
        )
        .unwrap();
        assert!(
            recover_resume_at(&paths)
                .unwrap_err()
                .contains("do not match")
        );

        fs::remove_file(paths.host_gate_dir.join("Ubuntu-24.04.json")).unwrap();
        assert!(
            recover_resume_at(&paths)
                .unwrap_err()
                .contains("recovery transaction is unavailable")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_refuses_bad_schema_wrong_boot_incident_identity_or_freshness() {
        for (name, mutate) in [
            (
                "bad_schema",
                Box::new(|paths: &RecoveryPaths| {
                    fs::write(&paths.guest_gate, r#"{"schema_version":9}"#).unwrap();
                }) as Box<dyn Fn(&RecoveryPaths)>,
            ),
            (
                "wrong_boot",
                Box::new(|paths: &RecoveryPaths| {
                    let mut gate: serde_json::Value =
                        serde_json::from_str(&fs::read_to_string(&paths.guest_gate).unwrap())
                            .unwrap();
                    gate["boot_id"] = serde_json::json!("foreign-boot");
                    fs::write(&paths.guest_gate, gate.to_string()).unwrap();
                }),
            ),
            (
                "wrong_incident",
                Box::new(|paths: &RecoveryPaths| {
                    let host_gate = paths.host_gate_dir.join("Ubuntu-24.04.json");
                    let mut gate: serde_json::Value =
                        serde_json::from_str(&fs::read_to_string(&host_gate).unwrap()).unwrap();
                    gate["incident_id"] = serde_json::json!("f".repeat(32));
                    fs::write(host_gate, gate.to_string()).unwrap();
                }),
            ),
            (
                "foreign_identity",
                Box::new(|paths: &RecoveryPaths| {
                    let mut status: serde_json::Value =
                        serde_json::from_str(&fs::read_to_string(&paths.supervisor_state).unwrap())
                            .unwrap();
                    status["supervisor_identity"]["pid"] = serde_json::json!(u32::MAX);
                    fs::write(&paths.supervisor_state, status.to_string()).unwrap();
                }),
            ),
            (
                "stale_supervisor",
                Box::new(|paths: &RecoveryPaths| {
                    let mut status: serde_json::Value =
                        serde_json::from_str(&fs::read_to_string(&paths.supervisor_state).unwrap())
                            .unwrap();
                    status["written_at_unix_ms"] = serde_json::json!(0);
                    fs::write(&paths.supervisor_state, status.to_string()).unwrap();
                }),
            ),
        ] {
            let root = fixture();
            let paths = recovery_fixture(&root);
            seed_recovery(&paths, true);
            mutate(&paths);
            assert!(
                recover_resume_at(&paths).is_err(),
                "{name} recovery evidence issued a lease"
            );
            assert!(
                !paths.resume_lease.exists(),
                "{name} created a resume lease"
            );
            assert!(paths.guest_gate.exists(), "{name} released the guest gate");
            fs::remove_dir_all(root).unwrap();
        }
    }
}
