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

use ramshared_tier::{TierPriorities, validate_order, vram_safety_net};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

const SOCK: &str = "/run/ramshared/wsl2d.sock";
const NBD: &str = "/dev/nbd0";
const ZRAM_DEV_FILE: &str = "/run/ramshared/zram-dev";
const SWAP_DEV_FILE: &str = "/run/ramshared/swap-dev";
const PID_FILE: &str = "/run/ramshared/ramsharedd.pid";
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
        let f = self.filename.to_ascii_lowercase();
        f.contains("nbd") || f.contains("ublk") || f.contains("zram") || f.contains("ramshared")
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

/// Allowlist for auto swapoff: only nbd / ublk / zram (never disk VHDX).
fn is_allowlisted_managed_path(path: &str) -> bool {
    let base = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    let base = base.split_whitespace().next().unwrap_or(&base);
    base.starts_with("nbd")
        || base.starts_with("ublk")
        || base.starts_with("ublkb")
        || base.starts_with("zram")
        || base.contains("ramshared")
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
        !e.is_ghost()
            && (e.filename.contains("nbd")
                || e.filename.contains("ublk")
                || e.filename.contains("\\040ublk"))
    })
}

fn lower_tier_present() -> bool {
    let vram_prio = TierPriorities::default().vram;
    read_swaps().iter().any(|e| {
        // Ignore our managed tiers when looking for DEMOTE sink.
        if e.filename.contains("zram") || e.filename.contains("nbd") || e.filename.contains("ublk")
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

fn arm_forensics() {
    let payload = format!(
        "armed_at={}\npid={}\nreason=ramshared-up\n",
        chrono_like_now(),
        std::process::id()
    );
    for path in ARMED_MARKER_CANDIDATES {
        if let Some(parent) = Path::new(path).parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::write(path, &payload).is_ok() {
            eprintln!("[up] forensics armed: {path}");
            return;
        }
    }
}

fn disarm_forensics() {
    for path in ARMED_MARKER_CANDIDATES {
        let _ = fs::remove_file(path);
    }
}

fn chrono_like_now() -> String {
    // Avoid chrono dep: unix seconds is enough for the marker.
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Paths we will try to `swapoff` during down (recorded + live scan).
fn swapoff_candidates(recorded_swap: Option<&str>, recorded_zram: Option<&str>) -> Vec<String> {
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
    for e in read_swaps() {
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
        match sh("swapoff", &[p]) {
            Ok(_) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Swapoff every candidate. Returns list of failures.
/// **Never** kills the daemon from here.
fn swapoff_all(paths: &[String]) -> Vec<(String, String)> {
    let mut fails = Vec::new();
    for p in paths {
        if !is_allowlisted_managed_path(p) {
            eprintln!("[down] swapoff skip (nao allowlist): {p}");
            continue;
        }
        // Ghost with used>0 cannot be recovered without reboot — report loudly.
        let entries = read_swaps();
        let p_canon = canonicalize_swap_path(p);
        if let Some(e) = entries.iter().find(|e| {
            e.filename.contains(p.trim())
                || e.bare_path() == *p
                || e.canonical_path() == p_canon
                || e.filename.contains(p_canon.trim_start_matches("/dev/"))
        }) && e.is_ghost()
            && e.used_kb > 0
        {
            fails.push((
                p.clone(),
                format!(
                    "ghost swap used_kb={} — NAO e recuperavel com swapoff; \
                     rode `wsl --shutdown` no Windows e suba de novo. \
                     NUNCA kill -9 ramsharedd com ublk/nbd em /proc/swaps.",
                    e.used_kb
                ),
            ));
            continue;
        }
        match swapoff_try(p) {
            Ok(_) => eprintln!("[down] swapoff ok: {p_canon}"),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("No such file") || msg.contains("Invalid argument") {
                    let still = read_swaps().iter().any(|e| {
                        (e.filename.contains(p)
                            || e.bare_path() == *p
                            || e.canonical_path() == p_canon)
                            && e.used_kb > 0
                    });
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

fn stop_daemon_gracefully() {
    // Prefer PID file if we wrote one
    if let Ok(pid_s) = fs::read_to_string(PID_FILE)
        && let Ok(pid) = pid_s.trim().parse::<i32>()
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
    // Wait up to 10s for voluntary exit (allows VRAM zero()).
    for _ in 0..100 {
        if sh("pgrep", &["-x", "ramsharedd"]).is_err() {
            let _ = fs::remove_file(PID_FILE);
            return;
        }
        sleep(Duration::from_millis(100));
    }
    // Only SIGTERM via pkill -x; never -9 from this tool.
    if !daemon_kill_allowed(&read_swaps()) {
        eprintln!(
            "[down] ABORT pkill: ainda ha nbd/ublk em /proc/swaps — \
             risco de ghost swap / hang WSL. Corrija swapoff manualmente."
        );
        return;
    }
    eprintln!("[down] daemon nao saiu em 10s; pkill -TERM (sem -9)");
    let _ = sh("pkill", &["-x", "ramsharedd"]);
    sleep(Duration::from_millis(500));
    let _ = fs::remove_file(PID_FILE);
}

fn setup_zram(mb: u64, prio: i32) -> Result<String, CascadeError> {
    if mb == 0 {
        eprintln!("[up] zram skipped (--zram 0)");
        return Ok(String::new());
    }
    let _ = sh("modprobe", &["zram"]);
    // Prefer free device via zramctl with algorithm fallbacks.
    let size = format!("{mb}M");
    let mut last_err = String::new();
    for algo in ZRAM_ALGOS {
        match sh("zramctl", &["--find", "--size", &size, "--algorithm", algo]) {
            Ok(zdev) => {
                if !matches!(zdev.strip_prefix("/dev/zram"), Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                {
                    last_err = format!("zramctl retornou device inesperado: {zdev}");
                    continue;
                }
                sh("mkswap", &[&zdev])?;
                sh("swapon", &["-p", &prio.to_string(), &zdev])?;
                fs::write(ZRAM_DEV_FILE, &zdev).map_err(|e| CascadeError::Io(e.to_string()))?;
                eprintln!("[up] zram {zdev} algo={algo} prio={prio}");
                return Ok(zdev);
            }
            Err(e) => {
                last_err = e.to_string();
                eprintln!("[up] zram algo {algo} falhou: {last_err}");
            }
        }
    }
    // Sysfs fallback on zram0
    if let Err(e) = setup_zram_sysfs(mb, prio) {
        return Err(CascadeError::Precondition(format!(
            "zram indisponivel (zramctl: {last_err}; sysfs: {e}). \
             Tente --zram 0 para so VRAM, ou `modprobe zram`."
        )));
    }
    Ok("/dev/zram0".into())
}

fn setup_zram_sysfs(mb: u64, prio: i32) -> Result<(), CascadeError> {
    let path = PathBuf::from("/sys/block/zram0");
    if !path.exists() {
        return Err(CascadeError::Precondition(
            "/sys/block/zram0 ausente".into(),
        ));
    }
    let _ = fs::write(path.join("reset"), "1");
    for algo in ZRAM_ALGOS {
        if fs::write(path.join("comp_algorithm"), algo.as_bytes()).is_ok() {
            break;
        }
    }
    let bytes = mb
        .checked_mul(1024 * 1024)
        .ok_or_else(|| CascadeError::Arg("zram size overflow".into()))?;
    fs::write(path.join("disksize"), bytes.to_string())
        .map_err(|e| CascadeError::Io(format!("disksize: {e}")))?;
    sh("mkswap", &["/dev/zram0"])?;
    sh("swapon", &["-p", &prio.to_string(), "/dev/zram0"])?;
    fs::write(ZRAM_DEV_FILE, "/dev/zram0").map_err(|e| CascadeError::Io(e.to_string()))?;
    eprintln!("[up] zram /dev/zram0 via sysfs prio={prio}");
    Ok(())
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
        e.used_kb > 0
            && (e.filename.contains("nbd")
                || e.filename.contains("ublk")
                || e.filename.contains("\\040ublk"))
    });
    if dirty_block {
        return OrphanPlan::RefuseDirtyBackend;
    }
    OrphanPlan::RecoverZeroUsed
}

fn clear_run_ramshared_state() {
    let _ = fs::remove_file(SOCK);
    let _ = fs::remove_file(ZRAM_DEV_FILE);
    let _ = fs::remove_file(SWAP_DEV_FILE);
    let _ = fs::remove_file(PID_FILE);
    let _ = fs::remove_file("/run/ramshared/.armed");
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
            let fails = swapoff_all(&candidates);
            for (p, msg) in &fails {
                eprintln!("[up] orphan recover swapoff fail {p}: {msg}");
            }
            // Disconnect any nbd still visible or known allowlist devices.
            for e in read_swaps() {
                if e.filename.contains("nbd") && !e.is_ghost() {
                    let dev = e.canonical_path();
                    let _ = sh("nbd-client", &["-d", &dev]);
                }
            }
            // Also disconnect default product nbd even if already off swaps.
            let _ = sh("nbd-client", &["-d", NBD]);

            if daemon_kill_allowed(&read_swaps()) {
                stop_daemon_gracefully();
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
                if e.filename.contains("zram") && !e.is_ghost() {
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
            && (e.filename.contains("nbd") || e.filename.contains("ublk"))
            && e.is_managed_or_orphan_vram_tier()
    });
    if !has_vram_swap {
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
    let has_vram = entries
        .iter()
        .any(|e| !e.is_ghost() && (e.filename.contains("nbd") || e.filename.contains("ublk")));
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

pub fn up() -> Result<(), CascadeError> {
    let a = parse_up_args()?;
    let prios = TierPriorities::default();
    validate_order(prios).map_err(|e| CascadeError::Precondition(e.to_string()))?;

    // cascade-transport-policy ITEM-3: ublk fail-closed before idempotent return (#16).
    // Auto already resolved to Nbd on WSL2; explicit ublk or Auto→Ublk (non-WSL2) still blocked
    // until full up wire-up exists (SPEC future + dedicated AUDIT-2.5 for teardown).
    if a.transport == Transport::Ublk {
        let msg = if is_wsl2() {
            "transport ublk recusado no WSL2 (freeze risk 2026-06-09; Day-1 = nbd). \
             Lab-only: daemon manual + RAMSHARED_ALLOW_UBLK_ON_WSL2=1 — nao e Day-0. \
             Kernel pode ter ublk_drv; produto cascade nao usa."
        } else {
            "transport ublk no `ramshared up` ainda nao implementado (SPEC futuro). \
             Use --transport nbd ou auto. Daemon ublk manual e lab-only."
        };
        return Err(CascadeError::Precondition(msg.into()));
    }

    // Ghosts: never auto-recover (#16).
    refuse_ghost_swap_state()?;

    // SPEC wsl2-cascade-boot ITEM-5: idempotent if already healthy.
    let entries_now = read_swaps();
    if cascade_already_healthy(&entries_now) {
        eprintln!("[up] cascata ja ativa — nada a fazer (idempotente)");
        return status();
    }

    // SPEC wsl2-cascade-orphan-recover ITEM-2: zero-used orphans → heal once.
    try_recover_zero_used_orphans()?;

    let entries_after = read_swaps();
    if cascade_already_healthy(&entries_after) {
        eprintln!("[up] cascata ja ativa apos recover — noop");
        return status();
    }
    refuse_half_cascade(&entries_after)?;

    // A1 — DEMOTE safety net (requires a tier below VRAM).
    let vram_bytes = a
        .vram_mb
        .checked_mul(1024 * 1024)
        .ok_or_else(|| CascadeError::Arg("--vram: overflow (MiB grande demais)".into()))?;
    let net = vram_safety_net(lower_tier_present(), mem_available_bytes(), vram_bytes);
    if !net.is_safe() && !a.force {
        return Err(CascadeError::Precondition(
            "sem rede de seguranca p/ DEMOTE (sem VHDX e RAM insuficiente); \
             use --force-no-safety-net se intencional"
                .into(),
        ));
    }
    eprintln!("[up] rede de seguranca A1: {net:?}");
    // Product order (always): zram (hot) > VRAM tier (cold, fast vs SSD) > disk VHDX (last).
    eprintln!(
        "[up] prioridade: zram({}) > VRAM/nbd({}) > VHDX(disk) — SSD so depois de VRAM",
        prios.zram, prios.vram
    );
    fs::create_dir_all("/run/ramshared").map_err(|e| CascadeError::Io(e.to_string()))?;

    arm_forensics();

    // zram tier (HOT). --zram 0 skips.
    setup_zram(a.zram_mb, prios.zram)?;

    // VRAM tier (COLD): daemon + nbd.
    sh("modprobe", &["nbd", "nbds_max=1", "max_part=0"])?;
    let _ = fs::remove_file(SOCK);
    let child = Command::new(&a.daemon)
        .args([
            "--size",
            &a.vram_mb.to_string(),
            "--sock",
            SOCK,
            "--nbd",
            &a.swap_dev,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CascadeError::Shell {
            cmd: a.daemon.clone(),
            msg: e.to_string(),
        })?;
    let _ = fs::write(PID_FILE, child.id().to_string());
    let mut ok = false;
    for _ in 0..120 {
        if Path::new(SOCK).exists() {
            ok = true;
            break;
        }
        sleep(Duration::from_millis(50));
    }
    if !ok {
        // Best-effort cleanup of failed spawn; no swap yet so kill is allowed.
        let _ = sh("pkill", &["-x", "ramsharedd"]);
        disarm_forensics();
        return Err(CascadeError::Precondition(
            "daemon nao subiu (socket ausente)".into(),
        ));
    }
    let conns = a.connections.to_string();
    let mut nbd_args: Vec<&str> = Vec::new();
    if a.connections > 1 {
        nbd_args.extend(["-C", conns.as_str()]);
    }
    nbd_args.extend(["-unix", SOCK, &a.swap_dev]);
    if let Err(e) = sh("nbd-client", &nbd_args) {
        let _ = sh("pkill", &["-x", "ramsharedd"]);
        disarm_forensics();
        return Err(e);
    }
    if let Err(e) = sh("mkswap", &["-L", "RAMSHARED", &a.swap_dev]) {
        let _ = sh("nbd-client", &["-d", &a.swap_dev]);
        let _ = sh("pkill", &["-x", "ramsharedd"]);
        disarm_forensics();
        return Err(e);
    }
    if let Err(e) = sh("swapon", &["-p", &prios.vram.to_string(), &a.swap_dev]) {
        let _ = sh("nbd-client", &["-d", &a.swap_dev]);
        let _ = sh("pkill", &["-x", "ramsharedd"]);
        disarm_forensics();
        return Err(e);
    }
    fs::write(SWAP_DEV_FILE, &a.swap_dev).map_err(|e| CascadeError::Io(e.to_string()))?;
    eprintln!(
        "[up] VRAM {} (prio {}, {} MiB, {} conexão(ões))",
        a.swap_dev, prios.vram, a.vram_mb, a.connections
    );
    eprintln!(
        "[up] cascata ativa: zram({}) > VRAM({}) > VHDX | anti-hang: down sempre swapoff antes de stop daemon",
        prios.zram, prios.vram
    );
    status()
}

pub fn down() -> Result<(), CascadeError> {
    let recorded_swap = fs::read_to_string(SWAP_DEV_FILE)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let recorded_zram = fs::read_to_string(ZRAM_DEV_FILE)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let candidates = swapoff_candidates(recorded_swap.as_deref(), recorded_zram.as_deref());
    eprintln!("[down] swapoff candidatos: {candidates:?}");

    // 1) ALWAYS swapoff first — never disconnect/kill with pages on the device.
    let fails = swapoff_all(&candidates);
    if !fails.is_empty() {
        for (p, msg) in &fails {
            eprintln!("[down] FALHA swapoff {p}: {msg}");
        }
        // If ghost with used>0, hard fail and do not kill daemon / nbd-disconnect.
        let swaps_now = read_swaps();
        let ghosts = ghost_vram_swaps(&swaps_now);
        if ghosts.iter().any(|e| e.used_kb > 0) {
            return Err(CascadeError::Precondition(
                "swap fantasma com paginas em uso — WSL pode hang se forcar. \
                 No Windows: `wsl --shutdown`. Depois `sudo ramshared down` e `up`."
                    .into(),
            ));
        }
        // Non-ghost failures: still refuse kill if block swap remains
        if active_vram_block_swap(&read_swaps()) {
            return Err(CascadeError::Precondition(
                "swapoff incompleto e nbd/ublk ainda em /proc/swaps; \
                 NAO mate o daemon. Intervenha com swapoff manual."
                    .into(),
            ));
        }
    }

    // 2) Reset zram devices we know about
    if let Some(ref z) = recorded_zram {
        let _ = sh("zramctl", &["-r", z]);
    }
    // Also try reset any leftover zram still listed
    for e in read_swaps() {
        if e.filename.contains("zram") && !e.is_ghost() {
            let z = e.canonical_path();
            let _ = swapoff_try(&z);
            let _ = sh("zramctl", &["-r", &z]);
        }
    }

    // 3) Disconnect NBD only after swapoff (EOF → daemon zero() VRAM)
    let nbd_targets: Vec<String> = recorded_swap
        .into_iter()
        .map(|s| canonicalize_swap_path(&s))
        .chain(
            read_swaps()
                .into_iter()
                .filter(|e| e.filename.contains("nbd"))
                .map(|e| e.canonical_path()),
        )
        .collect();
    for dev in &nbd_targets {
        if is_allowlisted_managed_path(dev) && dev.contains("nbd") {
            let _ = sh("nbd-client", &["-d", dev]);
        }
    }

    // 4) Daemon stop — only if no block VRAM swap remains
    stop_daemon_gracefully();

    let _ = fs::remove_file(SOCK);
    let _ = fs::remove_file(ZRAM_DEV_FILE);
    let _ = fs::remove_file(SWAP_DEV_FILE);
    let _ = fs::remove_file(PID_FILE);
    disarm_forensics();
    eprintln!("[down] cascata desmontada (swapoff-first, sem kill -9)");
    status()
}

pub fn status() -> Result<(), CascadeError> {
    println!("{}", sh("swapon", &["--show"])?);
    let entries = read_swaps();
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
        assert!(!is_allowlisted_managed_path("/dev/sdc"));
        assert!(!is_allowlisted_managed_path("/dev/sdb"));
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
        assert_eq!(plan_orphan_action(&e, false), OrphanPlan::RefuseDirtyBackend);
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
        let has_live_record =
            Path::new(SWAP_DEV_FILE).exists() || Path::new(PID_FILE).exists();
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
}
