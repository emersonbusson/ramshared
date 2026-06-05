//! Orquestração da cascata zram→VRAM→VHDX (SPEC §6.2–6.4). Roda como root.
//! Monta tiers por prioridade de `swapon` e desmonta na ordem inversa, com
//! `swapoff` **antes** de desconectar o NBD (anti-panic).

use ramshared_tier::{TierPriorities, validate_order, vram_safety_net};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

const SOCK: &str = "/run/ramshared/wsl2d.sock";
const NBD: &str = "/dev/nbd0";
const ZRAM_DEV_FILE: &str = "/run/ramshared/zram-dev";

fn sh(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "`{cmd} {}` falhou: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
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

/// Existe um tier de swap ESTRITAMENTE abaixo da VRAM (prio < VRAM) p/ o DEMOTE
/// drenar? (rede A1). Ignora zram/nbd (os tiers que este tool gere) e checa a
/// prioridade real em /proc/swaps — nao apenas "existe algum swap".
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
        .and_then(|p| p.parent().map(|d| d.join("ramshared-wsl2d")))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ramshared-wsl2d".to_string())
}

struct UpArgs {
    vram_mb: u64,
    zram_mb: u64,
    daemon: String,
    force: bool,
}

fn parse_up_args() -> Result<UpArgs, String> {
    let mut a = UpArgs {
        vram_mb: 1024,
        zram_mb: 1024,
        daemon: default_daemon(),
        force: false,
    };
    let args: Vec<String> = std::env::args().skip(2).collect(); // pula "ramshared up"
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--vram" => {
                i += 1;
                a.vram_mb = args
                    .get(i)
                    .ok_or("--vram requer MiB")?
                    .parse()
                    .map_err(|_| "vram invalido")?;
            }
            "--zram" => {
                i += 1;
                a.zram_mb = args
                    .get(i)
                    .ok_or("--zram requer MiB")?
                    .parse()
                    .map_err(|_| "zram invalido")?;
            }
            "--daemon" => {
                i += 1;
                a.daemon = args.get(i).ok_or("--daemon requer caminho")?.clone();
            }
            "--force-no-safety-net" => a.force = true,
            other => return Err(format!("arg desconhecido: {other}")),
        }
        i += 1;
    }
    Ok(a)
}

pub fn up() -> Result<(), String> {
    let a = parse_up_args()?;
    let prios = TierPriorities::default();
    validate_order(prios).map_err(|e| e.to_string())?;

    // A1 — rede de segurança do DEMOTE (precisa de um tier abaixo da VRAM).
    let vram_bytes = a
        .vram_mb
        .checked_mul(1024 * 1024)
        .ok_or("--vram: overflow (MiB grande demais)")?;
    let net = vram_safety_net(lower_tier_present(), mem_available_bytes(), vram_bytes);
    if !net.is_safe() && !a.force {
        return Err(
            "sem rede de seguranca p/ DEMOTE (sem VHDX e RAM insuficiente); \
             use --force-no-safety-net se intencional"
                .into(),
        );
    }
    eprintln!("[up] rede de seguranca A1: {net:?}");
    fs::create_dir_all("/run/ramshared").map_err(|e| e.to_string())?;

    // Tier zram (HOT, prio alta).
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
    sh("mkswap", &[&zdev])?;
    sh("swapon", &["-p", &prios.zram.to_string(), &zdev])?;
    fs::write(ZRAM_DEV_FILE, &zdev).map_err(|e| e.to_string())?;
    eprintln!("[up] zram {zdev} (prio {})", prios.zram);

    // Tier VRAM (COLD, prio média): daemon + nbd.
    sh("modprobe", &["nbd", "nbds_max=1", "max_part=0"])?;
    let _ = fs::remove_file(SOCK);
    Command::new(&a.daemon)
        .args([
            "--size",
            &a.vram_mb.to_string(),
            "--sock",
            SOCK,
            "--nbd",
            NBD,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn daemon ({}): {e}", a.daemon))?;
    let mut ok = false;
    for _ in 0..120 {
        if Path::new(SOCK).exists() {
            ok = true;
            break;
        }
        sleep(Duration::from_millis(50));
    }
    if !ok {
        return Err("daemon nao subiu (socket ausente)".into());
    }
    sh("nbd-client", &["-unix", SOCK, NBD])?;
    sh("mkswap", &["-L", "RAMSHARED", NBD])?;
    sh("swapon", &["-p", &prios.vram.to_string(), NBD])?;
    eprintln!("[up] VRAM {NBD} (prio {}, {} MiB)", prios.vram, a.vram_mb);
    eprintln!(
        "[up] cascata ativa: zram({}) > VRAM({}) > VHDX",
        prios.zram, prios.vram
    );
    status()
}

pub fn down() -> Result<(), String> {
    // Anti-panic: se a VRAM estiver em swap, swapoff DEVE concluir antes do disconnect.
    let nbd_in_swap = fs::read_to_string("/proc/swaps")
        .map(|s| s.contains(NBD))
        .unwrap_or(false);
    if nbd_in_swap {
        sh("swapoff", &[NBD]).map_err(|e| {
            format!("swapoff {NBD} falhou ({e}); NAO desconectando (risco de panic) — intervir")
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
    // nbd-client -d → daemon recebe EOF, zera a VRAM (§11) e sai sozinho.
    let _ = sh("nbd-client", &["-d", NBD]);
    // Espera ele sair por conta propria (ate ~5s) p/ NAO matar no meio do zero() da
    // VRAM (senao sobra dado residual na GPU). pkill so' como ultimo recurso.
    let mut exited = false;
    for _ in 0..50 {
        if sh("pgrep", &["-x", "ramshared-wsl2d"]).is_err() {
            exited = true;
            break;
        }
        sleep(Duration::from_millis(100));
    }
    if !exited {
        eprintln!("[down] daemon nao saiu em 5s; pkill (VRAM pode nao ter sido zerada)");
        let _ = sh("pkill", &["-x", "ramshared-wsl2d"]);
    }
    let _ = fs::remove_file(SOCK);
    let _ = fs::remove_file(ZRAM_DEV_FILE);
    eprintln!("[down] cascata desmontada");
    status()
}

pub fn status() -> Result<(), String> {
    println!("{}", sh("swapon", &["--show"])?);
    Ok(())
}
