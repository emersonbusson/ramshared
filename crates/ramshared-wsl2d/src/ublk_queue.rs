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
    let end = start + ublk::UBLK_IO_DESC_SIZE;
    let bytes = map.as_bytes();
    if end > bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "io-desc fora do buffer mapeado",
        ));
    }

    ublk::IoDesc::from_ne_bytes(&bytes[start..end])
        .ok_or_else(|| io::Error::other("io-desc menor que 24 bytes"))
}

/// FETCH session in a ublk queue: holds the `/dev/ublkcN` char device `File` and
/// the `ramshared-uring` ring that submitted the `FETCH_REQ`. Does not call `START_DEV`, does not
/// create `/dev/ublkbN`, and does not touch swap. The ring is dropped before the `File` (the fd
/// must remain open while the ring exists).
pub struct FetchSession {
    ring: ramshared_uring::UblkFetchRing,
    /// Char device `File`, kept open while the ring lives (drop guard).
    #[allow(dead_code)]
    char_dev: File,
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

        Ok(Self { ring, char_dev })
    }

    /// Drains available CQEs (does not block).
    pub fn drain(&mut self) -> Vec<ramshared_uring::UblkCompletion> {
        self.ring.drain()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_test_path() -> std::path::PathBuf {
        let id = std::process::id();
        let counter = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("ublk_queue_test_{}_{}.tmp", id, counter))
    }

    #[test]
    fn test_read_io_desc_success() {
        let path = unique_test_path();
        let mut f = File::create(&path).unwrap();

        let mut bytes = vec![0u8; 4096];
        // Populate an IoDesc at tag 1 (offset 24)
        let desc_offset = 24;
        let op_flags: u32 = 0xdead_beef;
        let nr_sectors: u32 = 0xcafe_babe;
        let start_sector: u64 = 0x0123_4567_89ab_cdef;
        let addr: u64 = 0xfedc_ba98_7654_3210;

        bytes[desc_offset..desc_offset + 4].copy_from_slice(&op_flags.to_ne_bytes());
        bytes[desc_offset + 4..desc_offset + 8].copy_from_slice(&nr_sectors.to_ne_bytes());
        bytes[desc_offset + 8..desc_offset + 16].copy_from_slice(&start_sector.to_ne_bytes());
        bytes[desc_offset + 16..desc_offset + 24].copy_from_slice(&addr.to_ne_bytes());

        f.write_all(&bytes).unwrap();

        // tag 1, queue_depth 4
        let desc = read_io_desc(&path, 4, 1).unwrap();

        assert_eq!(desc.op_flags, op_flags);
        assert_eq!(desc.nr_sectors_or_zones, nr_sectors);
        assert_eq!(desc.start_sector, start_sector);
        assert_eq!(desc.addr, addr);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_read_io_desc_out_of_bounds_tag() {
        let path = unique_test_path();
        let mut f = File::create(&path).unwrap();
        f.write_all(&[0u8; 4096]).unwrap();

        // tag 4, queue_depth 4 (invalid, tag must be < queue_depth)
        let res = read_io_desc(&path, 4, 4);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_fetch_session_open_and_drain() {
        let path = unique_test_path();
        let mut f = File::create(&path).unwrap();
        f.write_all(&[0u8; 4096]).unwrap();

        let mut session = FetchSession::open(&path, 4, 4096).unwrap();

        let cqes = session.drain();
        // Since we are running on a dummy file rather than a proper /dev/ublkc char device,
        // the kernel immediately rejects the io_uring commands with -EOPNOTSUPP (-95)
        // for each submitted tag in the queue_depth.
        assert_eq!(
            cqes.len(),
            4,
            "expected 4 completion events for queue_depth=4"
        );
        for cqe in cqes {
            assert_eq!(cqe.result, -95, "expected -EOPNOTSUPP on dummy file");
        }

        let _ = std::fs::remove_file(path);
    }
}
