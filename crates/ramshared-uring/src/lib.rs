//! Safe wrappers over the `io-uring` crate for Phase B.
//!
//! The main daemon `ramsharedd` (`ramshared-wsl2d` crate) remains `#![forbid(unsafe_code)]`.
//! Any raw SQE operations requiring `unsafe` are isolated within this crate, with invariants documented
//! in the narrowest scope possible.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
use std::io;
use std::os::fd::RawFd;
use std::ptr;

use io_uring::{IoUring, opcode, squeue, types};

/// A compile-time check enforcing the exact `#[repr(C)]` memory layout of the
/// submission queue entry, verifying that the structure is strictly 64 bytes.
/// This ensures ABI compatibility and that `user_data` aligns with the C kernel.
const _: () = {
    assert!(
        std::mem::size_of::<squeue::Entry>() == 64,
        "io_uring_sqe struct layout mismatch"
    );
};

/// Returns the system page size (`sysconf(_SC_PAGESIZE)`), falling back to 4096.
pub fn page_size() -> usize {
    // SAFETY: Calling `sysconf` with `_SC_PAGESIZE` has no side effects and is always
    // safe; on Linux it returns a value > 0.
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if value > 0 { value as usize } else { 4096 }
}

/// Rounds up `n` to the next page size boundary, mirroring the `round_up(.., PAGE_SIZE)`
/// logic the ublk driver uses to dimension command buffers per queue.
pub fn round_up_to_page(n: usize) -> usize {
    let page = page_size();
    n.div_ceil(page) * page
}

/// Read-only memory mapping (`mmap`) with automated cleanup (`munmap`) on `Drop` (RAII).
/// Used for the io-desc buffer of `/dev/ublkcN` which the kernel exposes read-only.
/// Writing to it triggers `-EPERM`.
pub struct MmapRo {
    ptr: *mut c_void,
    len: usize,
}

impl MmapRo {
    /// Maps `len` bytes from the given `fd` at the specified `offset` using `PROT_READ` and `MAP_SHARED`.
    pub fn map_readonly(fd: RawFd, len: usize, offset: i64) -> io::Result<Self> {
        if len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mmap len must be > 0",
            ));
        }

        // SAFETY: Passing null as `addr` lets the kernel select the mapping address;
        // we map only `PROT_READ` over the file descriptor of the ublk control device.
        // The return value is validated against `MAP_FAILED` below; on success, the
        // pointer covers `len` readable bytes valid until the matching `munmap` on `Drop`.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                offset,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        Ok(Self { ptr, len })
    }

    /// Copies one bounds-checked range into an owned array.
    ///
    /// The kernel may update this shared mapping. Returning a borrowed Rust
    /// slice would incorrectly express that the bytes are immutable for the
    /// borrow's lifetime. Callers decode only this point-in-time snapshot.
    pub fn copy_array<const N: usize>(&self, offset: usize) -> io::Result<[u8; N]> {
        let end = offset
            .checked_add(N)
            .filter(|end| *end <= self.len)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "mmap range out of bounds")
            })?;
        debug_assert!(end <= self.len);
        let mut snapshot = [0u8; N];
        // SAFETY: the checked range `[offset, offset + N)` lies within the
        // readable mapping. The destination is a distinct owned array of N
        // bytes, so the regions cannot overlap.
        unsafe {
            ptr::copy_nonoverlapping(self.ptr.cast::<u8>().add(offset), snapshot.as_mut_ptr(), N);
        }
        Ok(snapshot)
    }
}

impl Drop for MmapRo {
    fn drop(&mut self) {
        // SAFETY: `ptr` and `len` originate from a successful `mmap` call and have not
        // been unmapped yet. `munmap` is invoked exactly once during drop.
        unsafe {
            libc::munmap(self.ptr, self.len);
        }
    }
}

// SAFETY: `MmapRo` has exclusive ownership of a valid process-wide memory mapping;
// transferring ownership across thread boundaries is safe. It does not implement
// `Sync` (no shared concurrent access).
unsafe impl Send for MmapRo {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmokeReport {
    pub entries: u32,
    pub submitted: usize,
}

pub fn smoke(entries: u32) -> io::Result<SmokeReport> {
    let ring = io_uring::IoUring::new(entries)?;
    let submitted = ring.submit()?;

    Ok(SmokeReport { entries, submitted })
}

pub fn ublk_get_features(fd: RawFd) -> io::Result<u64> {
    const UBLK_U_CMD_GET_FEATURES: u32 = 0x8020_7513;
    const UBLK_FEATURES_LEN: u16 = 8;

    let mut features = 0u64;
    let cmd = ctrl_cmd(0, UBLK_FEATURES_LEN, (&mut features as *mut u64) as u64);

    let res = submit_uring_cmd80(fd, UBLK_U_CMD_GET_FEATURES, cmd)?;
    if res != 0 {
        return Err(io::Error::other(format!(
            "ublk GET_FEATURES returned unexpected result {res}"
        )));
    }

    Ok(features)
}

pub fn ublk_add_dev(fd: RawFd, dev_id: u32, info: &mut [u8; 64]) -> io::Result<()> {
    const UBLK_U_CMD_ADD_DEV: u32 = 0xc020_7504;
    const UBLK_CTRL_DEV_INFO_LEN: u16 = 64;

    let cmd = ctrl_cmd(dev_id, UBLK_CTRL_DEV_INFO_LEN, info.as_mut_ptr() as u64);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_ADD_DEV, cmd)?,
        "ublk ADD_DEV",
    )
}

pub fn ublk_del_dev(fd: RawFd, dev_id: u32) -> io::Result<()> {
    const UBLK_U_CMD_DEL_DEV: u32 = 0xc020_7505;

    let cmd = ctrl_cmd(dev_id, 0, 0);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_DEL_DEV, cmd)?,
        "ublk DEL_DEV",
    )
}

/// `SET_PARAMS`: Sends a `struct ublk_params` (112 B) to the device `dev_id`.
pub fn ublk_set_params(fd: RawFd, dev_id: u32, params: &mut [u8; 112]) -> io::Result<()> {
    const UBLK_U_CMD_SET_PARAMS: u32 = 0xc020_7508;

    let cmd = ctrl_cmd(dev_id, 112, params.as_mut_ptr() as u64);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_SET_PARAMS, cmd)?,
        "ublk SET_PARAMS",
    )
}

/// `GET_PARAMS`: Kernel populates the `struct ublk_params` (112 B) for device `dev_id`.
pub fn ublk_get_params(fd: RawFd, dev_id: u32, params: &mut [u8; 112]) -> io::Result<()> {
    const UBLK_U_CMD_GET_PARAMS: u32 = 0x8020_7509;

    let cmd = ctrl_cmd(dev_id, 112, params.as_mut_ptr() as u64);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_GET_PARAMS, cmd)?,
        "ublk GET_PARAMS",
    )
}

/// `START_DEV`: Creates `/dev/ublkbN` (blocks until queues are ready and `add_disk` runs).
/// The `ublksrv_pid` is stored in `data[0]` of `ublksrv_ctrl_cmd` (offset 16).
pub fn ublk_start_dev(fd: RawFd, dev_id: u32, ublksrv_pid: u32) -> io::Result<()> {
    const UBLK_U_CMD_START_DEV: u32 = 0xc020_7506;

    let mut cmd = ctrl_cmd(dev_id, 0, 0);
    cmd[16..24].copy_from_slice(&u64::from(ublksrv_pid).to_ne_bytes());
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_START_DEV, cmd)?,
        "ublk START_DEV",
    )
}

/// `STOP_DEV`: Removes `/dev/ublkbN` and aborts all pending FETCH requests.
pub fn ublk_stop_dev(fd: RawFd, dev_id: u32) -> io::Result<()> {
    const UBLK_U_CMD_STOP_DEV: u32 = 0xc020_7507;

    let cmd = ctrl_cmd(dev_id, 0, 0);
    expect_zero(
        submit_uring_cmd80(fd, UBLK_U_CMD_STOP_DEV, cmd)?,
        "ublk STOP_DEV",
    )
}

fn ctrl_cmd(dev_id: u32, len: u16, addr: u64) -> [u8; 80] {
    const UBLK_QUEUE_ID_NONE: u16 = u16::MAX;

    let mut cmd = [0u8; 80];
    cmd[0..4].copy_from_slice(&dev_id.to_ne_bytes());
    cmd[4..6].copy_from_slice(&UBLK_QUEUE_ID_NONE.to_ne_bytes());
    cmd[6..8].copy_from_slice(&len.to_ne_bytes());
    cmd[8..16].copy_from_slice(&addr.to_ne_bytes());
    cmd
}

fn expect_zero(result: i32, context: &str) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{context} returned unexpected result {result}"
        )))
    }
}

fn submit_uring_cmd80(fd: RawFd, cmd_op: u32, cmd: [u8; 80]) -> io::Result<i32> {
    let mut ring = IoUring::<squeue::Entry128>::builder().build(2)?;
    let entry = opcode::UringCmd80::new(types::Fd(fd), cmd_op)
        .cmd(cmd)
        .build()
        .user_data(1);

    {
        let mut sq = ring.submission();
        if sq.is_full() {
            return Err(io::Error::other("io_uring submission queue is full"));
        }
        // SAFETY: `cmd` is copied into the SQE before submission. Public wrappers
        // in this module pass null pointers, local stack pointers, or borrowed mutable
        // buffers, and this function awaits the CQE before returning.
        unsafe {
            let _ = sq.push(&entry);
        }
    }

    ring.submit_and_wait(1)?;

    let cqe = ring
        .completion()
        .next()
        .ok_or_else(|| io::Error::other("io_uring completion queue is empty"))?;
    let result = cqe.result();
    if result < 0 {
        Err(io::Error::from_raw_os_error(-result))
    } else {
        Ok(result)
    }
}

/// CQE completion of a ublk command on the ring: carries the `tag` (from `user_data`) and the `result`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UblkCompletion {
    pub tag: u16,
    pub result: i32,
}

/// Validates that fixed buffer parameters are aligned to 4096 bytes and that the
/// total allocation (`queue_depth * buf_size`) does not exceed `RLIMIT_MEMLOCK`.
pub fn validate_fixed_buffer_params(queue_depth: u16, buf_size: usize) -> io::Result<()> {
    if buf_size == 0 || !buf_size.is_multiple_of(4096) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "buffer size must be > 0 and a multiple of 4096 bytes",
        ));
    }

    let total_bytes = (queue_depth as usize)
        .checked_mul(buf_size)
        .ok_or_else(|| io::Error::from_raw_os_error(libc::ERANGE))?;

    let mut rlim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: Calling `getrlimit` with a valid mutable reference to a `rlimit` struct is safe.
    let res = unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut rlim) };
    if res == 0 && rlim.rlim_cur != libc::RLIM_INFINITY && (total_bytes as u64) > rlim.rlim_cur {
        return Err(io::Error::from_raw_os_error(libc::ERANGE));
    }

    Ok(())
}

/// Persistent io_uring instance that submits `UBLK_U_IO_FETCH_REQ` for ublk queue tags
/// **without waiting for CQE** (the driver parks each command with `-EIOCBQUEUED` until
/// I/O is ready or aborted). It owns the data buffers while FETCH calls are pending.
pub struct UblkFetchRing {
    ring: IoUring<squeue::Entry128>,
    /// Data buffers per tag: the `addr` of each FETCH points to its corresponding
    /// buffer, which must remain alive while the command is parked in the kernel.
    /// Never read directly; exists to enforce the lifetime (drop guard).
    _buffers: Vec<Vec<u8>>,
}

impl UblkFetchRing {
    /// Submits `FETCH_REQ` for tags in `[0, queue_depth)` of queue 0 on `fd`, each using
    /// a buffer of `buf_size` bytes. Does not wait for CQE (`submit()` with want=0).
    /// The `fd` must remain open for the lifetime of this ring.
    pub fn submit_fetch_all(fd: RawFd, queue_depth: u16, buf_size: usize) -> io::Result<Self> {
        validate_fixed_buffer_params(queue_depth, buf_size)?;
        const UBLK_U_IO_FETCH_REQ: u32 = 0xc010_7520;
        const QUEUE_ID_ZERO: u16 = 0;

        let entries = u32::from(queue_depth).max(1).next_power_of_two();
        let mut ring = IoUring::<squeue::Entry128>::builder().build(entries)?;
        let mut buffers: Vec<Vec<u8>> = (0..queue_depth).map(|_| vec![0u8; buf_size]).collect();

        for tag in 0..queue_depth {
            let addr = buffers[usize::from(tag)].as_mut_ptr() as u64;
            let cmd = fetch_cmd80(QUEUE_ID_ZERO, tag, addr);
            let entry = opcode::UringCmd80::new(types::Fd(fd), UBLK_U_IO_FETCH_REQ)
                .cmd(cmd)
                .build()
                .user_data(u64::from(tag));

            // SAFETY: `cmd` (including `addr`) is copied into the SQE during `push`.
            // The `addr` points to `buffers[tag]`, which remains valid inside this struct
            // while the FETCH calls are parked; the kernel only accesses the buffer when
            // serving I/O, which requires `START_DEV` (not invoked in this path).
            let mut sq = ring.submission();
            if sq.is_full() {
                return Err(io::Error::other("io_uring submission queue is full"));
            }
            unsafe {
                let _ = sq.push(&entry);
            }
        }

        // Does not block (want=0); the FETCH requests remain parked in the driver.
        ring.submit()?;

        Ok(Self {
            ring,
            _buffers: buffers,
        })
    }

    /// Drains currently available CQEs without blocking.
    pub fn drain(&mut self) -> Vec<UblkCompletion> {
        self.ring
            .completion()
            .map(|cqe| UblkCompletion {
                tag: cqe.user_data() as u16,
                result: cqe.result(),
            })
            .collect()
    }
}

/// Packs a `struct ublksrv_io_cmd` (16 B: q_id, tag, result, addr) into the
/// first bytes of the SQE's 80 B `UringCmd80` buffer; the remaining bytes are zeroed.
fn io_cmd80(q_id: u16, tag: u16, result: i32, addr: u64) -> [u8; 80] {
    let mut cmd = [0u8; 80];
    cmd[0..2].copy_from_slice(&q_id.to_ne_bytes());
    cmd[2..4].copy_from_slice(&tag.to_ne_bytes());
    cmd[4..8].copy_from_slice(&result.to_ne_bytes());
    cmd[8..16].copy_from_slice(&addr.to_ne_bytes());
    cmd
}

/// `ublksrv_io_cmd` structure for a `FETCH_REQ` (result initialized to zero).
fn fetch_cmd80(q_id: u16, tag: u16, addr: u64) -> [u8; 80] {
    io_cmd80(q_id, tag, 0, addr)
}

/// Persistent ublk queue server: manages a persistent `Entry128` ring, performs a
/// read-only `mmap` of the io-desc buffer, and owns data buffers per tag.
/// Submits `FETCH_REQ`, exposes request descriptors, and completes requests via
/// `COMMIT_AND_FETCH_REQ`. The `fd` of the char device must remain open for the
/// lifetime of the server.
pub struct UblkServer {
    fd: RawFd,
    ring: IoUring<squeue::Entry128>,
    iodesc: MmapRo,
    buffers: Vec<Vec<u8>>,
    queue_depth: u16,
}

impl UblkServer {
    /// Size of `struct ublksrv_io_desc` (matches `ublk::UBLK_IO_DESC_SIZE`).
    const IO_DESC_SIZE: usize = 24;

    /// Creates the ring and maps the io-desc buffer for queue 0; does NOT submit FETCH commands.
    pub fn new(fd: RawFd, queue_depth: u16, buf_size: usize) -> io::Result<Self> {
        validate_fixed_buffer_params(queue_depth, buf_size)?;
        let entries = u32::from(queue_depth).max(1).next_power_of_two();
        let ring = IoUring::<squeue::Entry128>::builder().build(entries)?;
        let iodesc_len = round_up_to_page(usize::from(queue_depth) * Self::IO_DESC_SIZE);
        let iodesc = MmapRo::map_readonly(fd, iodesc_len, 0)?;
        let buffers = (0..queue_depth).map(|_| vec![0u8; buf_size]).collect();
        Ok(Self {
            fd,
            ring,
            iodesc,
            buffers,
            queue_depth,
        })
    }

    /// Submits `FETCH_REQ` for all tags, marking the queue as ready. Does not wait for CQE.
    pub fn submit_initial_fetch(&mut self) -> io::Result<()> {
        const UBLK_U_IO_FETCH_REQ: u32 = 0xc010_7520;
        for tag in 0..self.queue_depth {
            let addr = self.buffers[usize::from(tag)].as_mut_ptr() as u64;
            self.push(UBLK_U_IO_FETCH_REQ, tag, 0, addr)?;
        }
        self.ring.submit()?;
        Ok(())
    }

    /// Drains available CQEs (non-blocking).
    pub fn drain(&mut self) -> Vec<UblkCompletion> {
        self.ring
            .completion()
            .map(|cqe| UblkCompletion {
                tag: cqe.user_data() as u16,
                result: cqe.result(),
            })
            .collect()
    }

    /// Blocks until at least one CQE is available (next request served or teardown abort)
    /// and drains it. Does not submit new SQEs (FETCH/COMMIT calls are already queued
    /// via `submit_initial_fetch` or `commit_and_fetch`).
    ///
    /// Retries on `EINTR` (interrupted `io_uring_enter` syscall): a daemon handling
    /// SIGINT/SIGTERM may receive signals on this thread; EINTR is not an error, we just
    /// resume waiting. Already submitted SQEs remain armed.
    pub fn wait_and_drain(&mut self) -> io::Result<Vec<UblkCompletion>> {
        loop {
            match self.ring.submit_and_wait(1) {
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(self.drain())
    }

    /// Returns an owned snapshot of `ublksrv_io_desc` for the given `tag`.
    pub fn io_desc_snapshot(&self, tag: u16) -> io::Result<[u8; Self::IO_DESC_SIZE]> {
        self.validate_tag(tag)?;
        let start = usize::from(tag) * Self::IO_DESC_SIZE;
        self.iodesc.copy_array::<{ Self::IO_DESC_SIZE }>(start)
    }

    /// Returns the mutable data buffer for `tag` (READ populates this; WRITE comes pre-populated).
    pub fn buffer_mut(&mut self, tag: u16) -> io::Result<&mut [u8]> {
        self.validate_tag(tag)?;
        Ok(&mut self.buffers[usize::from(tag)])
    }

    /// Completes the request on `tag` with `result` and re-arms the FETCH command.
    pub fn commit_and_fetch(&mut self, tag: u16, result: i32) -> io::Result<()> {
        self.validate_tag(tag)?;
        const UBLK_U_IO_COMMIT_AND_FETCH_REQ: u32 = 0xc010_7521;
        let addr = self.buffers[usize::from(tag)].as_mut_ptr() as u64;
        self.push(UBLK_U_IO_COMMIT_AND_FETCH_REQ, tag, result, addr)?;
        self.ring.submit()?;
        Ok(())
    }

    fn validate_tag(&self, tag: u16) -> io::Result<()> {
        if tag < self.queue_depth {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tag must be < queue_depth",
            ))
        }
    }

    fn push(&mut self, cmd_op: u32, tag: u16, result: i32, addr: u64) -> io::Result<()> {
        let cmd = io_cmd80(0, tag, result, addr);
        let entry = opcode::UringCmd80::new(types::Fd(self.fd), cmd_op)
            .cmd(cmd)
            .build()
            .user_data(u64::from(tag));

        // SAFETY: `cmd` (carrying `addr`) is copied into the SQE in `push`. `addr` points
        // to `self.buffers[tag]`, which remains valid for the server's lifetime; `self.fd`
        // remains open. The kernel only accesses the buffer to serve I/O on this thread.
        let mut sq = self.ring.submission();
        if sq.is_full() {
            return Err(io::Error::other("io_uring submission queue is full"));
        }
        unsafe {
            let _ = sq.push(&entry);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {

    #[test]
    fn test_validate_fixed_buffer_alignment_and_limits() {
        // Test invalid alignment
        let err = validate_fixed_buffer_params(2, 4095).expect_err("invalid alignment should fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        // Test valid alignment
        assert!(validate_fixed_buffer_params(2, 4096).is_ok());

        // Test buffer size 0
        let err_zero = validate_fixed_buffer_params(2, 0).expect_err("zero size should fail");
        assert_eq!(err_zero.kind(), io::ErrorKind::InvalidInput);

        // Test huge limit exceeding
        let mut rlim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: getrlimit is safe here.
        let res = unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut rlim) };
        if res == 0 && rlim.rlim_cur != libc::RLIM_INFINITY {
            // we create a massive buffer
            // wait, if we pass u16::MAX for queue depth and rlim_cur for buf size...
            let massive_buf_size = (((rlim.rlim_cur / 4096) + 2) * 4096) as usize;
            let huge_err = validate_fixed_buffer_params(2, massive_buf_size)
                .expect_err("massive buffer should fail");
            assert_eq!(huge_err.raw_os_error(), Some(libc::ERANGE));
        }
    }

    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::{Seek, SeekFrom, Write};
    use std::os::fd::AsRawFd;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn regular_file_fixture(label: &str, len: usize) -> (std::path::PathBuf, std::fs::File) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("ramshared-{label}-{}-{nonce}", std::process::id()));
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("create regular-file fixture");
        file.set_len(len as u64).expect("size regular-file fixture");
        (path, file)
    }

    #[test]
    fn fetch_cmd80_packs_ublksrv_io_cmd_in_first_16_bytes() {
        let cmd = fetch_cmd80(0, 7, 0xdead_beef);

        assert_eq!(u16::from_ne_bytes([cmd[0], cmd[1]]), 0);
        assert_eq!(u16::from_ne_bytes([cmd[2], cmd[3]]), 7);
        assert_eq!(i32::from_ne_bytes([cmd[4], cmd[5], cmd[6], cmd[7]]), 0);
        assert_eq!(
            u64::from_ne_bytes([
                cmd[8], cmd[9], cmd[10], cmd[11], cmd[12], cmd[13], cmd[14], cmd[15],
            ]),
            0xdead_beef
        );
        assert!(cmd[16..].iter().all(|&b| b == 0));
    }

    #[test]
    fn mmap_descriptor_snapshot_is_owned_and_bounds_checked() {
        let path = std::env::temp_dir().join(format!(
            "ramshared-mmap-snapshot-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("create mapping fixture");
        file.write_all(b"abcdefgh").expect("write fixture");
        file.flush().expect("flush fixture");
        let map = MmapRo::map_readonly(file.as_raw_fd(), 8, 0).expect("map fixture");

        let first = map.copy_array::<4>(0).expect("first snapshot");
        file.seek(SeekFrom::Start(0)).expect("seek fixture");
        file.write_all(b"WXYZ").expect("mutate fixture");
        file.flush().expect("flush mutation");
        let second = map.copy_array::<4>(0).expect("second snapshot");

        assert_eq!(first, *b"abcd");
        assert_eq!(second, *b"WXYZ");
        assert!(map.copy_array::<4>(6).is_err());
        drop(map);
        drop(file);
        fs::remove_file(path).expect("remove fixture");
    }

    #[test]
    fn regular_file_ublk_adapters_refuse_without_a_device() {
        let page = page_size();
        assert!(page >= 4096);
        assert_eq!(round_up_to_page(1), page);
        assert_eq!(round_up_to_page(page + 1), page * 2);
        assert!(MmapRo::map_readonly(-1, 0, 0).is_err());
        assert!(MmapRo::map_readonly(-1, page, 0).is_err());

        assert_eq!(expect_zero(0, "test").expect("zero result"), ());
        assert!(expect_zero(7, "test").is_err());

        let (path, file) = regular_file_fixture("ublk-refusal", page);
        let fd = file.as_raw_fd();
        let mut info = [0u8; 64];
        let mut params = [0u8; 112];
        assert!(ublk_get_features(fd).is_err());
        assert!(ublk_add_dev(fd, 9, &mut info).is_err());
        assert!(ublk_del_dev(fd, 9).is_err());
        assert!(ublk_set_params(fd, 9, &mut params).is_err());
        assert!(ublk_get_params(fd, 9, &mut params).is_err());
        assert!(ublk_start_dev(fd, 9, std::process::id()).is_err());
        assert!(ublk_stop_dev(fd, 9).is_err());

        let mut server = UblkServer::new(fd, 1, 4096).expect("regular-file server fixture");
        assert_eq!(
            server.io_desc_snapshot(0).expect("zero descriptor"),
            [0u8; 24]
        );
        assert!(server.io_desc_snapshot(1).is_err());
        assert_eq!(server.buffer_mut(0).expect("tag zero buffer").len(), 4096);
        assert!(server.buffer_mut(1).is_err());
        assert!(server.commit_and_fetch(1, 0).is_err());
        server
            .submit_initial_fetch()
            .expect("submit regular-file refusal");
        let completions = server.wait_and_drain().expect("drain regular-file refusal");
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result < 0);

        drop(server);
        drop(file);
        fs::remove_file(path).expect("remove regular-file fixture");
    }

    #[test]
    fn regular_file_fetch_ring_drains_refusal_without_a_device() {
        let (path, file) = regular_file_fixture("ublk-fetch-refusal", page_size());
        let mut ring = UblkFetchRing::submit_fetch_all(file.as_raw_fd(), 2, 4096)
            .expect("submit regular-file fetches");
        let deadline = Instant::now() + Duration::from_secs(2);
        let completions = loop {
            let current = ring.drain();
            if current.len() == 2 {
                break current;
            }
            assert!(Instant::now() < deadline, "regular-file CQE deadline");
            std::thread::yield_now();
        };
        assert!(completions.iter().all(|completion| completion.result < 0));

        drop(ring);
        drop(file);
        fs::remove_file(path).expect("remove regular-file fixture");
    }

    #[test]
    fn test_io_uring_worker_timeout_and_cancellation() {
        let (path, file) = regular_file_fixture("ublk-timeout-cancel", page_size());
        let fd = file.as_raw_fd();

        let mut server = UblkServer::new(fd, 2, 4096).expect("server fixture");

        // Simulate a Timeout directly in the server's ring
        let ts = types::Timespec::new().sec(0).nsec(1_000_000); // 1ms
        let entry = opcode::Timeout::new(&ts as *const _).build().user_data(99);
        let timeout_entry: squeue::Entry128 = entry.into();

        // SAFETY: The timespec struct outlives the kernel submission, and the server ring is local.
        unsafe {
            server
                .ring
                .submission()
                .push(&timeout_entry)
                .expect("push timeout");
        }

        // Use wait_and_drain which should block and then return the timeout CQE
        let completions = server.wait_and_drain().expect("wait and drain timeout");
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].tag, 99);
        assert_eq!(completions[0].result, -libc::ETIME);

        // Simulate Cancellation
        let ts_long = types::Timespec::new().sec(10).nsec(0);
        let entry2 = opcode::Timeout::new(&ts_long as *const _)
            .build()
            .user_data(100);
        let timeout_entry2: squeue::Entry128 = entry2.into();

        let centry = opcode::AsyncCancel::new(100).build().user_data(101);
        let cancel_entry: squeue::Entry128 = centry.into();

        // SAFETY: The timespec lives in the same frame, we wait before drop.
        unsafe {
            server
                .ring
                .submission()
                .push(&timeout_entry2)
                .expect("push long timeout");
            server
                .ring
                .submission()
                .push(&cancel_entry)
                .expect("push cancel");
        }

        // Wait for both the cancellation and the cancelled timeout
        server.ring.submit_and_wait(2).expect("submit cancel");
        let mut results = server.drain();
        results.sort_by_key(|c| c.tag);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tag, 100);
        assert_eq!(results[0].result, -libc::ECANCELED);

        assert_eq!(results[1].tag, 101);
        assert!(results[1].result == 0 || results[1].result == -libc::EALREADY);

        drop(server);
        drop(file);
        fs::remove_file(path).expect("remove fixture");
    }
    #[test]
    fn ublk_server_push_guard_clause_rejects_full_ring() {
        let (path, file) = regular_file_fixture("ublk-full-ring", page_size());
        let mut server = UblkServer::new(file.as_raw_fd(), 2, 4096).expect("server fixture");

        // Ring capacity is 2. Push 2 items, 3rd should fail.
        assert!(server.push(0, 0, 0, 0).is_ok());
        assert!(server.push(0, 1, 0, 0).is_ok());
        let err = server
            .push(0, 2, 0, 0)
            .expect_err("expected ring full error");
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(err.to_string(), "io_uring submission queue is full");

        drop(server);
        drop(file);
        fs::remove_file(path).expect("remove fixture");
    }

    #[test]
    fn test_io_uring_sqe_user_data_offset() {
        let e = io_uring::opcode::Nop::new()
            .build()
            .user_data(0xDEADBEEFCAFEBABEu64);
        let e_ptr = &e as *const _ as *const u8;
        // SAFETY: The Entry is guaranteed to be 64 bytes by the compile-time assertion.
        let slice = unsafe { std::slice::from_raw_parts(e_ptr, 64) };
        let expected = 0xDEADBEEFCAFEBABEu64.to_ne_bytes();
        let actual = &slice[32..40];
        assert_eq!(
            actual, expected,
            "user_data must be exactly at offset 32 to match io_uring_sqe"
        );
    }
}
