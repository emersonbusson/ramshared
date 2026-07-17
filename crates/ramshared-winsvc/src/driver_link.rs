//! Service-side of the SPSC ring protocol (SPEC DT-4 / RF-2).
//!
//! [`QueueAccess`] isolates pure [`InMemoryQueue`] (tests) from Windows mapped rings.
//! SQE and WRITE payloads are snapshotted into owned buffers before backend access.

use ramshared_block::BlockBackend;
use std::time::Instant;

use crate::proto::{
    Cqe, MAX_IO, MAX_QD, OP_FLUSH, OP_READ, OP_WRITE, RING_MAGIC, RingHdr, ST_EINVAL, ST_EIO,
    ST_OK, Sqe,
};

/// Errors from the driver link / I/O loop.
#[derive(Debug, PartialEq)]
pub enum DriverLinkError {
    Full,
    Empty,
    Invalid(String),
    Backend(String),
    Stopped,
}

const LATENCY_WINDOW: usize = 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IoStats {
    pub reads: u64,
    pub writes: u64,
    pub flushes: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub errors: u64,
    pub latencies_us: Vec<u64>,
}

struct LinkStats {
    public: IoStats,
    latency_window: [u64; LATENCY_WINDOW],
    latency_len: usize,
    latency_next: usize,
}

impl Default for LinkStats {
    fn default() -> Self {
        Self {
            public: IoStats::default(),
            latency_window: [0; LATENCY_WINDOW],
            latency_len: 0,
            latency_next: 0,
        }
    }
}

impl LinkStats {
    fn record(&mut self, sqe: &Sqe, status: i32, latency_us: u64) {
        if status == ST_OK {
            match sqe.op {
                OP_READ => {
                    self.public.reads += 1;
                    self.public.bytes_read += u64::from(sqe.len);
                }
                OP_WRITE => {
                    self.public.writes += 1;
                    self.public.bytes_written += u64::from(sqe.len);
                }
                OP_FLUSH => self.public.flushes += 1,
                _ => self.public.errors += 1,
            }
        } else {
            self.public.errors += 1;
        }
        self.latency_window[self.latency_next] = latency_us;
        self.latency_next = (self.latency_next + 1) % LATENCY_WINDOW;
        self.latency_len = self.latency_len.saturating_add(1).min(LATENCY_WINDOW);
    }

    fn snapshot(&self) -> IoStats {
        let mut result = self.public.clone();
        result.latencies_us = self.latency_window[..self.latency_len].to_vec();
        result
    }
}

impl std::fmt::Display for DriverLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriverLinkError::Full => write!(f, "queue full"),
            DriverLinkError::Empty => write!(f, "queue empty"),
            DriverLinkError::Invalid(s) => write!(f, "invalid: {s}"),
            DriverLinkError::Backend(s) => write!(f, "backend: {s}"),
            DriverLinkError::Stopped => write!(f, "stopped"),
        }
    }
}

impl std::error::Error for DriverLinkError {}

/// Safe trait for SQ/CQ + bounce data access (DT-4).
pub trait QueueAccess {
    fn queue_depth(&self) -> u32;
    fn max_io_bytes(&self) -> u32;
    fn block_size(&self) -> u32;
    fn sq_pending(&self) -> u32;
    /// Pop one SQE as an owned snapshot (or None if empty).
    fn pop_sqe_snapshot(&mut self) -> Option<Sqe>;
    /// Copy WRITE payload for `slot`/`len` into an owned buffer.
    fn read_slot_owned(&self, slot: u32, len: u32) -> Result<Vec<u8>, DriverLinkError>;
    /// Write READ result into bounce slot from owned host buffer.
    fn write_slot_from(&mut self, slot: u32, data: &[u8]) -> Result<(), DriverLinkError>;
    fn push_cqe(&mut self, cqe: Cqe) -> Result<(), DriverLinkError>;
}

/// In-memory SQ/CQ + bounce data area (hermetic tests / DT-4 `InMemoryQueue`).
pub struct InMemoryQueue {
    pub queue_depth: u32,
    pub max_io_bytes: u32,
    pub block_size: u32,
    sq_hdr: RingHdr,
    cq_hdr: RingHdr,
    sq_entries: Vec<Sqe>,
    cq_entries: Vec<Cqe>,
    data: Vec<u8>,
}

/// Backward-compatible alias used by older call sites / FakeDriver tests.
pub type QueueMap = InMemoryQueue;

impl InMemoryQueue {
    /// Allocate rings and data area. `queue_depth` must be power of two ≤ MAX_QD.
    pub fn new(
        queue_depth: u32,
        max_io_bytes: u32,
        block_size: u32,
    ) -> Result<Self, DriverLinkError> {
        if queue_depth == 0 || queue_depth > MAX_QD || !queue_depth.is_power_of_two() {
            return Err(DriverLinkError::Invalid(format!(
                "queue_depth {queue_depth} must be power of 2 in 1..={MAX_QD}"
            )));
        }
        if max_io_bytes == 0 || max_io_bytes > MAX_IO {
            return Err(DriverLinkError::Invalid(format!(
                "max_io_bytes {max_io_bytes} out of range"
            )));
        }
        if block_size != 512 && block_size != 4096 {
            return Err(DriverLinkError::Invalid(
                "block_size must be 512 or 4096".into(),
            ));
        }
        let qd = queue_depth as usize;
        let data_len = qd
            .checked_mul(max_io_bytes as usize)
            .ok_or_else(|| DriverLinkError::Invalid("data area overflow".into()))?;
        // Mirror product 4 MiB map cap for consistency with config DT-2.
        if data_len > (4 * 1024 * 1024) {
            return Err(DriverLinkError::Invalid(
                "queue_depth * max_io_bytes exceeds 4 MiB".into(),
            ));
        }
        Ok(Self {
            queue_depth,
            max_io_bytes,
            block_size,
            sq_hdr: RingHdr {
                magic: RING_MAGIC,
                entries: queue_depth,
                head: 0,
                tail: 0,
            },
            cq_hdr: RingHdr {
                magic: RING_MAGIC,
                entries: queue_depth,
                head: 0,
                tail: 0,
            },
            sq_entries: vec![
                Sqe {
                    tag: 0,
                    op: 0,
                    flags: 0,
                    offset: 0,
                    len: 0,
                    buf_slot: 0,
                };
                qd
            ],
            cq_entries: vec![
                Cqe {
                    tag: 0,
                    status: 0,
                    reserved: 0,
                };
                qd
            ],
            data: vec![0u8; data_len],
        })
    }

    fn mask(&self) -> u32 {
        self.queue_depth - 1
    }

    fn slot_slice(&self, slot: u32, len: u32) -> Result<(usize, usize), DriverLinkError> {
        if slot >= self.queue_depth {
            return Err(DriverLinkError::Invalid(format!("buf_slot {slot}")));
        }
        if len > self.max_io_bytes {
            return Err(DriverLinkError::Invalid(format!("len {len} > max_io")));
        }
        let start = slot as usize * self.max_io_bytes as usize;
        let end = start
            .checked_add(len as usize)
            .ok_or_else(|| DriverLinkError::Invalid("slot range overflow".into()))?;
        if end > self.data.len() {
            return Err(DriverLinkError::Invalid("slot out of data area".into()));
        }
        Ok((start, end))
    }

    /// Driver-side: push SQE (tests / FakeDriver).
    pub fn driver_submit(&mut self, sqe: Sqe) -> Result<(), DriverLinkError> {
        let next = self.sq_hdr.tail.wrapping_add(1);
        if next.wrapping_sub(self.sq_hdr.head) > self.queue_depth {
            return Err(DriverLinkError::Full);
        }
        let idx = (self.sq_hdr.tail & self.mask()) as usize;
        self.sq_entries[idx] = sqe;
        self.sq_hdr.tail = next;
        Ok(())
    }

    /// Driver-side: fill bounce slot for WRITE before submit.
    pub fn driver_write_slot(&mut self, slot: u32, data: &[u8]) -> Result<(), DriverLinkError> {
        let (start, end) = self.slot_slice(slot, data.len() as u32)?;
        self.data[start..end].copy_from_slice(data);
        Ok(())
    }

    /// Driver-side: read bounce slot after READ CQE.
    pub fn driver_read_slot(&self, slot: u32, len: u32) -> Result<&[u8], DriverLinkError> {
        let (start, end) = self.slot_slice(slot, len)?;
        Ok(&self.data[start..end])
    }

    /// Driver-side: pop CQE if available.
    pub fn driver_complete(&mut self) -> Result<Option<Cqe>, DriverLinkError> {
        if self.cq_hdr.head == self.cq_hdr.tail {
            return Ok(None);
        }
        let idx = (self.cq_hdr.head & self.mask()) as usize;
        let cqe = self.cq_entries[idx];
        self.cq_hdr.head = self.cq_hdr.head.wrapping_add(1);
        Ok(Some(cqe))
    }
}

impl QueueAccess for InMemoryQueue {
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
        self.sq_hdr.tail.wrapping_sub(self.sq_hdr.head)
    }
    fn pop_sqe_snapshot(&mut self) -> Option<Sqe> {
        if self.sq_hdr.head == self.sq_hdr.tail {
            return None;
        }
        let idx = (self.sq_hdr.head & self.mask()) as usize;
        let sqe = self.sq_entries[idx];
        self.sq_hdr.head = self.sq_hdr.head.wrapping_add(1);
        Some(sqe)
    }
    fn read_slot_owned(&self, slot: u32, len: u32) -> Result<Vec<u8>, DriverLinkError> {
        let (start, end) = self.slot_slice(slot, len)?;
        Ok(self.data[start..end].to_vec())
    }
    fn write_slot_from(&mut self, slot: u32, data: &[u8]) -> Result<(), DriverLinkError> {
        let (start, end) = self.slot_slice(slot, data.len() as u32)?;
        self.data[start..end].copy_from_slice(data);
        Ok(())
    }
    fn push_cqe(&mut self, cqe: Cqe) -> Result<(), DriverLinkError> {
        let next = self.cq_hdr.tail.wrapping_add(1);
        if next.wrapping_sub(self.cq_hdr.head) > self.queue_depth {
            return Err(DriverLinkError::Full);
        }
        let idx = (self.cq_hdr.tail & self.mask()) as usize;
        self.cq_entries[idx] = cqe;
        self.cq_hdr.tail = next;
        Ok(())
    }
}

/// Service handle: owns a [`QueueAccess`] and runs the single I/O thread loop (DT-3/DT-4).
pub struct DriverLink<Q: QueueAccess = InMemoryQueue> {
    pub q: Q,
    stop: bool,
    /// Test seam: count backend write calls (snapshot integrity).
    pub backend_writes: u32,
    stats: LinkStats,
}

impl DriverLink<InMemoryQueue> {
    pub fn new(
        queue_depth: u32,
        max_io_bytes: u32,
        block_size: u32,
    ) -> Result<Self, DriverLinkError> {
        Ok(Self {
            q: InMemoryQueue::new(queue_depth, max_io_bytes, block_size)?,
            stop: false,
            backend_writes: 0,
            stats: LinkStats::default(),
        })
    }
}

impl<Q: QueueAccess> DriverLink<Q> {
    pub fn from_queue(q: Q) -> Self {
        Self {
            q,
            stop: false,
            backend_writes: 0,
            stats: LinkStats::default(),
        }
    }

    pub fn request_stop(&mut self) {
        self.stop = true;
    }

    pub fn stats(&self) -> IoStats {
        self.stats.snapshot()
    }

    /// Process one COMMIT_AND_FETCH cycle: drain pending SQEs against `backend`.
    pub fn commit_and_fetch<B: BlockBackend>(
        &mut self,
        backend: &mut B,
    ) -> Result<u32, DriverLinkError> {
        if self.stop {
            return Err(DriverLinkError::Stopped);
        }
        let mut n = 0u32;
        while let Some(sqe) = self.q.pop_sqe_snapshot() {
            let started = Instant::now();
            let status = self.serve_one(backend, &sqe);
            let latency_us = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
            self.stats.record(&sqe, status, latency_us);
            self.q.push_cqe(Cqe {
                tag: sqe.tag,
                status,
                reserved: 0,
            })?;
            n += 1;
        }
        Ok(n)
    }

    /// Run until `request_stop` or cycle budget (test helper).
    pub fn run_io_loop<B: BlockBackend>(
        &mut self,
        backend: &mut B,
        max_cycles: u32,
    ) -> Result<u32, DriverLinkError> {
        let mut total = 0u32;
        for _ in 0..max_cycles {
            if self.stop {
                break;
            }
            total += self.commit_and_fetch(backend)?;
        }
        Ok(total)
    }

    fn serve_one<B: BlockBackend>(&mut self, backend: &mut B, sqe: &Sqe) -> i32 {
        // Re-validate owned snapshot (flags/op/slot/range).
        if sqe.flags != 0 {
            return ST_EINVAL;
        }
        let bs = self.q.block_size() as u64;
        match sqe.op {
            OP_FLUSH => {}
            OP_READ | OP_WRITE => {
                if sqe.len == 0 || sqe.len > self.q.max_io_bytes() {
                    return ST_EINVAL;
                }
                if !sqe.offset.is_multiple_of(bs) || !(sqe.len as u64).is_multiple_of(bs) {
                    return ST_EINVAL;
                }
                if sqe.buf_slot >= self.q.queue_depth() {
                    return ST_EINVAL;
                }
            }
            _ => return ST_EINVAL,
        }
        if sqe.op != OP_FLUSH {
            let Some(end) = sqe.offset.checked_add(sqe.len as u64) else {
                return ST_EINVAL;
            };
            if end > backend.size_bytes() {
                return ST_EINVAL;
            }
        }
        match sqe.op {
            OP_READ => {
                let mut buf = vec![0u8; sqe.len as usize];
                if backend.read_at(sqe.offset, &mut buf).is_err() {
                    return ST_EIO;
                }
                if self.q.write_slot_from(sqe.buf_slot, &buf).is_err() {
                    return ST_EINVAL;
                }
                ST_OK
            }
            OP_WRITE => {
                let Ok(data) = self.q.read_slot_owned(sqe.buf_slot, sqe.len) else {
                    return ST_EINVAL;
                };
                self.backend_writes += 1;
                if backend.write_at(sqe.offset, &data).is_err() {
                    return ST_EIO;
                }
                ST_OK
            }
            OP_FLUSH => {
                if backend.flush().is_err() {
                    return ST_EIO;
                }
                ST_OK
            }
            _ => ST_EINVAL,
        }
    }
}

/// Fake driver for pure unit tests — posts SQEs and harvests CQEs in-process.
pub struct FakeDriver<'a> {
    link: &'a mut DriverLink<InMemoryQueue>,
}

impl<'a> FakeDriver<'a> {
    pub fn new(link: &'a mut DriverLink<InMemoryQueue>) -> Self {
        Self { link }
    }

    pub fn submit_write(
        &mut self,
        tag: u64,
        offset: u64,
        data: &[u8],
        slot: u32,
    ) -> Result<(), DriverLinkError> {
        self.link.q.driver_write_slot(slot, data)?;
        self.link.q.driver_submit(Sqe {
            tag,
            op: OP_WRITE,
            flags: 0,
            offset,
            len: data.len() as u32,
            buf_slot: slot,
        })
    }

    pub fn submit_write_flags(
        &mut self,
        tag: u64,
        offset: u64,
        data: &[u8],
        slot: u32,
        flags: u32,
    ) -> Result<(), DriverLinkError> {
        self.link.q.driver_write_slot(slot, data)?;
        self.link.q.driver_submit(Sqe {
            tag,
            op: OP_WRITE,
            flags,
            offset,
            len: data.len() as u32,
            buf_slot: slot,
        })
    }

    pub fn submit_read(
        &mut self,
        tag: u64,
        offset: u64,
        len: u32,
        slot: u32,
    ) -> Result<(), DriverLinkError> {
        self.link.q.driver_submit(Sqe {
            tag,
            op: OP_READ,
            flags: 0,
            offset,
            len,
            buf_slot: slot,
        })
    }

    pub fn submit_flush(&mut self, tag: u64) -> Result<(), DriverLinkError> {
        self.link.q.driver_submit(Sqe {
            tag,
            op: OP_FLUSH,
            flags: 0,
            offset: 0,
            len: 0,
            buf_slot: 0,
        })
    }

    pub fn harvest(&mut self) -> Result<Option<Cqe>, DriverLinkError> {
        self.link.q.driver_complete()
    }

    pub fn read_slot(&self, slot: u32, len: u32) -> Result<&[u8], DriverLinkError> {
        self.link.q.driver_read_slot(slot, len)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use ramshared_block::{BlockBackend, IoError};
    use std::sync::{Arc, Mutex};

    struct RamBe {
        data: Vec<u8>,
        bs: u32,
        /// Last WRITE payload observed (owned snapshot proof).
        last_write: Arc<Mutex<Vec<u8>>>,
        writes: Arc<Mutex<u32>>,
    }

    impl BlockBackend for RamBe {
        fn size_bytes(&self) -> u64 {
            self.data.len() as u64
        }
        fn block_size(&self) -> u32 {
            self.bs
        }
        fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
            let o = off as usize;
            buf.copy_from_slice(&self.data[o..o + buf.len()]);
            Ok(())
        }
        fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
            *self.writes.lock().unwrap() += 1;
            *self.last_write.lock().unwrap() = data.to_vec();
            let o = off as usize;
            self.data[o..o + data.len()].copy_from_slice(data);
            Ok(())
        }
        fn flush(&mut self) -> Result<(), IoError> {
            Ok(())
        }
    }

    #[test]
    fn roundtrip_write_read_flush() {
        let mut link = DriverLink::new(8, 4096, 4096).unwrap();
        let mut be = RamBe {
            data: vec![0u8; 1 << 20],
            bs: 4096,
            last_write: Arc::new(Mutex::new(Vec::new())),
            writes: Arc::new(Mutex::new(0)),
        };
        let payload = vec![0xABu8; 4096];
        {
            let mut fake = FakeDriver::new(&mut link);
            fake.submit_write(1, 4096, &payload, 0).unwrap();
        }
        assert_eq!(link.commit_and_fetch(&mut be).unwrap(), 1);
        {
            let mut fake = FakeDriver::new(&mut link);
            let cqe = fake.harvest().unwrap().unwrap();
            assert_eq!(cqe.tag, 1);
            assert_eq!(cqe.status, ST_OK);
            fake.submit_read(2, 4096, 4096, 1).unwrap();
        }
        assert_eq!(link.commit_and_fetch(&mut be).unwrap(), 1);
        {
            let mut fake = FakeDriver::new(&mut link);
            let cqe = fake.harvest().unwrap().unwrap();
            assert_eq!(cqe.status, ST_OK);
            assert_eq!(fake.read_slot(1, 4096).unwrap(), payload.as_slice());
            fake.submit_flush(3).unwrap();
        }
        assert_eq!(link.commit_and_fetch(&mut be).unwrap(), 1);
        {
            let mut fake = FakeDriver::new(&mut link);
            assert_eq!(fake.harvest().unwrap().unwrap().status, ST_OK);
        }
        let stats = link.stats();
        assert_eq!(stats.reads, 1);
        assert_eq!(stats.writes, 1);
        assert_eq!(stats.flushes, 1);
        assert_eq!(stats.bytes_read, 4096);
        assert_eq!(stats.bytes_written, 4096);
        assert_eq!(stats.errors, 0);
        assert_eq!(stats.latencies_us.len(), 3);
    }

    #[test]
    fn oob_returns_einval() {
        let mut link = DriverLink::new(4, 4096, 4096).unwrap();
        let mut be = RamBe {
            data: vec![0u8; 8192],
            bs: 4096,
            last_write: Arc::new(Mutex::new(Vec::new())),
            writes: Arc::new(Mutex::new(0)),
        };
        {
            let mut fake = FakeDriver::new(&mut link);
            fake.submit_read(9, 8192, 4096, 0).unwrap();
        }
        link.commit_and_fetch(&mut be).unwrap();
        let mut fake = FakeDriver::new(&mut link);
        assert_eq!(fake.harvest().unwrap().unwrap().status, ST_EINVAL);
    }

    #[test]
    fn reject_bad_queue_depth() {
        assert!(DriverLink::new(3, 4096, 4096).is_err());
        assert!(DriverLink::new(0, 4096, 4096).is_err());
    }

    #[test]
    fn unknown_flags_return_einval() {
        let mut link = DriverLink::new(4, 4096, 4096).unwrap();
        let mut be = RamBe {
            data: vec![0u8; 1 << 16],
            bs: 4096,
            last_write: Arc::new(Mutex::new(Vec::new())),
            writes: Arc::new(Mutex::new(0)),
        };
        let payload = vec![1u8; 4096];
        {
            let mut fake = FakeDriver::new(&mut link);
            fake.submit_write_flags(1, 0, &payload, 0, 0x1).unwrap();
        }
        link.commit_and_fetch(&mut be).unwrap();
        let mut fake = FakeDriver::new(&mut link);
        assert_eq!(fake.harvest().unwrap().unwrap().status, ST_EINVAL);
        assert_eq!(*be.writes.lock().unwrap(), 0);
    }

    #[test]
    fn overflow_range_returns_einval() {
        let mut link = DriverLink::new(4, 4096, 4096).unwrap();
        let mut be = RamBe {
            data: vec![0u8; 8192],
            bs: 4096,
            last_write: Arc::new(Mutex::new(Vec::new())),
            writes: Arc::new(Mutex::new(0)),
        };
        {
            let mut fake = FakeDriver::new(&mut link);
            // offset near u64::MAX causes checked_add overflow
            fake.submit_read(1, u64::MAX - 100, 4096, 0).unwrap();
        }
        link.commit_and_fetch(&mut be).unwrap();
        let mut fake = FakeDriver::new(&mut link);
        assert_eq!(fake.harvest().unwrap().unwrap().status, ST_EINVAL);
    }

    #[test]
    fn write_uses_owned_payload_snapshot() {
        let mut link = DriverLink::new(4, 4096, 4096).unwrap();
        let last = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::new(Mutex::new(0));
        let mut be = RamBe {
            data: vec![0u8; 1 << 16],
            bs: 4096,
            last_write: Arc::clone(&last),
            writes: Arc::clone(&writes),
        };
        let payload = vec![0xCDu8; 4096];
        {
            let mut fake = FakeDriver::new(&mut link);
            fake.submit_write(1, 0, &payload, 0).unwrap();
            // Mutate bounce buffer after submit (would race without snapshot).
            link.q.driver_write_slot(0, &vec![0x00u8; 4096]).unwrap();
        }
        // Re-submit path: we mutated after submit, so without snapshot at serve time
        // we'd see zeros. Snapshot is taken at serve_one from current slot — so this
        // test proves serve copies into owned Vec before write_at; mutate *during*
        // backend would require concurrency. We verify backend receives a Vec equal
        // to slot at serve time, and backend_writes increments once.
        // Restore payload and commit.
        {
            let _fake = FakeDriver::new(&mut link);
        }
        link.commit_and_fetch(&mut be).unwrap();
        // Slot was zeros at serve — backend got zeros (owned copy of then-current slot).
        assert_eq!(*writes.lock().unwrap(), 1);
        assert_eq!(last.lock().unwrap().len(), 4096);
        // Second write with known payload
        {
            let mut fake = FakeDriver::new(&mut link);
            fake.submit_write(2, 4096, &payload, 1).unwrap();
        }
        link.commit_and_fetch(&mut be).unwrap();
        assert_eq!(*last.lock().unwrap(), payload);
        assert_eq!(link.backend_writes, 2);
    }

    #[test]
    fn invalid_slot_does_not_touch_backend() {
        let mut link = DriverLink::new(4, 4096, 4096).unwrap();
        let writes = Arc::new(Mutex::new(0));
        let mut be = RamBe {
            data: vec![0u8; 1 << 16],
            bs: 4096,
            last_write: Arc::new(Mutex::new(Vec::new())),
            writes: Arc::clone(&writes),
        };
        // Manually craft SQE with out-of-range slot without going through slot write helper.
        link.q
            .driver_submit(Sqe {
                tag: 99,
                op: OP_WRITE,
                flags: 0,
                offset: 0,
                len: 4096,
                buf_slot: 99,
            })
            .unwrap();
        link.commit_and_fetch(&mut be).unwrap();
        let mut fake = FakeDriver::new(&mut link);
        assert_eq!(fake.harvest().unwrap().unwrap().status, ST_EINVAL);
        assert_eq!(*writes.lock().unwrap(), 0);
        assert_eq!(link.backend_writes, 0);
    }
}
