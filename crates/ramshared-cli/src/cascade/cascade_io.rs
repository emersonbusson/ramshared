//! I/O orchestration for cascade `up`/`down` (shell, zram, daemon spawn).
//! Hang policy (parse, ghost, orphan plan, swapoff allowlist) stays in parent `cascade`.
//! E2E: `scripts/safety/cascade-health.sh` + BINARY_MATCH — not thrash unit tests.

use super::*;
use ramshared_tier::{TierPriorities, validate_order, vram_safety_net};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread::sleep;
use std::time::{Duration, Instant};

const SHORT_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const COMMAND_REAP_GRACE: Duration = Duration::from_millis(250);
const COMMAND_OUTPUT_LIMIT: usize = 64 * 1024;
const COMMAND_OUTPUT_GRACE: Duration = Duration::from_millis(250);

fn command_label(command: &str, args: &[&str]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{command} {}", args.join(" "))
    }
}

fn capture_stream<R>(mut reader: R) -> Result<Vec<u8>, String>
where
    R: Read,
{
    let mut output = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > COMMAND_OUTPUT_LIMIT {
            return Err(format!(
                "command output exceeded {} bytes",
                COMMAND_OUTPUT_LIMIT
            ));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

fn capture_stream_async<R>(reader: R) -> mpsc::Receiver<Result<Vec<u8>, String>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(capture_stream(reader));
    });
    receiver
}

fn receive_output(
    receiver: mpsc::Receiver<Result<Vec<u8>, String>>,
    command: &str,
    stream: &str,
) -> Result<Vec<u8>, CascadeError> {
    match receiver.recv_timeout(COMMAND_OUTPUT_GRACE) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(message)) => Err(CascadeError::Shell {
            cmd: command.to_string(),
            msg: format!("{stream} capture failed: {message}"),
        }),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(CascadeError::Shell {
            cmd: command.to_string(),
            msg: format!("{stream} capture did not close after child exit"),
        }),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(CascadeError::Shell {
            cmd: command.to_string(),
            msg: format!("{stream} capture worker disconnected"),
        }),
    }
}

fn wait_for_child(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Result<Option<std::process::ExitStatus>, CascadeError> {
    loop {
        match child.try_wait().map_err(|error| CascadeError::Shell {
            cmd: "child".into(),
            msg: error.to_string(),
        })? {
            Some(status) => return Ok(Some(status)),
            None if Instant::now() >= deadline => return Ok(None),
            None => sleep(COMMAND_POLL_INTERVAL),
        }
    }
}

/// Runs a direct child with a bounded wait and captures its bounded output.
///
/// Timeout cleanup targets only `child`, the handle created by this call. It
/// never selects a process by name, group, or inherited PID file.
fn run_command_bounded_for(
    command: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, CascadeError> {
    let label = command_label(command, args);
    let mut child = Command::new(command)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CascadeError::Shell {
            cmd: label.clone(),
            msg: error.to_string(),
        })?;
    let stdout = child.stdout.take().ok_or_else(|| CascadeError::Shell {
        cmd: label.clone(),
        msg: "stdout pipe was unavailable".into(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| CascadeError::Shell {
        cmd: label.clone(),
        msg: "stderr pipe was unavailable".into(),
    })?;
    let stdout_rx = capture_stream_async(stdout);
    let stderr_rx = capture_stream_async(stderr);
    let deadline = Instant::now() + timeout;

    let Some(status) = wait_for_child(&mut child, deadline)? else {
        let _ = child.kill();
        let reaped = wait_for_child(&mut child, Instant::now() + COMMAND_REAP_GRACE)?;
        let detail = if reaped.is_some() {
            "direct child reaped"
        } else {
            "direct child did not reap during grace window"
        };
        return Err(CascadeError::Shell {
            cmd: label,
            msg: format!("timed out after {} ms; {detail}", timeout.as_millis()),
        });
    };

    let stdout = receive_output(stdout_rx, &label, "stdout")?;
    let stderr = receive_output(stderr_rx, &label, "stderr")?;
    if status.success() {
        return Ok(String::from_utf8_lossy(&stdout).trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
    let status = status
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
    pid_file: PathBuf,
    forensics_markers: Vec<PathBuf>,
    zram_sysfs: PathBuf,
    zram_device: PathBuf,
}

impl RuntimePaths {
    fn system() -> Self {
        Self {
            runtime_dir: PathBuf::from("/run/ramshared"),
            socket: PathBuf::from(SOCK),
            zram_dev_file: PathBuf::from(ZRAM_DEV_FILE),
            swap_dev_file: PathBuf::from(SWAP_DEV_FILE),
            pid_file: PathBuf::from(PID_FILE),
            forensics_markers: ARMED_MARKER_CANDIDATES.iter().map(PathBuf::from).collect(),
            zram_sysfs: PathBuf::from("/sys/block/zram0"),
            zram_device: PathBuf::from("/dev/zram0"),
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
            pid_file: runtime_dir.join("ramsharedd.pid"),
            runtime_dir,
            forensics_markers: vec![forensics.join(".armed")],
            zram_sysfs: root.join("sys/block/zram0"),
            zram_device: root.join("dev/zram0"),
        }
    }
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

pub(crate) fn stop_daemon_gracefully() {
    stop_daemon_gracefully_at(&RuntimePaths::system(), Duration::from_secs(10));
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

fn signal_verified_daemon(pid: u32) {
    let pid = pid.to_string();
    let _ = run_command_bounded("kill", &["-TERM", "--", &pid]);
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

fn terminate_spawned_child(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    let _ = run_command_bounded("kill", &["-TERM", "--", &pid]);
    match wait_for_child(child, Instant::now() + COMMAND_REAP_GRACE) {
        Ok(Some(_)) => {}
        Ok(None) => {
            // This handle belongs only to the failed invocation; no NBD swap is
            // attached when this escalation is used. It is never a name match.
            let _ = child.kill();
            match wait_for_child(child, Instant::now() + COMMAND_REAP_GRACE) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    eprintln!("[warn] exact daemon child {pid} did not exit during bounded cleanup")
                }
                Err(error) => eprintln!("[warn] exact daemon child cleanup failed: {error}"),
            }
        }
        Err(error) => eprintln!("[warn] exact daemon child cleanup failed: {error}"),
    }
}

fn stop_daemon_gracefully_at(paths: &RuntimePaths, timeout: Duration) {
    let Some(pid) = verified_daemon_pid(paths) else {
        // A PID file is advisory. Never select a replacement process by name.
        remove_runtime_file(&paths.pid_file);
        return;
    };
    if !daemon_kill_allowed(&read_swaps()) {
        eprintln!(
            "[down] daemon stop refused: nbd/ublk remains in /proc/swaps; \
             correct swapoff manually to avoid a ghost swap or WSL hang."
        );
        return;
    }
    signal_verified_daemon(pid);
    let deadline = Instant::now() + timeout;
    while !process_is_gone_or_zombie(pid) && Instant::now() < deadline {
        sleep(Duration::from_millis(100));
    }
    if !process_is_gone_or_zombie(pid) {
        eprintln!(
            "[down] daemon {pid} did not exit within {} ms; no broad signal was sent",
            timeout.as_millis()
        );
        return;
    }
    remove_runtime_file(&paths.pid_file);
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
    if active && runner.run("swapoff", &[zram_device]).is_err() {
        return false;
    }
    if runner.run("zramctl", &["-r", zram_device]).is_err() {
        return false;
    }
    remove_runtime_file(&paths.zram_dev_file);
    true
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
        match runner.run("zramctl", &["--find", "--size", &size, "--algorithm", algo]) {
            Ok(output) => {
                let Some(zdev) = parse_zram_device(&output) else {
                    last_err = format!("zramctl returned an unexpected device: {output}");
                    continue;
                };
                let priority = prio.to_string();
                if let Err(error) = runner.run("mkswap", &[&zdev]) {
                    let _ = rollback_zram_tier(runner, paths, &zdev, false);
                    return Err(error);
                }
                if let Err(error) = fs::write(&paths.zram_dev_file, &zdev) {
                    let _ = rollback_zram_tier(runner, paths, &zdev, false);
                    return Err(CascadeError::Io(error.to_string()));
                }
                if let Err(error) = runner.run("swapon", &["-p", &priority, &zdev]) {
                    let _ = rollback_zram_tier(runner, paths, &zdev, false);
                    return Err(error);
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
    // Sysfs fallback on zram0
    if let Err(e) = setup_zram_sysfs_with(runner, paths, mb, prio) {
        return Err(CascadeError::Precondition(format!(
            "zram is unavailable (zramctl: {last_err}; sysfs: {e}). \
             Try --zram 0 for VRAM only, or `modprobe zram`."
        )));
    }
    Ok(paths.zram_device.to_string_lossy().into_owned())
}

fn setup_zram_sysfs_with<R: CommandRunner>(
    runner: &R,
    paths: &RuntimePaths,
    mb: u64,
    prio: i32,
) -> Result<(), CascadeError> {
    let path = &paths.zram_sysfs;
    if !path.exists() {
        return Err(CascadeError::Precondition(format!(
            "{} is absent",
            path.display()
        )));
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
    let metadata = fs::symlink_metadata(&paths.zram_device)
        .map_err(|e| CascadeError::Io(format!("stat {}: {e}", paths.zram_device.display())))?;
    use std::os::unix::fs::FileTypeExt;
    if !metadata.file_type().is_block_device() {
        return Err(CascadeError::Precondition(format!(
            "{} is not a block device",
            paths.zram_device.display()
        )));
    }
    let zram_device = paths.zram_device.to_string_lossy().into_owned();
    let priority = prio.to_string();
    if let Err(error) = runner.run("mkswap", &[&zram_device]) {
        let _ = rollback_zram_tier(runner, paths, &zram_device, false);
        return Err(error);
    }
    if let Err(error) = fs::write(&paths.zram_dev_file, &zram_device) {
        let _ = rollback_zram_tier(runner, paths, &zram_device, false);
        return Err(CascadeError::Io(error.to_string()));
    }
    if let Err(error) = runner.run("swapon", &["-p", &priority, &zram_device]) {
        let _ = rollback_zram_tier(runner, paths, &zram_device, false);
        return Err(error);
    }
    eprintln!("[up] zram {zram_device} via sysfs prio={prio}");
    Ok(())
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
             Lab-only: manual daemon + RAMSHARED_ALLOW_UBLK_ON_WSL2=1 is not Day-0. \
             The kernel may provide ublk_drv; the cascade product does not use it."
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
    let net = vram_safety_net(lower_tier_present(), mem_available_bytes(), vram_bytes);
    if !net.is_safe() && !force {
        return Err(CascadeError::Precondition(
            "no DEMOTE safety net (no VHDX and insufficient RAM); \
             use --force-no-safety-net only when intentional"
                .into(),
        ));
    }
    eprintln!("[up] A1 safety net: {net:?}");
    // Product order (always): zram (hot) > VRAM tier (cold, fast vs SSD) > disk VHDX (last).
    eprintln!(
        "[up] priority: zram({}) > VRAM/nbd({}) > VHDX(disk) — SSD only after VRAM",
        prios.zram, prios.vram
    );
    Ok(())
}

fn spawn_daemon_with_deadline(
    daemon_path: &str,
    vram_mb: u64,
    swap_dev: &str,
    paths: &RuntimePaths,
    readiness_timeout: Duration,
) -> Result<std::process::Child, CascadeError> {
    fs::create_dir_all(&paths.runtime_dir).map_err(|error| CascadeError::Io(error.to_string()))?;
    remove_runtime_file(&paths.socket);
    let size = vram_mb.to_string();
    let socket = paths.socket.to_string_lossy().into_owned();
    let mut child = Command::new(daemon_path)
        .args(["--size", &size, "--sock", &socket, "--nbd", swap_dev])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| CascadeError::Shell {
            cmd: daemon_path.to_string(),
            msg: error.to_string(),
        })?;
    if let Err(error) = fs::write(&paths.pid_file, child.id().to_string()) {
        terminate_spawned_child(&mut child);
        return Err(CascadeError::Io(error.to_string()));
    }
    let deadline = Instant::now() + readiness_timeout;
    while !paths.socket.exists() && Instant::now() < deadline {
        sleep(Duration::from_millis(50));
    }
    if !paths.socket.exists() {
        // No NBD attach exists yet. Cleanup is limited to our direct child.
        terminate_spawned_child(&mut child);
        remove_runtime_file(&paths.pid_file);
        disarm_forensics_at(paths);
        return Err(CascadeError::Precondition(
            "daemon did not start (socket missing)".into(),
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

fn rollback_nbd_attach<R: CommandRunner>(
    runner: &R,
    attached: bool,
    record_written: bool,
    swap_dev: &str,
    daemon: &mut std::process::Child,
    paths: &RuntimePaths,
) {
    if attached {
        let _ = runner.run("nbd-client", &["-d", swap_dev]);
    }
    terminate_spawned_child(daemon);
    if record_written {
        remove_runtime_file(&paths.swap_dev_file);
    }
    remove_runtime_file(&paths.pid_file);
    disarm_forensics_at(paths);
}

fn connect_nbd_with<R: CommandRunner>(
    runner: &R,
    connections: u32,
    swap_dev: &str,
    vram_prio: i32,
    daemon: &mut std::process::Child,
    paths: &RuntimePaths,
) -> Result<(), CascadeError> {
    if let Err(error) = validate_nbd_swap_device(swap_dev) {
        rollback_nbd_attach(runner, false, false, swap_dev, daemon, paths);
        return Err(error);
    }
    if connections == 0 {
        let error = CascadeError::Arg("--connections must be at least 1".into());
        rollback_nbd_attach(runner, false, false, swap_dev, daemon, paths);
        return Err(error);
    }
    let conns = connections.to_string();
    let socket = paths.socket.to_string_lossy().into_owned();
    let mut nbd_args: Vec<&str> = Vec::new();
    if connections > 1 {
        nbd_args.extend(["-C", conns.as_str()]);
    }
    nbd_args.extend(["-unix", socket.as_str(), swap_dev]);
    if let Err(error) = runner.run("nbd-client", &nbd_args) {
        rollback_nbd_attach(runner, false, false, swap_dev, daemon, paths);
        return Err(error);
    }
    if let Err(error) = runner.run("mkswap", &["-L", "RAMSHARED", swap_dev]) {
        rollback_nbd_attach(runner, true, false, swap_dev, daemon, paths);
        return Err(error);
    }
    if let Err(error) = fs::write(&paths.swap_dev_file, swap_dev) {
        rollback_nbd_attach(runner, true, true, swap_dev, daemon, paths);
        return Err(CascadeError::Io(error.to_string()));
    }
    let priority = vram_prio.to_string();
    if let Err(error) = runner.run("swapon", &["-p", &priority, swap_dev]) {
        rollback_nbd_attach(runner, true, true, swap_dev, daemon, paths);
        return Err(error);
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

    // VRAM tier (COLD): daemon + NBD.
    let daemon = (|| {
        validate_nbd_swap_device(&args.swap_dev)?;
        runner.run("modprobe", &["nbd", "nbds_max=1", "max_part=0"])?;
        let mut daemon = spawn_daemon_with_deadline(
            &args.daemon,
            args.vram_mb,
            &args.swap_dev,
            paths,
            Duration::from_secs(6),
        )?;
        connect_nbd_with(
            runner,
            args.connections,
            &args.swap_dev,
            prios.vram,
            &mut daemon,
            paths,
        )?;
        Ok(daemon)
    })();
    let daemon = match daemon {
        Ok(daemon) => daemon,
        Err(error) => {
            let zram_rolled_back =
                rollback_zram_tier(runner, paths, &zram_device, !zram_device.is_empty());
            remove_runtime_file(&paths.socket);
            remove_runtime_file(&paths.pid_file);
            if zram_rolled_back {
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
        "[up] VRAM {} (prio {}, {} MiB, {} connection(s))",
        args.swap_dev, prios.vram, args.vram_mb, args.connections
    );
    eprintln!(
        "[up] cascade active: zram({}) > VRAM({}) > VHDX | anti-hang: down always swapoff before daemon stop",
        prios.zram, prios.vram
    );
    Ok(daemon)
}

fn up_with_config(a: UpArgs) -> Result<(), CascadeError> {
    let prios = TierPriorities::default();
    let runner = SystemCommandRunner;
    let paths = RuntimePaths::system();
    validate_order(prios).map_err(|e| CascadeError::Precondition(e.to_string()))?;

    check_transport(a.transport)?;

    // Ghosts: never auto-recover (#16).
    refuse_ghost_swap_state()?;

    // SPEC wsl2-cascade-boot ITEM-5: idempotent if already healthy.
    let entries_now = read_swaps();
    if cascade_already_healthy(&entries_now) {
        eprintln!("[up] cascade already active — no action (idempotent)");
        return status(false);
    }

    // SPEC wsl2-cascade-orphan-recover ITEM-2: zero-used orphans → heal once.
    try_recover_zero_used_orphans()?;

    let entries_after = read_swaps();
    if cascade_already_healthy(&entries_after) {
        eprintln!("[up] cascade already active after recovery — no-op");
        return status(false);
    }
    refuse_half_cascade(&entries_after)?;

    check_safety_net(a.vram_mb, a.force, &prios)?;
    let _daemon = setup_new_cascade(&runner, &paths, &a, &prios)?;
    status(false)
}

fn down_with_runtime<R: CommandRunner>(
    runner: &R,
    paths: &RuntimePaths,
) -> Result<(), CascadeError> {
    let recorded_swap = fs::read_to_string(&paths.swap_dev_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let recorded_zram = fs::read_to_string(&paths.zram_dev_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let candidates = swapoff_candidates(recorded_swap.as_deref(), recorded_zram.as_deref());
    eprintln!("[down] swapoff candidates: {candidates:?}");

    // Fetch entries once to use in swapoff_all
    let current_entries = read_swaps();

    // 1) ALWAYS swapoff first — never disconnect/kill with pages on the device.
    let fails = swapoff_all(&candidates, &current_entries);
    if !fails.is_empty() {
        for (p, msg) in &fails {
            eprintln!("[down] swapoff failure {p}: {msg}");
        }
        // If ghost with used>0, hard fail and do not kill daemon / nbd-disconnect.
        let swaps_now = read_swaps();
        let ghosts = ghost_vram_swaps(&swaps_now);
        if ghosts.iter().any(|e| e.used_kb > 0) {
            return Err(CascadeError::Precondition(
                "ghost swap has pages in use — forcing it can hang WSL. \
                 On Windows run `wsl --shutdown`, then `sudo ramshared down` and `up`."
                    .into(),
            ));
        }
        // Non-ghost failures: still refuse kill if block swap remains
        if active_vram_block_swap(&read_swaps()) {
            return Err(CascadeError::Precondition(
                "swapoff is incomplete and nbd/ublk remains in /proc/swaps; \
                 do not kill the daemon. Intervene with manual swapoff."
                    .into(),
            ));
        }
    }

    // 2) Reset zram devices we know about
    if let Some(ref z) = recorded_zram {
        let _ = runner.run("zramctl", &["-r", z]);
    }
    // Also try reset any leftover zram still listed
    for e in read_swaps() {
        if is_zram_device_path(&e.filename) && !e.is_ghost() {
            let z = e.canonical_path();
            let _ = swapoff_try(&z);
            let _ = runner.run("zramctl", &["-r", &z]);
        }
    }

    // 3) Disconnect NBD only after swapoff (EOF → daemon zero() VRAM)
    let mut nbd_targets: Vec<String> = recorded_swap
        .into_iter()
        .map(|s| canonicalize_swap_path(&s))
        .chain(
            read_swaps()
                .into_iter()
                .filter(|e| is_nbd_device_path(&e.filename))
                .map(|e| e.canonical_path()),
        )
        .collect();
    nbd_targets.sort();
    nbd_targets.dedup();
    for dev in &nbd_targets {
        if is_allowlisted_managed_path(dev) && is_nbd_device_path(dev) {
            let _ = runner.run("nbd-client", &["-d", dev]);
        }
    }

    // 4) Daemon stop — only if no block VRAM swap remains
    stop_daemon_gracefully_at(paths, Duration::from_secs(10));

    remove_runtime_file(&paths.socket);
    remove_runtime_file(&paths.zram_dev_file);
    remove_runtime_file(&paths.swap_dev_file);
    remove_runtime_file(&paths.pid_file);
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
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ramshared-cascade-io-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path)
                .unwrap_or_else(|error| panic!("create test directory {path:?}: {error}"));
            Self { path }
        }

        fn program(&self, name: &str, source: &str) -> PathBuf {
            let path = self.path.join(name);
            fs::write(&path, source)
                .unwrap_or_else(|error| panic!("write test program {path:?}: {error}"));
            let mut permissions = fs::metadata(&path)
                .unwrap_or_else(|error| panic!("stat test program {path:?}: {error}"))
                .permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions)
                .unwrap_or_else(|error| panic!("chmod test program {path:?}: {error}"));
            path
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

    fn error_from<T>(result: Result<T, CascadeError>, expectation: &str) -> CascadeError {
        match result {
            Ok(_) => panic!("{expectation}"),
            Err(error) => error,
        }
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
            TEST_MEM_AVAILABLE.with(|cell| *cell.borrow_mut() = None);
            SH_SCRIPT.with(|queue| queue.borrow_mut().clear());
        }
    }

    fn controlled_child() -> std::process::Child {
        Command::new("/bin/sleep")
            .arg("10")
            .spawn()
            .unwrap_or_else(|error| panic!("spawn controlled child: {error}"))
    }

    #[test]
    fn bounded_command_captures_stdout_and_rejects_nonzero() {
        let fixture = TestDir::new();
        let success = fixture.program("success", "#!/bin/sh\nprintf ' /dev/zram7\\n'\n");
        let output =
            run_command_bounded_for(&as_program(&success), &[], Duration::from_millis(250))
                .unwrap_or_else(|error| panic!("bounded success command: {error}"));
        assert_eq!(output, "/dev/zram7");

        let failure = fixture.program(
            "failure",
            "#!/bin/sh\nprintf 'fixture failure' >&2\nexit 12\n",
        );
        let error = error_from(
            run_command_bounded_for(&as_program(&failure), &[], Duration::from_millis(250)),
            "non-zero child must fail",
        );
        let message = error.to_string();
        assert!(message.contains("exit 12"), "{message}");
        assert!(message.contains("fixture failure"), "{message}");

        let signal = fixture.program("signal", "#!/bin/sh\nkill -TERM $$\n");
        let error = error_from(
            run_command_bounded_for(&as_program(&signal), &[], Duration::from_millis(250)),
            "signal-terminated child must fail",
        );
        assert!(
            error.to_string().contains("terminated by signal"),
            "{error}"
        );

        let oversized = fixture.program("oversized", "#!/bin/sh\nhead -c 65537 /dev/zero\n");
        let error = error_from(
            run_command_bounded_for(&as_program(&oversized), &[], Duration::from_millis(250)),
            "bounded output must reject an oversized stream",
        );
        assert!(error.to_string().contains("output exceeded"), "{error}");
    }

    #[test]
    fn bounded_command_times_out_and_reaps_its_direct_child() {
        let fixture = TestDir::new();
        let pid_file = fixture.path.join("owned.pid");
        let sleeper = fixture.program(
            "sleeper",
            "#!/bin/sh\nprintf '%s' \"$$\" > \"$1\"\nexec sleep 10\n",
        );
        let pid_arg = pid_file.to_string_lossy().into_owned();
        let started = Instant::now();
        let error = error_from(
            run_command_bounded_for(
                &as_program(&sleeper),
                &[pid_arg.as_str()],
                Duration::from_millis(40),
            ),
            "sleeping direct child must time out",
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.to_string().contains("timed out"), "{error}");

        let pid = fs::read_to_string(&pid_file)
            .unwrap_or_else(|read_error| panic!("read owned child pid: {read_error}"))
            .trim()
            .parse::<u32>()
            .unwrap_or_else(|parse_error| panic!("parse owned child pid: {parse_error}"));
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "timed-out direct child {pid} was not reaped"
        );
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
                "/dev/nbd0",
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
        let socket = paths.socket.to_string_lossy().into_owned();
        let attach = format!("nbd-client -unix {socket} /dev/nbd0");
        let runner = ScriptedRunner::new(vec![
            (attach.clone(), Ok(String::new())),
            (
                "mkswap -L RAMSHARED /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "mkswap -L RAMSHARED /dev/nbd0".into(),
                    msg: "fixture format failure".into(),
                }),
            ),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
        ]);

        let error = error_from(
            connect_nbd_with(&runner, 1, "/dev/nbd0", 100, &mut daemon, &paths),
            "mkswap error must be returned after rollback",
        );
        assert!(
            error.to_string().contains("fixture format failure"),
            "primary error was replaced: {error}"
        );
        assert_eq!(
            runner.calls(),
            vec![
                attach,
                "mkswap -L RAMSHARED /dev/nbd0".into(),
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
            connect_nbd_with(&runner, 0, "/dev/nbd0", 100, &mut daemon, &paths),
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
    fn connect_nbd_rolls_back_in_reverse_order_on_swapon_failure() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        let mut daemon = controlled_child();
        let socket = paths.socket.to_string_lossy().into_owned();
        let attach = format!("nbd-client -unix {socket} /dev/nbd0");
        let runner = ScriptedRunner::new(vec![
            (attach.clone(), Ok(String::new())),
            ("mkswap -L RAMSHARED /dev/nbd0".into(), Ok(String::new())),
            (
                "swapon -p 100 /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "swapon -p 100 /dev/nbd0".into(),
                    msg: "fixture activate failure".into(),
                }),
            ),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
        ]);

        let error = error_from(
            connect_nbd_with(&runner, 1, "/dev/nbd0", 100, &mut daemon, &paths),
            "swapon failure must roll back attach",
        );
        assert!(
            error.to_string().contains("fixture activate failure"),
            "{error}"
        );
        assert_eq!(
            runner.calls(),
            vec![
                attach,
                "mkswap -L RAMSHARED /dev/nbd0".into(),
                "swapon -p 100 /dev/nbd0".into(),
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
        assert!(!paths.swap_dev_file.exists());
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
    fn zram_activation_failure_removes_pending_runtime_record() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
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
            ("zramctl -r /dev/zram7".into(), Ok(String::new())),
        ]);

        let error = error_from(
            setup_zram_with(&runner, &paths, 64, 200),
            "zram activation failure must be returned after cleanup",
        );
        assert!(
            error
                .to_string()
                .contains("fixture zram activation failure")
        );
        assert_eq!(
            runner.calls(),
            vec![
                "modprobe zram".to_string(),
                "zramctl --find --size 64M --algorithm lzo-rle".to_string(),
                "mkswap /dev/zram7".to_string(),
                "swapon -p 200 /dev/zram7".to_string(),
                "zramctl -r /dev/zram7".to_string(),
            ]
        );
        assert!(!paths.zram_dev_file.exists());
    }

    #[test]
    fn sysfs_fallback_refuses_non_block_device_and_safety_checks_fail_closed() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.zram_sysfs)
            .unwrap_or_else(|error| panic!("create test sysfs root: {error}"));
        fs::write(paths.zram_sysfs.join("reset"), "")
            .unwrap_or_else(|error| panic!("create test reset: {error}"));
        fs::write(paths.zram_sysfs.join("comp_algorithm"), "")
            .unwrap_or_else(|error| panic!("create test algorithm: {error}"));
        fs::create_dir_all(
            paths
                .zram_device
                .parent()
                .unwrap_or_else(|| panic!("test zram device has no parent")),
        )
        .unwrap_or_else(|error| panic!("create test device root: {error}"));
        fs::write(&paths.zram_device, "not a block device")
            .unwrap_or_else(|error| panic!("create test device: {error}"));
        let runner = ScriptedRunner::new(Vec::new());
        let error = error_from(
            setup_zram_sysfs_with(&runner, &paths, 1, 200),
            "regular file must not be accepted as zram block device",
        );
        assert!(error.to_string().contains("not a block device"), "{error}");

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
        stop_daemon_gracefully_at(&paths, Duration::from_millis(10));
        assert!(!paths.pid_file.exists());
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
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--sock\" ]; then touch \"$2\"; fi\n  shift\ndone\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
        );
        let paths = RuntimePaths::under(&fixture.path);
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
            ("mkswap -L RAMSHARED /dev/nbd0".into(), Ok(String::new())),
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
        };

        let mut daemon = setup_new_cascade(&runner, &paths, &args, &TierPriorities::default())
            .unwrap_or_else(|error| panic!("isolated cascade setup: {error}"));
        assert_eq!(
            fs::read_to_string(&paths.zram_dev_file).ok().as_deref(),
            Some("/dev/zram7")
        );
        assert_eq!(
            fs::read_to_string(&paths.swap_dev_file).ok().as_deref(),
            Some("/dev/nbd0")
        );
        assert!(
            paths.socket.exists(),
            "fixture daemon did not create its socket path"
        );
        assert_eq!(runner.calls().len(), 8);

        stop_daemon_gracefully_at(&paths, Duration::from_secs(1));
        daemon
            .wait()
            .unwrap_or_else(|error| panic!("reap fixture daemon: {error}"));
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
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--sock\" ]; then touch \"$2\"; fi\n  shift\ndone\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
        );
        let paths = RuntimePaths::under(&fixture.path);
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
                "mkswap -L RAMSHARED /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "mkswap -L RAMSHARED /dev/nbd0".into(),
                    msg: "fixture nbd format failure".into(),
                }),
            ),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
            ("swapoff /dev/zram7".into(), Ok(String::new())),
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
        };

        let error = error_from(
            setup_new_cascade(&runner, &paths, &args, &TierPriorities::default()),
            "NBD failure must roll back the just-created zram tier",
        );
        assert!(error.to_string().contains("fixture nbd format failure"));
        assert_eq!(
            runner.calls(),
            vec![
                "modprobe zram".to_string(),
                "zramctl --find --size 64M --algorithm lzo-rle".to_string(),
                "mkswap /dev/zram7".to_string(),
                "swapon -p 200 /dev/zram7".to_string(),
                "modprobe nbd nbds_max=1 max_part=0".to_string(),
                format!("nbd-client -unix {socket} /dev/nbd0"),
                "mkswap -L RAMSHARED /dev/nbd0".to_string(),
                "nbd-client -d /dev/nbd0".to_string(),
                "swapoff /dev/zram7".to_string(),
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
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--sock\" ]; then touch \"$2\"; fi\n  shift\ndone\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
        );
        let paths = RuntimePaths::under(&fixture.path);
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
                "mkswap -L RAMSHARED /dev/nbd0".into(),
                Err(CascadeError::Shell {
                    cmd: "mkswap -L RAMSHARED /dev/nbd0".into(),
                    msg: "fixture nbd format failure".into(),
                }),
            ),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
            (
                "swapoff /dev/zram7".into(),
                Err(CascadeError::Shell {
                    cmd: "swapoff /dev/zram7".into(),
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
        };

        let error = error_from(
            setup_new_cascade(&runner, &paths, &args, &TierPriorities::default()),
            "swapoff refusal must preserve the original NBD error and zram record",
        );
        assert!(error.to_string().contains("fixture nbd format failure"));
        assert_eq!(
            runner.calls(),
            vec![
                "modprobe zram".to_string(),
                "zramctl --find --size 64M --algorithm lzo-rle".to_string(),
                "mkswap /dev/zram7".to_string(),
                "swapon -p 200 /dev/zram7".to_string(),
                "modprobe nbd nbds_max=1 max_part=0".to_string(),
                format!("nbd-client -unix {socket} /dev/nbd0"),
                "mkswap -L RAMSHARED /dev/nbd0".to_string(),
                "nbd-client -d /dev/nbd0".to_string(),
                "swapoff /dev/zram7".to_string(),
            ]
        );
        assert_eq!(
            fs::read_to_string(&paths.zram_dev_file).ok().as_deref(),
            Some("/dev/zram7")
        );
        assert!(!paths.swap_dev_file.exists());
        assert!(!paths.pid_file.exists());
        assert!(!paths.socket.exists());
        assert!(paths.forensics_markers[0].exists());
    }

    #[test]
    fn down_with_runtime_preserves_swapoff_first_and_cleans_temp_state() {
        let fixture = TestDir::new();
        let paths = RuntimePaths::under(&fixture.path);
        fs::create_dir_all(&paths.runtime_dir)
            .unwrap_or_else(|error| panic!("create test runtime: {error}"));
        fs::create_dir_all(
            paths.forensics_markers[0]
                .parent()
                .unwrap_or_else(|| panic!("test marker has no parent")),
        )
        .unwrap_or_else(|error| panic!("create test forensics: {error}"));
        fs::write(&paths.swap_dev_file, "/dev/nbd0")
            .unwrap_or_else(|error| panic!("write test swap record: {error}"));
        fs::write(&paths.zram_dev_file, "/dev/zram0")
            .unwrap_or_else(|error| panic!("write test zram record: {error}"));
        fs::write(&paths.forensics_markers[0], "armed")
            .unwrap_or_else(|error| panic!("write test marker: {error}"));
        let _seams = ParentSeams::install(
            "Filename Type Size Used Priority\n/dev/nbd0 partition 1024 0 100\n/dev/zram0 partition 1024 0 200\n",
            8,
        );
        let runner = ScriptedRunner::new(vec![
            ("zramctl -r /dev/zram0".into(), Ok(String::new())),
            ("zramctl -r /dev/zram0".into(), Ok(String::new())),
            ("nbd-client -d /dev/nbd0".into(), Ok(String::new())),
        ]);

        down_with_runtime(&runner, &paths)
            .unwrap_or_else(|error| panic!("isolated cascade down: {error}"));
        assert_eq!(
            runner.calls(),
            vec![
                "zramctl -r /dev/zram0",
                "zramctl -r /dev/zram0",
                "nbd-client -d /dev/nbd0",
            ]
        );
        assert!(!paths.swap_dev_file.exists());
        assert!(!paths.zram_dev_file.exists());
        assert!(!paths.forensics_markers[0].exists());
    }

    #[test]
    fn daemon_pid_requires_positive_pid_and_exact_identity() {
        assert!(daemon_pid_matches(42, "ramsharedd\n"));
        assert!(!daemon_pid_matches(0, "ramsharedd\n"));
        assert!(!daemon_pid_matches(42, "ramsharedd-helper\n"));
        assert!(!daemon_pid_matches(42, "other\n"));
    }
}
