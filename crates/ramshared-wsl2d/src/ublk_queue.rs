//! Preparation of the ublk queue in the char device `/dev/ublkcN`.
//!
//! Maps the io-desc buffer (read-only) exposed by the kernel and decodes
//! descriptors by tag. Does not call `START_DEV`, does not create `/dev/ublkbN`, and does not touch
//! swap. The `unsafe` of `mmap` is isolated in `ramshared-uring`.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use crate::ublk;

/// Maps queue 0 of the char device `char_path` (read-only) and decodes the
/// `ublksrv_io_desc` of the `tag`. The map size is `round_up(queue_depth * 24, page)`
/// and the offset is 0 (queue 0); additional queues require `ublk_max_cmd_buf_size` — see
/// `docs/decisions/ADR-0004-ublk-io-uring-crate.md` §3.
pub fn read_io_desc(
    char_path: impl AsRef<Path>,
    queue_depth: u16,
    tag: u16,
) -> io::Result<ublk::IoDesc> {
    if tag >= queue_depth {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tag must be < queue_depth",
        ));
    }

    let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
    let len = ramshared_uring::round_up_to_page(usize::from(queue_depth) * ublk::UBLK_IO_DESC_SIZE);
    let map = ramshared_uring::MmapRo::map_readonly(char_dev.as_raw_fd(), len, 0)?;

    let start = usize::from(tag) * ublk::UBLK_IO_DESC_SIZE;
    let snapshot = map.copy_array::<{ ublk::UBLK_IO_DESC_SIZE }>(start)?;
    ublk::IoDesc::from_ne_bytes(&snapshot)
        .ok_or_else(|| io::Error::other("io-desc is shorter than 24 bytes"))
}

/// FETCH session in a ublk queue: holds the `/dev/ublkcN` char device `File` and
/// the `ramshared-uring` ring that submitted the `FETCH_REQ`. Does not call `START_DEV`, does not
/// create `/dev/ublkbN`, and does not touch swap. The ring is dropped before the `File` (the fd
/// must remain open while the ring exists).
pub struct FetchSession {
    ring: ramshared_uring::UblkFetchRing,
    /// Char device `File`, kept open while the ring lives (drop guard).
    _char_dev: File,
}

impl FetchSession {
    /// Opens `char_path`, submits `FETCH_REQ` for the `queue_depth` tags of queue 0
    /// (buffer of `buf_size` per tag) and returns without waiting for CQE.
    pub fn open(
        char_path: impl AsRef<Path>,
        queue_depth: u16,
        buf_size: usize,
    ) -> io::Result<Self> {
        let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
        let ring = ramshared_uring::UblkFetchRing::submit_fetch_all(
            char_dev.as_raw_fd(),
            queue_depth,
            buf_size,
        )?;

        Ok(Self {
            ring,
            _char_dev: char_dev,
        })
    }

    /// Drains available CQEs (does not block).
    pub fn drain(&mut self) -> Vec<ramshared_uring::UblkCompletion> {
        self.ring.drain()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn descriptor_fixture(label: &str) -> (std::path::PathBuf, File) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("ramshared-{label}-{}-{nonce}", std::process::id()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("create descriptor fixture");
        file.set_len(ramshared_uring::page_size() as u64)
            .expect("size descriptor fixture");
        let descriptor = ublk::IoDesc {
            op_flags: u32::from(ublk::UBLK_IO_OP_READ),
            nr_sectors_or_zones: 8,
            start_sector: 17,
            addr: 0x1234_5678,
        };
        let mut bytes = [0u8; ublk::UBLK_IO_DESC_SIZE];
        bytes[0..4].copy_from_slice(&descriptor.op_flags.to_ne_bytes());
        bytes[4..8].copy_from_slice(&descriptor.nr_sectors_or_zones.to_ne_bytes());
        bytes[8..16].copy_from_slice(&descriptor.start_sector.to_ne_bytes());
        bytes[16..24].copy_from_slice(&descriptor.addr.to_ne_bytes());
        file.write_all(&bytes).expect("write descriptor fixture");
        file.flush().expect("flush descriptor fixture");
        (path, file)
    }

    #[test]
    fn regular_file_descriptor_queue_decodes_owned_snapshot() {
        let (path, file) = descriptor_fixture("queue-descriptor");
        let descriptor = read_io_desc(&path, 1, 0).expect("decode descriptor");
        assert_eq!(descriptor.op_flags, u32::from(ublk::UBLK_IO_OP_READ));
        assert_eq!(descriptor.nr_sectors_or_zones, 8);
        assert_eq!(descriptor.start_sector, 17);
        assert_eq!(descriptor.addr, 0x1234_5678);
        assert_eq!(
            read_io_desc(&path, 1, 1)
                .expect_err("out-of-range tag")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(read_io_desc(path.with_extension("missing"), 1, 0).is_err());

        drop(file);
        fs::remove_file(path).expect("remove descriptor fixture");
    }

    #[test]
    fn regular_file_fetch_session_drains_refusal_without_a_device() {
        let (path, file) = descriptor_fixture("queue-fetch");
        let mut session = FetchSession::open(&path, 1, 4096).expect("open fetch session");
        let deadline = Instant::now() + Duration::from_secs(2);
        let completions = loop {
            let current = session.drain();
            if !current.is_empty() {
                break current;
            }
            assert!(Instant::now() < deadline, "regular-file CQE deadline");
            std::thread::yield_now();
        };
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result < 0);

        drop(session);
        drop(file);
        fs::remove_file(path).expect("remove fetch fixture");
    }
}
