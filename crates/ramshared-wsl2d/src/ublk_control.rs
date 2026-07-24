//! Safe smoke test of `/dev/ublk-control`.
//!
//! This module only queries `GET_FEATURES`. It does not call `ADD_DEV`, does not create
//! `/dev/ublkcN`/`/dev/ublkbN`, and does not touch swap.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process;

use crate::ublk;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeatureReport {
    pub features: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceSpec {
    pub dev_id: u32,
    pub nr_hw_queues: u16,
    pub queue_depth: u16,
    pub max_io_buf_bytes: u32,
    pub flags: u64,
}

impl DeviceSpec {
    pub fn smoke_auto() -> Self {
        Self {
            dev_id: ublk::UBLK_DEV_ID_AUTO,
            nr_hw_queues: 1,
            queue_depth: 1,
            max_io_buf_bytes: 4096,
            flags: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceReport {
    pub dev_id: u32,
    pub nr_hw_queues: u16,
    pub queue_depth: u16,
    pub state: u16,
    pub max_io_buf_bytes: u32,
    pub flags: u64,
    pub owner_uid: u32,
    pub owner_gid: u32,
}

pub fn get_features(path: impl AsRef<Path>) -> io::Result<FeatureReport> {
    let control = OpenOptions::new().read(true).write(true).open(path)?;
    let features = ramshared_uring::ublk_get_features(control.as_raw_fd())?;

    Ok(FeatureReport { features })
}

pub fn add_device(path: impl AsRef<Path>, spec: DeviceSpec) -> io::Result<DeviceReport> {
    let control = OpenOptions::new().read(true).write(true).open(path)?;
    let ublksrv_pid =
        i32::try_from(process::id()).map_err(|_| io::Error::other("process id exceeds i32"))?;
    let info = ublk::CtrlDevInfo {
        nr_hw_queues: spec.nr_hw_queues,
        queue_depth: spec.queue_depth,
        max_io_buf_bytes: spec.max_io_buf_bytes,
        dev_id: spec.dev_id,
        ublksrv_pid,
        flags: spec.flags,
        ..Default::default()
    };
    let mut info_bytes = encode_dev_info(info);

    ramshared_uring::ublk_add_dev(control.as_raw_fd(), spec.dev_id, &mut info_bytes)?;

    Ok(DeviceReport::from(decode_dev_info(info_bytes)))
}

pub fn delete_device(path: impl AsRef<Path>, dev_id: u32) -> io::Result<()> {
    let control = OpenOptions::new().read(true).write(true).open(path)?;

    ramshared_uring::ublk_del_dev(control.as_raw_fd(), dev_id)
}

/// Applies `params` to the device `dev_id` via `SET_PARAMS` (required before `START_DEV`).
pub fn set_params(path: impl AsRef<Path>, dev_id: u32, params: ublk::Params) -> io::Result<()> {
    let control = OpenOptions::new().read(true).write(true).open(path)?;
    let mut bytes = params.to_bytes();

    ramshared_uring::ublk_set_params(control.as_raw_fd(), dev_id, &mut bytes)
}

/// Reads current parameters of device `dev_id` via `GET_PARAMS`.
pub fn get_params(path: impl AsRef<Path>, dev_id: u32) -> io::Result<ublk::Params> {
    let control = OpenOptions::new().read(true).write(true).open(path)?;
    let mut bytes = [0u8; ublk::UBLK_PARAMS_LEN];
    // The kernel validates the buffer via the `len` field of the struct; populate before GET.
    bytes[0..4].copy_from_slice(&(ublk::UBLK_PARAMS_LEN as u32).to_ne_bytes());

    ramshared_uring::ublk_get_params(control.as_raw_fd(), dev_id, &mut bytes)?;

    Ok(ublk::Params::from_bytes(&bytes))
}

/// Starts the device (`START_DEV`): creates `/dev/ublkbN`. Blocks until queues are ready and
/// `add_disk`; the server thread must be active to serve the partition scan.
pub fn start_dev(path: impl AsRef<Path>, dev_id: u32, ublksrv_pid: u32) -> io::Result<()> {
    let control = OpenOptions::new().read(true).write(true).open(path)?;

    ramshared_uring::ublk_start_dev(control.as_raw_fd(), dev_id, ublksrv_pid)
}

/// Stops the device (`STOP_DEV`): removes `/dev/ublkbN` and aborts pending FETCH requests.
pub fn stop_dev(path: impl AsRef<Path>, dev_id: u32) -> io::Result<()> {
    let control = OpenOptions::new().read(true).write(true).open(path)?;

    ramshared_uring::ublk_stop_dev(control.as_raw_fd(), dev_id)
}

impl From<ublk::CtrlDevInfo> for DeviceReport {
    fn from(info: ublk::CtrlDevInfo) -> Self {
        Self {
            dev_id: info.dev_id,
            nr_hw_queues: info.nr_hw_queues,
            queue_depth: info.queue_depth,
            state: info.state,
            max_io_buf_bytes: info.max_io_buf_bytes,
            flags: info.flags,
            owner_uid: info.owner_uid,
            owner_gid: info.owner_gid,
        }
    }
}

fn encode_dev_info(info: ublk::CtrlDevInfo) -> [u8; 64] {
    let mut bytes = [0u8; 64];
    bytes[0..2].copy_from_slice(&info.nr_hw_queues.to_ne_bytes());
    bytes[2..4].copy_from_slice(&info.queue_depth.to_ne_bytes());
    bytes[4..6].copy_from_slice(&info.state.to_ne_bytes());
    bytes[6..8].copy_from_slice(&info.pad0.to_ne_bytes());
    bytes[8..12].copy_from_slice(&info.max_io_buf_bytes.to_ne_bytes());
    bytes[12..16].copy_from_slice(&info.dev_id.to_ne_bytes());
    bytes[16..20].copy_from_slice(&info.ublksrv_pid.to_ne_bytes());
    bytes[20..24].copy_from_slice(&info.pad1.to_ne_bytes());
    bytes[24..32].copy_from_slice(&info.flags.to_ne_bytes());
    bytes[32..40].copy_from_slice(&info.ublksrv_flags.to_ne_bytes());
    bytes[40..44].copy_from_slice(&info.owner_uid.to_ne_bytes());
    bytes[44..48].copy_from_slice(&info.owner_gid.to_ne_bytes());
    bytes[48..56].copy_from_slice(&info.reserved1.to_ne_bytes());
    bytes[56..64].copy_from_slice(&info.reserved2.to_ne_bytes());
    bytes
}

fn decode_dev_info(bytes: [u8; 64]) -> ublk::CtrlDevInfo {
    ublk::CtrlDevInfo {
        nr_hw_queues: u16::from_ne_bytes([bytes[0], bytes[1]]),
        queue_depth: u16::from_ne_bytes([bytes[2], bytes[3]]),
        state: u16::from_ne_bytes([bytes[4], bytes[5]]),
        pad0: u16::from_ne_bytes([bytes[6], bytes[7]]),
        max_io_buf_bytes: u32::from_ne_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        dev_id: u32::from_ne_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        ublksrv_pid: i32::from_ne_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        pad1: u32::from_ne_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
        flags: u64::from_ne_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31],
        ]),
        ublksrv_flags: u64::from_ne_bytes([
            bytes[32], bytes[33], bytes[34], bytes[35], bytes[36], bytes[37], bytes[38], bytes[39],
        ]),
        owner_uid: u32::from_ne_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]),
        owner_gid: u32::from_ne_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]),
        reserved1: u64::from_ne_bytes([
            bytes[48], bytes[49], bytes[50], bytes[51], bytes[52], bytes[53], bytes[54], bytes[55],
        ]),
        reserved2: u64::from_ne_bytes([
            bytes[56], bytes[57], bytes[58], bytes[59], bytes[60], bytes[61], bytes[62], bytes[63],
        ]),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::fs::OpenOptions;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn smoke_auto_spec() {
        let spec = DeviceSpec::smoke_auto();
        assert_eq!(spec.dev_id, ublk::UBLK_DEV_ID_AUTO);
        assert_eq!(spec.nr_hw_queues, 1);
        assert_eq!(spec.queue_depth, 1);
        assert_eq!(spec.max_io_buf_bytes, 4096);
        assert_eq!(spec.flags, 0);
    }

    #[test]
    fn device_report_from_info() {
        let info = ublk::CtrlDevInfo {
            dev_id: 42,
            nr_hw_queues: 2,
            queue_depth: 128,
            state: 1,
            max_io_buf_bytes: 8192,
            flags: 0xdeadbeef,
            owner_uid: 1000,
            owner_gid: 1001,
            ..Default::default()
        };

        let report = DeviceReport::from(info);
        assert_eq!(report.dev_id, 42);
        assert_eq!(report.nr_hw_queues, 2);
        assert_eq!(report.queue_depth, 128);
        assert_eq!(report.state, 1);
        assert_eq!(report.max_io_buf_bytes, 8192);
        assert_eq!(report.flags, 0xdeadbeef);
        assert_eq!(report.owner_uid, 1000);
        assert_eq!(report.owner_gid, 1001);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let info = ublk::CtrlDevInfo {
            dev_id: 42,
            nr_hw_queues: 2,
            queue_depth: 128,
            state: 1,
            pad0: 2,
            max_io_buf_bytes: 8192,
            ublksrv_pid: 9999,
            pad1: 3,
            flags: 0xdeadbeef12345678,
            ublksrv_flags: 0x87654321feadc0de,
            owner_uid: 1000,
            owner_gid: 1001,
            reserved1: 0x1111111111111111,
            reserved2: 0x2222222222222222,
        };

        let bytes = encode_dev_info(info);
        let decoded = decode_dev_info(bytes);
        assert_eq!(info, decoded);
    }

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn get_unique_temp_path() -> std::path::PathBuf {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!("ramshared_ublk_control_test_{}_{}", pid, counter))
    }

    #[test]
    fn not_found_errors() {
        let path = get_unique_temp_path();

        assert_eq!(
            get_features(&path).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            add_device(&path, DeviceSpec::smoke_auto())
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            delete_device(&path, 0).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            set_params(&path, 0, ublk::Params::default())
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            get_params(&path, 0).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            start_dev(&path, 0, 1).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            stop_dev(&path, 0).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn unsupported_errors() {
        let path = get_unique_temp_path();
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();

        // A regular file must never be accepted as the ublk control device.
        assert!(get_features(&path).is_err());
        assert!(add_device(&path, DeviceSpec::smoke_auto()).is_err());
        assert!(delete_device(&path, 0).is_err());
        assert!(set_params(&path, 0, ublk::Params::default()).is_err());
        assert!(get_params(&path, 0).is_err());
        assert!(start_dev(&path, 0, 1).is_err());
        assert!(stop_dev(&path, 0).is_err());

        std::fs::remove_file(&path).unwrap();
    }
}
