//! I/O orchestration for cascade `up`/`down` (shell, zram, daemon spawn).
//! Hang policy (parse, ghost, orphan plan, swapoff allowlist) stays in parent `cascade`.
//! E2E: `scripts/safety/cascade-health.sh` + BINARY_MATCH — not thrash unit tests.

use super::*;
use ramshared_tier::{TierPriorities, validate_order, vram_safety_net};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

pub(crate) fn arm_forensics() {
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

pub(crate) fn disarm_forensics() {
    for path in ARMED_MARKER_CANDIDATES {
        let _ = fs::remove_file(path);
    }
}

pub(crate) fn stop_daemon_gracefully() {
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

pub(crate) fn setup_zram(mb: u64, prio: i32) -> Result<String, CascadeError> {
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

pub(crate) fn setup_zram_sysfs(mb: u64, prio: i32) -> Result<(), CascadeError> {
    let path = PathBuf::from("/sys/block/zram0");
    let canon_path = fs::canonicalize(&path).map_err(|e| {
        CascadeError::Precondition(format!("/sys/block/zram0 ausente ou inacessivel: {e}"))
    })?;

    if !canon_path.starts_with("/sys/") {
        return Err(CascadeError::Precondition(
            "Caminho sysfs do zram escapou do /sys/ (possivel symlink malicioso)".into(),
        ));
    }

    let _ = fs::write(canon_path.join("reset"), "1");
    for algo in ZRAM_ALGOS {
        if fs::write(canon_path.join("comp_algorithm"), algo.as_bytes()).is_ok() {
            break;
        }
    }
    let bytes = mb
        .checked_mul(1024 * 1024)
        .ok_or_else(|| CascadeError::Arg("zram size overflow".into()))?;
    fs::write(canon_path.join("disksize"), bytes.to_string())
        .map_err(|e| CascadeError::Io(format!("disksize: {e}")))?;

    let dev_path = fs::canonicalize("/dev/zram0").map_err(|e| {
        CascadeError::Precondition(format!("/dev/zram0 ausente ou inacessivel: {e}"))
    })?;

    if !dev_path.starts_with("/dev/") {
        return Err(CascadeError::Precondition(
            "Caminho do dispositivo zram escapou do /dev/ (possivel symlink malicioso)".into(),
        ));
    }
    let dev_str = dev_path.to_str().unwrap_or("/dev/zram0");

    sh("mkswap", &[dev_str])?;
    sh("swapon", &["-p", &prio.to_string(), dev_str])?;
    fs::write(ZRAM_DEV_FILE, dev_str).map_err(|e| CascadeError::Io(e.to_string()))?;
    eprintln!("[up] zram {dev_str} via sysfs prio={prio}");
    Ok(())
}

fn check_transport(transport: Transport) -> Result<(), CascadeError> {
    // cascade-transport-policy ITEM-3: ublk fail-closed before idempotent return (#16).
    // Auto already resolved to Nbd on WSL2; explicit ublk or Auto→Ublk (non-WSL2) still blocked
    // until full up wire-up exists (SPEC future + dedicated AUDIT-2.5 for teardown).
    if transport == Transport::Ublk {
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
    Ok(())
}

fn check_safety_net(vram_mb: u64, force: bool, prios: &TierPriorities) -> Result<(), CascadeError> {
    // A1 — DEMOTE safety net (requires a tier below VRAM).
    let vram_bytes = vram_mb
        .checked_mul(1024 * 1024)
        .ok_or_else(|| CascadeError::Arg("--vram: overflow (MiB grande demais)".into()))?;
    let net = vram_safety_net(lower_tier_present(), mem_available_bytes(), vram_bytes);
    if !net.is_safe() && !force {
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
    Ok(())
}

fn spawn_daemon(daemon_path: &str, vram_mb: u64, swap_dev: &str) -> Result<(), CascadeError> {
    let _ = fs::remove_file(SOCK);
    let child = Command::new(daemon_path)
        .args([
            "--size",
            &vram_mb.to_string(),
            "--sock",
            SOCK,
            "--nbd",
            swap_dev,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CascadeError::Shell {
            cmd: daemon_path.to_string(),
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
    Ok(())
}

fn connect_nbd(connections: u32, swap_dev: &str, vram_prio: i32) -> Result<(), CascadeError> {
    let conns = connections.to_string();
    let mut nbd_args: Vec<&str> = Vec::new();
    if connections > 1 {
        nbd_args.extend(["-C", conns.as_str()]);
    }
    nbd_args.extend(["-unix", SOCK, swap_dev]);
    if let Err(e) = sh("nbd-client", &nbd_args) {
        let _ = sh("pkill", &["-x", "ramsharedd"]);
        disarm_forensics();
        return Err(e);
    }
    if let Err(e) = sh("mkswap", &["-L", "RAMSHARED", swap_dev]) {
        let _ = sh("nbd-client", &["-d", swap_dev]);
        let _ = sh("pkill", &["-x", "ramsharedd"]);
        disarm_forensics();
        return Err(e);
    }
    if let Err(e) = sh("swapon", &["-p", &vram_prio.to_string(), swap_dev]) {
        let _ = sh("nbd-client", &["-d", swap_dev]);
        let _ = sh("pkill", &["-x", "ramsharedd"]);
        disarm_forensics();
        return Err(e);
    }
    Ok(())
}

pub fn up() -> Result<(), CascadeError> {
    let a = parse_up_args()?;
    let prios = TierPriorities::default();
    validate_order(prios).map_err(|e| CascadeError::Precondition(e.to_string()))?;

    check_transport(a.transport)?;

    // Ghosts: never auto-recover (#16).
    refuse_ghost_swap_state()?;

    // SPEC wsl2-cascade-boot ITEM-5: idempotent if already healthy.
    let entries_now = read_swaps();
    if cascade_already_healthy(&entries_now) {
        eprintln!("[up] cascata ja ativa — nada a fazer (idempotente)");
        return status(false);
    }

    // SPEC wsl2-cascade-orphan-recover ITEM-2: zero-used orphans → heal once.
    try_recover_zero_used_orphans()?;

    let entries_after = read_swaps();
    if cascade_already_healthy(&entries_after) {
        eprintln!("[up] cascata ja ativa apos recover — noop");
        return status(false);
    }
    refuse_half_cascade(&entries_after)?;

    check_safety_net(a.vram_mb, a.force, &prios)?;

    fs::create_dir_all("/run/ramshared").map_err(|e| CascadeError::Io(e.to_string()))?;

    arm_forensics();

    // zram tier (HOT). --zram 0 skips.
    setup_zram(a.zram_mb, prios.zram)?;

    // VRAM tier (COLD): daemon + nbd.
    sh("modprobe", &["nbd", "nbds_max=1", "max_part=0"])?;
    spawn_daemon(&a.daemon, a.vram_mb, &a.swap_dev)?;
    connect_nbd(a.connections, &a.swap_dev, prios.vram)?;

    fs::write(SWAP_DEV_FILE, &a.swap_dev).map_err(|e| CascadeError::Io(e.to_string()))?;
    eprintln!(
        "[up] VRAM {} (prio {}, {} MiB, {} conexão(ões))",
        a.swap_dev, prios.vram, a.vram_mb, a.connections
    );
    eprintln!(
        "[up] cascata ativa: zram({}) > VRAM({}) > VHDX | anti-hang: down sempre swapoff antes de stop daemon",
        prios.zram, prios.vram
    );
    status(false)
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

    // Fetch entries once to use in swapoff_all
    let current_entries = read_swaps();

    // 1) ALWAYS swapoff first — never disconnect/kill with pages on the device.
    let fails = swapoff_all(&candidates, &current_entries);
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
    status(false)
}
