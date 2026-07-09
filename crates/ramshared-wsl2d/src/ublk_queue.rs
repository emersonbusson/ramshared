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
/// `docs/ublk-backend/SPEC-ring-loop.md` §3.
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
