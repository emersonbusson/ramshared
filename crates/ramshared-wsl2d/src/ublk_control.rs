//! Smoke seguro do `/dev/ublk-control`.
//!
//! Este módulo só consulta `GET_FEATURES`. Ele não chama `ADD_DEV`, não cria
//! `/dev/ublkcN`/`/dev/ublkbN` e não toca em swap.

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
