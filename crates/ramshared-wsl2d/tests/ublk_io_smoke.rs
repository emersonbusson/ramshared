use ramshared_block::BlockBackend;
use ramshared_wsl2d::{ublk, ublk_control, ublk_server};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::thread;
use std::time::{Duration, Instant};

const UBLK_CONTROL: &str = "/dev/ublk-control";
const SECTOR: u64 = 512;
const TEST_SECTOR: u64 = 100;

#[test]
#[ignore = "requires root; creates /dev/ublkbN and serves I/O from a RAM backend, no swap"]
fn serves_read_from_ram_backend_over_block_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    let dev_sectors = 256u64; // 128 KiB
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 9, 9),
    )
    .expect("ublk SET_PARAMS");

    // Backend de RAM com um padrao conhecido no setor de teste (fora do partition scan).
    let mut backend = ublk_server::RamBackend::new((dev_sectors * SECTOR) as usize);
    let pattern: Vec<u8> = (0..SECTOR).map(|i| (i % 251) as u8).collect();
    backend
        .write_at(TEST_SECTOR * SECTOR, &pattern)
        .expect("pre-carrega o backend");

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);

    // Sobe a thread servidora (submete FETCH + loop). Ela serve o partition scan que
    // o START_DEV dispara, por isso precisa estar viva antes/durante o START_DEV.
    let server = ublk_server::spawn_server(&char_path, report.queue_depth, 4096, backend)
        .expect("spawn server");

    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())
        .expect("ublk START_DEV");
    assert!(
        fs::metadata(&block_path).is_ok(),
        "{block_path} deveria existir apos START_DEV"
    );

    // READ do setor de teste via block device -> loop serve do backend -> padrao.
    let got = read_sector(&block_path, TEST_SECTOR);
    assert_eq!(
        got, pattern,
        "READ deve devolver o padrao gravado no backend"
    );

    // Teardown: STOP_DEV remove o gendisk e aborta os FETCH -> a thread sai do loop.
    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id).expect("ublk STOP_DEV");
    server.join().expect("server loop terminou ok");

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert_eq!(ublk_nodes(), before);
}

#[test]
#[ignore = "requires root; writes through /dev/ublkbN into the RAM backend, no swap"]
fn serves_write_into_ram_backend_over_block_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    let dev_sectors = 256u64;
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 9, 9),
    )
    .expect("ublk SET_PARAMS");

    let disk_size = (dev_sectors * SECTOR) as usize;
    let backend = ublk_server::RamBackend::new(disk_size);
    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);
    // Buffer por tag cobre o disco inteiro: qualquer request de writeback cabe.
    let server = ublk_server::spawn_server(&char_path, report.queue_depth, disk_size, backend)
        .expect("spawn server");

    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())
        .expect("ublk START_DEV");
    assert!(
        fs::metadata(&block_path).is_ok(),
        "{block_path} deveria existir"
    );

    // WRITE de um padrao no setor de teste via block device + fsync (forca writeback).
    let pattern: Vec<u8> = (0..SECTOR).map(|i| ((i * 7 + 1) % 251) as u8).collect();
    write_sector(&block_path, TEST_SECTOR, &pattern);

    // Teardown: a thread devolve o backend para inspecao direta (sem page cache).
    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id).expect("ublk STOP_DEV");
    let backend = server.join().expect("server loop terminou ok");

    let mut got = vec![0u8; SECTOR as usize];
    backend
        .read_at(TEST_SECTOR * SECTOR, &mut got)
        .expect("le o backend devolvido");
    assert_eq!(got, pattern, "o WRITE deve ter chegado ao backend");

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert_eq!(ublk_nodes(), before);
}

#[test]
#[ignore = "requires root; DT-3 ring owner + worker thread serve I/O, no swap"]
fn dt3_serves_read_from_ram_backend_over_block_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    let dev_sectors = 256u64;
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 9, 9),
    )
    .expect("ublk SET_PARAMS");

    let mut backend = ublk_server::RamBackend::new((dev_sectors * SECTOR) as usize);
    let pattern: Vec<u8> = (0..SECTOR).map(|i| (i % 251) as u8).collect();
    backend
        .write_at(TEST_SECTOR * SECTOR, &pattern)
        .expect("pre-carrega o backend");

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);

    // Arquitetura DT-3: thread ring owner + thread worker (dona do backend).
    let server = ublk_server::spawn_server_dt3(&char_path, report.queue_depth, 4096, backend)
        .expect("spawn DT-3 server");

    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())
        .expect("ublk START_DEV");
    assert!(
        fs::metadata(&block_path).is_ok(),
        "{block_path} deveria existir"
    );

    let got = read_sector(&block_path, TEST_SECTOR);
    assert_eq!(got, pattern, "DT-3 READ deve devolver o padrao do backend");

    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id).expect("ublk STOP_DEV");
    let _backend = server.join().expect("DT-3 server terminou ok");

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert_eq!(ublk_nodes(), before);
}

#[test]
#[ignore = "requires root + CUDA GPU; serves /dev/ublkbN from VRAM (cuMemcpy), no swap"]
fn dt3_serves_io_from_vram_over_block_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    let block_size = 4096u32;
    let dev_sectors = 256u64; // 128 KiB
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 12, 12),
    )
    .expect("ublk SET_PARAMS");

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);
    let vram_bytes = (dev_sectors * SECTOR) as usize;

    // Worker DT-3 dono da VRAM (cria o stack CUDA na própria thread).
    let server = ublk_server::spawn_server_dt3_vram(
        &char_path,
        report.queue_depth,
        vram_bytes, // buffer por tag = disco inteiro
        vram_bytes,
        block_size,
    )
    .expect("spawn DT-3 VRAM server");

    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())
        .expect("ublk START_DEV");
    assert!(
        fs::metadata(&block_path).is_ok(),
        "{block_path} deveria existir"
    );

    // WRITE bloco alinhado -> fsync -> drop cache -> READ deve vir da VRAM.
    let off = 8192u64; // alinhado ao block size 4096
    let pattern: Vec<u8> = (0..block_size).map(|i| ((i * 7 + 3) % 251) as u8).collect();
    write_block(&block_path, off, &pattern);
    drop_page_cache();
    let got = read_block(&block_path, off, block_size as usize);
    assert_eq!(got, pattern, "READ deve devolver da VRAM o bloco escrito");

    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id).expect("ublk STOP_DEV");
    server.join().expect("DT-3 VRAM server terminou ok");

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert_eq!(ublk_nodes(), before);
}

fn read_sector(path: &str, sector: u64) -> Vec<u8> {
    read_block(path, sector * SECTOR, SECTOR as usize)
}

fn read_block(path: &str, off: u64, len: usize) -> Vec<u8> {
    let mut file = File::open(path).expect("abrir block device");
    file.seek(SeekFrom::Start(off)).expect("seek");
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf).expect("read_exact");
    buf
}

fn write_block(path: &str, off: u64, data: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("abrir block device para escrita");
    file.seek(SeekFrom::Start(off)).expect("seek");
    file.write_all(data).expect("write_all");
    file.sync_all().expect("sync_all");
}

fn drop_page_cache() {
    let _ = std::process::Command::new("sync").status();
    if let Ok(mut f) = OpenOptions::new()
        .write(true)
        .open("/proc/sys/vm/drop_caches")
    {
        let _ = f.write_all(b"1\n");
    }
}

fn write_sector(path: &str, sector: u64, data: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("abrir block device para escrita");
    file.seek(SeekFrom::Start(sector * SECTOR)).expect("seek");
    file.write_all(data).expect("write_all");
    file.sync_all().expect("sync_all");
}

#[test]
#[ignore = "requires root + CUDA GPU; bounded mkswap/swapon/swapoff on VRAM-ublk (no memory pressure)"]
fn vram_ublk_round_trips_as_swap_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let block_path = format!("/dev/ublkb{}", report.dev_id);
    let char_path = format!("/dev/ublkc{}", report.dev_id);
    // Guard: garante swapoff ANTES de stop/del mesmo se o teste falhar.
    let mut guard = SwapGuard::new(report.dev_id, block_path.clone());

    let block_size = 4096u32;
    let dev_sectors = 128 * 1024 * 1024 / SECTOR; // 128 MiB de swap na VRAM
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 12, 12),
    )
    .expect("ublk SET_PARAMS");

    let vram_bytes = (dev_sectors * SECTOR) as usize;
    let server = ublk_server::spawn_server_dt3_vram(
        &char_path,
        report.queue_depth,
        2 * 1024 * 1024, // buffer por tag cobre clusters de swap
        vram_bytes,
        block_size,
    )
    .expect("spawn DT-3 VRAM server");

    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())
        .expect("ublk START_DEV");
    assert!(
        fs::metadata(&block_path).is_ok(),
        "{block_path} deveria existir"
    );

    // mkswap escreve o header de swap -> ublk WRITE -> cuMemcpyHtoD na VRAM.
    run_ok("mkswap", &[&block_path]);
    // swapon (sem -p: prioridade auto baixa) -> kernel le o header (ublk READ) e
    // registra a VRAM-ublk como area de swap.
    run_ok("swapon", &[&block_path]);

    let swaps = fs::read_to_string("/proc/swaps").expect("/proc/swaps");
    assert!(
        swaps.contains(&block_path),
        "VRAM-ublk nao foi registrada como swap:\n{swaps}"
    );

    // swapoff imediato (sem gerar pressao) -> desativa e drena.
    run_ok("swapoff", &[&block_path]);
    let swaps = fs::read_to_string("/proc/swaps").expect("/proc/swaps");
    assert!(
        !swaps.contains(&block_path),
        "swap deveria estar desativado"
    );

    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id).expect("ublk STOP_DEV");
    server.join().expect("DT-3 VRAM server terminou ok");
    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert_eq!(ublk_nodes(), before);
}

#[test]
#[ignore = "requires root + CUDA GPU; mede latencia de leitura 4KB do ublk-VRAM, no swap"]
fn bench_vram_ublk_read_latency() {
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    let block_size = 4096u32;
    let dev_sectors = 16 * 1024 * 1024 / SECTOR; // 16 MiB
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 12, 12),
    )
    .expect("ublk SET_PARAMS");

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);
    let vram_bytes = (dev_sectors * SECTOR) as usize;
    let server = ublk_server::spawn_server_dt3_vram(
        &char_path,
        report.queue_depth,
        64 * 1024,
        vram_bytes,
        block_size,
    )
    .expect("spawn DT-3 VRAM server");
    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())
        .expect("ublk START_DEV");

    let n_blocks = vram_bytes / block_size as usize;
    let (p50, p90, p99, p999, max) = bench_read_latency(&block_path, block_size, n_blocks);
    println!(
        "ublk-VRAM 4KB READ O_DIRECT (n=4000): p50={p50:?} p90={p90:?} p99={p99:?} p99.9={p999:?} max={max:?}"
    );
    // Sanidade: latencia plausivel (microssegundos a poucos ms), nao travada.
    assert!(p50 < Duration::from_millis(50), "p50 implausivel: {p50:?}");

    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id).expect("ublk STOP_DEV");
    server.join().expect("DT-3 VRAM server terminou ok");
    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
}

/// Mede a latencia de leituras 4KB `O_DIRECT` (cada uma bate no device, sem cache)
/// em offsets pseudo-aleatorios. Retorna (p50, p90, p99, p99.9, max).
fn bench_read_latency(
    path: &str,
    block_size: u32,
    n_blocks: usize,
) -> (Duration, Duration, Duration, Duration, Duration) {
    const O_DIRECT: i32 = 0o40000; // x86_64 Linux
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECT)
        .open(path)
        .expect("open O_DIRECT");

    let bs = block_size as usize;
    // Buffer alinhado ao block size (exigencia do O_DIRECT).
    let mut raw = vec![0u8; bs * 2];
    let pad = (bs - (raw.as_ptr() as usize % bs)) % bs;
    let n = n_blocks as u64;

    // xorshift64 para offsets pseudo-aleatorios alinhados.
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next_off = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % n) * bs as u64
    };

    for _ in 0..128 {
        let off = next_off();
        file.read_exact_at(&mut raw[pad..pad + bs], off)
            .expect("warmup");
    }

    let iters = 4000usize;
    let mut lat = Vec::with_capacity(iters);
    for _ in 0..iters {
        let off = next_off();
        let t = Instant::now();
        file.read_exact_at(&mut raw[pad..pad + bs], off)
            .expect("read");
        lat.push(t.elapsed());
    }
    lat.sort_unstable();
    let last = lat.len() - 1;
    let pct = |q: usize| lat[last * q / 100];
    (
        pct(50),
        pct(90),
        pct(99),
        lat[lat.len() * 999 / 1000],
        lat[last],
    )
}

#[test]
#[ignore = "requires root + CUDA GPU + fio; latencia fio do ublk-VRAM (compara com NBD), no swap"]
fn fio_bench_vram_ublk() {
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    let block_size = 4096u32;
    let dev_sectors = 64 * 1024 * 1024 / SECTOR; // 64 MiB
    ublk_control::set_params(
        UBLK_CONTROL,
        report.dev_id,
        ublk::Params::basic_disk(dev_sectors, 12, 12),
    )
    .expect("ublk SET_PARAMS");

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);
    let vram_bytes = (dev_sectors * SECTOR) as usize;
    let server = ublk_server::spawn_server_dt3_vram(
        &char_path,
        report.queue_depth,
        64 * 1024,
        vram_bytes,
        block_size,
    )
    .expect("spawn DT-3 VRAM server");
    ublk_control::start_dev(UBLK_CONTROL, report.dev_id, std::process::id())
        .expect("ublk START_DEV");

    let out = fio_randread(&block_path, "ublk-vram");
    print!("{out}");

    ublk_control::stop_dev(UBLK_CONTROL, report.dev_id).expect("ublk STOP_DEV");
    server.join().expect("DT-3 VRAM server terminou ok");
    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
}

/// Roda `fio` randread 4KB O_DIRECT iodepth=1 num block device e devolve o stdout.
fn fio_randread(dev: &str, name: &str) -> String {
    let out = std::process::Command::new("fio")
        .args([
            &format!("--name={name}"),
            &format!("--filename={dev}"),
            "--rw=randread",
            "--bs=4k",
            "--direct=1",
            "--ioengine=psync",
            "--iodepth=1",
            "--runtime=4",
            "--time_based",
            "--norandommap",
        ])
        .output()
        .unwrap_or_else(|e| panic!("nao executou fio: {e}"));
    assert!(
        out.status.success(),
        "fio falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run_ok(cmd: &str, args: &[&str]) {
    let out = std::process::Command::new(cmd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("nao executou {cmd}: {e}"));
    assert!(
        out.status.success(),
        "{cmd} {args:?} falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Guard de teardown para o teste de swap: `swapoff` (best-effort) antes de
/// `stop_dev`/`delete_device`, já que um device com swap ativo não pode ser deletado.
struct SwapGuard {
    dev_id: Option<u32>,
    block_path: String,
}

impl SwapGuard {
    fn new(dev_id: u32, block_path: String) -> Self {
        Self {
            dev_id: Some(dev_id),
            block_path,
        }
    }

    fn disarm(&mut self) {
        self.dev_id = None;
    }
}

impl Drop for SwapGuard {
    fn drop(&mut self) {
        if let Some(dev_id) = self.dev_id.take() {
            let _ = std::process::Command::new("swapoff")
                .arg(&self.block_path)
                .status();
            let _ = ublk_control::stop_dev(UBLK_CONTROL, dev_id);
            let _ = ublk_control::delete_device(UBLK_CONTROL, dev_id);
        }
    }
}

struct DeviceGuard {
    dev_id: Option<u32>,
}

impl DeviceGuard {
    fn new(dev_id: u32) -> Self {
        Self {
            dev_id: Some(dev_id),
        }
    }

    fn disarm(&mut self) {
        self.dev_id = None;
    }
}

impl Drop for DeviceGuard {
    fn drop(&mut self) {
        if let Some(dev_id) = self.dev_id.take() {
            let _ = ublk_control::stop_dev(UBLK_CONTROL, dev_id);
            let _ = ublk_control::delete_device(UBLK_CONTROL, dev_id);
        }
    }
}

fn ublk_nodes() -> Vec<String> {
    let mut nodes = fs::read_dir("/dev")
        .expect("/dev read_dir")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name == "ublk-control" || name.starts_with("ublkc") || name.starts_with("ublkb")
        })
        .collect::<Vec<_>>();
    nodes.sort();
    nodes
}

fn wait_until_missing(path: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while fs::metadata(path).is_ok() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
}
