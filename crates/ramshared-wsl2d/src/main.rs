//! ramshared-wsl2d — daemon do tier VRAM (SPEC §4, §8).
//!
//! Serve NBD fixed-newstyle num socket Unix; `nbd-client -unix <sock> /dev/nbdX`
//! faz a fiação do kernel (os ioctls). Assim o daemon fica **sem `unsafe`** — o
//! único `unsafe` do projeto vive isolado no `ramshared-cuda`.
//!
//! MVP/smoke: aloca a VRAM, serve **uma** conexão e sai. Sequência completa
//! (`mlockall`, `oom_score_adj`, backoff, canário §9) entra nos próximos passos.

use std::io::{BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;

use ramshared_block::protocol::{NBD_FLAG_HAS_FLAGS, NBD_FLAG_SEND_FLUSH, REQUEST_LEN};
use ramshared_block::{BlockBackend, Command, parse_request, serve, server_handshake};
use ramshared_cuda::Cuda;
use ramshared_wsl2d::{Canary, ResidencyConfig, Verdict, VramBackend};

// Disciplina 3 (anti-deadlock): o daemon serve o swap, logo nao pode ser swapado.
unsafe extern "C" {
    fn mlockall(flags: core::ffi::c_int) -> core::ffi::c_int;
}
const MCL_CURRENT: core::ffi::c_int = 1;
const MCL_FUTURE: core::ffi::c_int = 2;

const DEFAULT_SIZE: u64 = 256 * 1024 * 1024;
const BLOCK_SIZE: u32 = 4096;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[wsl2d] erro: {e}");
            std::process::ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut size = DEFAULT_SIZE;
    let mut sock = "/run/ramshared/wsl2d.sock".to_string();
    let mut force = false;
    let mut nbd_dev = "/dev/nbd0".to_string();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                i += 1;
                let mb: u64 = args.get(i).ok_or("--size requer valor (MiB)")?.parse()?;
                size = mb
                    .checked_mul(1024 * 1024)
                    .ok_or("--size: overflow (MiB grande demais)")?;
            }
            "--sock" => {
                i += 1;
                sock = args.get(i).ok_or("--sock requer caminho")?.clone();
            }
            "--force" => force = true,
            "--nbd" => {
                i += 1;
                nbd_dev = args.get(i).ok_or("--nbd requer caminho")?.clone();
            }
            other => return Err(format!("argumento desconhecido: {other}").into()),
        }
        i += 1;
    }
    size -= size % BLOCK_SIZE as u64; // alinhar ao block size

    // --- CUDA: aloca e zera a VRAM ---
    let cuda = Cuda::load()?;
    let dev = cuda.device(0)?;
    eprintln!("[wsl2d] GPU: {}", dev.name());
    let ctx = cuda.create_context(&dev)?;
    let (free, total) = ctx.mem_info()?;
    eprintln!(
        "[wsl2d] VRAM livre={} MiB total={} MiB",
        free >> 20,
        total >> 20
    );
    let mut mem = ctx.alloc(size as usize)?;
    mem.zero()?;

    // Disciplina 3: trava memoria + protege do OOM killer ANTES de servir swap.
    // SAFETY: mlockall e' uma syscall sem efeitos de memoria inseguros.
    let locked = unsafe { mlockall(MCL_CURRENT | MCL_FUTURE) } == 0;
    if !locked && !force {
        return Err("mlockall falhou; rode como root ou use --force".into());
    }
    let oom_ok = std::fs::write("/proc/self/oom_score_adj", "-1000").is_ok();
    if !oom_ok && !force {
        return Err("nao consegui setar oom_score_adj=-1000; rode como root ou use --force".into());
    }
    if locked && oom_ok {
        eprintln!("[wsl2d] memoria travada (mlockall) + oom_score_adj=-1000");
    } else {
        // --force: seguimos sem a protecao anti-deadlock. Avisa explicitamente: o daemon
        // serve swap e pode ser swapado/morto pelo OOM (Disciplina 3 NAO garantida).
        eprintln!(
            "[wsl2d] AVISO --force: mlockall={} oom_score_adj={} (anti-deadlock NAO garantido)",
            if locked { "ok" } else { "FALHOU" },
            if oom_ok { "ok" } else { "FALHOU" }
        );
    }
    let mut backend = VramBackend::new(mem, BLOCK_SIZE);
    eprintln!(
        "[wsl2d] VRAM alocada: {} MiB, block_size={}",
        size >> 20,
        BLOCK_SIZE
    );

    // --- socket Unix ---
    let path = Path::new(&sock);
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    eprintln!("[wsl2d] escutando em {sock}");
    eprintln!("[wsl2d] conecte: sudo nbd-client -unix {sock} /dev/nbd0");

    // --- serve UMA conexão (MVP) ---
    let (stream, _) = listener.accept()?;
    eprintln!("[wsl2d] cliente conectado");
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    let tx_flags = NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH;
    server_handshake(&mut reader, &mut writer, backend.size_bytes(), tx_flags)?;
    eprintln!("[wsl2d] handshake ok; em transmissão");

    let mut canary: Option<Canary> = None;
    let mut baseline: Vec<u64> = Vec::new();
    let mut demoted = false;
    let mut demote_rx: Option<std::sync::mpsc::Receiver<bool>> = None;
    let mut hdr = [0u8; REQUEST_LEN];
    loop {
        match reader.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let req = parse_request(&hdr)?;
        // Defesa anti-DoS: um WRITE nunca pode exceder o device; len absurdo => request
        // malformado => desconecta em vez de alocar (evita alloc de ate ~4 GiB).
        if req.cmd == Command::Write && req.len as u64 > backend.size_bytes() {
            eprintln!(
                "[wsl2d] WRITE len {} excede o device; desconectando",
                req.len
            );
            break;
        }
        let payload = if req.cmd == Command::Write {
            let mut p = vec![0u8; req.len as usize];
            reader.read_exact(&mut p)?;
            p
        } else {
            Vec::new()
        };
        let touches_vram = matches!(req.cmd, Command::Read | Command::Write);
        let t0 = std::time::Instant::now();
        let out = serve(&req, &payload, &mut backend);
        let lat_us = t0.elapsed().as_micros() as u64;
        writer.write_all(&out.reply)?;
        if !out.read_data.is_empty() {
            writer.write_all(&out.read_data)?;
        }
        writer.flush()?;
        // Poll nao-bloqueante do swapoff de DEMOTE em curso: o serve loop PRECISA
        // continuar atendendo o read-back do swapoff (senao paginas sujas se perdem).
        if let Some(rx) = demote_rx.take() {
            match rx.try_recv() {
                Ok(true) => {
                    demoted = true; // confirmado: desarma o canario de vez
                    eprintln!("[wsl2d] DEMOTE: swapoff {nbd_dev} OK (canario desarmado)");
                }
                Ok(false) => {
                    // swapoff falhou: device ainda e' swap vivo e degradado. NAO engole —
                    // deixa demote_rx=None p/ re-armar e tentar de novo no proximo spike.
                    eprintln!("[wsl2d] DEMOTE: swapoff {nbd_dev} FALHOU; canario re-armado");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    demote_rx = Some(rx); // ainda em curso; devolve
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    eprintln!("[wsl2d] DEMOTE: thread de swapoff sumiu; canario re-armado");
                }
            }
        }
        // Canario inline (§9): mede a latencia do I/O da VRAM e dispara DEMOTE sob spike.
        // content_ok=true e free=u64::MAX sao DE PROPOSITO: a eviction WDDM e' data-safe
        // (Fase 0), entao a latencia e' o gatilho vivo aqui. O canario dedicado de
        // conteudo/free-floor (§9.4) exige uma regiao-canario propria (trabalho futuro).
        if touches_vram && !demoted && demote_rx.is_none() {
            match canary.as_mut() {
                None => {
                    baseline.push(lat_us);
                    if baseline.len() >= 16 {
                        baseline.sort_unstable();
                        let med = baseline[baseline.len() / 2].max(1);
                        canary = Some(Canary::new(ResidencyConfig::default(), med));
                        eprintln!("[wsl2d] canario armado (baseline={med} us)");
                    }
                }
                Some(c) => {
                    if let Verdict::Demote(reason) = c.sample(lat_us, true, u64::MAX) {
                        eprintln!(
                            "[wsl2d] DEMOTE ({reason:?}) lat={lat_us}us -> swapoff {nbd_dev}"
                        );
                        // Thread separada (nao bloqueia o serve); o resultado volta pelo
                        // canal e so' entao decidimos desarmar (ver poll acima).
                        let dev = nbd_dev.clone();
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let ok = std::process::Command::new("swapoff")
                                .arg(&dev)
                                .status()
                                .map(|s| s.success())
                                .unwrap_or(false);
                            let _ = tx.send(ok);
                        });
                        demote_rx = Some(rx);
                    }
                }
            }
        }
        if out.disconnect {
            eprintln!("[wsl2d] disconnect");
            break;
        }
    }

    // --- teardown: zera a VRAM (§11) e remove o socket ---
    backend.zero()?;
    let _ = std::fs::remove_file(path);
    eprintln!("[wsl2d] encerrado (VRAM zerada)");
    Ok(())
}
