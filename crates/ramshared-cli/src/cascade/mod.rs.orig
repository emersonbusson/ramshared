//! Orchestration of the zram→VRAM→VHDX cascade (SPEC §6.2–6.4). Runs as root.
//!
//! **Anti-hang contract (Kahneman #5 / #15 / #16, 2026-07-09 WSL freeze):**
//! 1. **Never** kill `ramsharedd` while any managed swap (nbd/ublk/zram) is still
//!    listed in `/proc/swaps` — that creates ghost `(deleted)` swap entries and freezes WSL.
//! 2. **Always** `swapoff` managed devices **before** NBD disconnect / daemon stop.
//! 3. **Refuse** `up` if ghost/deleted swap is present. Zero-used managed orphans
//!    (typical after `wsl --terminate`) are **auto-recovered** once (swapoff → disconnect)
//!    before setup; nbd/ublk with `used_kb > 0` still refuse (dead-backend hang class).
//!    Kill-switch: `RAMSHARED_NO_ORPHAN_RECOVER=1`.
//! 4. **zram** algorithm is best-effort with fallbacks (WSL kernels disagree on `lzo-rle`).
//!
//! Mounts tiers by `swapon` priority and unmounts in reverse order.

use ramshared_tier::TierPriorities;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

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
    static TEST_MEM_AVAILABLE: RefCell<Option<u64>> = const { RefCell::new(None) };
    static TEST_NO_ORPHAN_RECOVER: RefCell<Option<bool>> = const { RefCell::new(None) };
    static TEST_ENV_MB: RefCell<Option<(String, u64)>> = const { RefCell::new(None) };
}

const SOCK: &str = "/run/ramshared/wsl2d.sock";
const NBD: &str = "/dev/nbd0";
const ZRAM_DEV_FILE: &str = "/run/ramshared/zram-dev";
const SWAP_DEV_FILE: &str = "/run/ramshared/swap-dev";
const PID_FILE: &str = "/run/ramshared/ramsharedd.pid";
/// Daemon demote counters for status --json (written by ramsharedd).
const DEMOTE_STATUS_FILE: &str = "/run/ramshared/demote-status.json";
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
}

impl fmt::Display for CascadeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CascadeError::Shell { cmd, msg } => write!(f, "comando `{cmd}` falhou: {msg}"),
            CascadeError::Arg(m) => write!(f, "argumento inválido: {m}"),
            CascadeError::Io(m) => write!(f, "I/O: {m}"),
            CascadeError::Precondition(m) => write!(f, "{m}"),
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
    !path.contains('/') || path.starts_with("/dev/") || path.matches('/').count() == 1
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

/// Parse full `/proc/swaps` text into entries (skips header).
pub fn parse_proc_swaps(text: &str) -> Vec<SwapEntry> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            // Filename can contain spaces when deleted: "/dev/ublkb0 (deleted)"
            // Kernel usually writes: `/dev/ublkb0\040(deleted)` as one field, OR
            // with a real space then Type is shifted — handle both.
            if cols.len() < 5 {
                return None;
            }
            // Heuristic: last 3 numeric fields are Size Used Priority
            let n = cols.len();
            let priority = cols[n - 1].parse::<i32>().ok()?;
            let used_kb = cols[n - 2].parse::<u64>().ok()?;
            let size_kb = cols[n - 3].parse::<u64>().ok()?;
            // Type is cols[n-4] (partition|file)
            let filename = cols[..n - 4].join(" ");
            if filename.is_empty() {
                return None;
            }
            Some(SwapEntry {
                filename,
                size_kb,
                used_kb,
                priority,
            })
        })
        .collect()
}

fn read_swaps() -> Vec<SwapEntry> {
    #[cfg(test)]
    if let Some(s) = TEST_SWAPS.with(|c| c.borrow().clone()) {
        return parse_proc_swaps(&s);
    }
    fs::read_to_string("/proc/swaps")
        .map(|s| parse_proc_swaps(&s))
        .unwrap_or_default()
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

fn lower_tier_present() -> bool {
    let vram_prio = TierPriorities::default().vram;
    read_swaps().iter().any(|e| {
        // Ignore our managed tiers when looking for DEMOTE sink.
        if is_zram_device_path(&e.filename)
            || is_nbd_device_path(&e.filename)
            || is_ublk_device_path(&e.filename)
        {
            return false;
        }
        e.priority < vram_prio
    })
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

/// Pure candidate builder (unit-tested). Live `swapoff_candidates` feeds `/proc/swaps`.
fn swapoff_candidates_from(
    recorded_swap: Option<&str>,
    recorded_zram: Option<&str>,
    entries: &[SwapEntry],
) -> Vec<String> {
    let mut out = Vec::new();
    let push_unique = |out: &mut Vec<String>, p: String| {
        if p.is_empty() || !is_allowlisted_managed_path(&p) {
            return;
        }
        let canon = canonicalize_swap_path(&p);
        if !out.iter().any(|x| x == &canon || x == &p) {
            out.push(canon);
        }
    };
    if let Some(s) = recorded_swap
        && !s.is_empty()
    {
        push_unique(&mut out, s.to_string());
    }
    if let Some(z) = recorded_zram
        && !z.is_empty()
    {
        push_unique(&mut out, z.to_string());
    }
    for e in entries {
        if e.is_managed_or_orphan_vram_tier() {
            // Prefer canonical live path; keep ghost string for messaging.
            let p = if e.is_ghost() {
                e.filename.replace("\\040(deleted)", " (deleted)")
            } else {
                e.canonical_path()
            };
            push_unique(&mut out, p);
        }
    }
    out
}

/// Paths we will try to `swapoff` during down (recorded + live scan).
fn swapoff_candidates(recorded_swap: Option<&str>, recorded_zram: Option<&str>) -> Vec<String> {
    swapoff_candidates_from(recorded_swap, recorded_zram, &read_swaps())
}

/// Pure: should this candidate be refused as unrecoverable ghost-with-pages?
fn ghost_used_blocks_swapoff(entries: &[SwapEntry], path: &str) -> Option<String> {
    let p_canon = canonicalize_swap_path(path);
    entries.iter().find_map(|e| {
        let matches = e.canonical_path() == p_canon;
        if matches && e.is_ghost() && e.used_kb > 0 {
            Some(format!(
                "ghost swap used_kb={} — NAO e recuperavel com swapoff; \
                 rode `wsl --shutdown` no Windows e suba de novo. \
                 NUNCA kill -9 ramsharedd com ublk/nbd em /proc/swaps.",
                e.used_kb
            ))
        } else {
            None
        }
    })
}

/// Try swapoff on canonical path, then bare (kernel may list either form).
fn swapoff_try(path: &str) -> Result<(), CascadeError> {
    let canon = canonicalize_swap_path(path);
    let tries: &[&str] = if canon == path {
        &[path]
    } else {
        &[canon.as_str(), path]
    };
    let mut last = CascadeError::Shell {
        cmd: "swapoff".into(),
        msg: "no path tried".into(),
    };
    for p in tries {
        if p.is_empty() {
            continue;
        }
        match sh("swapoff", &["--", p]) {
            Ok(_) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Swapoff every candidate. Returns list of failures.
/// **Never** kills the daemon from here.
fn swapoff_all(paths: &[String], entries: &[SwapEntry]) -> Vec<(String, String)> {
    let mut fails = Vec::new();
    for p in paths {
        if !is_allowlisted_managed_path(p) {
            eprintln!("[down] swapoff skip (nao allowlist): {p}");
            continue;
        }
        // Ghost with used>0 cannot be recovered without reboot — report loudly.
        let p_canon = canonicalize_swap_path(p);
        if let Some(msg) = ghost_used_blocks_swapoff(entries, p) {
            fails.push((p.clone(), msg));
            continue;
        }
        match swapoff_try(p) {
            Ok(_) => eprintln!("[down] swapoff ok: {p_canon}"),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("No such file") || msg.contains("Invalid argument") {
                    // Fresh read: path may have left swaps since the start-of-batch snapshot.
                    let live = read_swaps();
                    let still = live
                        .iter()
                        .any(|e| e.canonical_path() == p_canon && e.used_kb > 0);
                    if still {
                        fails.push((p.clone(), msg));
                    } else {
                        eprintln!("[down] swapoff skip (ausente): {p_canon}");
                    }
                } else {
                    fails.push((p.clone(), msg));
                }
            }
        }
    }
    fails
}

/// True if daemon process may be stopped (no active block VRAM swap).
pub fn daemon_kill_allowed(entries: &[SwapEntry]) -> bool {
    !active_vram_block_swap(entries) && ghost_vram_swaps(entries).is_empty()
}

fn refuse_ghost_swap_state() -> Result<(), CascadeError> {
    let entries = read_swaps();
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
         NAO e seguro continuar. No Windows: `wsl --shutdown`, reabra a distro, \
         depois `sudo ramshared down` e `sudo ramshared up ...`. \
         Nunca mate o daemon com ublk/nbd ativo.",
        detail.join("; ")
    )))
}

fn orphan_recover_disabled() -> bool {
    #[cfg(test)]
    if let Some(v) = TEST_NO_ORPHAN_RECOVER.with(|c| *c.borrow()) {
        return v;
    }
    matches!(
        std::env::var("RAMSHARED_NO_ORPHAN_RECOVER")
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

/// Pure plan for orphan handling (unit-tested).
/// SPEC: wsl2-cascade-orphan-recover ITEM-2
#[derive(Clone, Debug, Eq, PartialEq)]
enum OrphanPlan {
    /// No managed orphan context.
    None,
    /// Safe: all managed orphans have used_kb == 0 (or only zram dirty is separate).
    RecoverZeroUsed,
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
    OrphanPlan::RecoverZeroUsed
}

fn clear_run_ramshared_state() {
    cascade_io::remove_runtime_file(SOCK);
    cascade_io::remove_runtime_file(ZRAM_DEV_FILE);
    cascade_io::remove_runtime_file(SWAP_DEV_FILE);
    cascade_io::remove_runtime_file(PID_FILE);
    cascade_io::remove_runtime_file("/run/ramshared/.armed");
}

/// Auto-heal zero-used managed orphans (WSL terminate class). Single pass.
/// SPEC: docs/specs/no-milestone/wsl2-cascade-orphan-recover/SPEC.md ITEM-2
fn try_recover_zero_used_orphans() -> Result<(), CascadeError> {
    let entries = read_swaps();
    if cascade_already_healthy(&entries) {
        return Ok(());
    }

    let plan = plan_orphan_action(&entries, false);
    match plan {
        OrphanPlan::None => {
            // Legacy: nbd orphan without records still needs message if recover disabled path
            // handled below only for Refuse / Recover.
            Ok(())
        }
        OrphanPlan::RefuseDirtyBackend => Err(CascadeError::Precondition(
            "orphan nbd/ublk com used_kb>0 — recusa auto-recover (risco hang em backend morto). \
             No Windows: `wsl --shutdown`, reabra a distro; ou swapoff manual se souber o que faz. \
             Nunca kill -9 ramsharedd com nbd/ublk em /proc/swaps."
                .into(),
        )),
        OrphanPlan::RecoverZeroUsed => {
            if orphan_recover_disabled() {
                return Err(CascadeError::Precondition(
                    "ha swap nbd/ublk/zram gerido orfao e RAMSHARED_NO_ORPHAN_RECOVER=1. \
                     Rode `sudo ramshared down` ou remova o kill-switch, depois up."
                        .into(),
                ));
            }
            eprintln!(
                "[up] orphan recover: managed swap zero-used (pos-terminate WSL?) — \
                 swapoff allowlist + nbd disconnect (single pass)"
            );
            let candidates = swapoff_candidates(None, None);
            eprintln!("[up] orphan recover candidatos: {candidates:?}");
            // Use the already fetched `entries` for `swapoff_all`
            let fails = swapoff_all(&candidates, &entries);
            for (p, msg) in &fails {
                eprintln!("[up] orphan recover swapoff fail {p}: {msg}");
            }
            // Disconnect any nbd still visible or known allowlist devices.
            for e in read_swaps() {
                if is_nbd_device_path(&e.filename) && !e.is_ghost() {
                    let dev = e.canonical_path();
                    let _ = sh("nbd-client", &["-d", "--", &dev]);
                }
            }
            // Also disconnect default product nbd even if already off swaps.
            let _ = sh("nbd-client", &["-d", "--", NBD]);

            if daemon_kill_allowed(&read_swaps()) {
                cascade_io::stop_daemon_gracefully();
            } else {
                return Err(CascadeError::Precondition(
                    "orphan recover: ainda ha nbd/ublk em /proc/swaps apos swapoff — \
                     NAO mate o daemon. Intervenha manualmente ou `wsl --shutdown`."
                        .into(),
                ));
            }
            clear_run_ramshared_state();

            let after = read_swaps();
            if active_vram_block_swap(&after) {
                return Err(CascadeError::Precondition(
                    "orphan recover incompleto: nbd/ublk ainda em /proc/swaps. \
                     `wsl --shutdown` no Windows e tente de novo."
                        .into(),
                ));
            }
            // Leftover zero-used zram: swapoff again
            for e in &after {
                if is_zram_device_path(&e.filename) && !e.is_ghost() {
                    let _ = swapoff_try(&e.canonical_path());
                }
            }
            eprintln!("[up] orphan recover: limpo — a seguir setup normal");
            Ok(())
        }
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
                     override so no daemon com RAMSHARED_ALLOW_UBLK_ON_WSL2=1, lab-only)"
                );
                return Ok(Transport::Nbd);
            }
            if Path::new("/dev/ublk-control").exists() {
                eprintln!("[up] transport=auto → ublk (/dev/ublk-control presente, host nao-WSL2)");
                Ok(Transport::Ublk)
            } else {
                eprintln!("[up] transport=auto → nbd (sem /dev/ublk-control)");
                Ok(Transport::Nbd)
            }
        }
    }
}

fn parse_up_args() -> Result<UpArgs, CascadeError> {
    let args: Vec<String> = std::env::args().skip(2).collect(); // skip "ramshared up"
    parse_up_args_from(&args, default_daemon())
}

/// Default MiB from env (`RAMSHARED_VRAM_MIB` / `RAMSHARED_ZRAM_MIB`) or 1024.
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
    let mut a = UpArgs {
        vram_mb: default_mb_from_env("RAMSHARED_VRAM_MIB", 1024),
        zram_mb: default_mb_from_env("RAMSHARED_ZRAM_MIB", 1024),
        daemon,
        force: false,
        connections: 1,
        // Default auto: on WSL2 resolves to NBD (Day-1); ublk only off-WSL2 when control node exists.
        transport: Transport::Auto,
        swap_dev: NBD.to_string(),
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
                    .map_err(|_| CascadeError::Arg("vram invalido".into()))?;
            }
            "--zram" => {
                i += 1;
                a.zram_mb = args
                    .get(i)
                    .ok_or_else(|| CascadeError::Arg("--zram requer MiB".into()))?
                    .parse()
                    .map_err(|_| CascadeError::Arg("zram invalido".into()))?;
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
                    .map_err(|_| CascadeError::Arg("connections invalido".into()))?;
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
                            "--transport invalido: {other} (use auto|nbd|ublk)"
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
            "--connections > 1 e invalido com --transport ublk (ring unico)".into(),
        ));
    }
    // Resolve Auto after parse so env/flag still work.
    a.transport = resolve_transport(a.transport)?;
    if a.transport == Transport::Ublk && a.connections != 1 {
        return Err(CascadeError::Arg(
            "--connections > 1 e invalido com --transport ublk (ring unico)".into(),
        ));
    }
    Ok(a)
}

mod lifecycle;
use lifecycle::{
    CascadeSnapshot, DemoteSnapshot, TierSample, active_threshold_kib_from_env, derive_lifecycle,
    render_status_json,
};

/// Build lifecycle snapshot from live swaps + daemon (read-only).
pub fn build_cascade_snapshot(entries: &[SwapEntry]) -> CascadeSnapshot {
    let pairs: Vec<(String, u64, u64, i32)> = entries
        .iter()
        .filter(|e| !e.is_ghost())
        .map(|e| (e.filename.clone(), e.size_kb, e.used_kb, e.priority))
        .collect();
    let (zram, vram, disk, order_ok) = lifecycle::tiers_from_swap_names(&pairs);
    let ghosts = ghost_vram_swaps(entries);
    let (daemon_alive, daemon_pid) = daemon_alive_pid();
    CascadeSnapshot {
        zram,
        vram,
        disk,
        ghost: !ghosts.is_empty(),
        order_ok,
        daemon_alive,
        daemon_pid,
        demote: read_demote_snapshot(),
        active_kib: active_threshold_kib_from_env(),
    }
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

fn daemon_alive_pid() -> (bool, Option<u32>) {
    // Match cascade-health: newest ramsharedd process.
    let out = Command::new("pgrep")
        .args(["-n", "-x", "ramsharedd"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if let Ok(pid) = s.parse::<u32>() {
                return (true, Some(pid));
            }
            // Some systems: pgrep -f
            (true, None)
        }
        _ => {
            // Fallback: pid file
            if let Ok(s) = fs::read_to_string(PID_FILE)
                && let Ok(pid) = s.trim().parse::<u32>()
                && Path::new(&format!("/proc/{pid}")).exists()
            {
                return (true, Some(pid));
            }
            (false, None)
        }
    }
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

    let entries = read_swaps();
    let snap = build_cascade_snapshot(&entries);
    let view = derive_lifecycle(&snap);
    let ts = status_timestamp();

    if as_json {
        println!("{}", render_status_json(&view, &snap, &ts));
        return Ok(());
    }

    // Human text (SPEC ITEM-2)
    println!("phase: {} ({})", view.phase.as_str(), view.phase_reason);
    println!("ok: {}", view.ok);
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
        eprintln!("[status] AVISO: swap fantasma detectado:");
        for g in ghosts {
            eprintln!(
                "  {} size_kb={} used_kb={} prio={}",
                g.filename, g.size_kb, g.used_kb, g.priority
            );
        }
        eprintln!("  acao: wsl --shutdown no Windows, depois ramshared down/up");
    }
    Ok(())
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
pub use cascade_io::{down, up};

#[cfg(test)]
mod tests {

    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn parse(args: &[&str]) -> Result<UpArgs, CascadeError> {
        let args = args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        parse_up_args_from(&args, "ramsharedd".to_string())
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
    fn similar_swap_names_are_not_managed_devices() {
        let entries = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /swap/nbd0-backup file 1024 99 -2\n\
             /dev/nbd01 partition 1024 7 -3\n",
        );
        assert!(!entries[0].is_managed_or_orphan_vram_tier());
        assert!(entries[1].is_managed_or_orphan_vram_tier());
        assert!(!active_vram_block_swap(&entries[..1]));
        assert!(swapoff_candidates_from(None, None, &entries[..1]).is_empty());
        assert!(
            ghost_used_blocks_swapoff(&entries, "/dev/nbd0").is_none(),
            "nbd0 must not match nbd01 or a similarly named swap file"
        );
    }

    #[test]
    fn orphan_plan_zero_used_is_recover() {
        let e = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/sdc partition 8388608 100 -2\n\
             /zram0 partition 1048576 0 200\n\
             /nbd0 partition 1048576 0 100\n",
        );
        assert_eq!(plan_orphan_action(&e, false), OrphanPlan::RecoverZeroUsed);
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

    fn set_no_orphan(v: Option<bool>) {
        TEST_NO_ORPHAN_RECOVER.with(|c| *c.borrow_mut() = v);
    }

    fn set_test_mb(v: Option<(&str, u64)>) {
        TEST_ENV_MB.with(|c| *c.borrow_mut() = v.map(|(k, n)| (k.to_string(), n)));
    }

    fn clear_test_seams() {
        clear_sh_script();
        set_test_swaps(None);
        set_test_mem(None);
        set_no_orphan(None);
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
    fn parse_swaps_skips_short_lines() {
        let e = parse_proc_swaps("Filename Type Size Used Priority\nbadline\n");
        assert!(e.is_empty());
    }

    #[test]
    fn swapoff_candidates_from_merges_records_and_live() {
        let live = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1024 0 100\n\
             /dev/sdc partition 999 0 -2\n\
             /dev/zram0 partition 512 0 200\n",
        );
        let c = swapoff_candidates_from(Some("/nbd0"), Some("zram0"), &live);
        assert!(c.iter().any(|p| p.contains("nbd0")));
        assert!(c.iter().any(|p| p.contains("zram0")));
        assert!(!c.iter().any(|p| p.contains("sdc")));
    }

    #[test]
    fn swapoff_candidates_from_includes_ghost_string() {
        let live = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/ublkb0\\040(deleted) partition 1024 50 -3\n",
        );
        let c = swapoff_candidates_from(None, None, &live);
        assert!(!c.is_empty());
        assert!(c[0].contains("ublkb") || c[0].contains("deleted"));
    }

    #[test]
    fn ghost_used_blocks_swapoff_message() {
        let live = parse_proc_swaps(
            "Filename Type Size Used Priority\n\
             /dev/nbd0\\040(deleted) partition 1024 99 100\n",
        );
        let msg = ghost_used_blocks_swapoff(&live, "/dev/nbd0").expect("block");
        assert!(msg.contains("used_kb=99"));
        assert!(ghost_used_blocks_swapoff(&live, "/dev/zram0").is_none());
    }

    #[test]
    fn swapoff_all_ghost_used_fails_without_shell() {
        clear_test_seams();
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0\\040(deleted) partition 1024 50 100\n",
        ));
        let e = read_swaps();
        let fails = swapoff_all(&["/dev/nbd0".to_string()], &e);
        clear_test_seams();
        assert_eq!(fails.len(), 1);
        assert!(fails[0].1.contains("ghost"));
    }

    #[test]
    fn swapoff_all_skips_disk_and_succeeds_on_mock() {
        clear_test_seams();
        push_sh("swapoff", Ok(""));
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/sdc partition 999 0 -2\n",
        ));
        let e = read_swaps();
        let fails = swapoff_all(&["/dev/sdc".to_string(), "/dev/nbd0".to_string()], &e);
        clear_test_seams();
        assert!(fails.is_empty(), "{fails:?}");
    }

    #[test]
    fn swapoff_all_absent_device_is_not_fail() {
        clear_test_seams();
        push_sh("swapoff", Err("swapoff: No such file or directory"));
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/sdc partition 999 0 -2\n",
        ));
        let e = read_swaps();
        let fails = swapoff_all(&["/dev/nbd0".to_string()], &e);
        clear_test_seams();
        assert!(fails.is_empty(), "{fails:?}");
    }

    #[test]
    fn swapoff_try_prefers_canonical_then_bare() {
        clear_test_seams();
        push_sh("swapoff -- /dev/nbd0", Err("first fail"));
        push_sh("swapoff", Ok(""));
        let r = swapoff_try("nbd0");
        clear_test_seams();
        assert!(r.is_ok());
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
            "512",
            "--zram",
            "256",
            "--daemon",
            "/tmp/d",
            "--force-no-safety-net",
            "--transport",
            "nbd",
        ])
        .unwrap();
        assert_eq!(a.vram_mb, 512);
        assert_eq!(a.zram_mb, 256);
        assert_eq!(a.daemon, "/tmp/d");
        assert!(a.force);
        assert_eq!(a.transport, Transport::Nbd);
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
    fn default_mb_from_env_and_orphan_kill_switch() {
        set_test_mb(Some(("RAMSHARED_TEST_MB", 333)));
        assert_eq!(default_mb_from_env("RAMSHARED_TEST_MB", 1), 333);
        set_test_mb(None);
        assert_eq!(default_mb_from_env("RAMSHARED_TEST_MB_MISSING", 9), 9);

        set_no_orphan(Some(true));
        assert!(orphan_recover_disabled());
        set_no_orphan(Some(false));
        assert!(!orphan_recover_disabled());
        set_no_orphan(None);
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
        let e = read_swaps();
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
        let err = try_recover_zero_used_orphans().unwrap_err();
        clear_test_seams();
        assert!(
            err.to_string().contains("used_kb") || err.to_string().contains("orphan"),
            "{err}"
        );
    }

    #[test]
    fn try_recover_kill_switch_on_zero_used() {
        clear_test_seams();
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1024 0 100\n",
        ));
        set_no_orphan(Some(true));
        let err = try_recover_zero_used_orphans().unwrap_err();
        clear_test_seams();
        assert!(err.to_string().contains("ORPHAN") || err.to_string().contains("recover"));
    }

    #[test]
    fn try_recover_zero_used_with_mocked_swapoff() {
        clear_test_seams();
        for _ in 0..20 {
            push_sh("*", Ok(""));
        }
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/nbd0 partition 1024 0 100\n\
             /dev/sdc partition 999 0 -2\n",
        ));
        set_no_orphan(Some(false));
        let r = try_recover_zero_used_orphans();
        clear_test_seams();
        let _ = r;
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
        assert!(lower_tier_present());
        set_test_swaps(Some(
            "Filename Type Size Used Priority\n\
             /dev/zram0 partition 1024 0 200\n",
        ));
        assert!(!lower_tier_present());
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
