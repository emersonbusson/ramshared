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
use std::slice;

use io_uring::{IoUring, opcode, squeue, types};

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

    /// Read-only view of the mapped bytes.
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: `ptr` originates from a successful `mmap` call exposing `len` readable
        // bytes (`PROT_READ`) and remains valid until `self` is dropped (`munmap`).
        unsafe { slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
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
        // SAFETY: `cmd` is copied into the SQE before submission. Public wrappers
        // in this module pass null pointers, local stack pointers, or borrowed mutable
        // buffers, and this function awaits the CQE before returning.
        unsafe {
            sq.push(&entry)
                .map_err(|_| io::Error::other("io_uring submission queue is full"))?;
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

/// Persistent io_uring instance that submits `UBLK_U_IO_FETCH_REQ` for ublk queue tags
/// **without waiting for CQE** (the driver parks each command with `-EIOCBQUEUED` until
/// I/O is ready or aborted). It owns the data buffers while FETCH calls are pending.
pub struct UblkFetchRing {
    ring: IoUring<squeue::Entry128>,
    /// The char device file descriptor, kept open for the lifetime of this ring.
    #[allow(dead_code)]
    fd: std::os::fd::OwnedFd,
    /// Data buffers per tag: the `addr` of each FETCH points to its corresponding
    /// buffer, which must remain alive while the command is parked in the kernel.
    /// Never read directly; exists to enforce the lifetime (drop guard).
    #[allow(dead_code)]
    buffers: Vec<Vec<u8>>,
}

impl UblkFetchRing {
    /// Submits `FETCH_REQ` for tags in `[0, queue_depth)` of queue 0 on `fd`, each using
    /// a buffer of `buf_size` bytes. Does not wait for CQE (`submit()` with want=0).
    /// The `fd` must remain open for the lifetime of this ring.
    pub fn submit_fetch_all(
        fd: std::os::fd::OwnedFd,
        queue_depth: u16,
        buf_size: usize,
    ) -> io::Result<Self> {
        const UBLK_U_IO_FETCH_REQ: u32 = 0xc010_7520;
        const QUEUE_ID_ZERO: u16 = 0;

        let entries = u32::from(queue_depth).max(1).next_power_of_two();
        let mut ring = IoUring::<squeue::Entry128>::builder().build(entries)?;
        let mut buffers: Vec<Vec<u8>> = (0..queue_depth).map(|_| vec![0u8; buf_size]).collect();

        for tag in 0..queue_depth {
            let addr = buffers[usize::from(tag)].as_mut_ptr() as u64;
            let cmd = fetch_cmd80(QUEUE_ID_ZERO, tag, addr);
            let entry = opcode::UringCmd80::new(
                types::Fd(std::os::fd::AsRawFd::as_raw_fd(&fd)),
                UBLK_U_IO_FETCH_REQ,
            )
            .cmd(cmd)
            .build()
            .user_data(u64::from(tag));

            // SAFETY: `cmd` (including `addr`) is copied into the SQE during `push`.
            // The `addr` points to `buffers[tag]`, which remains valid inside this struct
            // while the FETCH calls are parked; the kernel only accesses the buffer when
            // serving I/O, which requires `START_DEV` (not invoked in this path).
            unsafe {
                ring.submission()
                    .push(&entry)
                    .map_err(|_| io::Error::other("io_uring submission queue is full"))?;
            }
        }

        // Does not block (want=0); the FETCH requests remain parked in the driver.
        ring.submit()?;

        Ok(Self { ring, buffers, fd })
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
    fd: std::os::fd::OwnedFd,
    ring: IoUring<squeue::Entry128>,
    iodesc: MmapRo,
    buffers: Vec<Vec<u8>>,
    queue_depth: u16,
}

impl UblkServer {
    /// Size of `struct ublksrv_io_desc` (matches `ublk::UBLK_IO_DESC_SIZE`).
    const IO_DESC_SIZE: usize = 24;

    /// Creates the ring and maps the io-desc buffer for queue 0; does NOT submit FETCH commands.
    pub fn new(fd: std::os::fd::OwnedFd, queue_depth: u16, buf_size: usize) -> io::Result<Self> {
        let entries = u32::from(queue_depth).max(1).next_power_of_two();
        let ring = IoUring::<squeue::Entry128>::builder().build(entries)?;
        let iodesc_len = round_up_to_page(usize::from(queue_depth) * Self::IO_DESC_SIZE);
        let iodesc = MmapRo::map_readonly(std::os::fd::AsRawFd::as_raw_fd(&fd), iodesc_len, 0)?;
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

    /// Returns the read-only mapped bytes of `ublksrv_io_desc` for the given `tag`.
    pub fn io_desc_bytes(&self, tag: u16) -> &[u8] {
        let start = usize::from(tag) * Self::IO_DESC_SIZE;
        &self.iodesc.as_bytes()[start..start + Self::IO_DESC_SIZE]
    }

    /// Returns the mutable data buffer for `tag` (READ populates this; WRITE comes pre-populated).
    pub fn buffer_mut(&mut self, tag: u16) -> &mut [u8] {
        &mut self.buffers[usize::from(tag)]
    }

    /// Completes the request on `tag` with `result` and re-arms the FETCH command.
    pub fn commit_and_fetch(&mut self, tag: u16, result: i32) -> io::Result<()> {
        const UBLK_U_IO_COMMIT_AND_FETCH_REQ: u32 = 0xc010_7521;
        let addr = self.buffers[usize::from(tag)].as_mut_ptr() as u64;
        self.push(UBLK_U_IO_COMMIT_AND_FETCH_REQ, tag, result, addr)?;
        self.ring.submit()?;
        Ok(())
    }

    fn push(&mut self, cmd_op: u32, tag: u16, result: i32, addr: u64) -> io::Result<()> {
        let cmd = io_cmd80(0, tag, result, addr);
        let entry =
            opcode::UringCmd80::new(types::Fd(std::os::fd::AsRawFd::as_raw_fd(&self.fd)), cmd_op)
                .cmd(cmd)
                .build()
                .user_data(u64::from(tag));

        // SAFETY: `cmd` (carrying `addr`) is copied into the SQE in `push`. `addr` points
        // to `self.buffers[tag]`, which remains valid for the server's lifetime; `self.fd`
        // remains open. The kernel only accesses the buffer to serve I/O on this thread.
        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| io::Error::other("io_uring submission queue is full"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
