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
use ramshared_wsl2d::VramBackend;

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
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                i += 1;
                let mb: u64 = args.get(i).ok_or("--size requer valor (MiB)")?.parse()?;
                size = mb * 1024 * 1024;
            }
            "--sock" => {
                i += 1;
                sock = args.get(i).ok_or("--sock requer caminho")?.clone();
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

    let mut hdr = [0u8; REQUEST_LEN];
    loop {
        match reader.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let req = parse_request(&hdr)?;
        let payload = if req.cmd == Command::Write {
            let mut p = vec![0u8; req.len as usize];
            reader.read_exact(&mut p)?;
            p
        } else {
            Vec::new()
        };
        let out = serve(&req, &payload, &mut backend);
        writer.write_all(&out.reply)?;
        if !out.read_data.is_empty() {
            writer.write_all(&out.read_data)?;
        }
        writer.flush()?;
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
