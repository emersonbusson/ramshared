use ramshared_wsl2d::{ublk, ublk_control};
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

const UBLK_CONTROL: &str = "/dev/ublk-control";

#[test]
#[ignore = "requires root and /dev/ublk-control; does not create a ublk device"]
fn get_features_from_ublk_control_without_creating_device() {
    let report = ublk_control::get_features(UBLK_CONTROL).expect("ublk GET_FEATURES");

    assert_ne!(report.features & ublk::UBLK_F_CMD_IOCTL_ENCODE, 0);
    assert_eq!(report.features & ublk::UBLK_F_SUPPORT_ZERO_COPY, 0);
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
    assert!(fs::metadata(&char_path).is_ok(), "{char_path} ausente");
    assert!(
        fs::metadata(&block_path).is_err(),
        "{block_path} nao deveria existir sem START_DEV"
    );

    ublk_control::delete_device(UBLK_CONTROL, report.dev_id).expect("ublk DEL_DEV");
    guard.disarm();
    wait_until_missing(&char_path);
    assert!(
        fs::metadata(&block_path).is_err(),
        "{block_path} nao deveria existir apos DEL_DEV"
    );
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
