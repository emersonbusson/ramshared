#![allow(clippy::unwrap_used, clippy::expect_used)] // test: unwrap/expect is idiomatic

use ramshared_wsl2d::{ublk, ublk_control, ublk_queue};
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

const UBLK_CONTROL: &str = "/dev/ublk-control";

#[test]
#[ignore = "requires root and /dev/ublk-control; does not create a ublk device"]
fn get_features_from_ublk_control_without_creating_device() {
    let report = ublk_control::get_features(UBLK_CONTROL).expect("ublk GET_FEATURES");

    assert_ne!(report.features & ublk::UBLK_F_CMD_IOCTL_ENCODE, 0);
    assert_ne!(
        report.features & ublk::UBLK_F_SUPPORT_ZERO_COPY,
        0,
        "current WSL2 ublk advertises zero-copy support"
    );
}

#[test]
#[ignore = "requires root and /dev/ublk-control; creates then removes /dev/ublkcN only"]
fn add_then_delete_char_device_without_starting_block_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    assert_ne!(report.dev_id, ublk::UBLK_DEV_ID_AUTO);
    assert_eq!(report.nr_hw_queues, 1);
    assert_eq!(report.queue_depth, 1);
    assert_ne!(report.flags & ublk::UBLK_F_CMD_IOCTL_ENCODE, 0);
    assert_ne!(report.flags & ublk::UBLK_F_URING_CMD_COMP_IN_TASK, 0);
    assert_eq!(report.flags & ublk::UBLK_F_SUPPORT_ZERO_COPY, 0);

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let block_path = format!("/dev/ublkb{}", report.dev_id);
    assert!(fs::metadata(&char_path).is_ok(), "{char_path} absent");
    assert!(
        fs::metadata(&block_path).is_err(),
        "{block_path} should not exist without START_DEV"
    );

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert!(
        fs::metadata(&block_path).is_err(),
        "{block_path} should not exist after DEL_DEV"
    );
    assert_eq!(ublk_nodes(), before);
}

#[test]
#[ignore = "requires root and /dev/ublk-control; maps /dev/ublkcN read-only, no START_DEV"]
fn mmap_io_desc_buffer_read_only_without_starting_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    // Without I/O submitted, the io-desc of tag 0 must be zeroed. Success proves that the
    // mmap PROT_READ of queue 0 works in the custom kernel; no START_DEV is called.
    let desc0 = ublk_queue::read_io_desc(&char_path, report.queue_depth, 0)
        .expect("mmap + reading of io-desc");
    assert_eq!(desc0, ublk::IoDesc::default());

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert_eq!(ublk_nodes(), before);
}

#[test]
#[ignore = "requires root and /dev/ublk-control; submits FETCH then aborts via DEL_DEV, no START_DEV"]
fn fetch_req_parks_until_delete_aborts_without_starting_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    let char_path = format!("/dev/ublkc{}", report.dev_id);
    let want = usize::from(report.queue_depth);

    let mut session = ublk_queue::FetchSession::open(&char_path, report.queue_depth, 4096)
        .expect("open char device + submit FETCH_REQ");

    // FETCH remains parked (-EIOCBQUEUED): no CQE before I/O or abort.
    assert!(
        session.drain().is_empty(),
        "FETCH should not complete without I/O or START_DEV"
    );

    // DEL_DEV posts the aborts (ublk_cancel_dev) and then waits for the char device to close
    // (wait_event idr_freed, ublk_drv.c:2523). This thread is the sole owner of the ring
    // (DT-3): drains the aborts and, upon completion, drops `session` (closes the char),
    // unlocking DEL_DEV. Without concurrent draining, control hangs.
    let drainer = thread::spawn(move || {
        let mut aborts = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while aborts.len() < want && Instant::now() < deadline {
            aborts.extend(session.drain());
            thread::sleep(Duration::from_millis(5));
        }
        aborts
    });

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();

    let aborts = drainer.join().expect("drainer thread");
    assert_eq!(aborts.len(), want, "all FETCHes must abort on DEL_DEV");
    for completion in &aborts {
        assert_eq!(
            completion.result,
            ublk::UBLK_IO_RES_ABORT,
            "parked FETCH must complete with -ENODEV"
        );
    }

    wait_until_missing(&char_path);
    assert_eq!(ublk_nodes(), before);
}

#[test]
#[ignore = "requires root and /dev/ublk-control; sets/gets params, no START_DEV"]
fn set_params_roundtrips_without_starting_device() {
    let before = ublk_nodes();
    let report = ublk_control::add_device(UBLK_CONTROL, ublk_control::DeviceSpec::smoke_auto())
        .expect("ublk ADD_DEV");
    let mut guard = DeviceGuard::new(report.dev_id);

    // 2048 sectors of 512 B = 1 MiB; logical bs 512 (shift 9), physical 4 KiB (shift 12).
    let params = ublk::Params::basic_disk(2048, 9, 12);
    ublk_control::set_params(UBLK_CONTROL, report.dev_id, params).expect("ublk SET_PARAMS");

    let got = ublk_control::get_params(UBLK_CONTROL, report.dev_id).expect("ublk GET_PARAMS");
    assert_eq!(got.basic.dev_sectors, 2048);
    assert_eq!(got.basic.logical_bs_shift, 9);
    assert_eq!(got.basic.physical_bs_shift, 12);
    assert_ne!(got.types & ublk::UBLK_PARAM_TYPE_BASIC, 0);

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&format!("/dev/ublkc{}", report.dev_id));
    assert_eq!(ublk_nodes(), before);
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
    let deadline = Instant::now() + Duration::from_secs(1);
    while fs::metadata(path).is_ok() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
}
