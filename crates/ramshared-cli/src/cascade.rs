//! Orchestration of the zram→VRAM→VHDX cascade (SPEC §6.2–6.4). Runs as root.
//! Mounts tiers by `swapon` priority and unmounts in reverse order, with
//! `swapoff` **before** disconnecting the NBD (anti-panic).

use ramshared_tier::{TierPriorities, validate_order, vram_safety_net};
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

const SOCK: &str = "/run/ramshared/wsl2d.sock";
const NBD: &str = "/dev/nbd0";
const ZRAM_DEV_FILE: &str = "/run/ramshared/zram-dev";
const SWAP_DEV_FILE: &str = "/run/ramshared/swap-dev";

/// Typed error for the cascade orchestration (no external dependency — follows the
/// `CudaError` pattern: enum + `Display` + `Error`). Zero-creativity: variants map to the
/// real failure modes (external command, argument, I/O, precondition).
#[derive(Debug)]
pub enum CascadeError {
    /// External command failed (spawn or status != 0).
    Shell { cmd: String, msg: String },
    /// Invalid CLI argument.
    Arg(String),
    /// I/O error (fs / `/proc`).
    Io(String),
    /// Cascade precondition violated (tier order, A1 safety net, device, daemon).
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

/// Is there a swap tier STRICTLY below VRAM (priority < VRAM) for DEMOTE
/// to drain into? (A1 safety net). Ignores zram/nbd (the tiers managed by this tool) and
/// checks the actual priority in /proc/swaps — not just "does any swap exist".
fn lower_tier_present() -> bool {
    let vram_prio = TierPriorities::default().vram;
    fs::read_to_string("/proc/swaps")
        .map(|s| {
            s.lines()
                .skip(1)
                .filter_map(|l| {
                    let c: Vec<&str> = l.split_whitespace().collect();
                    if c.len() < 5 || c[0].contains("zram") || c[0].contains("nbd") {
                        return None;
                    }
                    c[4].parse::<i32>().ok()
                })
                .any(|p| p < vram_prio)
        })
        .unwrap_or(false)
}

fn default_daemon() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("ramsharedd")))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ramsharedd".to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Transport {
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

fn parse_up_args() -> Result<UpArgs, CascadeError> {
    let args: Vec<String> = std::env::args().skip(2).collect(); // skip "ramshared up"
    parse_up_args_from(&args, default_daemon())
}

fn parse_up_args_from(args: &[String], daemon: String) -> Result<UpArgs, CascadeError> {
    let mut a = UpArgs {
        vram_mb: 1024,
        zram_mb: 1024,
        daemon,
        force: false,
        connections: 1,
        transport: Transport::Nbd,
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
                    "nbd" => Transport::Nbd,
                    "ublk" => Transport::Ublk,
                    other => {
                        return Err(CascadeError::Arg(format!(
                            "--transport invalido: {other} (use nbd|ublk)"
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
    Ok(a)
}

pub fn up() -> Result<(), CascadeError> {
    let a = parse_up_args()?;
    let prios = TierPriorities::default();
    validate_order(prios).map_err(|e| CascadeError::Precondition(e.to_string()))?;

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
    fs::create_dir_all("/run/ramshared").map_err(|e| CascadeError::Io(e.to_string()))?;

    if a.transport == Transport::Ublk {
        return Err(CascadeError::Precondition(
            "transport ublk ainda nao implementado; servidor io_uring pendente".into(),
        ));
    }

    // zram tier (HOT, high priority).
    sh("modprobe", &["zram", "num_devices=1"])?;
    let zdev = sh(
        "zramctl",
        &[
            "--find",
            "--size",
            &format!("{}M", a.zram_mb),
            "--algorithm",
            "lzo-rle",
        ],
    )?;
    // M5: zramctl should return /dev/zramN; validate before passing to privileged commands.
    if !matches!(zdev.strip_prefix("/dev/zram"), Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(CascadeError::Precondition(format!(
            "zramctl retornou device inesperado: {zdev}"
        )));
    }
    sh("mkswap", &[&zdev])?;
    sh("swapon", &["-p", &prios.zram.to_string(), &zdev])?;
    fs::write(ZRAM_DEV_FILE, &zdev).map_err(|e| CascadeError::Io(e.to_string()))?;
    eprintln!("[up] zram {zdev} (prio {})", prios.zram);

    // VRAM tier (COLD, medium priority): daemon + nbd.
    sh("modprobe", &["nbd", "nbds_max=1", "max_part=0"])?;
    let _ = fs::remove_file(SOCK);
    Command::new(&a.daemon)
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
    let mut ok = false;
    for _ in 0..120 {
        if Path::new(SOCK).exists() {
            ok = true;
            break;
        }
        sleep(Duration::from_millis(50));
    }
    if !ok {
        return Err(CascadeError::Precondition(
            "daemon nao subiu (socket ausente)".into(),
        ));
    }
    // H1: multi-connection (-C N) only when N>1; the daemon is N-agnostic (accepts whatever comes).
    let conns = a.connections.to_string();
    let mut nbd_args: Vec<&str> = Vec::new();
    if a.connections > 1 {
        nbd_args.extend(["-C", conns.as_str()]);
    }
    nbd_args.extend(["-unix", SOCK, &a.swap_dev]);
    sh("nbd-client", &nbd_args)?;
    sh("mkswap", &["-L", "RAMSHARED", &a.swap_dev])?;
    sh("swapon", &["-p", &prios.vram.to_string(), &a.swap_dev])?;
    fs::write(SWAP_DEV_FILE, &a.swap_dev).map_err(|e| CascadeError::Io(e.to_string()))?;
    eprintln!(
        "[up] VRAM {} (prio {}, {} MiB, {} conexão(ões))",
        a.swap_dev, prios.vram, a.vram_mb, a.connections
    );
    eprintln!(
        "[up] cascata ativa: zram({}) > VRAM({}) > VHDX",
        prios.zram, prios.vram
    );
    status()
}

pub fn down() -> Result<(), CascadeError> {
    let swap_dev = fs::read_to_string(SWAP_DEV_FILE)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| NBD.to_string());
    // Anti-panic: if VRAM is in swap, swapoff MUST complete before disconnect.
    let nbd_in_swap = fs::read_to_string("/proc/swaps")
        .map(|s| s.contains(&swap_dev))
        .unwrap_or(false);
    if nbd_in_swap {
        sh("swapoff", &[&swap_dev]).map_err(|e| {
            CascadeError::Precondition(format!(
                "swapoff {swap_dev} falhou ({e}); NAO desconectando (risco de panic) — intervir"
            ))
        })?;
    }
    // zram tier
    if let Ok(z) = fs::read_to_string(ZRAM_DEV_FILE) {
        let z = z.trim().to_string();
        if !z.is_empty() {
            let _ = sh("swapoff", &[&z]);
            let _ = sh("zramctl", &["-r", &z]);
        }
    }
    // nbd-client -d → daemon receives EOF, zeroes VRAM (§11) and exits on its own.
    let _ = sh("nbd-client", &["-d", &swap_dev]);
    // Wait for it to exit on its own (up to ~5s) to NOT kill it in the middle of zero()
    // VRAM (otherwise residual data remains on GPU). pkill only as a last resort.
    let mut exited = false;
    for _ in 0..50 {
        if sh("pgrep", &["-x", "ramsharedd"]).is_err() {
            exited = true;
            break;
        }
        sleep(Duration::from_millis(100));
    }
    if !exited {
        eprintln!("[down] daemon nao saiu em 5s; pkill (VRAM pode nao ter sido zerada)");
        let _ = sh("pkill", &["-x", "ramsharedd"]);
    }
    let _ = fs::remove_file(SOCK);
    let _ = fs::remove_file(ZRAM_DEV_FILE);
    let _ = fs::remove_file(SWAP_DEV_FILE);
    eprintln!("[down] cascata desmontada");
    status()
}

pub fn status() -> Result<(), CascadeError> {
    println!("{}", sh("swapon", &["--show"])?);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test: unwrap/expect is idiomatic
    use super::*;

    fn parse(args: &[&str]) -> Result<UpArgs, CascadeError> {
        let args = args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        parse_up_args_from(&args, "ramsharedd".to_string())
    }

    #[test]
    fn defaults_to_nbd_transport_and_nbd0_swap_dev() {
        let args = parse(&[]).unwrap();

        assert_eq!(args.transport, Transport::Nbd);
        assert_eq!(args.swap_dev, "/dev/nbd0");
        assert_eq!(args.connections, 1);
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
}
