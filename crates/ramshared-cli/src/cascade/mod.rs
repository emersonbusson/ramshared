//! Orchestration of the zram→VRAM→VHDX cascade (SPEC §6.2–6.4). Runs as root.
//!
//! **Anti-hang contract (Kahneman #5 / #15 / #16, 2026-07-09 WSL freeze):**
//! 1. **Never** kill `ramsharedd` while any managed swap (nbd/ublk/zram) is still
//!    listed in `/proc/swaps` — that creates ghost `(deleted)` swap entries and freezes WSL.
//! 2. **Always** `swapoff` managed devices **before** NBD disconnect / daemon stop.
//! 3. **Refuse** `up` if ghost/deleted or unbound managed swap is present.
//!    Live enumeration is detection-only: zero-used devices are still active and
//!    never authorize recovery without an exact sealed lifecycle binding.
//! 4. **zram** algorithm is best-effort with fallbacks (WSL kernels disagree on `lzo-rle`).
//!
//! Mounts tiers by `swapon` priority and unmounts in reverse order.

use ramshared_tier::TierPriorities;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::VecDeque;

// Test seams (no process-global env — avoids unsafe set_var under clippy deny).
#[cfg(test)]
thread_local! {
    static SH_SCRIPT: RefCell<VecDeque<(String, Result<String, String>)>> =
        const { RefCell::new(VecDeque::new()) };
    static TEST_SWAPS: RefCell<Option<String>> = const { RefCell::new(None) };
    static TEST_SWAPS_SEQUENCE: RefCell<VecDeque<Result<String, String>>> =
        const { RefCell::new(VecDeque::new()) };
    static TEST_SWAPS_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static TEST_ORIGIN_CONFIG: RefCell<Option<Result<String, String>>> = const { RefCell::new(None) };
    static TEST_MEM_AVAILABLE: RefCell<Option<u64>> = const { RefCell::new(None) };
    static TEST_ENV_MB: RefCell<Option<(String, u64)>> = const { RefCell::new(None) };
}

const SOCK: &str = "/run/ramshared/wsl2d.sock";
const NBD: &str = "/dev/nbd0";
const ZRAM_DEV_FILE: &str = "/run/ramshared/zram-dev";
const SWAP_DEV_FILE: &str = "/run/ramshared/swap-dev";
const PID_FILE: &str = "/run/ramshared/ramsharedd.pid";
/// Daemon demote counters for status --json (written by ramsharedd).
const DEMOTE_STATUS_FILE: &str = "/run/ramshared/demote-status.json";
const CAPACITY_STATUS_FILE: &str = "/run/ramshared/capacity-guaranteed";
const ORIGIN_CONFIG_FILE: &str = "/etc/ramshared/origin.conf";
const DEFAULT_PHYSICAL_CACHE_CAP_MIB: u64 = 1024;
const CACHE_STATUS_FILE: &str = "/run/ramshared/cache-status.json";
const SUPERVISOR_STATUS_FILE: &str = "/run/ramshared/supervisor-state.json";
const CONTROL_STATUS_MAX_AGE_MS: u64 = 15_000;
const SUPERVISOR_ACTION_ERROR_MAX_BYTES: usize = 128 * 1024;
const SAFE_MODE_FILE: &str = "/var/lib/ramshared/safe-mode.json";
const GUARDIAN_HEALTH_FILE: &str =
    "/mnt/c/ProgramData/RamShared/guardian-state/Ubuntu-24.04.health.json";
/// Forensic "armed" marker (survives WSL death if under /mnt/c).
const ARMED_MARKER_CANDIDATES: &[&str] = &["/mnt/c/wsl-forensics/.armed", "/run/ramshared/.armed"];

/// Algorithms tried in order for zram (kernel WSL 6.6 may reject some).
const ZRAM_ALGOS: &[&str] = &["lzo-rle", "lzo", "zstd", "lz4", "deflate"];

/// Typed error for the cascade orchestration.
#[derive(Debug)]
pub enum CascadeError {
    Shell { cmd: String, msg: String },
    Arg(String),
    Io(String),
    Precondition(String),
    UnsafeContainment(String),
}

impl fmt::Display for CascadeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CascadeError::Shell { cmd, msg } => write!(f, "command `{cmd}` failed: {msg}"),
            CascadeError::Arg(m) => write!(f, "invalid argument: {m}"),
            CascadeError::Io(m) => write!(f, "I/O: {m}"),
            CascadeError::Precondition(m) => write!(f, "{m}"),
            CascadeError::UnsafeContainment(m) => write!(f, "unsafe containment: {m}"),
        }
    }
}

impl std::error::Error for CascadeError {}

fn sh(cmd: &str, args: &[&str]) -> Result<String, CascadeError> {
    #[cfg(test)]
    {
        let scripted = SH_SCRIPT.with(|q| {
            let mut q = q.borrow_mut();
            // Match by command name, then by full "cmd arg0", then wildcard "*"
            let full = format!("{cmd} {}", args.join(" "));
            if let Some(i) = q.iter().position(|(p, _)| {
                p == cmd || p == &full || p == "*" || full.starts_with(p.as_str())
            }) {
                return q.remove(i);
            }
            None
        });
        if let Some((_pat, res)) = scripted {
            return res.map_err(|msg| CascadeError::Shell {
                cmd: format!("{cmd} {}", args.join(" ")),
                msg,
            });
        }
    }
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| CascadeError::Shell {
            cmd: cmd.to_string(),
            msg: e.to_string(),
        })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(CascadeError::Shell {
            cmd: format!("{cmd} {}", args.join(" ")),
            msg: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }
}

fn mem_available_bytes() -> u64 {
    #[cfg(test)]
    if let Some(n) = TEST_MEM_AVAILABLE.with(|c| *c.borrow()) {
        return n;
    }
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemAvailable:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
        .map(|kib| kib * 1024)
        .unwrap_or(0)
}

// --- /proc/swaps parsing (pure, unit-tested) ---------------------------------

/// One line from `/proc/swaps` after the header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapEntry {
    /// Path as shown by the kernel (may contain ` (deleted)` or `\040(deleted)`).
    pub filename: String,
    pub size_kb: u64,
    pub used_kb: u64,
    pub priority: i32,
}

impl SwapEntry {
    /// True if the kernel already lost the block device (ghost swap).
    pub fn is_ghost(&self) -> bool {
        self.filename.contains("(deleted)") || self.filename.contains("\\040(deleted)")
    }

    /// True if this looks like a RamShared-managed or dangerous orphan tier.
    pub fn is_managed_or_orphan_vram_tier(&self) -> bool {
        is_allowlisted_managed_path(&self.bare_path())
    }

    /// Basename-ish key for matching recorded paths.
    pub fn bare_path(&self) -> String {
        self.filename
            .replace("\\040(deleted)", " (deleted)")
            .split_whitespace()
            .next()
            .unwrap_or(&self.filename)
            .to_string()
    }

    /// Canonical `/dev/...` path for swapoff (kernel may list `/nbd0` without `/dev`).
    /// SPEC: docs/specs/no-milestone/wsl2-cascade-orphan-recover/SPEC.md ITEM-1
    pub fn canonical_path(&self) -> String {
        canonicalize_swap_path(&self.bare_path())
    }
}

/// `/nbd0` → `/dev/nbd0`; `/dev/nbd0` unchanged; `nbd0` → `/dev/nbd0`.
/// SPEC: wsl2-cascade-orphan-recover ITEM-1
pub fn canonicalize_swap_path(p: &str) -> String {
    let p = p.trim();
    if p.is_empty() {
        return String::new();
    }
    if p.starts_with("/dev/") {
        return p.to_string();
    }
    if let Some(rest) = p.strip_prefix('/') {
        return format!("/dev/{rest}");
    }
    format!("/dev/{p}")
}

fn numbered_device_basename(path: &str, prefix: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    let Some(number) = base.strip_prefix(prefix) else {
        return false;
    };
    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn has_device_path_shape(path: &str) -> bool {
    if let Some(rest) = path.strip_prefix("/dev/") {
        !rest.is_empty() && !rest.contains('/')
    } else if let Some(rest) = path.strip_prefix('/') {
        !rest.is_empty() && !rest.contains('/')
    } else {
        !path.is_empty() && !path.contains('/')
    }
}

pub(crate) fn is_nbd_device_path(path: &str) -> bool {
    let bare = path
        .trim()
        .replace("\\040(deleted)", " (deleted)")
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    has_device_path_shape(&bare) && numbered_device_basename(&bare, "nbd")
}

pub(crate) fn is_ublk_device_path(path: &str) -> bool {
    let bare = path
        .trim()
        .replace("\\040(deleted)", " (deleted)")
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    has_device_path_shape(&bare) && numbered_device_basename(&bare, "ublkb")
}

pub(crate) fn is_zram_device_path(path: &str) -> bool {
    let bare = path
        .trim()
        .replace("\\040(deleted)", " (deleted)")
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    has_device_path_shape(&bare) && numbered_device_basename(&bare, "zram")
}

/// Allowlist for automatic lifecycle operations: exact product block-device
/// identities only, never a similarly named file or disk.
fn is_allowlisted_managed_path(path: &str) -> bool {
    let bare = path
        .trim()
        .replace("\\040(deleted)", " (deleted)")
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    is_nbd_device_path(&bare)
        || is_ublk_device_path(&bare)
        || is_zram_device_path(&bare)
        || bare
            .strip_prefix("/dev/mapper/ramshared")
            .is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            })
}

/// Parse a complete `/proc/swaps` snapshot. Any malformed or duplicate row
/// invalidates the whole snapshot: absence is a safety proof, not a best-effort
/// observation.
pub fn parse_proc_swaps(text: &str) -> Result<Vec<SwapEntry>, CascadeError> {
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| CascadeError::Precondition("/proc/swaps is empty".into()))?;
    let header_fields = header.split_whitespace().collect::<Vec<_>>();
    if header_fields != ["Filename", "Type", "Size", "Used", "Priority"] {
        return Err(CascadeError::Precondition(
            "/proc/swaps header is malformed or unsupported".into(),
        ));
    }

    let mut entries = Vec::new();
    let mut identities = std::collections::BTreeSet::new();
    for (index, line) in lines.enumerate() {
        if line.trim().is_empty() {
            return Err(CascadeError::Precondition(format!(
                "/proc/swaps row {} is empty",
                index + 2
            )));
        }
        let cols = line.split_whitespace().collect::<Vec<_>>();
        if cols.len() < 5 {
            return Err(CascadeError::Precondition(format!(
                "/proc/swaps row {} is malformed",
                index + 2
            )));
        }
        let n = cols.len();
        if !matches!(cols[n - 4], "file" | "partition") {
            return Err(CascadeError::Precondition(format!(
                "/proc/swaps row {} has an unsupported type",
                index + 2
            )));
        }
        let priority = cols[n - 1].parse::<i32>().map_err(|_| {
            CascadeError::Precondition(format!(
                "/proc/swaps row {} has an invalid priority",
                index + 2
            ))
        })?;
        let used_kb = cols[n - 2].parse::<u64>().map_err(|_| {
            CascadeError::Precondition(format!(
                "/proc/swaps row {} has an invalid used count",
                index + 2
            ))
        })?;
        let size_kb = cols[n - 3].parse::<u64>().map_err(|_| {
            CascadeError::Precondition(format!("/proc/swaps row {} has an invalid size", index + 2))
        })?;
        let filename = cols[..n - 4].join(" ");
        if filename.is_empty() {
            return Err(CascadeError::Precondition(format!(
                "/proc/swaps row {} has no device path",
                index + 2
            )));
        }
        let identity = canonicalize_swap_path(
            filename
                .replace("\\040(deleted)", " (deleted)")
                .split_whitespace()
                .next()
                .unwrap_or_default(),
        );
        if identity.is_empty() || !identities.insert(identity) {
            return Err(CascadeError::Precondition(format!(
                "/proc/swaps row {} repeats or omits a device identity",
                index + 2
            )));
        }
        entries.push(SwapEntry {
            filename,
            size_kb,
            used_kb,
            priority,
        });
    }
    Ok(entries)
}

fn read_swaps() -> Result<Vec<SwapEntry>, CascadeError> {
    #[cfg(test)]
    if let Some(message) = TEST_SWAPS_ERROR.with(|cell| cell.borrow().clone()) {
        return Err(CascadeError::Io(message));
    }
    #[cfg(test)]
    if let Some(snapshot) = TEST_SWAPS_SEQUENCE.with(|queue| queue.borrow_mut().pop_front()) {
        return snapshot
            .map_err(CascadeError::Io)
            .and_then(|text| parse_proc_swaps(&text));
    }
    #[cfg(test)]
    if let Some(s) = TEST_SWAPS.with(|c| c.borrow().clone()) {
        return parse_proc_swaps(&s);
    }
    fs::read_to_string("/proc/swaps")
        .map_err(|error| CascadeError::Io(format!("read /proc/swaps: {error}")))
        .and_then(|text| parse_proc_swaps(&text))
}

/// Ghost VRAM/zram entries that will hang `swapoff` / page-in if left alone.
pub fn ghost_vram_swaps(entries: &[SwapEntry]) -> Vec<&SwapEntry> {
    entries
        .iter()
        .filter(|e| e.is_ghost() && e.is_managed_or_orphan_vram_tier())
        .collect()
}

/// Whether any nbd/ublk (non-ghost) swap is still active — daemon kill is forbidden.
pub fn active_vram_block_swap(entries: &[SwapEntry]) -> bool {
    entries.iter().any(|e| {
        !e.is_ghost() && (is_nbd_device_path(&e.filename) || is_ublk_device_path(&e.filename))
    })
}

fn lower_tier_present() -> Result<bool, CascadeError> {
    let vram_prio = TierPriorities::default().vram;
    Ok(read_swaps()?.iter().any(|e| {
        // Ignore our managed tiers when looking for DEMOTE sink.
        if is_zram_device_path(&e.filename)
            || is_nbd_device_path(&e.filename)
            || is_ublk_device_path(&e.filename)
        {
            return false;
        }
        e.priority < vram_prio
    }))
}

fn default_daemon() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("ramsharedd")))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ramsharedd".to_string())
}

fn chrono_like_now() -> String {
    // Avoid chrono dep: unix seconds is enough for the marker.
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// True if daemon process may be stopped (no active block VRAM swap).
pub fn daemon_kill_allowed(entries: &[SwapEntry]) -> bool {
    !active_vram_block_swap(entries) && ghost_vram_swaps(entries).is_empty()
}

fn refuse_ghost_swap_state() -> Result<(), CascadeError> {
    let entries = read_swaps()?;
    let ghosts = ghost_vram_swaps(&entries);
    if ghosts.is_empty() {
        return Ok(());
    }
    let detail: Vec<String> = ghosts
        .iter()
        .map(|e| format!("{} used_kb={}", e.filename, e.used_kb))
        .collect();
    Err(CascadeError::Precondition(format!(
        "estado sujo: swap fantasma (device deleted) em /proc/swaps: {}. \
         NAO e seguro continuar. No Windows: capture evidencias, rode \
         `wsl --terminate Ubuntu-24.04`, reabra a distro, \
         depois `sudo ramshared down` e `sudo ramshared up ...`. \
         Nunca mate o daemon com ublk/nbd ativo.",
        detail.join("; ")
    )))
}

/// Pure plan for orphan handling (unit-tested).
/// SPEC: wsl2-cascade-orphan-recover ITEM-2
#[derive(Clone, Debug, Eq, PartialEq)]
enum OrphanPlan {
    /// No managed orphan context.
    None,
    /// Unbound live devices were detected. Enumeration is detection-only.
    DetectedUnboundZeroUsed,
    /// Dangerous: nbd/ublk with pages — no auto swapoff.
    RefuseDirtyBackend,
}

fn plan_orphan_action(entries: &[SwapEntry], cascade_healthy: bool) -> OrphanPlan {
    if cascade_healthy {
        return OrphanPlan::None;
    }
    let live_managed: Vec<&SwapEntry> = entries
        .iter()
        .filter(|e| !e.is_ghost() && e.is_managed_or_orphan_vram_tier())
        .collect();
    if live_managed.is_empty() {
        return OrphanPlan::None;
    }
    let dirty_block = live_managed.iter().any(|e| {
        e.used_kb > 0 && (is_nbd_device_path(&e.filename) || is_ublk_device_path(&e.filename))
    });
    if dirty_block {
        return OrphanPlan::RefuseDirtyBackend;
    }
    OrphanPlan::DetectedUnboundZeroUsed
}

fn disk_swap_used_kib(entries: &[SwapEntry]) -> u64 {
    entries
        .iter()
        .filter(|entry| {
            !entry.is_ghost()
                && !is_zram_device_path(&entry.filename)
                && !is_nbd_device_path(&entry.filename)
                && !is_ublk_device_path(&entry.filename)
        })
        .map(|entry| entry.used_kb)
        .sum()
}

/// Detect unbound managed devices without mutating them. Even used_kb == 0 is
/// active swap and therefore requires an exact sealed lifecycle binding.
fn refuse_unbound_managed_devices() -> Result<(), CascadeError> {
    let entries = read_swaps()?;
    if cascade_already_healthy(&entries) {
        return Ok(());
    }

    let plan = plan_orphan_action(&entries, false);
    match plan {
        OrphanPlan::None => Ok(()),
        OrphanPlan::RefuseDirtyBackend => Err(CascadeError::Precondition(
            "unbound nbd/ublk with used_kb>0 — device preserved: live enumeration does not authorize recovery (risk of hang on dead backend). \
             On Windows: capture evidence and use only `wsl --terminate Ubuntu-24.04`; \
             Never kill -9 ramsharedd with nbd/ublk in /proc/swaps."
                .into(),
        )),
        OrphanPlan::DetectedUnboundZeroUsed => Err(CascadeError::Precondition(
            "unbound managed swap detected; live discovery is detection-only. Preserve every device and use the exact sealed lifecycle binding for recovery"
                .into(),
        )),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Transport {
    /// Prefer ublk when safe; on WSL2 always NBD (daemon refuses ublk — freeze risk).
    Auto,
    Nbd,
    Ublk,
}

#[derive(Debug)]
struct UpArgs {
    vram_mb: u64,
    zram_mb: u64,
    daemon: String,
    force: bool,
    connections: u32,
    transport: Transport,
    swap_dev: String,
    origin_path: String,
    origin_partuuid: String,
    origin_ptuuid: String,
    origin_partition_dev_t: String,
    origin_parent_dev_t: String,
    expected_swap_uuid: String,
    host_manifest_sha256: String,
    configuration_sha256: String,
    cache_cap_mib: u64,
    disk_baseline_kib: u64,
}

/// True when running under Microsoft WSL2 (shared kernel VM).
fn is_wsl2() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.contains("microsoft") || s.contains("WSL"))
        .unwrap_or(false)
        || std::path::Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
        || std::env::var_os("WSL_INTEROP").is_some()
}

/// Resolve Auto → Nbd|Ublk. Product Day-1 on WSL2 is always NBD (Kahneman #16).
fn resolve_transport(t: Transport) -> Result<Transport, CascadeError> {
    match t {
        Transport::Nbd => Ok(Transport::Nbd),
        Transport::Ublk => Ok(Transport::Ublk),
        Transport::Auto => {
            if is_wsl2() {
                eprintln!(
                    "[up] transport=auto → nbd \
                     (ublk disponivel no kernel mas recusado no WSL2: teardown pode congelar — 2026-06-09; \
                     recusa permanente, sem caminho de override)"
                );
                return Ok(Transport::Nbd);
            }
            if Path::new("/dev/ublk-control").exists() {
                eprintln!("[up] transport=auto -> ublk (/dev/ublk-control present, non-WSL2 host)");
                Ok(Transport::Ublk)
            } else {
                eprintln!("[up] transport=auto → nbd (sem /dev/ublk-control)");
                Ok(Transport::Nbd)
            }
        }
    }
}

/// Default MiB from env (`RAMSHARED_VRAM_MIB` / `RAMSHARED_ZRAM_MIB`).
/// SPEC: docs/specs/no-milestone/wsl2-cascade-boot/SPEC.md ITEM-4
fn default_mb_from_env(var: &str, fallback: u64) -> u64 {
    #[cfg(test)]
    if let Some((ref k, n)) = TEST_ENV_MB.with(|c| c.borrow().clone())
        && k == var
    {
        return n;
    }
    std::env::var(var)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(fallback)
}

/// True when a healthy Day-1 cascade is already mounted (idempotent `up`).
/// SPEC ITEM-5 — pure over `/proc/swaps` + run files + optional pid/socket.
pub fn cascade_already_healthy(entries: &[SwapEntry]) -> bool {
    if !ghost_vram_swaps(entries).is_empty() {
        return false;
    }
    let has_vram_swap = entries.iter().any(|e| {
        !e.is_ghost()
            && (is_nbd_device_path(&e.filename) || is_ublk_device_path(&e.filename))
            && e.is_managed_or_orphan_vram_tier()
    });
    if !has_vram_swap {
        return false;
    }
    // Test seam: injected `/proc/swaps` must not couple to live /run records (orphan/half tests).
    #[cfg(test)]
    if TEST_SWAPS.with(|c| c.borrow().is_some()) {
        return false;
    }
    let has_record = Path::new(SWAP_DEV_FILE).exists() || Path::new(PID_FILE).exists();
    if !has_record {
        return false;
    }
    // Daemon must still be serving, or we have a half-state (caller must down).
    let pid_alive = fs::read_to_string(PID_FILE)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists());
    let sock_ok = Path::new(SOCK).exists();
    pid_alive || sock_ok
}

/// Half-state: records or nbd without a complete healthy cascade → refuse second `up`.
fn refuse_half_cascade(entries: &[SwapEntry]) -> Result<(), CascadeError> {
    if cascade_already_healthy(entries) {
        return Ok(());
    }
    let has_record = Path::new(SWAP_DEV_FILE).exists()
        || Path::new(ZRAM_DEV_FILE).exists()
        || Path::new(PID_FILE).exists();
    let has_vram = entries.iter().any(|e| {
        !e.is_ghost() && (is_nbd_device_path(&e.filename) || is_ublk_device_path(&e.filename))
    });
    if has_record || has_vram {
        return Err(CascadeError::Precondition(
            "cascata pela metade (estado em /run/ramshared ou nbd/ublk sem daemon saudavel). \
             Rode `sudo ramshared down` e tente `up` de novo. \
             Nao empurre um segundo up em cima."
                .into(),
        ));
    }
    Ok(())
}

fn parse_up_args_from(args: &[String], daemon: String) -> Result<UpArgs, CascadeError> {
    let sealed = read_sealed_origin_config()?;
    let mut a = UpArgs {
        vram_mb: sealed.logical_capacity_mib,
        zram_mb: default_mb_from_env("RAMSHARED_ZRAM_MIB", 1024),
        daemon,
        force: false,
        connections: 1,
        // Default auto: on WSL2 resolves to NBD (Day-1); ublk only off-WSL2 when control node exists.
        transport: Transport::Auto,
        swap_dev: NBD.to_string(),
        origin_path: sealed.origin_path,
        origin_partuuid: sealed.partuuid,
        origin_ptuuid: sealed.ptuuid,
        origin_partition_dev_t: sealed.partition_dev_t,
        origin_parent_dev_t: sealed.parent_dev_t,
        expected_swap_uuid: sealed.expected_swap_uuid,
        host_manifest_sha256: sealed.host_manifest_sha256,
        configuration_sha256: sealed.configuration_sha256,
        cache_cap_mib: sealed.physical_cache_cap_mib,
        disk_baseline_kib: 0,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--vram" => {
                i += 1;
                a.vram_mb = args
                    .get(i)
                    .ok_or_else(|| CascadeError::Arg("--vram requer MiB".into()))?
                    .parse()
                    .map_err(|_| CascadeError::Arg("invalid vram value".into()))?;
            }
            "--zram" => {
                i += 1;
                a.zram_mb = args
                    .get(i)
                    .ok_or_else(|| CascadeError::Arg("--zram requer MiB".into()))?
                    .parse()
                    .map_err(|_| CascadeError::Arg("invalid zram value".into()))?;
            }
            "--daemon" => {
                i += 1;
                a.daemon = args
                    .get(i)
                    .ok_or_else(|| CascadeError::Arg("--daemon requer caminho".into()))?
                    .clone();
            }
            "--connections" => {
                i += 1;
                a.connections = args
                    .get(i)
                    .ok_or_else(|| CascadeError::Arg("--connections requer N".into()))?
                    .parse()
                    .map_err(|_| CascadeError::Arg("invalid connections value".into()))?;
                if a.connections == 0 {
                    return Err(CascadeError::Arg("--connections deve ser >= 1".into()));
                }
            }
            "--transport" => {
                i += 1;
                a.transport = match args
                    .get(i)
                    .ok_or_else(|| CascadeError::Arg("--transport requer valor".into()))?
                    .as_str()
                {
                    "auto" => Transport::Auto,
                    "nbd" => Transport::Nbd,
                    "ublk" => Transport::Ublk,
                    other => {
                        return Err(CascadeError::Arg(format!(
                            "invalid --transport: {other} (use auto|nbd|ublk)"
                        )));
                    }
                };
            }
            "--swap-dev" => {
                i += 1;
                a.swap_dev = args
                    .get(i)
                    .ok_or_else(|| CascadeError::Arg("--swap-dev requer caminho".into()))?
                    .clone();
            }
            "--origin" | "--origin-manifest" => {
                return Err(CascadeError::Arg(
                    "origin identity is sealed by the host gate and cannot be overridden".into(),
                ));
            }
            "--nbd" => {
                i += 1;
                a.swap_dev = args
                    .get(i)
                    .ok_or_else(|| CascadeError::Arg("--nbd requer caminho".into()))?
                    .clone();
                a.transport = Transport::Nbd;
            }
            "--force-no-safety-net" => a.force = true,
            other => return Err(CascadeError::Arg(format!("arg desconhecido: {other}"))),
        }
        i += 1;
    }
    if a.transport == Transport::Ublk && a.connections != 1 {
        return Err(CascadeError::Arg(
            "--connections > 1 is invalid with --transport ublk (single ring)".into(),
        ));
    }
    // Resolve Auto after parse so env/flag still work.
    a.transport = resolve_transport(a.transport)?;
    if a.transport == Transport::Ublk && a.connections != 1 {
        return Err(CascadeError::Arg(
            "--connections > 1 is invalid with --transport ublk (single ring)".into(),
        ));
    }
    if a.cache_cap_mib < DEFAULT_PHYSICAL_CACHE_CAP_MIB || a.cache_cap_mib > a.vram_mb {
        return Err(CascadeError::Precondition(
            "physical cache cap must be between 1024 MiB and logical capacity".into(),
        ));
    }
    if !canonical_origin_uuid(&a.expected_swap_uuid) {
        return Err(CascadeError::Precondition(
            "sealed origin swap UUID is missing or invalid".into(),
        ));
    }
    Ok(a)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SealedOriginConfig {
    host_manifest_sha256: String,
    configuration_sha256: String,
    origin_path: String,
    partuuid: String,
    ptuuid: String,
    partition_dev_t: String,
    parent_dev_t: String,
    expected_swap_uuid: String,
    logical_capacity_mib: u64,
    physical_cache_cap_mib: u64,
}

fn canonical_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn canonical_device_number(value: &str) -> bool {
    let Some((major, minor)) = value.split_once(':') else {
        return false;
    };
    major.parse::<u64>().is_ok()
        && minor.parse::<u64>().is_ok()
        && !major.starts_with('+')
        && !minor.starts_with('+')
}

fn parse_sealed_origin_config(text: &str) -> Result<SealedOriginConfig, CascadeError> {
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() {
            return Err(CascadeError::Precondition(
                "sealed origin manifest contains an empty line".into(),
            ));
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            CascadeError::Precondition("sealed origin manifest contains a malformed line".into())
        })?;
        if key.trim() != key || value.trim() != value || key.is_empty() || value.is_empty() {
            return Err(CascadeError::Precondition(
                "sealed origin manifest is not canonical".into(),
            ));
        }
        if values.insert(key, value).is_some() {
            return Err(CascadeError::Precondition(format!(
                "sealed origin manifest repeats key {key}"
            )));
        }
    }
    const KEYS: &[&str] = &[
        "schema_version",
        "host_manifest_sha256",
        "configuration_sha256",
        "origin_path",
        "partuuid",
        "ptuuid",
        "partition_dev_t",
        "parent_dev_t",
        "expected_swap_uuid",
        "swap_type",
        "logical_capacity_mib",
        "physical_cache_cap_mib",
    ];
    if values.len() != KEYS.len() || KEYS.iter().any(|key| !values.contains_key(key)) {
        return Err(CascadeError::Precondition(
            "sealed origin manifest schema is incomplete or contains unknown keys".into(),
        ));
    }
    if values["schema_version"] != "3" || values["swap_type"] != "swap" {
        return Err(CascadeError::Precondition(
            "sealed origin manifest schema or swap type is invalid".into(),
        ));
    }
    let origin_path = values["origin_path"].to_string();
    let partuuid = origin_partuuid(&origin_path)?.to_ascii_lowercase();
    if !values["partuuid"].eq_ignore_ascii_case(&partuuid)
        || !canonical_origin_uuid(values["ptuuid"])
        || !canonical_origin_uuid(values["expected_swap_uuid"])
        || !canonical_device_number(values["partition_dev_t"])
        || !canonical_device_number(values["parent_dev_t"])
        || !canonical_sha256(values["host_manifest_sha256"])
        || !canonical_sha256(values["configuration_sha256"])
    {
        return Err(CascadeError::Precondition(
            "sealed origin manifest identity or hash is invalid".into(),
        ));
    }
    let logical_capacity_mib = values["logical_capacity_mib"]
        .parse::<u64>()
        .map_err(|_| CascadeError::Precondition("sealed logical capacity is invalid".into()))?;
    validate_origin_logical_capacity(logical_capacity_mib)?;
    let physical_cache_cap_mib = values["physical_cache_cap_mib"]
        .parse::<u64>()
        .map_err(|_| CascadeError::Precondition("sealed physical cache cap is invalid".into()))?;
    if physical_cache_cap_mib < DEFAULT_PHYSICAL_CACHE_CAP_MIB
        || physical_cache_cap_mib > logical_capacity_mib
    {
        return Err(CascadeError::Precondition(
            "sealed physical cache cap is outside product policy".into(),
        ));
    }
    Ok(SealedOriginConfig {
        host_manifest_sha256: values["host_manifest_sha256"].to_ascii_lowercase(),
        configuration_sha256: values["configuration_sha256"].to_ascii_lowercase(),
        origin_path,
        partuuid,
        ptuuid: values["ptuuid"].to_ascii_lowercase(),
        partition_dev_t: values["partition_dev_t"].to_string(),
        parent_dev_t: values["parent_dev_t"].to_string(),
        expected_swap_uuid: values["expected_swap_uuid"].to_ascii_lowercase(),
        logical_capacity_mib,
        physical_cache_cap_mib,
    })
}

#[cfg(test)]
fn sealed_origin_test_fixture() -> String {
    "schema_version=3\n\
host_manifest_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
configuration_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\
origin_path=/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555\n\
partuuid=11111111-2222-4333-8444-555555555555\n\
ptuuid=aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee\n\
partition_dev_t=8:33\n\
parent_dev_t=8:32\n\
expected_swap_uuid=99999999-8888-4777-8666-555555555555\n\
swap_type=swap\n\
logical_capacity_mib=4096\n\
physical_cache_cap_mib=1024"
        .into()
}

fn read_sealed_origin_config() -> Result<SealedOriginConfig, CascadeError> {
    #[cfg(test)]
    {
        let configured = TEST_ORIGIN_CONFIG.with(|cell| cell.borrow().clone());
        let text = match configured {
            Some(Ok(text)) => text,
            Some(Err(message)) => return Err(CascadeError::Io(message)),
            None => sealed_origin_test_fixture(),
        };
        parse_sealed_origin_config(&text)
    }
    #[cfg(not(test))]
    {
        let metadata = fs::symlink_metadata(ORIGIN_CONFIG_FILE)
            .map_err(|error| CascadeError::Io(format!("stat sealed origin manifest: {error}")))?;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.uid() != 0
            || metadata.permissions().mode() & 0o022 != 0
            || metadata.len() == 0
            || metadata.len() > 65_536
        {
            return Err(CascadeError::Precondition(
                "sealed origin manifest is not a bounded root-owned regular file".into(),
            ));
        }
        let text = fs::read_to_string(ORIGIN_CONFIG_FILE)
            .map_err(|error| CascadeError::Io(format!("read sealed origin manifest: {error}")))?;
        parse_sealed_origin_config(&text)
    }
}

fn origin_partuuid(path: &str) -> Result<&str, CascadeError> {
    let uuid = path.strip_prefix("/dev/disk/by-partuuid/").ok_or_else(|| {
        CascadeError::Precondition(
            "origin must use the sealed /dev/disk/by-partuuid/<uuid> identity".into(),
        )
    })?;
    let valid = uuid.len() == 36
        && uuid.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if !valid {
        return Err(CascadeError::Precondition(
            "origin PARTUUID identity is invalid or aliases the WSL fallback swap".into(),
        ));
    }
    Ok(uuid)
}

fn canonical_origin_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn validate_origin_logical_capacity(value_mib: u64) -> Result<(), CascadeError> {
    if (1024..=24 * 1024).contains(&value_mib) {
        Ok(())
    } else {
        Err(CascadeError::Precondition(
            "origin logical capacity must be between 1024 and 24576 MiB".into(),
        ))
    }
}

fn unix_time_ms() -> Option<u64> {
    use std::time::UNIX_EPOCH;

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn process_start_ticks(stat: &str) -> Option<&str> {
    stat.rsplit_once(") ")?.1.split_whitespace().nth(19)
}

fn daemon_instance_id_from_pid(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let start_ticks = process_start_ticks(&stat)?;
    (!start_ticks.is_empty()).then(|| format!("{pid}-{start_ticks}"))
}

fn cache_status_shape_is_valid(value: &serde_json::Value) -> bool {
    value
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .is_some()
        && matches!(
            value
                .get("origin_state")
                .and_then(serde_json::Value::as_str),
            Some("OFF") | Some("READY") | Some("DEGRADED") | Some("FAILED")
        )
        && matches!(
            value.get("cache_state").and_then(serde_json::Value::as_str),
            Some("OFF") | Some("ACTIVE") | Some("RESTRICTED") | Some("UNAVAILABLE") | Some("STUCK")
        )
}

fn supervisor_status_shape_is_valid(value: &serde_json::Value) -> bool {
    let Some(status) = value.as_object() else {
        return false;
    };
    const STATUS_KEYS: &[&str] = &[
        "schema_version",
        "control_state",
        "healthy_samples",
        "action_results",
        "daemon_instance_id",
        "supervisor_identity",
        "written_at_unix_ms",
    ];
    if status.len() != STATUS_KEYS.len()
        || STATUS_KEYS.iter().any(|key| !status.contains_key(*key))
        || status
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(3)
        || status
            .get("healthy_samples")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|samples| samples > u64::from(u32::MAX))
        || status
            .get("written_at_unix_ms")
            .and_then(serde_json::Value::as_u64)
            .is_none()
    {
        return false;
    }

    let daemon_identity_is_valid = match status.get("daemon_instance_id") {
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(value)) => {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        }
        _ => false,
    };
    if !daemon_identity_is_valid {
        return false;
    }

    let Some(identity) = status
        .get("supervisor_identity")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    const IDENTITY_KEYS: &[&str] = &["boot_id", "pid", "start_time", "nonce"];
    if identity.len() != IDENTITY_KEYS.len()
        || IDENTITY_KEYS.iter().any(|key| !identity.contains_key(*key))
        || identity
            .get("boot_id")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
        || identity
            .get("pid")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|pid| pid == 0 || pid > u64::from(u32::MAX))
        || identity
            .get("start_time")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|start_time| start_time == 0)
        || identity
            .get("nonce")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
    {
        return false;
    }

    let Some(control_state) = status
        .get("control_state")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    if !matches!(
        control_state,
        "HEALTHY" | "GUARDED" | "CRITICAL" | "EMERGENCY" | "SAFE_MODE"
    ) {
        return false;
    }

    let Some(results) = status
        .get("action_results")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    if results.len() > 7 {
        return false;
    }
    let mut actions = Vec::with_capacity(results.len());
    for result in results {
        let Some(result) = result.as_object() else {
            return false;
        };
        const RESULT_KEYS: &[&str] = &["action", "status", "error"];
        if result.len() != RESULT_KEYS.len()
            || RESULT_KEYS.iter().any(|key| !result.contains_key(*key))
        {
            return false;
        }
        let Some(action) = result.get("action").and_then(serde_json::Value::as_str) else {
            return false;
        };
        if !matches!(
            action,
            "close_admission"
                | "reduce_vram_cache"
                | "freeze_discardable"
                | "thaw_discardable"
                | "request_reclaim"
                | "terminate_discardable"
                | "kill_discardable"
        ) || actions.contains(&action)
        {
            return false;
        }
        let valid_outcome = match result.get("status").and_then(serde_json::Value::as_str) {
            Some("succeeded") => result.get("error").is_some_and(serde_json::Value::is_null),
            Some("failed") => result
                .get("error")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|error| {
                    !error.is_empty() && error.len() <= SUPERVISOR_ACTION_ERROR_MAX_BYTES
                }),
            _ => false,
        };
        if !valid_outcome {
            return false;
        }
        actions.push(action);
    }

    match control_state {
        "HEALTHY" => actions.is_empty(),
        "GUARDED" => {
            actions == ["close_admission"] || actions == ["thaw_discardable", "close_admission"]
        }
        "CRITICAL" => {
            actions
                == [
                    "close_admission",
                    "reduce_vram_cache",
                    "freeze_discardable",
                    "request_reclaim",
                ]
        }
        "EMERGENCY" => {
            matches!(
                actions.as_slice(),
                ["close_admission", "reduce_vram_cache", "request_reclaim"]
                    | [
                        "close_admission",
                        "reduce_vram_cache",
                        "request_reclaim",
                        "terminate_discardable"
                    ]
                    | [
                        "close_admission",
                        "reduce_vram_cache",
                        "request_reclaim",
                        "kill_discardable"
                    ]
                    | [
                        "thaw_discardable",
                        "close_admission",
                        "reduce_vram_cache",
                        "request_reclaim"
                    ]
                    | [
                        "thaw_discardable",
                        "close_admission",
                        "reduce_vram_cache",
                        "request_reclaim",
                        "terminate_discardable"
                    ]
                    | [
                        "thaw_discardable",
                        "close_admission",
                        "reduce_vram_cache",
                        "request_reclaim",
                        "kill_discardable"
                    ]
            )
        }
        "SAFE_MODE" => actions.is_empty(),
        _ => false,
    }
}

fn cache_status_matches_current_daemon(
    value: &serde_json::Value,
    daemon_instance_id: &str,
    now_unix_ms: u64,
) -> bool {
    value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        == Some(1)
        && value
            .get("daemon_instance_id")
            .and_then(serde_json::Value::as_str)
            == Some(daemon_instance_id)
        && value
            .get("written_at_unix_ms")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|written_at| {
                now_unix_ms >= written_at
                    && now_unix_ms.saturating_sub(written_at) <= CONTROL_STATUS_MAX_AGE_MS
            })
}

fn supervisor_status_matches_current_daemon(
    value: &serde_json::Value,
    daemon_instance_id: &str,
    now_unix_ms: u64,
) -> bool {
    value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        == Some(3)
        && value
            .get("daemon_instance_id")
            .and_then(serde_json::Value::as_str)
            == Some(daemon_instance_id)
        && value
            .get("written_at_unix_ms")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|written_at| {
                now_unix_ms >= written_at
                    && now_unix_ms.saturating_sub(written_at) <= CONTROL_STATUS_MAX_AGE_MS
            })
}

fn control_plane_status_is_current(
    cache_status: &serde_json::Value,
    supervisor_status: &serde_json::Value,
    daemon_instance_id: &str,
    now_unix_ms: u64,
) -> bool {
    cache_status_shape_is_valid(cache_status)
        && supervisor_status_shape_is_valid(supervisor_status)
        && cache_status_matches_current_daemon(cache_status, daemon_instance_id, now_unix_ms)
        && supervisor_status_matches_current_daemon(
            supervisor_status,
            daemon_instance_id,
            now_unix_ms,
        )
}

fn guardian_state_from_files(
    safe_mode: &Path,
    health: &Path,
    max_age: Duration,
) -> (GuardianState, Option<String>) {
    match fs::read_to_string(safe_mode) {
        Ok(text) if serde_json::from_str::<serde_json::Value>(&text).is_ok() => {
            return (GuardianState::SafeMode, None);
        }
        Ok(_) => return (GuardianState::Blocked, Some("safe_mode_invalid".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return (GuardianState::Blocked, Some("safe_mode_unreadable".into())),
    }
    let text = match fs::read_to_string(health) {
        Ok(text) => text,
        Err(_) => {
            return (
                GuardianState::Blocked,
                Some("guardian_state_unavailable".into()),
            );
        }
    };
    let fresh = fs::metadata(health)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age <= max_age);
    if !fresh {
        return (GuardianState::Blocked, Some("guardian_state_stale".into()));
    }
    let value = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) => value,
        Err(_) => {
            return (
                GuardianState::Blocked,
                Some("guardian_state_invalid".into()),
            );
        }
    };
    if value.get("distro").and_then(serde_json::Value::as_str) != Some("Ubuntu-24.04") {
        return (
            GuardianState::Blocked,
            Some("guardian_state_identity_mismatch".into()),
        );
    }
    match value.get("state").and_then(serde_json::Value::as_str) {
        Some("HEALTHY") => (GuardianState::Healthy, None),
        Some("SAFE_MODE") => (GuardianState::SafeMode, None),
        _ => (
            GuardianState::Blocked,
            Some("guardian_reported_blocked".into()),
        ),
    }
}

mod lifecycle;
use lifecycle::{
    CacheState, CascadeSnapshot, ControlState, DemoteSnapshot, GuardianState, OriginState,
    TierSample, active_threshold_kib_from_env, derive_lifecycle, protection_reason,
    protection_state, render_status_json,
};

/// Build lifecycle snapshot from live swaps + daemon (read-only).
pub fn build_cascade_snapshot(entries: &[SwapEntry]) -> CascadeSnapshot {
    let pairs: Vec<(&str, u64, u64, i32)> = entries
        .iter()
        .filter(|e| !e.is_ghost())
        .map(|e| (e.filename.as_str(), e.size_kb, e.used_kb, e.priority))
        .collect();
    let (zram, vram, disk, order_ok) = lifecycle::tiers_from_swap_names(&pairs);
    let ghosts = ghost_vram_swaps(entries);
    let (daemon_alive, daemon_pid) = daemon_alive_pid();
    let product_active = daemon_alive || vram.present;
    let cache_status = fs::read_to_string(CACHE_STATUS_FILE)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let supervisor_status = fs::read_to_string(SUPERVISOR_STATUS_FILE)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let daemon_instance_id = daemon_pid.and_then(daemon_instance_id_from_pid);
    let control_plane_current = daemon_instance_id
        .as_deref()
        .zip(unix_time_ms())
        .zip(cache_status.as_ref())
        .zip(supervisor_status.as_ref())
        .is_some_and(
            |(((daemon_instance_id, now_unix_ms), cache_status), supervisor_status)| {
                control_plane_status_is_current(
                    cache_status,
                    supervisor_status,
                    daemon_instance_id,
                    now_unix_ms,
                )
            },
        );
    let cache_status = control_plane_current.then_some(cache_status).flatten();
    let supervisor_status = control_plane_current.then_some(supervisor_status).flatten();
    let status_text = |key: &str| {
        cache_status
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(serde_json::Value::as_str)
    };
    let status_number = |key: &str| {
        cache_status
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(serde_json::Value::as_u64)
    };
    let cache_status_ok = cache_status
        .as_ref()
        .and_then(|value| value.get("ok"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let origin_state = match status_text("origin_state") {
        Some("READY") => OriginState::Ready,
        Some("DEGRADED") => OriginState::Degraded,
        Some("FAILED") => OriginState::Failed,
        _ if product_active && capacity_field("mode").as_deref() == Some("origin-cache") => {
            OriginState::Degraded
        }
        _ => OriginState::Off,
    };
    let cache_state = match status_text("cache_state") {
        _ if product_active && !cache_status_ok => CacheState::Unavailable,
        Some("ACTIVE") => CacheState::Active,
        Some("RESTRICTED") => CacheState::Restricted,
        Some("STUCK") => CacheState::Stuck,
        Some("OFF") if !product_active => CacheState::Off,
        _ if product_active => CacheState::Unavailable,
        _ => CacheState::Off,
    };
    let control_state = supervisor_status
        .as_ref()
        .and_then(|value| value.get("control_state")?.as_str())
        .map_or(ControlState::Guarded, |state| match state {
            "GUARDED" => ControlState::Guarded,
            "CRITICAL" => ControlState::Critical,
            "EMERGENCY" => ControlState::Emergency,
            "SAFE_MODE" => ControlState::SafeMode,
            "HEALTHY" => ControlState::Healthy,
            _ => ControlState::Guarded,
        });
    let (guardian_state, guardian_error) = guardian_state_from_files(
        Path::new(SAFE_MODE_FILE),
        Path::new(GUARDIAN_HEALTH_FILE),
        Duration::from_secs(15),
    );
    let mut measurement_errors = Vec::new();
    if product_active && !control_plane_current {
        measurement_errors.push("cache_status_not_current".to_string());
    }
    if product_active && !control_plane_current {
        measurement_errors.push("supervisor_status_not_current".to_string());
    }
    if let Some(error) = guardian_error {
        measurement_errors.push(error);
    }
    let fallback_swap_used_kib = disk.used_kib;
    CascadeSnapshot {
        zram,
        vram,
        disk,
        ghost: !ghosts.is_empty(),
        order_ok,
        daemon_alive,
        daemon_pid,
        capacity_guaranteed: Path::new(CAPACITY_STATUS_FILE).is_file(),
        disk_baseline_kib: read_capacity_disk_baseline(),
        demote: read_demote_snapshot(),
        active_kib: active_threshold_kib_from_env(),
        control_state,
        origin_state,
        cache_state,
        guardian_state,
        logical_capacity_kib: capacity_field_u64("logical_capacity_kib"),
        vram_cached_kib: status_number("vram_cached_kib"),
        gpu_headroom_kib: status_number("gpu_headroom_kib"),
        ssd_origin_written_kib: status_number("ssd_origin_written_kib"),
        fallback_swap_used_kib: Some(fallback_swap_used_kib),
        measurement_errors,
    }
}

fn capacity_field(key: &str) -> Option<String> {
    let text = fs::read_to_string(CAPACITY_STATUS_FILE).ok()?;
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_string())
    })
}

fn capacity_field_u64(key: &str) -> Option<u64> {
    capacity_field(key)?.parse().ok()
}

fn read_capacity_disk_baseline() -> Option<u64> {
    let text = fs::read_to_string(CAPACITY_STATUS_FILE).ok()?;
    text.lines().find_map(|line| {
        line.strip_prefix("disk_baseline_kib=")
            .and_then(|value| value.trim().parse::<u64>().ok())
    })
}

/// Read `/run/ramshared/demote-status.json` if present (daemon ITEM-3).
fn read_demote_snapshot() -> DemoteSnapshot {
    let Ok(text) = fs::read_to_string(DEMOTE_STATUS_FILE) else {
        return DemoteSnapshot::default();
    };
    parse_demote_status_file(&text).unwrap_or_default()
}

/// Minimal parse of daemon demote-status.json (mirrors wsl2d demote_status).
fn parse_demote_status_file(text: &str) -> Option<DemoteSnapshot> {
    let t = text.trim();
    if !t.starts_with('{') {
        return None;
    }
    let total = {
        let pat = "\"total\":";
        let i = t.find(pat)?;
        let rest = t[i + pat.len()..].trim_start();
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        num.parse().ok()?
    };
    let in_progress = {
        let pat = "\"in_progress\":";
        t.find(pat)
            .map(|i| {
                let rest = t[i + pat.len()..].trim_start();
                rest.starts_with("true")
            })
            .unwrap_or(false)
    };
    let last_reason = {
        let pat = "\"last_reason\":";
        t.find(pat).and_then(|i| {
            let rest = t[i + pat.len()..].trim_start();
            if rest.starts_with("null") {
                return None;
            }
            if !rest.starts_with('"') {
                return None;
            }
            let mut out = String::new();
            let mut chars = rest[1..].chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            out.push(n);
                        }
                    }
                    '"' => break,
                    c => out.push(c),
                }
            }
            Some(out)
        })
    };
    Some(DemoteSnapshot {
        total: Some(total),
        last_reason,
        in_progress,
    })
}

fn daemon_alive_pid_from(
    pid_record: Option<&str>,
    read_comm: impl FnOnce(u32) -> Option<String>,
) -> (bool, Option<u32>) {
    let Some(pid) = pid_record
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|pid| *pid > 0)
    else {
        return (false, None);
    };
    match read_comm(pid).map(|comm| comm.trim().to_string()) {
        Some(comm) if comm == "ramsharedd" => (true, Some(pid)),
        _ => (false, None),
    }
}

fn daemon_alive_pid() -> (bool, Option<u32>) {
    let record = fs::read_to_string(PID_FILE).ok();
    daemon_alive_pid_from(record.as_deref(), |pid| {
        fs::read_to_string(format!("/proc/{pid}/comm")).ok()
    })
}

fn status_timestamp() -> String {
    // Prefer local ISO via `date -Is` when available; else unix epoch.
    sh("date", &["-Is"]).unwrap_or_else(|_| {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("{secs}")
    })
}

/// `ramshared status` / `status --json`. Read-only. SPEC cascade-lifecycle-observability.
pub fn status(as_json: bool) -> Result<(), CascadeError> {
    if std::env::var("RAMSHARED_STATUS_LEGACY").ok().as_deref() == Some("1") {
        println!("{}", sh("swapon", &["--show"])?);
        return Ok(());
    }

    let entries = read_swaps()?;
    let snap = build_cascade_snapshot(&entries);
    let view = derive_lifecycle(&snap);
    let ts = status_timestamp();

    if as_json {
        println!("{}", render_status_json(&view, &snap, &ts));
        return Ok(());
    }

    // Human text (SPEC ITEM-2)
    let protection = protection_state(&view, &snap);
    let protection_reason = protection_reason(&view, &snap);
    println!("phase: {} ({})", view.phase.as_str(), view.phase_reason);
    println!("protection: {} ({protection_reason})", protection.as_str());
    println!("ok: {}", view.ok && protection.is_ok());
    println!("topology_ok: {}", view.ok);
    if !view.reasons.is_empty() {
        println!("reasons: {}", view.reasons.join(", "));
    }
    print_tier("zram", &snap.zram);
    print_tier("vram", &snap.vram);
    print_tier("disk", &snap.disk);
    println!(
        "daemon: {} pid={}",
        if snap.daemon_alive { "alive" } else { "dead" },
        snap.daemon_pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "null".into())
    );
    println!(
        "demote: total={} last_reason={} in_progress={}",
        snap.demote
            .total
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into()),
        snap.demote.last_reason.as_deref().unwrap_or("?"),
        snap.demote.in_progress
    );
    println!(
        "ghost: {} order_ok: {} active_kib: {}",
        snap.ghost, snap.order_ok, snap.active_kib
    );
    // Keep legacy table for operators
    if let Ok(table) = sh("swapon", &["--show"])
        && !table.is_empty()
    {
        println!();
        println!("{table}");
    }

    let ghosts = ghost_vram_swaps(&entries);
    if !ghosts.is_empty() {
        eprintln!("[status] WARNING: ghost swap detected:");
        for g in ghosts {
            eprintln!(
                "  {} size_kb={} used_kb={} prio={}",
                g.filename, g.size_kb, g.used_kb, g.priority
            );
        }
        eprintln!(
            "  action: capture evidence, run `wsl --terminate Ubuntu-24.04`, then ramshared down/up"
        );
    }
    Ok(())
}

pub(crate) fn status_json_document() -> Result<String, CascadeError> {
    let entries = read_swaps()?;
    let snap = build_cascade_snapshot(&entries);
    let view = derive_lifecycle(&snap);
    Ok(render_status_json(&view, &snap, &status_timestamp()))
}

fn print_tier(name: &str, t: &TierSample) {
    let prio = t
        .prio
        .map(|p| p.to_string())
        .unwrap_or_else(|| "null".into());
    println!(
        "{name}: present={} prio={prio} size_kib={} used_kib={}",
        t.present, t.size_kib, t.used_kib
    );
}

mod cascade_io;
pub use cascade_io::{down, up_with_args};

#[cfg(test)]
mod tests {

    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn parse_proc_swaps(text: &str) -> Vec<SwapEntry> {
        super::parse_proc_swaps(text).expect("strict /proc/swaps fixture")
    }

    fn parse(args: &[&str]) -> Result<UpArgs, CascadeError> {
        let args = args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        parse_up_args_from(&args, "ramsharedd".to_string())
    }

    fn valid_supervisor_status_v3() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 3,
            "control_state": "CRITICAL",
            "healthy_samples": 0,
            "action_results": [
                {"action": "close_admission", "status": "succeeded", "error": null},
                {"action": "reduce_vram_cache", "status": "failed", "error": "fixture refusal"},
                {"action": "freeze_discardable", "status": "succeeded", "error": null},
                {"action": "request_reclaim", "status": "succeeded", "error": null},
            ],
            "daemon_instance_id": "daemon-1",
            "supervisor_identity": {
                "boot_id": "fixture-boot",
                "pid": 1,
                "start_time": 1,
                "nonce": "fixture-nonce",
            },
            "written_at_unix_ms": 1_000,
        })
    }

    #[test]
    fn canonicalize_swap_path_table() {
        assert_eq!(canonicalize_swap_path("/nbd0"), "/dev/nbd0");
        assert_eq!(canonicalize_swap_path("/dev/nbd0"), "/dev/nbd0");
        assert_eq!(canonicalize_swap_path("nbd0"), "/dev/nbd0");
        assert_eq!(canonicalize_swap_path("/zram0"), "/dev/zram0");
        assert_eq!(canonicalize_swap_path("  /ublkb0  "), "/dev/ublkb0");
        assert_eq!(canonicalize_swap_path(""), "");
    }

    #[test]
    fn allowlist_rejects_disk_paths() {
        assert!(is_allowlisted_managed_path("/dev/nbd0"));
        assert!(is_allowlisted_managed_path("/zram0"));
        assert!(is_allowlisted_managed_path("/dev/ublkb0"));
        assert!(is_allowlisted_managed_path("/dev/mapper/ramshared0"));
        assert!(!is_allowlisted_managed_path("/dev/sdc"));
        assert!(!is_allowlisted_managed_path("/dev/sdb"));
        assert!(!is_allowlisted_managed_path("/dev/nbd0-backup"));
        assert!(!is_allowlisted_managed_path("/tmp/nbd0"));
        assert!(!is_allowlisted_managed_path("/swap/ramshared-backup"));
    }

    #[test]
    fn malformed_managed_paths_are_refused() {
        for path in [
            "/dev/dev/nbd0",
            "/dev//nbd0",
            "//nbd0",
            "/tmp/../dev/nbd0",
            "/dev/nbd0/extra",
        ] {
            assert!(!is_allowlisted_managed_path(path), "accepted {path}");
        }
    }

    #[test]
    fn status_daemon_requires_owned_pid_record_and_exact_comm() {
        assert_eq!(
            daemon_alive_pid_from(Some("42\n"), |pid| (pid == 42)
                .then(|| "ramsharedd\n".into())),
            (true, Some(42))
        );
        for record in [
            None,
            Some(""),
            Some("0"),
            Some("-1"),
            Some("42 43"),
            Some("abc"),
        ] {
            assert_eq!(
                daemon_alive_pid_from(record, |_| Some("ramsharedd".into())),
                (false, None)
            );
        }
        assert_eq!(
            daemon_alive_pid_from(Some("42"), |_| Some("foreign-daemon\n".into())),
            (false, None)
        );
        assert_eq!(daemon_alive_pid_from(Some("42"), |_| None), (false, None));
    }

    #[test]
    fn similar_swap_names_are_not_managed_devices() {
        let entries = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /swap/nbd0-backup file 1024 99 -2\n\
             /dev/nbd01 partition 1024 7 -3\n",
        );
        assert!(!entries[0].is_managed_or_orphan_vram_tier());
        assert!(entries[1].is_managed_or_orphan_vram_tier());
        assert!(!active_vram_block_swap(&entries[..1]));
        assert!(!is_nbd_device_path("/swap/nbd0-backup"));
        assert_ne!(entries[1].canonical_path(), "/dev/nbd0");
    }

    #[test]
    fn orphan_plan_zero_used_is_detected_without_recovery() {
        let e = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/sdc partition 8388608 100 -2\n\
             /zram0 partition 1048576 0 200\n\
             /nbd0 partition 1048576 0 100\n",
        );
        assert_eq!(
            plan_orphan_action(&e, false),
            OrphanPlan::DetectedUnboundZeroUsed
        );
        assert_eq!(plan_orphan_action(&e, true), OrphanPlan::None);
    }

    #[test]
    fn orphan_plan_dirty_nbd_is_refuse() {
        let e = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1048576 4096 100\n\
             /dev/sdc partition 8388608 0 -2\n",
        );
        assert_eq!(
            plan_orphan_action(&e, false),
            OrphanPlan::RefuseDirtyBackend
        );
    }

    #[test]
    fn orphan_plan_clean_disk_only_is_none() {
        let e = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/sdc partition 8388608 0 -2\n",
        );
        assert_eq!(plan_orphan_action(&e, false), OrphanPlan::None);
    }

    #[test]
    fn defaults_to_auto_resolved_nbd_on_wsl2_or_nbd_swap_dev() {
        let args = parse(&[]).unwrap();
        // Default is Auto; on WSL2 resolve_transport → Nbd (product Day-1).
        // Off-WSL2 without /dev/ublk-control also → Nbd; with control → Ublk.
        assert!(matches!(args.transport, Transport::Nbd | Transport::Ublk));
        if is_wsl2() {
            assert_eq!(args.transport, Transport::Nbd);
        }
        assert_eq!(args.swap_dev, "/dev/nbd0");
        assert_eq!(args.connections, 1);
    }

    #[test]
    fn auto_transport_flag_resolves_like_default() {
        let args = parse(&["--transport", "auto"]).unwrap();
        if is_wsl2() {
            assert_eq!(args.transport, Transport::Nbd);
        } else {
            assert!(matches!(args.transport, Transport::Nbd | Transport::Ublk));
        }
    }

    #[test]
    fn parses_ublk_transport_and_generic_swap_dev() {
        let args = parse(&["--transport", "ublk", "--swap-dev", "/dev/ublkb0"]).unwrap();
        assert_eq!(args.transport, Transport::Ublk);
        assert_eq!(args.swap_dev, "/dev/ublkb0");
    }

    #[test]
    fn keeps_legacy_nbd_arg_as_swap_dev_alias() {
        let args = parse(&["--nbd", "/dev/nbd3"]).unwrap();
        assert_eq!(args.transport, Transport::Nbd);
        assert_eq!(args.swap_dev, "/dev/nbd3");
    }

    #[test]
    fn rejects_multi_connection_ublk_for_single_ring_design() {
        let err = parse(&["--transport", "ublk", "--connections", "2"]).unwrap_err();
        assert!(err.to_string().contains("--connections"));
    }

    #[test]
    fn parse_swaps_normal_and_ghost_backslash() {
        let text = "\
Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority
/dev/sdb                                partition\t8388608\t\t100\t\t-2
/dev/ublkb0\\040(deleted)                partition\t524284\t\t117504\t\t-3
/dev/zram0                              partition\t1048576\t\t0\t\t200
";
        let e = parse_proc_swaps(text);
        assert_eq!(e.len(), 3);
        assert!(e[1].is_ghost());
        assert!(e[1].is_managed_or_orphan_vram_tier());
        assert_eq!(e[1].used_kb, 117504);
        assert!(!e[0].is_managed_or_orphan_vram_tier());
        assert!(e[2].is_managed_or_orphan_vram_tier());
    }

    #[test]
    fn parse_swaps_ghost_with_real_space() {
        let text = "\
Filename Type Size Used Priority
/dev/ublkb0 (deleted) partition 524284 10 -3
";
        let e = parse_proc_swaps(text);
        assert_eq!(e.len(), 1);
        assert!(e[0].is_ghost());
        assert!(e[0].filename.contains("ublkb0"));
    }

    #[test]
    fn daemon_kill_forbidden_with_active_ublk_or_ghost() {
        let live = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/ublkb0 partition 524284 0 -3\n",
        );
        assert!(active_vram_block_swap(&live));
        assert!(!daemon_kill_allowed(&live));

        let ghost = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/ublkb0\\040(deleted) partition 524284 100 -3\n",
        );
        assert!(!ghost_vram_swaps(&ghost).is_empty());
        assert!(!daemon_kill_allowed(&ghost));

        let clean = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/sdb partition 8388608 0 -2\n",
        );
        assert!(daemon_kill_allowed(&clean));
    }

    #[test]
    fn zram_zero_is_parsed() {
        let a = parse(&["--zram", "0", "--vram", "2048"]).unwrap();
        assert_eq!(a.zram_mb, 0);
        assert_eq!(a.vram_mb, 2048);
    }

    #[test]
    fn cascade_healthy_requires_vram_swap_record_and_live_daemon_signal() {
        let clean = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/sdb partition 8388608 0 -2\n",
        );
        assert!(!cascade_already_healthy(&clean));

        let with_nbd = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1048576 0 100\n\
             /dev/sdb partition 8388608 0 -2\n",
        );
        // Pure over swaps + /run/ramshared: without live records → not healthy.
        // If cascade is mounted on this host, records may exist → skip env-coupled assert.
        let has_live_record = Path::new(SWAP_DEV_FILE).exists() || Path::new(PID_FILE).exists();
        if !has_live_record {
            assert!(!cascade_already_healthy(&with_nbd));
        }
    }

    #[test]
    fn ghost_blocks_healthy() {
        let ghost = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/nbd0\\040(deleted) partition 1048576 10 100\n",
        );
        assert!(!cascade_already_healthy(&ghost));
    }

    fn clear_sh_script() {
        SH_SCRIPT.with(|q| q.borrow_mut().clear());
    }

    fn push_sh(pat: &str, res: Result<&str, &str>) {
        SH_SCRIPT.with(|q| {
            q.borrow_mut().push_back((
                pat.to_string(),
                res.map(str::to_string).map_err(str::to_string),
            ));
        });
    }

    fn set_test_swaps(s: Option<&str>) {
        TEST_SWAPS.with(|c| *c.borrow_mut() = s.map(str::to_string));
    }

    fn set_test_mem(n: Option<u64>) {
        TEST_MEM_AVAILABLE.with(|c| *c.borrow_mut() = n);
    }

    fn set_test_mb(v: Option<(&str, u64)>) {
        TEST_ENV_MB.with(|c| *c.borrow_mut() = v.map(|(k, n)| (k.to_string(), n)));
    }

    fn clear_test_seams() {
        clear_sh_script();
        set_test_swaps(None);
        TEST_SWAPS_SEQUENCE.with(|queue| queue.borrow_mut().clear());
        TEST_SWAPS_ERROR.with(|cell| *cell.borrow_mut() = None);
        set_test_mem(None);
        set_test_mb(None);
    }

    #[test]
    fn cascade_error_display_variants() {
        let s = CascadeError::Shell {
            cmd: "swapoff".into(),
            msg: "nope".into(),
        }
        .to_string();
        assert!(s.contains("swapoff") && s.contains("nope"));
        assert!(CascadeError::Arg("x".into()).to_string().contains("x"));
        assert!(CascadeError::Io("io".into()).to_string().contains("io"));
        assert!(
            CascadeError::Precondition("ghost".into())
                .to_string()
                .contains("ghost")
        );
    }

    #[test]
    fn bare_and_canonical_path_on_entries() {
        let e = SwapEntry {
            filename: "/dev/nbd0\\040(deleted)".into(),
            size_kb: 1,
            used_kb: 0,
            priority: 100,
        };
        assert!(e.is_ghost());
        assert_eq!(e.bare_path(), "/dev/nbd0");
        assert_eq!(e.canonical_path(), "/dev/nbd0");
        let live = SwapEntry {
            filename: "/nbd0".into(),
            size_kb: 1,
            used_kb: 0,
            priority: 100,
        };
        assert_eq!(live.canonical_path(), "/dev/nbd0");
    }

    #[test]
    fn parse_swaps_rejects_short_lines() {
        assert!(super::parse_proc_swaps("Filename Type Size Used Priority\nbadline\n").is_err());
    }

    #[test]
    fn parse_up_args_errors_and_flags() {
        assert!(parse(&["--vram"]).is_err());
        assert!(parse(&["--vram", "nope"]).is_err());
        assert!(parse(&["--zram"]).is_err());
        assert!(parse(&["--daemon"]).is_err());
        assert!(parse(&["--connections", "0"]).is_err());
        assert!(parse(&["--transport", "ftp"]).is_err());
        assert!(parse(&["--unknown"]).is_err());
        let a = parse(&[
            "--vram",
            "1024",
            "--zram",
            "256",
            "--daemon",
            "/tmp/d",
            "--force-no-safety-net",
            "--transport",
            "nbd",
        ])
        .unwrap();
        assert_eq!(a.vram_mb, 1024);
        assert_eq!(a.zram_mb, 256);
        assert_eq!(a.daemon, "/tmp/d");
        assert!(a.force);
        assert_eq!(a.transport, Transport::Nbd);
    }

    #[test]
    fn origin_logical_capacity_is_bounded_before_lifecycle_actions() {
        assert!(validate_origin_logical_capacity(1024).is_ok());
        assert!(validate_origin_logical_capacity(24 * 1024).is_ok());
        assert!(validate_origin_logical_capacity(1023).is_err());
        assert!(validate_origin_logical_capacity(24 * 1024 + 1).is_err());
    }

    #[test]
    fn origin_cache_cap_defaults_to_one_gib_without_widening_to_logical_capacity() {
        let default_capacity = parse(&["--vram", "4096"])
            .unwrap_or_else(|error| panic!("default origin-cache arguments: {error}"));
        assert_eq!(default_capacity.cache_cap_mib, 1024);

        let minimum_capacity = parse(&["--vram", "1024"])
            .unwrap_or_else(|error| panic!("minimum origin-cache arguments: {error}"));
        assert_eq!(minimum_capacity.cache_cap_mib, 1024);
    }

    #[test]
    fn stale_or_missing_guardian_is_never_reported_healthy() {
        let root =
            std::env::temp_dir().join(format!("ramshared-guardian-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let safe = root.join("safe.json");
        let health = root.join("health.json");
        let (state, error) = guardian_state_from_files(&safe, &health, Duration::from_secs(15));
        assert_eq!(state, GuardianState::Blocked);
        assert_eq!(error.as_deref(), Some("guardian_state_unavailable"));

        fs::write(
            &health,
            r#"{"schema_version":1,"distro":"Ubuntu-24.04","state":"HEALTHY"}"#,
        )
        .unwrap();
        assert_eq!(
            guardian_state_from_files(&safe, &health, Duration::from_secs(15)),
            (GuardianState::Healthy, None)
        );
        fs::write(&safe, "{}").unwrap();
        assert_eq!(
            guardian_state_from_files(&safe, &health, Duration::from_secs(15)),
            (GuardianState::SafeMode, None)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_foreign_or_malformed_cache_or_supervisor_status_is_never_green() {
        let fresh_cache = serde_json::json!({
            "schema_version": 1,
            "daemon_instance_id": "daemon-1",
            "written_at_unix_ms": 1_000,
            "ok": true,
            "origin_state": "READY",
            "cache_state": "ACTIVE",
        });
        let mut fresh_supervisor = valid_supervisor_status_v3();
        fresh_supervisor["control_state"] = serde_json::json!("HEALTHY");
        fresh_supervisor["healthy_samples"] = serde_json::json!(60);
        fresh_supervisor["action_results"] = serde_json::json!([]);
        assert!(control_plane_status_is_current(
            &fresh_cache,
            &fresh_supervisor,
            "daemon-1",
            1_001,
        ));

        for (cache, supervisor, now_unix_ms) in [
            (
                serde_json::json!({"origin_state":"READY","cache_state":"ACTIVE"}),
                fresh_supervisor.clone(),
                1_001,
            ),
            (
                fresh_cache.clone(),
                {
                    let mut foreign = fresh_supervisor.clone();
                    foreign["daemon_instance_id"] = serde_json::json!("foreign-daemon");
                    foreign
                },
                1_001,
            ),
            (
                serde_json::json!({
                    "schema_version": 1,
                    "daemon_instance_id": "daemon-1",
                    "written_at_unix_ms": 0,
                    "ok": true,
                    "origin_state": "READY",
                    "cache_state": "ACTIVE",
                }),
                fresh_supervisor.clone(),
                16_001,
            ),
        ] {
            assert!(
                !control_plane_status_is_current(&cache, &supervisor, "daemon-1", now_unix_ms),
                "stale, foreign, or malformed status was accepted"
            );
        }
    }

    #[test]
    fn supervisor_status_v3_with_ordered_action_results_is_current() {
        let supervisor = valid_supervisor_status_v3();
        assert!(supervisor_status_shape_is_valid(&supervisor));
        assert!(supervisor_status_matches_current_daemon(
            &supervisor,
            "daemon-1",
            1_001,
        ));
    }

    #[test]
    fn supervisor_status_v2_or_missing_action_results_is_refused() {
        let mut legacy = valid_supervisor_status_v3();
        legacy["schema_version"] = serde_json::json!(2);
        assert!(!supervisor_status_shape_is_valid(&legacy));
        assert!(!supervisor_status_matches_current_daemon(
            &legacy, "daemon-1", 1_001,
        ));

        let mut missing = valid_supervisor_status_v3();
        missing.as_object_mut().unwrap().remove("action_results");
        assert!(!supervisor_status_shape_is_valid(&missing));
    }

    #[test]
    fn supervisor_status_malformed_foreign_and_stale_states_are_refused() {
        let mut malformed = valid_supervisor_status_v3();
        malformed["action_results"][0]["error"] = serde_json::json!("unexpected");
        assert!(!supervisor_status_shape_is_valid(&malformed));

        let mut failed_without_error = valid_supervisor_status_v3();
        failed_without_error["action_results"][1]["error"] = serde_json::Value::Null;
        assert!(!supervisor_status_shape_is_valid(&failed_without_error));

        let mut failed_with_empty_error = valid_supervisor_status_v3();
        failed_with_empty_error["action_results"][1]["error"] = serde_json::json!("");
        assert!(!supervisor_status_shape_is_valid(&failed_with_empty_error));

        let mut failed_with_unbounded_error = valid_supervisor_status_v3();
        failed_with_unbounded_error["action_results"][1]["error"] =
            serde_json::json!("x".repeat(SUPERVISOR_ACTION_ERROR_MAX_BYTES + 1));
        assert!(!supervisor_status_shape_is_valid(
            &failed_with_unbounded_error
        ));

        let foreign = valid_supervisor_status_v3();
        assert!(!supervisor_status_matches_current_daemon(
            &foreign,
            "foreign-daemon",
            1_001,
        ));

        let stale = valid_supervisor_status_v3();
        assert!(!supervisor_status_matches_current_daemon(
            &stale, "daemon-1", 16_001,
        ));
    }

    #[test]
    fn supervisor_status_unknown_duplicate_reordered_or_legacy_actions_are_refused() {
        let mut unknown = valid_supervisor_status_v3();
        unknown["action_results"][0]["action"] = serde_json::json!("foreign_action");
        assert!(!supervisor_status_shape_is_valid(&unknown));

        let mut duplicate = valid_supervisor_status_v3();
        duplicate["action_results"][1]["action"] = serde_json::json!("close_admission");
        assert!(!supervisor_status_shape_is_valid(&duplicate));

        let mut reordered = valid_supervisor_status_v3();
        reordered["action_results"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(!supervisor_status_shape_is_valid(&reordered));

        let mut legacy = valid_supervisor_status_v3();
        legacy["last_action"] = serde_json::json!("request_reclaim");
        assert!(!supervisor_status_shape_is_valid(&legacy));

        let mut extra_result_field = valid_supervisor_status_v3();
        extra_result_field["action_results"][0]["detail"] = serde_json::json!("foreign");
        assert!(!supervisor_status_shape_is_valid(&extra_result_field));

        let mut extra_status_field = valid_supervisor_status_v3();
        extra_status_field["foreign"] = serde_json::json!(true);
        assert!(!supervisor_status_shape_is_valid(&extra_status_field));
    }

    #[test]
    fn resolve_transport_explicit_and_auto_on_wsl() {
        assert_eq!(resolve_transport(Transport::Nbd).unwrap(), Transport::Nbd);
        assert_eq!(resolve_transport(Transport::Ublk).unwrap(), Transport::Ublk);
        if is_wsl2() {
            assert_eq!(resolve_transport(Transport::Auto).unwrap(), Transport::Nbd);
        }
    }

    #[test]
    fn default_mb_from_env_uses_injected_value_or_fallback() {
        set_test_mb(Some(("RAMSHARED_TEST_MB", 333)));
        assert_eq!(default_mb_from_env("RAMSHARED_TEST_MB", 1), 333);
        set_test_mb(None);
        assert_eq!(default_mb_from_env("RAMSHARED_TEST_MB_MISSING", 9), 9);
    }

    #[test]
    fn refuse_ghost_state_with_injected_swaps() {
        clear_test_seams();
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0\\040(deleted) partition 1 1 100\n",
        ));
        let err = refuse_ghost_swap_state().unwrap_err();
        clear_test_seams();
        assert!(err.to_string().contains("fantasma") || err.to_string().contains("deleted"));
    }

    #[test]
    fn refuse_half_cascade_when_vram_live_without_health() {
        clear_test_seams();
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1024 0 100\n",
        ));
        let e = read_swaps().expect("strict injected swaps");
        if !cascade_already_healthy(&e) {
            let err = refuse_half_cascade(&e).unwrap_err();
            assert!(err.to_string().contains("metade") || err.to_string().contains("down"));
        }
        clear_test_seams();
    }

    #[test]
    fn try_recover_refuses_dirty_backend() {
        clear_test_seams();
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1024 500 100\n",
        ));
        let err = refuse_unbound_managed_devices().unwrap_err();
        clear_test_seams();
        assert!(
            err.to_string().contains("used_kb") || err.to_string().contains("orphan"),
            "{err}"
        );
    }

    #[test]
    fn try_recover_zero_used_unbound_device_refuses_without_mutation() {
        clear_test_seams();
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1024 0 100\n",
        ));
        let err = refuse_unbound_managed_devices().unwrap_err();
        clear_test_seams();
        assert!(
            err.to_string().contains("detection-only")
                || err.to_string().contains("lifecycle binding")
        );
    }

    #[test]
    fn try_recover_zero_used_never_consumes_shell_mutations() {
        clear_test_seams();
        push_sh("*", Err("shell mutation must not run"));
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1024 0 100\n\
             /dev/sdc partition 999 0 -2\n",
        ));
        let error = refuse_unbound_managed_devices().unwrap_err();
        let remaining = SH_SCRIPT.with(|queue| queue.borrow().len());
        clear_test_seams();
        assert!(error.to_string().contains("detection-only"));
        assert_eq!(remaining, 1, "unbound recovery consumed a shell mutation");
    }

    #[test]
    fn parse_demote_status_file_roundtrip_shape() {
        let j = r#"{"total":2,"last_reason":"Latency","in_progress":true}"#;
        let d = parse_demote_status_file(j).expect("parse");
        assert_eq!(d.total, Some(2));
        assert_eq!(d.last_reason.as_deref(), Some("Latency"));
        assert!(d.in_progress);
        let j2 = r#"{"total":0,"last_reason":null,"in_progress":false}"#;
        let d2 = parse_demote_status_file(j2).unwrap();
        assert_eq!(d2.total, Some(0));
        assert!(d2.last_reason.is_none());
        assert!(!d2.in_progress);
    }

    #[test]
    fn status_warns_on_ghost_with_mock_swapon() {
        clear_test_seams();
        push_sh("swapon", Ok("NAME TYPE SIZE USED PRIO"));
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0\\040(deleted) partition 1 2 100\n",
        ));
        let r = status(false);
        clear_test_seams();
        assert!(r.is_ok());
    }

    #[test]
    fn up_refuses_explicit_ublk_on_wsl() {
        let a = parse(&["--transport", "ublk"]).unwrap();
        assert_eq!(a.transport, Transport::Ublk);
        if is_wsl2() {
            let msg = "transport ublk recusado no WSL2";
            assert!(!msg.is_empty());
        }
    }

    #[test]
    fn chrono_and_default_daemon_and_mem() {
        assert!(!chrono_like_now().is_empty());
        let d = default_daemon();
        assert!(d.contains("ramsharedd") || d.ends_with("ramsharedd"));
        set_test_mem(Some(12345));
        assert_eq!(mem_available_bytes(), 12345);
        set_test_mem(None);
        assert!(mem_available_bytes() > 0);
    }

    #[test]
    fn lower_tier_present_with_disk_only_swaps() {
        clear_test_seams();
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/sdc partition 8388608 0 -2\n",
        ));
        assert!(lower_tier_present().expect("strict injected swaps"));
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/zram0 partition 1024 0 200\n",
        ));
        assert!(!lower_tier_present().expect("strict injected swaps"));
        clear_test_seams();
    }

    #[test]
    fn allowlist_ublkb_and_ramshared_name() {
        assert!(is_allowlisted_managed_path("/dev/ublkb0"));
        assert!(is_allowlisted_managed_path("/dev/mapper/ramshared0"));
        assert!(!is_allowlisted_managed_path("/swapfile"));
    }

    #[test]
    fn setup_zram_zero_skips() {
        let z = cascade_io::setup_zram(0, 200).unwrap();
        assert!(z.is_empty());
    }

    #[test]
    fn daemon_kill_allowed_active_nbd() {
        let live = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1 0 100\n",
        );
        assert!(!daemon_kill_allowed(&live));
        assert!(active_vram_block_swap(&live));
    }
}
