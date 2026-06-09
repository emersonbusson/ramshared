use ramshared_wsl2d::{ublk, ublk_control, ublk_server};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
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
    assert!(backend.write_from(TEST_SECTOR * SECTOR, &pattern) >= 0);

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
    assert_eq!(
        backend.read_into(TEST_SECTOR * SECTOR, &mut got),
        SECTOR as i32
    );
    assert_eq!(got, pattern, "o WRITE deve ter chegado ao backend");

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert_eq!(ublk_nodes(), before);
}

fn read_sector(path: &str, sector: u64) -> Vec<u8> {
    let mut file = File::open(path).expect("abrir block device");
    file.seek(SeekFrom::Start(sector * SECTOR)).expect("seek");
    let mut buf = vec![0u8; SECTOR as usize];
    file.read_exact(&mut buf).expect("read_exact");
    buf
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
