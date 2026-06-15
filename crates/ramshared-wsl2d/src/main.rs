//! ramsharedd (crate `ramshared-wsl2d`) — daemon do tier VRAM + Memory Broker (SPEC §4, §8).
//!
//! Serve NBD fixed-newstyle num socket Unix; `nbd-client -unix <sock> /dev/nbdX`
//! faz a fiação do kernel (os ioctls). Assim o daemon fica **sem `unsafe`** — o
//! único `unsafe` do projeto vive isolado no `ramshared-cuda`.
//!
//! Aloca a VRAM e serve **N conexões** NBD (`nbd-client -C N`) por um leitor/escritor
//! dedicados por conexão + um **worker CUDA único** (afinidade de thread, §9.4/H1), com
//! `mlockall`+`oom_score_adj` (Disciplina 3) e o canário de residência §9 (latência
//! por-request, **serve-only**) + §9.4 (sonda de conteúdo/free).
//! Backoff segue como trabalho futuro.

use core::ffi::c_int;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ramshared_block::protocol::{NBD_FLAG_CAN_MULTI_CONN, NBD_FLAG_HAS_FLAGS, NBD_FLAG_SEND_FLUSH};
use ramshared_block::{BlockBackend, Command, serve};
use ramshared_broker::arbiter::ArbiterConfig;
use ramshared_broker::slices::SliceMap;
use ramshared_cuda::Cuda;
use ramshared_wsl2d::broker_srv::{BrokerConfig, EndpointCfg, spawn_broker};
use ramshared_wsl2d::swap::spawn_swapoff;
use ramshared_wsl2d::{
    CANARY_BYTES, CANARY_EVERY, CHAN_CAP, Cadence, Canary, CanaryProbe, DemoteReason, LiveCount,
    RamBackend, Reply, ResidencyConfig, ResidencySampler, SliceView, Verdict, VramBackend, WMsg,
    spawn_acceptor,
};
use ramshared_wsl2d::{ublk, ublk_control, ublk_server};

// Disciplina 3 (anti-deadlock): o daemon serve o swap, logo nao pode ser swapado.
unsafe extern "C" {
    fn mlockall(flags: c_int) -> c_int;
    // Registro de handler de sinal (sighandler_t é um ponteiro de função; o retorno
    // anterior é ignorado). Usado só para SIGINT/SIGTERM no modo ublk.
    fn signal(signum: c_int, handler: extern "C" fn(c_int)) -> usize;
}
const MCL_CURRENT: c_int = 1;
const MCL_FUTURE: c_int = 2;
const SIGINT: c_int = 2;
const SIGTERM: c_int = 15;

const DEFAULT_SIZE: u64 = 256 * 1024 * 1024;
const BLOCK_SIZE: u32 = 4096;
const UBLK_CONTROL: &str = "/dev/ublk-control";
const SECTOR: u64 = 512;

/// Transporte do tier VRAM: NBD (socket Unix) ou ublk (block device direto).
enum Transport {
    Nbd,
    Ublk,
}

/// Backend do tier ublk: VRAM (GPU, com residência §9/§9.4) ou RAM (sem GPU). O RAM
/// existe para validar o **ciclo de vida/teardown** do daemon ublk em **qemu** (onde
/// não há GPU); o bug de teardown que travou o WSL2 é independente do backend.
#[derive(Clone, Copy)]
enum BackendKind {
    Vram,
    Ram,
}

impl BackendKind {
    fn label(self) -> &'static str {
        match self {
            BackendKind::Vram => "vram",
            BackendKind::Ram => "ram",
        }
    }
}

/// Une os dois tipos de handle do servidor DT-3 (VRAM-com-residência ou RAM puro) para
/// um teardown único no `run_ublk`.
enum UblkHandle {
    Vram(ublk_server::ServerHandleDt3VramResidency),
    Ram(ublk_server::ServerHandleDt3<RamBackend>),
}

impl UblkHandle {
    fn join(self) -> std::io::Result<()> {
        match self {
            UblkHandle::Vram(h) => h.join(),
            UblkHandle::Ram(h) => h.join().map(|_| ()),
        }
    }
}

/// Pedido de encerramento (SIGINT/SIGTERM). O handler só faz um store atômico
/// (async-signal-safe); o laço do daemon ublk faz poll desta flag.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_shutdown(_sig: c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[ramsharedd] erro: {e}");
            std::process::ExitCode::from(1)
        }
    }
}

/// Parseia `IP:PORT` (aceita o prefixo `tcp://`) e **recusa endereços unspecified** (0.0.0.0/::)
/// — RNF-2: bind só em rede privada/loopback, nunca público. Falha ANTES de qualquer `bind()`.
fn parse_private_listen(s: &str) -> Result<std::net::SocketAddr, String> {
    let raw = s.strip_prefix("tcp://").unwrap_or(s);
    let addr: std::net::SocketAddr = raw
        .parse()
        .map_err(|_| format!("endereço inválido '{s}' (use IP:PORT)"))?;
    if addr.ip().is_unspecified() {
        return Err(format!(
            "bind em {} recusado — RNF-2: só rede privada/loopback, nunca 0.0.0.0/::",
            addr.ip()
        ));
    }
    Ok(addr)
}

/// Valida o combo de flags de slice (DT-3: ublk é single-device no WSL2; `--slice-mb` obrigatório).
fn validate_slice_flags(slices: u16, slice_mb: u64, is_ublk: bool) -> Result<(), String> {
    if slices > 0 && is_ublk {
        return Err(
            "--slices não combina com --transport ublk (DT-3: ublk single-device no WSL2)".into(),
        );
    }
    if slices > 0 && slice_mb == 0 {
        return Err("--slices > 0 exige --slice-mb N".into());
    }
    Ok(())
}

/// Zera a janela `[base, base+len)` do backend em chunks de 1 MiB (higiene de slice, DT-17).
/// Roda na thread dona do backend (worker CUDA único) — `WMsg::ZeroExport`.
fn zero_window<B: BlockBackend>(
    backend: &mut B,
    base: u64,
    len: u64,
) -> Result<(), ramshared_block::IoError> {
    const CHUNK: usize = 1 << 20;
    let buf = vec![0u8; CHUNK.min(len as usize)];
    let mut off = 0u64;
    while off < len {
        let n = ((len - off) as usize).min(buf.len());
        backend.write_at(base + off, &buf[..n])?;
        off += n as u64;
    }
    Ok(())
}

/// Residência por-request compartilhada pelos workers NBD (single e broker): arma o canário
/// de latência (§9, baseline→Canary; serve-only, DT-16) e roda a sonda §9.4 (conteúdo/free em
/// cadência, com histerese via streak). Devolve `Some(reason)` se algum sinal pede DEMOTE; o
/// chamador decide a AÇÃO (swapoff local no single, `DemoteAll` via broker no multi-slice).
fn residency_check<F: Fn() -> Option<u64>>(
    lat_us: u64,
    canary: &mut Option<Canary>,
    baseline: &mut Vec<u64>,
    sampler: &mut ResidencySampler,
    cadence: &mut Cadence,
    probe: &mut CanaryProbe,
    mem_free: F,
) -> Option<DemoteReason> {
    // §9: canário de latência por-request. content_ok=true/free=u64::MAX DE PROPÓSITO — o sinal
    // aqui é a latência; conteúdo e free-floor vêm da sonda §9.4 abaixo.
    match canary.as_mut() {
        None => {
            baseline.push(lat_us);
            if baseline.len() >= 16 {
                baseline.sort_unstable();
                let med = baseline[baseline.len() / 2].max(1);
                *canary = Some(Canary::new(ResidencyConfig::default(), med));
                eprintln!("[ramsharedd] canario armado (baseline={med} us)");
            }
        }
        Some(c) => {
            if let Verdict::Demote(reason) = c.sample(lat_us, true, u64::MAX) {
                return Some(reason);
            }
        }
    }
    // §9.4: sonda dedicada de conteúdo/free em cadência (conteúdo corrompido demove imediato;
    // free-floor/erro transiente exigem streak).
    if cadence.tick() {
        let content = probe.check_content().ok();
        let free = mem_free();
        if let Verdict::Demote(reason) = sampler.sample(content, free) {
            eprintln!(
                "[ramsharedd] sonda §9.4: content={content:?} free={free:?} streak={}",
                sampler.bad_streak()
            );
            return Some(reason);
        }
    }
    None
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut size = DEFAULT_SIZE;
    let mut sock = "/run/ramshared/wsl2d.sock".to_string();
    let mut force = false;
    let mut nbd_dev = "/dev/nbd0".to_string();
    let mut transport = Transport::Nbd;
    let mut queue_depth = 1u16;
    let mut backend = BackendKind::Vram;
    // ITEM-8 (broker): flags do modo multi-slice. Parsing+validação aqui (puro/testável); o
    // runtime do broker (broker_srv + rework do run_nbd) vem nos próximos recortes do ITEM-8.
    let mut slices = 0u16;
    let mut slice_mb = 0u64;
    let mut listen_nbd: Option<String> = None;
    let mut arbiter: Option<String> = None;
    let mut advertise_nbd: Option<String> = None;
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
            "--transport" => {
                i += 1;
                transport = match args.get(i).map(String::as_str) {
                    Some("nbd") => Transport::Nbd,
                    Some("ublk") => Transport::Ublk,
                    _ => return Err("--transport requer 'nbd' ou 'ublk'".into()),
                };
            }
            "--queue-depth" => {
                i += 1;
                queue_depth = args
                    .get(i)
                    .ok_or("--queue-depth requer valor")?
                    .parse()
                    .map_err(|_| "--queue-depth invalido")?;
            }
            "--backend" => {
                i += 1;
                backend = match args.get(i).map(String::as_str) {
                    Some("vram") => BackendKind::Vram,
                    Some("ram") => BackendKind::Ram,
                    _ => return Err("--backend requer 'vram' ou 'ram'".into()),
                };
            }
            "--slices" => {
                i += 1;
                slices = args
                    .get(i)
                    .ok_or("--slices requer valor")?
                    .parse()
                    .map_err(|_| "--slices inválido")?;
            }
            "--slice-mb" => {
                i += 1;
                slice_mb = args
                    .get(i)
                    .ok_or("--slice-mb requer valor (MiB)")?
                    .parse()
                    .map_err(|_| "--slice-mb inválido")?;
            }
            "--listen-nbd" => {
                i += 1;
                listen_nbd = Some(
                    args.get(i)
                        .ok_or("--listen-nbd requer tcp://IP:PORT")?
                        .clone(),
                );
            }
            "--arbiter-listen" => {
                i += 1;
                arbiter = Some(
                    args.get(i)
                        .ok_or("--arbiter-listen requer IP:PORT")?
                        .clone(),
                );
            }
            "--advertise-nbd" => {
                i += 1;
                advertise_nbd = Some(
                    args.get(i)
                        .ok_or("--advertise-nbd requer HOST:PORT")?
                        .clone(),
                );
            }
            other => return Err(format!("argumento desconhecido: {other}").into()),
        }
        i += 1;
    }
    size -= size % BLOCK_SIZE as u64; // alinhar ao block size

    // ITEM-8: validação das flags do broker (pura/testável). RNF-2: bind nunca em 0.0.0.0/::.
    if let Err(e) = validate_slice_flags(slices, slice_mb, matches!(transport, Transport::Ublk)) {
        return Err(e.into());
    }
    let listen_nbd_addr = listen_nbd
        .as_deref()
        .map(parse_private_listen)
        .transpose()?;
    let arbiter_addr = arbiter.as_deref().map(parse_private_listen).transpose()?;
    let advertise_nbd_addr = advertise_nbd
        .as_deref()
        .map(parse_private_listen)
        .transpose()?;
    // --advertise-nbd só faz sentido com --listen-nbd (anunciar um endpoint TCP que se serve).
    if advertise_nbd_addr.is_some() && listen_nbd_addr.is_none() {
        return Err(
            "--advertise-nbd exige --listen-nbd (anunciar um endpoint que se serve)".into(),
        );
    }
    // Endpoint TCP anunciado aos agentes no SwapOn (DT-25): por padrão = addr de bind; com
    // --advertise-nbd, o addr forwarded do host (caso civm via port-forward, ITEM-12).
    let advertise_tcp = advertise_nbd_addr
        .or(listen_nbd_addr)
        .map(|a| (a.ip().to_string(), a.port()));

    // Modo broker (ITEM-8): --slices > 0 fatia a memória e sobe o árbitro. Exige --arbiter-listen
    // (o ponto de controle do broker). --listen-nbd é opcional (tenants TCP/civm além do Unix).
    // --backend ram serve sem GPU (validação em qemu, ITEM-11); vram é o caminho de produção.
    if slices > 0 {
        let arbiter_addr =
            arbiter_addr.ok_or("--slices exige --arbiter-listen IP:PORT (ponto de controle)")?;
        let slice_bytes = slice_mb
            .checked_mul(1024 * 1024)
            .ok_or("--slice-mb: overflow (MiB grande demais)")?;
        return match backend {
            BackendKind::Vram => run_broker(
                slice_bytes,
                slices,
                sock,
                force,
                listen_nbd_addr,
                advertise_tcp,
                arbiter_addr,
            ),
            BackendKind::Ram => run_broker_ram(
                slice_bytes,
                slices,
                sock,
                listen_nbd_addr,
                advertise_tcp,
                arbiter_addr,
            ),
        };
    }
    // Sem slices não há o que arbitrar nem exportar por TCP.
    if arbiter_addr.is_some() || listen_nbd_addr.is_some() {
        return Err("--arbiter-listen/--listen-nbd exigem --slices N (N > 0)".into());
    }

    match transport {
        Transport::Nbd => run_nbd(size, sock, force, nbd_dev),
        Transport::Ublk => run_ublk(size, force, queue_depth, backend),
    }
}

/// Caminho NBD (fixed-newstyle em socket Unix). Worker CUDA unico na thread atual.
fn run_nbd(
    size: u64,
    sock: String,
    force: bool,
    nbd_dev: String,
) -> Result<(), Box<dyn std::error::Error>> {
    // --- CUDA: aloca e zera a VRAM ---
    let cuda = Cuda::load()?;
    let dev = cuda.device(0)?;
    eprintln!("[ramsharedd] GPU: {}", dev.name());
    let ctx = cuda.create_context(&dev)?;
    let (free, total) = ctx.mem_info()?;
    eprintln!(
        "[ramsharedd] VRAM livre={} MiB total={} MiB",
        free >> 20,
        total >> 20
    );
    let mut mem = ctx.alloc(size as usize)?;
    mem.zero()?;

    // Disciplina 3: trava memoria + protege do OOM killer ANTES de servir swap.
    lock_memory(force)?;
    let mut backend = VramBackend::new(mem, BLOCK_SIZE);
    eprintln!(
        "[ramsharedd] VRAM alocada: {} MiB, block_size={}",
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
    eprintln!("[ramsharedd] escutando em {sock}");
    eprintln!("[ramsharedd] conecte: sudo nbd-client -C <N> -unix {sock} {nbd_dev}");

    // --- multi-conexão (H1): acceptor + leitor/escritor por conexão alimentam o worker
    // CUDA único (esta thread). O canal WMsg é o ÚNICO ponto de backpressure (réplica por
    // conexão é ilimitada, DT-7). SPEC: docs/daemon-multiconn/SPECv3.md ---
    let tx_flags = NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_CAN_MULTI_CONN; // DT-10
    let device_size = backend.size_bytes();
    // ITEM-7: tabela de exports. Modo single = 1 export "default" (nome vazio → índice 0,
    // byte-compat Fase B). O broker (ITEM-8) passará a tabela de slices do `SliceMap`.
    let exports = std::sync::Arc::new(vec![ramshared_block::handshake::Export {
        name: "default".to_string(),
        size: device_size,
    }]);
    let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel::<WMsg>(CHAN_CAP);
    let _acceptor = spawn_acceptor(listener, exports, tx_flags, jobs_tx); // move o único sender
    eprintln!("[ramsharedd] em transmissão (worker CUDA único; multi-conexão)");

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
            WMsg::ZeroExport { base, len, done } => {
                // Higiene de slice (DT-17): zera a janela [base,len) na thread dona do backend.
                let ok = zero_window(&mut backend, base, len).is_ok();
                let _ = done.send(ok);
                continue;
            }
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
                    eprintln!("[ramsharedd] DEMOTE: swapoff {nbd_dev} OK (canario desarmado)");
                }
                Ok(false) => {
                    eprintln!("[ramsharedd] DEMOTE: swapoff {nbd_dev} FALHOU; canario re-armado");
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    demote_rx = Some(rx); // ainda em curso; devolve
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    eprintln!("[ramsharedd] DEMOTE: thread de swapoff sumiu; canario re-armado");
                }
            }
        }

        // Residência (§9 latência + §9.4 conteúdo/free): lógica compartilhada com o worker do
        // broker. A AÇÃO aqui é o swapoff local do device servido (single-device).
        if touches_vram
            && !demoted
            && demote_rx.is_none()
            && let Some(reason) = residency_check(
                lat_us,
                &mut canary,
                &mut baseline,
                &mut sampler,
                &mut cadence,
                &mut probe,
                || ctx.mem_info().ok().map(|(f, _)| f as u64),
            )
        {
            eprintln!("[ramsharedd] DEMOTE ({reason:?}) lat={lat_us}us -> swapoff {nbd_dev}");
            demote_rx = Some(spawn_swapoff(&nbd_dev));
        }
    }

    // --- teardown (DT-17): espera (bounded) o swapoff em voo, loga honesto, zera ambos.
    // Aqui todas as conexões NBD já caíram → ninguém lê a VRAM por NBD → zerar é safe.
    if let Some(rx) = demote_rx.take() {
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(true) => {
                eprintln!("[ramsharedd] teardown: swapoff {nbd_dev} confirmado (DEMOTE limpo)")
            }
            Ok(false) => eprintln!(
                "[ramsharedd] teardown: AVISO swapoff {nbd_dev} NAO confirmou (swap pode estar inconsistente)"
            ),
            Err(_) => eprintln!(
                "[ramsharedd] teardown: AVISO swapoff {nbd_dev} sem confirmacao em 5s (timeout/thread sumiu)"
            ),
        }
    }
    let zeroed = backend.zero();
    let _ = probe.zero(); // DT-12/DT-17: zera tambem a regiao-canario (§11)
    let _ = std::fs::remove_file(path);
    zeroed?;
    eprintln!("[ramsharedd] encerrado (VRAM zerada)");
    Ok(())
}

/// Peças do control-plane do broker que o worker (data-plane) consome. Backend-agnóstico
/// (vale p/ VRAM e RAM) — só o backend e a residência diferem entre os modos.
struct BrokerRuntime {
    geom: Vec<(u64, u64)>,
    jobs_rx: std::sync::mpsc::Receiver<WMsg>,
    demote_tx: std::sync::mpsc::Sender<DemoteReason>,
    shutdown: std::sync::Arc<AtomicBool>,
    broker: std::thread::JoinHandle<()>,
}

/// Sobe o control-plane do broker (independente do backend): mapa de slices + geometria +
/// exports NBD ("s0".."sN"), acceptors (Unix sempre; TCP se `--listen-nbd`) alimentando o
/// MESMO canal `jobs` do worker, o árbitro (`spawn_broker`, que compartilha `jobs` p/ os
/// `ZeroExport` de higiene DT-17 e consome o canal de DEMOTE) e a ponte de `SHUTDOWN`
/// (handler de sinal só toca o estático async-signal-safe → espelhado no `Arc`).
fn broker_setup(
    slices: u16,
    slice_bytes: u64,
    sock: &str,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
) -> Result<BrokerRuntime, Box<dyn std::error::Error>> {
    // Mapa de slices: o índice do export (resolvido pelo handshake) == índice na geom == índice
    // em exports (nomes "s{id}" idênticos aos que o broker emite no SwapOn).
    let slice_map = SliceMap::new(slices, slice_bytes);
    let geom: Vec<(u64, u64)> = slice_map
        .slices()
        .iter()
        .map(|s| (s.offset, s.len))
        .collect();
    let exports = std::sync::Arc::new(
        slice_map
            .exports()
            .into_iter()
            .map(|(name, size)| ramshared_block::handshake::Export { name, size })
            .collect::<Vec<_>>(),
    );

    unsafe {
        signal(SIGINT, handle_shutdown);
        signal(SIGTERM, handle_shutdown);
    }
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    {
        let mirror = std::sync::Arc::clone(&shutdown);
        std::thread::spawn(move || {
            while !SHUTDOWN.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
            }
            mirror.store(true, Ordering::SeqCst);
        });
    }

    let tx_flags = NBD_FLAG_HAS_FLAGS | NBD_FLAG_SEND_FLUSH | NBD_FLAG_CAN_MULTI_CONN;
    let (jobs_tx, jobs_rx) = std::sync::mpsc::sync_channel::<WMsg>(CHAN_CAP);
    let (demote_tx, demote_rx) = std::sync::mpsc::channel::<DemoteReason>();

    let path = Path::new(sock);
    let _ = std::fs::remove_file(path);
    let unix = UnixListener::bind(path)?;
    eprintln!("[ramsharedd] NBD unix em {sock}");
    let _ = spawn_acceptor(
        unix,
        std::sync::Arc::clone(&exports),
        tx_flags,
        jobs_tx.clone(),
    );
    if let Some(addr) = listen_nbd_addr {
        let tcp = std::net::TcpListener::bind(addr)?;
        eprintln!("[ramsharedd] NBD tcp em {addr}");
        let _ = ramshared_wsl2d::conn::spawn_acceptor_tcp(
            tcp,
            std::sync::Arc::clone(&exports),
            tx_flags,
            jobs_tx.clone(),
        );
    }

    let bcfg = BrokerConfig {
        listen: arbiter_addr,
        endpoints: EndpointCfg {
            nbd_unix: Some(sock.to_string()),
            nbd_tcp: advertise_tcp,
        },
        swap_prio: None,
        arbiter: ArbiterConfig::default(),
        tick: Duration::from_secs(1),
    };
    let (broker, broker_addr) = spawn_broker(
        bcfg,
        slice_map,
        demote_rx,
        jobs_tx.clone(),
        std::sync::Arc::clone(&shutdown),
    )?;
    eprintln!("[ramsharedd] broker (árbitro) em {broker_addr}");
    drop(jobs_tx); // os clones (acceptors + broker) mantêm o canal; o worker é dono do rx

    Ok(BrokerRuntime {
        geom,
        jobs_rx,
        demote_tx,
        shutdown,
        broker,
    })
}

/// Worker do broker (data-plane), genérico sobre o backend (VRAM ou RAM): serve cada `Job`
/// via [`SliceView`] da geometria do export. DT-28: roda até `shutdown`, NÃO encerra quando as
/// conexões NBD caem (o broker persiste). A residência é injetada por closure — VRAM passa o
/// canário §9/§9.4; RAM passa `|_| None` (RAM não sofre eviction WDDM). Em DEMOTE, notifica o
/// broker (`DemoteAll` a todos os tenants; a VRAM compartilhada compromete TODAS as slices) e
/// para de amostrar. Devolve o backend p/ o teardown (wipe seguro é responsabilidade do dono).
fn serve_broker_jobs<B: BlockBackend>(
    mut backend: B,
    rt: &BrokerRuntime,
    mut residency: impl FnMut(u64) -> Option<DemoteReason>,
) -> B {
    let mut demoted = false;
    eprintln!("[ramsharedd] em transmissão (worker único; multi-slice/broker)");
    loop {
        let msg = match rt.jobs_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(m) => m,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if rt.shutdown.load(Ordering::SeqCst) {
                    break; // DT-28: encerra só no SIGINT/SIGTERM
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let job = match msg {
            // DT-28: conexões NBD indo e vindo NÃO encerram o daemon (o broker persiste).
            WMsg::Opened | WMsg::Closed => continue,
            WMsg::Job(job) => job,
            WMsg::ZeroExport { base, len, done } => {
                let ok = zero_window(&mut backend, base, len).is_ok();
                let _ = done.send(ok);
                continue;
            }
        };

        let touches = matches!(job.req.cmd, Command::Read | Command::Write);
        // Geometria do export (handshake já resolveu nome→índice). Fallback defensivo: backend
        // inteiro (não deve ocorrer — todo Job carrega um export válido).
        let (base, len) = rt
            .geom
            .get(job.export)
            .copied()
            .unwrap_or((0, backend.size_bytes()));
        let t0 = std::time::Instant::now();
        let out = {
            let mut view = SliceView::new(&mut backend, base, len);
            serve(&job.req, &job.payload, &mut view)
        };
        let lat_us = t0.elapsed().as_micros() as u64;
        let _ = job.reply.send(Reply {
            reply: out.reply,
            data: out.read_data,
            disconnect: out.disconnect,
        });

        if touches
            && !demoted
            && let Some(reason) = residency(lat_us)
        {
            eprintln!("[ramsharedd] DEMOTE ({reason:?}) lat={lat_us}us -> broker DemoteAll");
            let _ = rt.demote_tx.send(reason);
            demoted = true;
        }
    }
    backend
}

/// Caminho broker VRAM (ITEM-8): fatia a VRAM em `slices` exports NBD servidos por Unix +
/// (opcional) TCP, com o árbitro decidindo quem usa cada slice. O worker único é dono da
/// VRAM/contexto CUDA e roda a residência §9/§9.4. Execução ao vivo é o gate qemu (`--backend
/// ram`, ITEM-11) / civm (ITEM-12) — VRAM real não roda em qemu (sem GPU).
fn run_broker(
    slice_bytes: u64,
    slices: u16,
    sock: String,
    force: bool,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let total = (slices as u64)
        .checked_mul(slice_bytes)
        .ok_or("--slices * --slice-mb: overflow")?;

    let cuda = Cuda::load()?;
    let dev = cuda.device(0)?;
    eprintln!("[ramsharedd] GPU: {}", dev.name());
    let ctx = cuda.create_context(&dev)?;
    let (free, total_vram) = ctx.mem_info()?;
    eprintln!(
        "[ramsharedd] VRAM livre={} MiB total={} MiB",
        free >> 20,
        total_vram >> 20
    );
    let mut mem = ctx.alloc(total as usize)?;
    mem.zero()?;
    lock_memory(force)?; // Disciplina 3: trava memória ANTES de servir swap
    let backend = VramBackend::new(mem, BLOCK_SIZE);
    eprintln!(
        "[ramsharedd] broker VRAM: {slices} slices x {} MiB = {} MiB, block_size={BLOCK_SIZE}",
        slice_bytes >> 20,
        total >> 20
    );

    // Canário de residência (§9.4): região separada, não endereçável por NBD.
    let canary_region = ctx.alloc(CANARY_BYTES)?;
    let mut probe = CanaryProbe::new(canary_region);
    let mut cadence = Cadence::new(CANARY_EVERY);
    let mut sampler = ResidencySampler::new(ResidencyConfig::default());
    let mut canary: Option<Canary> = None;
    let mut baseline: Vec<u64> = Vec::new();

    let rt = broker_setup(
        slices,
        slice_bytes,
        &sock,
        listen_nbd_addr,
        advertise_tcp,
        arbiter_addr,
    )?;
    let mut backend = serve_broker_jobs(backend, &rt, |lat_us| {
        residency_check(
            lat_us,
            &mut canary,
            &mut baseline,
            &mut sampler,
            &mut cadence,
            &mut probe,
            || ctx.mem_info().ok().map(|(f, _)| f as u64),
        )
    });

    let _ = rt.broker.join();
    let zeroed = backend.zero();
    let _ = probe.zero(); // DT-12/DT-17: zera também a região-canário
    let _ = std::fs::remove_file(Path::new(&sock));
    zeroed?;
    eprintln!("[ramsharedd] broker VRAM encerrado (VRAM zerada)");
    Ok(())
}

/// Caminho broker RAM (sem GPU): mesmo control-plane, backend em heap. Existe para validar a
/// arbitragem + ciclo de vida do swap em **qemu** (ITEM-11), onde não há GPU. Sem residência
/// (RAM não sofre eviction). `Cuda::load()` nunca é chamado → roda sem libcuda.
fn run_broker_ram(
    slice_bytes: u64,
    slices: u16,
    sock: String,
    listen_nbd_addr: Option<std::net::SocketAddr>,
    advertise_tcp: Option<(String, u16)>,
    arbiter_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let total = (slices as u64)
        .checked_mul(slice_bytes)
        .ok_or("--slices * --slice-mb: overflow")?;
    let backend = RamBackend::new(total as usize);
    eprintln!(
        "[ramsharedd] broker RAM (sem GPU): {slices} slices x {} MiB = {} MiB, block_size={BLOCK_SIZE}",
        slice_bytes >> 20,
        total >> 20
    );

    let rt = broker_setup(
        slices,
        slice_bytes,
        &sock,
        listen_nbd_addr,
        advertise_tcp,
        arbiter_addr,
    )?;
    let _ = serve_broker_jobs(backend, &rt, |_| None); // RAM: sem residência

    let _ = rt.broker.join();
    let _ = std::fs::remove_file(Path::new(&sock));
    eprintln!("[ramsharedd] broker RAM encerrado");
    Ok(())
}

/// Caminho ublk: serve `/dev/ublkbN` direto (io_uring), sem socket. O worker DT-3 e
/// dono da VRAM/contexto CUDA e roda a residencia (canario §9/§9.4); o DEMOTE faz
/// swapoff do proprio device servido. O ciclo de vida vai ate SIGINT/SIGTERM.
/// SPEC: docs/ublk-daemon-integration/SPEC.md F2.
fn run_ublk(
    size: u64,
    force: bool,
    queue_depth: u16,
    backend: BackendKind,
) -> Result<(), Box<dyn std::error::Error>> {
    // TRAVA DE SEGURANCA: recusa servir ublk no WSL2. Um teardown malsucedido do
    // daemon orfana o /dev/ublkbN -> I/O em D-state -> CONGELA o WSL2 (2026-06-09).
    guard_not_wsl2()?;
    // Disciplina 3: trava memoria + protege do OOM (processo todo; o worker e dono
    // da CUDA, mas mlockall/oom_score_adj sao process-wide).
    lock_memory(force)?;

    // SAFETY: registra handler async-signal-safe (so um store atomico) para encerrar
    // de forma ordenada. signal() so guarda o ponteiro; o retorno antigo e ignorado.
    unsafe {
        signal(SIGINT, handle_shutdown);
        signal(SIGTERM, handle_shutdown);
    }

    let dev_sectors = size / SECTOR;
    let mut spec = ublk_control::DeviceSpec::smoke_auto();
    spec.queue_depth = queue_depth;
    let report = ublk_control::add_device(UBLK_CONTROL, spec)?;
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 12, 12),
    )?;

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);

    // Backend: VRAM (worker dono do contexto CUDA + residencia §9/§9.4; swap_dev = o
    // proprio device, que o DEMOTE tira do swap) ou RAM (sem GPU; reusa o
    // spawn_server_dt3 ja validado em-processo, sem residencia — para validar o ciclo
    // de vida/teardown em qemu).
    let server = match backend {
        BackendKind::Vram => UblkHandle::Vram(ublk_server::spawn_server_dt3_vram_with_residency(
            &char_path,
            report.queue_depth,
            BLOCK_SIZE as usize, // buf_size por tag: requests do device sao <= 4KB (smoke_auto)
            size as usize,
            BLOCK_SIZE,
            block_path.clone(),
            ResidencyConfig::default(),
        )?),
        BackendKind::Ram => UblkHandle::Ram(ublk_server::spawn_server_dt3(
            &char_path,
            report.queue_depth,
            BLOCK_SIZE as usize,
            RamBackend::new(size as usize),
        )?),
    };
    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())?;
    eprintln!(
        "[ramsharedd] ublk device: {block_path} ({} MiB, qd={}, backend={})",
        size >> 20,
        report.queue_depth,
        backend.label()
    );
    eprintln!("[ramsharedd] swapon: sudo swapon {block_path}");
    eprintln!("[ramsharedd] Ctrl-C / SIGTERM para encerrar");

    // Aguarda o sinal de encerramento (poll da flag; sleep ignora EINTR).
    while !SHUTDOWN.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!("[ramsharedd] sinal recebido — encerrando");

    // Teardown ordenado: STOP_DEV aborta os FETCH -> o worker sai do loop (e zera a
    // VRAM no fim, no caminho VRAM) -> join -> DEL_DEV. (Quem fez swapon deve swapoff
    // antes: del_gendisk espera os openers do block device.)
    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id)?;
    server.join()?;
    ublk_control::delete_device(UBLK_CONTROL, report.dev_id)?;
    eprintln!("[ramsharedd] ublk device removido");
    Ok(())
}

/// Recusa servir ublk no WSL2 (a menos do override consciente
/// `RAMSHARED_ALLOW_UBLK_ON_WSL2=1`). Motivo: o teardown do daemon ublk standalone,
/// se falhar (corrida SIGTERM-tarde -> SIGKILL, ou bug em STOP_DEV/join), deixa o
/// `/dev/ublkbN` SEM servidor com I/O em voo -> processos em D-state no caminho de
/// writeback/memoria -> com `mlockall(MCL_FUTURE)` + `drop_caches` o kernel nao
/// progride -> stall global -> WSL2 CONGELA (incidente 2026-06-09). Validar o daemon
/// completo so em VM/qemu (`scripts/kernel/qemu-validate.sh`), onde um stall e
/// recuperavel sem derrubar o host.
fn guard_not_wsl2() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("RAMSHARED_ALLOW_UBLK_ON_WSL2")
        .ok()
        .as_deref()
        == Some("1")
    {
        eprintln!("[ramsharedd] AVISO: RAMSHARED_ALLOW_UBLK_ON_WSL2=1 — trava do WSL2 ignorada");
        return Ok(());
    }
    let osrelease = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let lower = osrelease.to_ascii_lowercase();
    if lower.contains("microsoft") || lower.contains("wsl") {
        return Err(format!(
            "RECUSADO: --transport ublk no WSL2 ({}) pode CONGELAR o sistema se o teardown \
             do daemon falhar (device orfao -> I/O em D-state). Valide o daemon em VM/qemu. \
             Override consciente: RAMSHARED_ALLOW_UBLK_ON_WSL2=1.",
            osrelease.trim()
        )
        .into());
    }
    Ok(())
}

/// Trava a memoria (mlockall) + protege do OOM killer (oom_score_adj=-1000) ANTES de
/// servir swap (Disciplina 3, anti-deadlock). `--force` segue sem a protecao, avisando.
fn lock_memory(force: bool) -> Result<(), Box<dyn std::error::Error>> {
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
        eprintln!("[ramsharedd] memoria travada (mlockall) + oom_score_adj=-1000");
    } else {
        eprintln!(
            "[ramsharedd] AVISO --force: mlockall={} oom_score_adj={} (anti-deadlock NAO garantido)",
            if locked { "ok" } else { "FALHOU" },
            if oom_ok { "ok" } else { "FALHOU" }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn private_listen_accepts_loopback_and_lan() {
        assert_eq!(parse_private_listen("127.0.0.1:7777").unwrap().port(), 7777);
        assert!(parse_private_listen("tcp://192.168.0.50:10809").is_ok());
    }

    #[test]
    fn private_listen_rejects_unspecified() {
        // RNF-2 / #5 abort trigger: bind público recusado ANTES de qualquer bind().
        assert!(parse_private_listen("0.0.0.0:10809").is_err());
        assert!(parse_private_listen("tcp://0.0.0.0:7777").is_err());
        assert!(parse_private_listen("[::]:7777").is_err());
    }

    #[test]
    fn private_listen_rejects_garbage() {
        assert!(parse_private_listen("nao-eh-addr").is_err());
        assert!(parse_private_listen("127.0.0.1").is_err()); // sem porta
    }

    #[test]
    fn slice_flags_reject_ublk_with_slices() {
        assert!(validate_slice_flags(2, 64, true).is_err()); // DT-3
        assert!(validate_slice_flags(0, 0, true).is_ok()); // ublk single ok
    }

    #[test]
    fn slice_flags_require_slice_mb() {
        assert!(validate_slice_flags(2, 0, false).is_err());
        assert!(validate_slice_flags(2, 64, false).is_ok());
        assert!(validate_slice_flags(0, 0, false).is_ok()); // single-mode ok
    }
}
