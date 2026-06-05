//! ramshared-wsl2d — daemon do tier VRAM (SPEC §4, §8).
//!
//! Serve NBD fixed-newstyle num socket Unix; `nbd-client -unix <sock> /dev/nbdX`
//! faz a fiação do kernel (os ioctls). Assim o daemon fica **sem `unsafe`** — o
//! único `unsafe` do projeto vive isolado no `ramshared-cuda`.
//!
//! Aloca a VRAM e serve **N conexões** NBD (`nbd-client -C N`) por um leitor/escritor
//! dedicados por conexão + um **worker CUDA único** (afinidade de thread, §9.4/H1), com
//! `mlockall`+`oom_score_adj` (Disciplina 3) e o canário de residência §9 (latência
//! por-request, agora incluindo a espera na fila) + §9.4 (sonda de conteúdo/free).
//! Backoff segue como trabalho futuro.

use std::os::unix::net::UnixListener;
use std::path::Path;

use ramshared_block::protocol::{NBD_FLAG_CAN_MULTI_CONN, NBD_FLAG_HAS_FLAGS, NBD_FLAG_SEND_FLUSH};
use ramshared_block::{BlockBackend, Command, serve};
use ramshared_cuda::Cuda;
use ramshared_wsl2d::{
    CANARY_BYTES, CANARY_EVERY, CHAN_CAP, Cadence, Canary, CanaryProbe, LiveCount, Reply,
    ResidencyConfig, ResidencySampler, Verdict, VramBackend, WMsg, spawn_acceptor,
};

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

    // --- canário dedicado de residência (§9.4): região separada da swap, NÃO
    // endereçável por NBD (o device anunciado segue = região de swap). Alimenta a
    // sonda de conteúdo/free em cadência (SPECv3 DT-1/DT-9). ---
    let canary_region = ctx.alloc(CANARY_BYTES)?;
    let mut probe = CanaryProbe::new(canary_region);
    let mut cadence = Cadence::new(CANARY_EVERY);
    let mut sampler = ResidencySampler::new(ResidencyConfig::default());

    // --- socket Unix ---
    let path = Path::new(&sock);
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    eprintln!("[wsl2d] escutando em {sock}");
    eprintln!("[wsl2d] conecte: sudo nbd-client -C <N> -unix {sock} {nbd_dev}");

    // --- multi-conexão (H1): acceptor + leitor/escritor por conexão alimentam o worker
    // CUDA único (esta thread). O canal WMsg é o ÚNICO ponto de backpressure (réplica por
    // conexão é ilimitada, DT-7). SPEC: docs/daemon-multiconn/SPECv3.md ---
    let tx_flags = NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_CAN_MULTI_CONN; // DT-10
    let device_size = backend.size_bytes();
    let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel::<WMsg>(CHAN_CAP);
    let _acceptor = spawn_acceptor(listener, device_size, tx_flags, jobs_tx); // move o único sender
    eprintln!("[wsl2d] em transmissão (worker CUDA único; multi-conexão)");

    // Estado do worker (esta thread é dona de backend/probe/ctx — afinidade CUDA).
    let mut canary: Option<Canary> = None;
    let mut baseline: Vec<u64> = Vec::new();
    let mut demoted = false;
    let mut demote_rx: Option<std::sync::mpsc::Receiver<bool>> = None;
    let mut live = LiveCount::new();

    while let Ok(msg) = jobs_rx.recv() {
        let job = match msg {
            WMsg::Opened => {
                live.on_open();
                continue;
            }
            WMsg::Closed => {
                if live.on_close() {
                    break; // todas as conexões abertas fecharam (DT-15)
                }
                continue;
            }
            WMsg::Job(job) => job,
        };

        let touches_vram = matches!(job.req.cmd, Command::Read | Command::Write);
        // DT-16 (revisado): latência SERVE-ONLY (tempo da op de VRAM). Medir a espera na
        // fila dava falso-positivo de DEMOTE sob carga normal (§14.3 ao vivo: baseline
        // 85us idle vs 1.1ms sob fila = 13x → demote indevido). A falha REAL (eviction
        // WDDM) spike o serve ~330x (Fase 0) → o canário dispara nela, não na fila.
        let t0 = std::time::Instant::now();
        let out = serve(&job.req, &job.payload, &mut backend);
        let lat_us = t0.elapsed().as_micros() as u64;
        let _ = job.reply.send(Reply {
            reply: out.reply,
            data: out.read_data,
            disconnect: out.disconnect,
        });

        // Poll nao-bloqueante do swapoff de DEMOTE em curso (re-arma se falhar).
        if let Some(rx) = demote_rx.take() {
            match rx.try_recv() {
                Ok(true) => {
                    demoted = true;
                    eprintln!("[wsl2d] DEMOTE: swapoff {nbd_dev} OK (canario desarmado)");
                }
                Ok(false) => {
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

        // Canário de latência por-request (§9, gatilho PRIMÁRIO): inclui a espera na fila
        // (DT-16). content_ok=true/free=u64::MAX DE PROPÓSITO aqui: o sinal é a latência;
        // conteúdo e free-floor vêm da sonda dedicada §9.4 logo abaixo.
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
                        demote_rx = Some(spawn_swapoff(&nbd_dev));
                    }
                }
            }
        }

        // Canário dedicado §9.4 (SPECv3): sonda de conteúdo/free em cadência. Conteúdo
        // corrompido demove imediato; free-floor/erro transiente exigem streak (histerese).
        if touches_vram && !demoted && demote_rx.is_none() && cadence.tick() {
            let content = probe.check_content().ok();
            let free = ctx.mem_info().ok().map(|(f, _)| f as u64);
            if let Verdict::Demote(reason) = sampler.sample(content, free) {
                // M4: loga os valores reais (None = erro de sonda/meminfo) e o streak.
                eprintln!(
                    "[wsl2d] DEMOTE ({reason:?}) content={content:?} free={free:?} streak={} -> swapoff {nbd_dev}",
                    sampler.bad_streak()
                );
                demote_rx = Some(spawn_swapoff(&nbd_dev));
            }
        }
    }

    // --- teardown (DT-17): espera (bounded) o swapoff em voo, loga honesto, zera ambos.
    // Aqui todas as conexões NBD já caíram → ninguém lê a VRAM por NBD → zerar é safe.
    if let Some(rx) = demote_rx.take() {
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(true) => eprintln!("[wsl2d] teardown: swapoff {nbd_dev} confirmado (DEMOTE limpo)"),
            Ok(false) => eprintln!(
                "[wsl2d] teardown: AVISO swapoff {nbd_dev} NAO confirmou (swap pode estar inconsistente)"
            ),
            Err(_) => eprintln!(
                "[wsl2d] teardown: AVISO swapoff {nbd_dev} sem confirmacao em 5s (timeout/thread sumiu)"
            ),
        }
    }
    let zeroed = backend.zero();
    let _ = probe.zero(); // DT-12/DT-17: zera tambem a regiao-canario (§11)
    let _ = std::fs::remove_file(path);
    zeroed?;
    eprintln!("[wsl2d] encerrado (VRAM zerada)");
    Ok(())
}

/// Caminho absoluto do `swapoff` (#2c: um daemon root NAO deve depender do `$PATH`;
/// evita shim malicioso no ambiente). Fallback p/ `$PATH` so' como ultimo recurso.
fn swapoff_bin() -> &'static str {
    const CANDIDATES: &[&str] = &["/usr/sbin/swapoff", "/sbin/swapoff"];
    for c in CANDIDATES {
        if std::path::Path::new(c).exists() {
            return c;
        }
    }
    "swapoff"
}

/// Dispara `swapoff <dev>` numa thread separada (nao bloqueia o worker) e devolve o
/// canal que confirma o resultado. Caminho unico de DEMOTE (DT-8): usado pela latencia
/// por-request e pela sonda em cadencia.
fn spawn_swapoff(dev: &str) -> std::sync::mpsc::Receiver<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    let dev = dev.to_string();
    std::thread::spawn(move || {
        let ok = std::process::Command::new(swapoff_bin())
            .arg(&dev)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let _ = tx.send(ok);
    });
    rx
}
