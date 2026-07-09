//! Service-side of the SPSC ring protocol (SPEC ITEM-6 / RF-2 / DT-2 / DT-3 / DT-22).
//!
//! Primary wake path: COMMIT_AND_FETCH (pending until SQ has work). Tests use
//! [`FakeDriver`] in process memory — no Windows IOCTL required.

use ramshared_block::BlockBackend;

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

/// In-memory SQ/CQ + bounce data area owned by the service (DT-2).
pub struct QueueMap {
    pub queue_depth: u32,
    pub max_io_bytes: u32,
    pub block_size: u32,
    sq_hdr: RingHdr,
    cq_hdr: RingHdr,
    sq_entries: Vec<Sqe>,
    cq_entries: Vec<Cqe>,
    data: Vec<u8>,
}

impl QueueMap {
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
        let data_len = qd * max_io_bytes as usize;
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
        Ok((start, start + len as usize))
    }

    /// Driver-side: push SQE (tests / FakeDriver).
    pub fn driver_submit(&mut self, sqe: Sqe) -> Result<(), DriverLinkError> {
        let next = self.sq_hdr.tail.wrapping_add(1);
        if next.wrapping_sub(self.sq_hdr.head) > self.queue_depth {
            return Err(DriverLinkError::Full);
        }
        let idx = (self.sq_hdr.tail & self.mask()) as usize;
        self.sq_entries[idx] = sqe;
        // Store-release of entry before advancing tail (DT-22).
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

    /// Service-side: number of pending SQEs.
    pub fn sq_pending(&self) -> u32 {
        self.sq_hdr.tail.wrapping_sub(self.sq_hdr.head)
    }

    fn service_pop_sqe(&mut self) -> Option<Sqe> {
        if self.sq_hdr.head == self.sq_hdr.tail {
            return None;
        }
        let idx = (self.sq_hdr.head & self.mask()) as usize;
        let sqe = self.sq_entries[idx];
        self.sq_hdr.head = self.sq_hdr.head.wrapping_add(1);
        Some(sqe)
    }

    fn service_push_cqe(&mut self, cqe: Cqe) -> Result<(), DriverLinkError> {
        let next = self.cq_hdr.tail.wrapping_add(1);
        if next.wrapping_sub(self.cq_hdr.head) > self.queue_depth {
            return Err(DriverLinkError::Full);
        }
        let idx = (self.cq_hdr.tail & self.mask()) as usize;
        self.cq_entries[idx] = cqe;
        self.cq_hdr.tail = next;
        Ok(())
    }

    fn validate_sqe(&self, sqe: &Sqe) -> Result<(), i32> {
        let bs = self.block_size as u64;
        match sqe.op {
            OP_FLUSH => Ok(()),
            OP_READ | OP_WRITE => {
                if sqe.len == 0 || sqe.len > self.max_io_bytes {
                    return Err(ST_EINVAL);
                }
                if !sqe.offset.is_multiple_of(bs) || !(sqe.len as u64).is_multiple_of(bs) {
                    return Err(ST_EINVAL);
                }
                if sqe.buf_slot >= self.queue_depth {
                    return Err(ST_EINVAL);
                }
                Ok(())
            }
            _ => Err(ST_EINVAL),
        }
    }
}

/// Service handle: owns [`QueueMap`] and runs the single I/O thread loop (DT-3).
pub struct DriverLink {
    pub q: QueueMap,
    stop: bool,
}

impl DriverLink {
    pub fn new(
        queue_depth: u32,
        max_io_bytes: u32,
        block_size: u32,
    ) -> Result<Self, DriverLinkError> {
        Ok(Self {
            q: QueueMap::new(queue_depth, max_io_bytes, block_size)?,
            stop: false,
        })
    }

    pub fn request_stop(&mut self) {
        self.stop = true;
    }

    /// Process one COMMIT_AND_FETCH cycle: drain all pending SQEs against `backend`.
    ///
    /// Returns the number of CQEs posted. Empty SQ → `Ok(0)` (caller may block/wait).
    pub fn commit_and_fetch<B: BlockBackend>(
        &mut self,
        backend: &mut B,
    ) -> Result<u32, DriverLinkError> {
        if self.stop {
            return Err(DriverLinkError::Stopped);
        }
        let mut n = 0u32;
        while let Some(sqe) = self.q.service_pop_sqe() {
            let status = self.serve_one(backend, &sqe);
            self.q.service_push_cqe(Cqe {
                tag: sqe.tag,
                status,
                reserved: 0,
            })?;
            n += 1;
        }
        Ok(n)
    }

    /// Run until `request_stop` or backend fatal (test helper processes up to `max_cycles`).
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
        if let Err(st) = self.q.validate_sqe(sqe) {
            return st;
        }
        // Bounds against backend size (mirrors ramshared_block::validate).
        if sqe.op != OP_FLUSH {
            let end = match sqe.offset.checked_add(sqe.len as u64) {
                Some(e) => e,
                None => return ST_EINVAL,
            };
            if end > backend.size_bytes() {
                return ST_EINVAL;
            }
        }
        match sqe.op {
            OP_READ => {
                let (start, end) = match self.q.slot_slice(sqe.buf_slot, sqe.len) {
                    Ok(r) => r,
                    Err(_) => return ST_EINVAL,
                };
                let buf = &mut self.q.data[start..end];
                match backend.read_at(sqe.offset, buf) {
                    Ok(()) => ST_OK,
                    Err(_) => ST_EIO,
                }
            }
            OP_WRITE => {
                let (start, end) = match self.q.slot_slice(sqe.buf_slot, sqe.len) {
                    Ok(r) => r,
                    Err(_) => return ST_EINVAL,
                };
                let data = &self.q.data[start..end];
                match backend.write_at(sqe.offset, data) {
                    Ok(()) => ST_OK,
                    Err(_) => ST_EIO,
                }
            }
            OP_FLUSH => match backend.flush() {
                Ok(()) => ST_OK,
                Err(_) => ST_EIO,
            },
            _ => ST_EINVAL,
        }
    }
}

/// Fake driver for pure unit tests — posts SQEs and harvests CQEs in-process.
pub struct FakeDriver<'a> {
    link: &'a mut DriverLink,
}

impl<'a> FakeDriver<'a> {
    pub fn new(link: &'a mut DriverLink) -> Self {
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

    struct RamBe {
        data: Vec<u8>,
        bs: u32,
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
        let mut fake = FakeDriver::new(&mut link);
        assert_eq!(fake.harvest().unwrap().unwrap().status, ST_OK);
    }

    #[test]
    fn oob_returns_einval() {
        let mut link = DriverLink::new(4, 4096, 4096).unwrap();
        let mut be = RamBe {
            data: vec![0u8; 8192],
            bs: 4096,
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
}
