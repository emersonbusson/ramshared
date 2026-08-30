//! Windows control-handle / mapped queue adapter (SPEC DT-4 / ITEM-3).
//!
//! Isolates all Windows unsafe mapping + IOCTL. Pure [`crate::driver_link::InMemoryQueue`]
//! remains the hermetic path. Cover: N/A — E2E-only (WDK + Verifier).

#![cfg(windows)]
#![allow(unsafe_code)]

use std::mem::{size_of, zeroed};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, FALSE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualLock,
    VirtualUnlock,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, WaitForSingleObject,
};

use crate::driver_link::{DriverLinkError, QueueAccess};
use crate::proto::{
    ABI_VERSION, Cqe, DiskParams, MAX_IO, MAX_QD, RING_MAGIC, Register, RingHdr, Sqe,
};

/// CTL_CODE(FILE_DEVICE_MASS_STORAGE=0x2d, 0x800|N, METHOD_BUFFERED, FILE_READ|FILE_WRITE).
const fn ioctl_code(fn_n: u32) -> u32 {
    const FILE_DEVICE_MASS_STORAGE: u32 = 0x0000_002d;
    const METHOD_BUFFERED: u32 = 0;
    const FILE_READ_ACCESS: u32 = 0x0001;
    const FILE_WRITE_ACCESS: u32 = 0x0002;
    const ACCESS: u32 = FILE_READ_ACCESS | FILE_WRITE_ACCESS;
    (FILE_DEVICE_MASS_STORAGE << 16) | (ACCESS << 14) | ((0x800 + fn_n) << 2) | METHOD_BUFFERED
}

const IOCTL_REGISTER: u32 = ioctl_code(0);
const IOCTL_UNREGISTER: u32 = ioctl_code(1);
const IOCTL_COMMIT: u32 = ioctl_code(2);
const IOCTL_CREATE: u32 = ioctl_code(3);
const IOCTL_DESTROY: u32 = ioctl_code(4);

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

/// Contiguous page-aligned SQ/CQ/data regions for REGISTER (DT-4).
///
/// Shared headers use aligned 32-bit words with Rust Release/Acquire publication.
pub struct WindowsMappedQueue {
    pub queue_depth: u32,
    pub max_io_bytes: u32,
    pub block_size: u32,
    sq: *mut u8,
    cq: *mut u8,
    data: *mut u8,
    sq_bytes: usize,
    cq_bytes: usize,
    data_bytes: usize,
}

// SAFETY: regions are exclusively owned by this process and only accessed via
// QueueAccess with owned snapshots after Acquire barriers.
unsafe impl Send for WindowsMappedQueue {}

impl WindowsMappedQueue {
    pub fn try_new(queue_depth: u32, max_io_bytes: u32, block_size: u32) -> std::io::Result<Self> {
        if queue_depth == 0 || queue_depth > MAX_QD || !queue_depth.is_power_of_two() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "queue_depth",
            ));
        }
        if max_io_bytes == 0 || max_io_bytes > MAX_IO {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "max_io_bytes",
            ));
        }
        if block_size != 512 && block_size != 4096 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "block_size",
            ));
        }
        let data_bytes = (queue_depth as usize)
            .checked_mul(max_io_bytes as usize)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "data area overflow")
            })?;
        if data_bytes > 4 * 1024 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "data area > 4 MiB",
            ));
        }
        let sq_bytes = size_of::<RingHdr>() + (queue_depth as usize) * size_of::<Sqe>();
        let cq_bytes = size_of::<RingHdr>() + (queue_depth as usize) * size_of::<Cqe>();

        let sq = alloc_region(sq_bytes)?;
        let cq = match alloc_region(cq_bytes) {
            Ok(region) => region,
            Err(error) => {
                free_region(sq, sq_bytes);
                return Err(error);
            }
        };
        let data = match alloc_region(data_bytes) {
            Ok(region) => region,
            Err(error) => {
                free_region(cq, cq_bytes);
                free_region(sq, sq_bytes);
                return Err(error);
            }
        };

        // Zero + init ring headers (magic, entries, head=0, tail=0).
        unsafe {
            ptr::write_bytes(sq, 0, sq_bytes);
            ptr::write_bytes(cq, 0, cq_bytes);
            ptr::write_bytes(data, 0, data_bytes);
            init_ring_hdr(sq, queue_depth);
            init_ring_hdr(cq, queue_depth);
        }

        Ok(Self {
            queue_depth,
            max_io_bytes,
            block_size,
            sq,
            cq,
            data,
            sq_bytes,
            cq_bytes,
            data_bytes,
        })
    }

    /// Build ABI-v1 REGISTER descriptor with mapped VAs (disk_id must be 0 for v1).
    pub fn registration(&self, disk_id: u32) -> Register {
        Register {
            abi_version: ABI_VERSION,
            disk_id,
            queue_depth: self.queue_depth,
            block_size: self.block_size,
            max_io_bytes: self.max_io_bytes,
            reserved: 0,
            sq_ring_va: self.sq as u64,
            cq_ring_va: self.cq as u64,
            data_area_va: self.data as u64,
            data_area_len: self.data_bytes as u64,
            sq_event_handle: 0,
            cq_event_handle: 0,
        }
    }

    fn mask(&self) -> u32 {
        self.queue_depth - 1
    }

    fn sq_hdr(&self) -> *mut RingHdr {
        self.sq as *mut RingHdr
    }

    fn cq_hdr(&self) -> *mut RingHdr {
        self.cq as *mut RingHdr
    }

    fn load_idx(ptr: *const u32) -> u32 {
        // SAFETY: ring header indices are 32-bit aligned shared words; Acquire via atomic view.
        unsafe { (*(ptr as *const AtomicU32)).load(Ordering::Acquire) }
    }

    fn store_idx(ptr: *mut u32, v: u32) {
        // SAFETY: ring header indices are 32-bit aligned shared words; Release publication.
        unsafe { (*(ptr as *const AtomicU32)).store(v, Ordering::Release) }
    }
}

impl Drop for WindowsMappedQueue {
    fn drop(&mut self) {
        free_region(self.sq, self.sq_bytes);
        free_region(self.cq, self.cq_bytes);
        free_region(self.data, self.data_bytes);
        self.sq = ptr::null_mut();
        self.cq = ptr::null_mut();
        self.data = ptr::null_mut();
    }
}

impl QueueAccess for WindowsMappedQueue {
    fn queue_depth(&self) -> u32 {
        self.queue_depth
    }
    fn max_io_bytes(&self) -> u32 {
        self.max_io_bytes
    }
    fn block_size(&self) -> u32 {
        self.block_size
    }
    fn sq_pending(&self) -> u32 {
        let hdr = self.sq_hdr();
        unsafe {
            let head = Self::load_idx(ptr::addr_of!((*hdr).head));
            let tail = Self::load_idx(ptr::addr_of!((*hdr).tail));
            tail.wrapping_sub(head)
        }
    }
    fn pop_sqe_snapshot(&mut self) -> Option<Sqe> {
        let hdr = self.sq_hdr();
        unsafe {
            let head = Self::load_idx(ptr::addr_of!((*hdr).head));
            let tail = Self::load_idx(ptr::addr_of!((*hdr).tail));
            if head == tail {
                return None;
            }
            // Revalidate wrap distance (DT-5).
            if tail.wrapping_sub(head) > self.queue_depth {
                return None;
            }
            let idx = (head & self.mask()) as usize;
            let entry_ptr =
                self.sq.add(size_of::<RingHdr>() + idx * size_of::<Sqe>()) as *const Sqe;
            // Owned snapshot — never re-read shared entry after this.
            let sqe = ptr::read_unaligned(entry_ptr);
            Self::store_idx(ptr::addr_of_mut!((*hdr).head), head.wrapping_add(1));
            Some(sqe)
        }
    }
    fn read_slot_owned(&self, slot: u32, len: u32) -> Result<Vec<u8>, DriverLinkError> {
        if slot >= self.queue_depth || len > self.max_io_bytes {
            return Err(DriverLinkError::Invalid("slot/len".into()));
        }
        let start = slot as usize * self.max_io_bytes as usize;
        let end = start + len as usize;
        if end > self.data_bytes {
            return Err(DriverLinkError::Invalid("slot range".into()));
        }
        let mut out = vec![0u8; len as usize];
        unsafe {
            ptr::copy_nonoverlapping(self.data.add(start), out.as_mut_ptr(), len as usize);
        }
        Ok(out)
    }
    fn write_slot_from(&mut self, slot: u32, data: &[u8]) -> Result<(), DriverLinkError> {
        if slot >= self.queue_depth || data.len() as u32 > self.max_io_bytes {
            return Err(DriverLinkError::Invalid("slot/len".into()));
        }
        let start = slot as usize * self.max_io_bytes as usize;
        if start + data.len() > self.data_bytes {
            return Err(DriverLinkError::Invalid("slot range".into()));
        }
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), self.data.add(start), data.len());
        }
        Ok(())
    }
    fn push_cqe(&mut self, cqe: Cqe) -> Result<(), DriverLinkError> {
        let hdr = self.cq_hdr();
        unsafe {
            let head = Self::load_idx(ptr::addr_of!((*hdr).head));
            let tail = Self::load_idx(ptr::addr_of!((*hdr).tail));
            let next = tail.wrapping_add(1);
            if next.wrapping_sub(head) > self.queue_depth {
                return Err(DriverLinkError::Full);
            }
            let idx = (tail & self.mask()) as usize;
            let entry_ptr = self.cq.add(size_of::<RingHdr>() + idx * size_of::<Cqe>()) as *mut Cqe;
            ptr::write_unaligned(entry_ptr, cqe);
            Self::store_idx(ptr::addr_of_mut!((*hdr).tail), next);
        }
        Ok(())
    }
}

unsafe fn init_ring_hdr(base: *mut u8, entries: u32) {
    // SAFETY: caller provides a valid VirtualAlloc region of at least RingHdr size.
    unsafe {
        let hdr = base as *mut RingHdr;
        (*hdr).magic = RING_MAGIC;
        (*hdr).entries = entries;
        (*hdr).head = 0;
        (*hdr).tail = 0;
    }
}

fn alloc_region(bytes: usize) -> std::io::Result<*mut u8> {
    if bytes == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "zero alloc",
        ));
    }
    // SAFETY: VirtualAlloc returns page-aligned RW region or null.
    let p = unsafe { VirtualAlloc(ptr::null(), bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
    if p.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // Best-effort lock to reduce page-out during MDL probe.
    unsafe {
        let _ = VirtualLock(p, bytes);
    }
    Ok(p as *mut u8)
}

fn free_region(p: *mut u8, bytes: usize) {
    if p.is_null() {
        return;
    }
    unsafe {
        let _ = VirtualUnlock(p as *mut _, bytes);
        let _ = VirtualFree(p as *mut _, 0, MEM_RELEASE);
    }
}

/// Owns the control device handle and one pending COMMIT_AND_FETCH OVERLAPPED.
pub struct WindowsDriverLink {
    handle: HANDLE,
    event: HANDLE,
    pending: bool,
}

impl WindowsDriverLink {
    pub fn open() -> std::io::Result<Self> {
        let path: Vec<u16> = "\\\\.\\RamSharedCtl\0".encode_utf16().collect();
        // SAFETY: path is NUL-terminated wide string.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            let err = std::io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            return Err(err);
        }
        Ok(Self {
            handle,
            event,
            pending: false,
        })
    }

    pub fn create_disk(&mut self, params: &DiskParams) -> std::io::Result<()> {
        if params.reserved != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "disk reserved non-zero",
            ));
        }
        let bytes = struct_bytes(params);
        self.ioctl_sync(IOCTL_CREATE, Some(&bytes), None)
    }

    pub fn register_queue(&mut self, reg: &Register) -> std::io::Result<()> {
        if reg.reserved != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "register reserved non-zero",
            ));
        }
        if reg.disk_id != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "disk_id must be 0",
            ));
        }
        let bytes = struct_bytes(reg);
        self.ioctl_sync(IOCTL_REGISTER, Some(&bytes), None)
    }

    /// One pending COMMIT_AND_FETCH only (DT-4). Timeout uses CancelIoEx + GetOverlappedResult.
    pub fn commit_and_fetch(&mut self, timeout: Duration) -> Result<(), IoctlError> {
        if self.handle == INVALID_HANDLE_VALUE || self.handle.is_null() {
            return Err(IoctlError::Invalid("invalid handle".into()));
        }
        if self.pending {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "commit already pending",
            ));
        }
        unsafe {
            let _ = ResetEvent(self.event);
        }
        let mut ov: OVERLAPPED = unsafe { zeroed() };
        ov.hEvent = self.event;
        let mut ret = 0u32;
        // SAFETY: handle is open; ov event is valid; zero-input COMMIT.
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_COMMIT,
                ptr::null(),
                0,
                ptr::null_mut(),
                0,
                &mut ret,
                &mut ov,
            )
        };
        if ok != FALSE {
            self.pending = false;
            return Ok(());
        }

        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if err != ERROR_IO_PENDING {
            return Err(IoctlError::Ioctl(format!("COMMIT win32={err}")));
        }

        self.pending = true;
        let ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let wr = unsafe { WaitForSingleObject(self.event, if ms == 0 { INFINITE } else { ms }) };
        if wr == WAIT_TIMEOUT {
            self.cancel_and_drain(&ov);
            return Err(IoctlError::Timeout);
        }
        if wr != WAIT_OBJECT_0 {
            self.cancel_and_drain(&ov);
            return Err(IoctlError::Ioctl(format!("WaitForSingleObject={wr}")));
        }

        let mut xfer = 0u32;
        let gor = unsafe { GetOverlappedResult(self.handle, &ov, &mut xfer, 0) };
        self.pending = false;
        if gor == FALSE {
            return Err(IoctlError::Ioctl(last_error_string("GetOverlappedResult")));
        }

        Ok(())
    }

    pub fn cancel_fetch(&mut self) -> std::io::Result<()> {
        if !self.pending {
            return Ok(());
        }
        // A pending operation must only be cancelled by its owner while its
        // OVERLAPPED remains in scope. commit_and_fetch drains before return.
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "pending fetch cannot be cancelled without its OVERLAPPED owner",
        ))
    }

    pub fn unregister_queue(&mut self) -> std::io::Result<()> {
        self.ioctl_sync(IOCTL_UNREGISTER, None, None)
    }

    pub fn destroy_disk(&mut self) -> std::io::Result<()> {
        self.ioctl_sync(IOCTL_DESTROY, None, None)
    }

    fn ioctl_sync(
        &mut self,
        code: u32,
        input: Option<&[u8]>,
        _output: Option<&mut [u8]>,
    ) -> Result<(), IoctlError> {
        if self.handle == INVALID_HANDLE_VALUE || self.handle.is_null() {
            return Err(IoctlError::Invalid("invalid handle".into()));
        }

        let (in_ptr, in_len) = match input {
            Some(b) => {
                if !(b.as_ptr() as usize).is_multiple_of(8) {
                    return Err(IoctlError::Invalid(
                        "input buffer not 8-byte aligned".into(),
                    ));
                }
                (b.as_ptr() as *const _, b.len() as u32)
            }
            None => (ptr::null(), 0u32),
        };

        unsafe {
            let _ = ResetEvent(self.event);
        }
        let mut ov: OVERLAPPED = unsafe { zeroed() };
        ov.hEvent = self.event;
        let mut ret = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                in_ptr,
                in_len,
                ptr::null_mut(),
                0,
                &mut ret,
                &mut ov,
            )
        };
        if ok != FALSE {
            return Ok(());
        }
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if err != ERROR_IO_PENDING {
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        let wr = unsafe { WaitForSingleObject(self.event, 30_000) };
        if wr != WAIT_OBJECT_0 {
            self.cancel_and_drain(&ov);
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"));
        }
        let mut xfer = 0u32;
        let gor = unsafe { GetOverlappedResult(self.handle, &ov, &mut xfer, 0) };
        if gor == FALSE {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn cancel_and_drain(&mut self, ov: &OVERLAPPED) {
        unsafe {
            let _ = CancelIoEx(self.handle, ov);
            let mut transferred = 0u32;
            // The OVERLAPPED is stack-owned by the caller and must not leave
            // scope until cancellation/completion has been observed.
            let _ = GetOverlappedResult(self.handle, ov, &mut transferred, 1);
        }
        self.pending = false;
    }
}

impl Drop for WindowsDriverLink {
    fn drop(&mut self) {
        if self.pending {
            let _ = self.cancel_fetch();
        }
        if !self.event.is_null() {
            unsafe {
                CloseHandle(self.event);
            }
            self.event = ptr::null_mut();
        }
        if self.handle != INVALID_HANDLE_VALUE && !self.handle.is_null() {
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = INVALID_HANDLE_VALUE;
        }
    }
}

fn struct_bytes<T>(v: &T) -> Vec<u8> {
    let n = size_of::<T>();
    // Standard allocators guarantee sufficient alignment for most structs, but we
    // force it to be 8-byte aligned here to satisfy the guard clause without UB.
    let mut out = vec![0u8; n];

    // Instead of raw parts trickery, we just rely on Vec's standard allocation,
    // which for sizes >= 8 is usually aligned. However, to explicitly force
    // it in tests and avoid the UB of different layouts, we will just use a Vec.
    // In Rust, global allocators align to at least the size of usize (8 bytes on 64-bit),
    // and up to 16 bytes.
    unsafe {
        ptr::copy_nonoverlapping((v as *const T) as *const u8, out.as_mut_ptr(), n);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_handle_guard_clause_rejects_commit() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: ptr::null_mut(),
            pending: false,
        };
        let err = link.commit_and_fetch(Duration::from_millis(1)).unwrap_err();
        match err {
            IoctlError::Invalid(msg) => assert!(msg.contains("invalid handle")),
            _ => panic!("Expected IoctlError::Invalid"),
        }
    }

    #[test]
    fn unaligned_buffer_guard_clause_rejects_ioctl() {
        let mut link = WindowsDriverLink {
            // Using a non-invalid handle so the handle check passes to exercise buffer alignment check.
            handle: 1 as HANDLE,
            event: ptr::null_mut(),
            pending: false,
        };
        // Create an unaligned slice. We allocate a buffer and shift by 1 byte.
        let buf = vec![0u8; 16];
        let unaligned_slice = &buf[1..9];
        assert!(!(unaligned_slice.as_ptr() as usize).is_multiple_of(8));

        let err = link
            .ioctl_sync(IOCTL_REGISTER, Some(unaligned_slice), None)
            .unwrap_err();
        match err {
            IoctlError::Invalid(msg) => assert!(msg.contains("aligned")),
            _ => panic!("Expected IoctlError::Invalid, got {:?}", err),
        }
        // Disarm handle so Drop does not invoke CloseHandle on arbitrary handle value.
        link.handle = INVALID_HANDLE_VALUE;
    }

    #[test]
    fn create_disk_rejects_non_zero_reserved() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: ptr::null_mut(),
            pending: false,
        };
        let params = DiskParams {
            size_bytes: 1024 * 1024,
            block_size: 512,
            reserved: 1,
            serial: [0u8; 16],
        };
        let res = link.create_disk(&params);
        assert!(
            matches!(res, Err(IoctlError::Invalid(msg)) if msg.contains("disk reserved non-zero"))
        );
    }

    #[test]
    fn register_queue_rejects_invalid_params() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: ptr::null_mut(),
            pending: false,
        };

        let mut reg = Register {
            abi_version: ABI_VERSION,
            disk_id: 0,
            queue_depth: 32,
            block_size: 512,
            max_io_bytes: 65536,
            reserved: 1,
            sq_ring_va: 0,
            cq_ring_va: 0,
            data_area_va: 0,
            data_area_len: 0,
            sq_event_handle: 0,
            cq_event_handle: 0,
        };
        let res1 = link.register_queue(&reg);
        assert!(
            matches!(res1, Err(IoctlError::Invalid(msg)) if msg.contains("register reserved non-zero"))
        );

        reg.reserved = 0;
        reg.disk_id = 1;
        let res2 = link.register_queue(&reg);
        assert!(matches!(res2, Err(IoctlError::Invalid(msg)) if msg.contains("disk_id must be 0")));
    }

    #[test]
    fn commit_and_fetch_rejects_already_pending() {
        let mut link = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: ptr::null_mut(),
            pending: true,
        };
        let res = link.commit_and_fetch(Duration::from_millis(100));
        assert!(
            matches!(res, Err(IoctlError::Invalid(msg)) if msg.contains("commit already pending"))
        );
    }

    #[test]
    fn cancel_fetch_state_transitions() {
        let mut link_idle = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: ptr::null_mut(),
            pending: false,
        };
        assert!(link_idle.cancel_fetch().is_ok());

        let mut link_pending = WindowsDriverLink {
            handle: INVALID_HANDLE_VALUE,
            event: ptr::null_mut(),
            pending: true,
        };
        let res = link_pending.cancel_fetch();
        assert!(res.is_err());
    }
}
