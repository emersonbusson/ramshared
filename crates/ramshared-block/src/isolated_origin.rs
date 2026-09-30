//! Authoritative origin I/O with a bounded, revocable cache boundary.
//!
//! The origin never depends on a cache response for correctness. Cache reads
//! have a hard deadline; cache mutations are non-blocking and any queue,
//! transport, protocol, or timeout fault permanently revokes that client.

use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::Duration;

use crate::origin_cache::{CacheState, CacheTelemetry, OriginState, OriginStorage};
use crate::{BlockBackend, IoError, WriteOptions};
use ramshared_vram::{GpuBudgetTelemetry, WorkerCacheTelemetry};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheRead {
    Hit,
    Miss,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheMutation {
    /// The cache implementation accepted the mutation. For the process-isolated
    /// GPU cache this means the complete frame was queued to the worker; it does
    /// not prove that the worker applied it or allocated VRAM. Origin data stays
    /// authoritative regardless of this result.
    Accepted,
    Skipped,
    Failed,
}

/// Cache-side data messages never carry the origin file handle.
#[derive(Debug)]
pub enum IsolatedCacheRequest {
    Read {
        offset: u64,
        len: usize,
        reply: SyncSender<Result<Option<Vec<u8>>, String>>,
    },
    Update {
        offset: u64,
        data: Vec<u8>,
    },
    Promote {
        offset: u64,
        data: Vec<u8>,
    },
}

/// Revocation uses a dedicated lane so a saturated data queue cannot conceal
/// release. The worker acknowledges only after its cache resources are gone.
#[derive(Debug)]
pub enum IsolatedCacheControl {
    Disable {
        reply: SyncSender<Result<(), String>>,
    },
}

pub struct IsolatedCacheWorker {
    pub requests: Receiver<IsolatedCacheRequest>,
    pub control: Receiver<IsolatedCacheControl>,
}

/// Largest payload a single `BestEffortCache::update` / `promote` call carries.
///
/// The GPU cache transport frames each mutation as one nonblocking send, so the
/// origin write path must split any larger write into consecutive slices of at
/// most this size. A block-layer request can be far larger (the NBD queue on
/// this surface admits up to 4 MiB), and forwarding one of those whole would
/// permanently revoke the cache on the first frame that does not fit.
pub const MAX_CACHE_MUTATION_BYTES: usize = 64 * 1024;

/// Split a logical cache mutation into consecutive transport-sized frames.
///
/// Yields `(offset, slice)` pairs covering `data` exactly, each slice at most
/// `MAX_CACHE_MUTATION_BYTES`. Empty `data` yields nothing: a zero-length
/// mutation is a no-op and must not occupy a frame.
fn mutation_frames(offset: u64, data: &[u8]) -> impl Iterator<Item = (u64, &[u8])> {
    data.chunks(MAX_CACHE_MUTATION_BYTES)
        .enumerate()
        .map(move |(index, slice)| {
            let slice_offset = offset
                .saturating_add((index as u64).saturating_mul(MAX_CACHE_MUTATION_BYTES as u64));
            (slice_offset, slice)
        })
}

pub trait BestEffortCache {
    fn read(&mut self, offset: u64, destination: &mut [u8]) -> CacheRead;
    fn update(&mut self, offset: u64, data: &[u8]) -> CacheMutation;
    fn promote(&mut self, offset: u64, data: &[u8]) -> CacheMutation;
    fn disable(&mut self) -> CacheMutation;
    fn state(&self) -> CacheState;

    fn cached_bytes(&self) -> u64 {
        0
    }

    fn refresh_cached_bytes(&mut self) -> Result<u64, &'static str> {
        Ok(self.cached_bytes())
    }

    fn target_bytes(&self) -> u64 {
        0
    }

    fn gpu_budget_telemetry(&self) -> Option<&GpuBudgetTelemetry> {
        None
    }

    /// Cache occupancy and codec health from the last heartbeat (DT-8).
    ///
    /// Logical cache bytes are cache occupancy in the worker's address space —
    /// never guest or host RAM.
    fn cache_telemetry(&self) -> Option<&WorkerCacheTelemetry> {
        None
    }
}

/// Fail-closed cache used until a separately supervised GPU worker is wired.
#[derive(Default)]
pub struct DisabledCache;

impl BestEffortCache for DisabledCache {
    fn read(&mut self, _offset: u64, _destination: &mut [u8]) -> CacheRead {
        CacheRead::Miss
    }

    fn update(&mut self, _offset: u64, _data: &[u8]) -> CacheMutation {
        CacheMutation::Skipped
    }

    fn promote(&mut self, _offset: u64, _data: &[u8]) -> CacheMutation {
        CacheMutation::Skipped
    }

    fn disable(&mut self) -> CacheMutation {
        CacheMutation::Accepted
    }

    fn state(&self) -> CacheState {
        CacheState::Unavailable
    }
}

/// Bounded client for an isolated cache worker. It never performs a blocking
/// send. Reads and release acknowledgements wait for `read_timeout` at most.
pub struct BoundedCacheClient {
    requests: SyncSender<IsolatedCacheRequest>,
    control: SyncSender<IsolatedCacheControl>,
    read_timeout: Duration,
    state: CacheState,
}

pub fn isolated_cache_channel(
    capacity: usize,
    read_timeout: Duration,
) -> (BoundedCacheClient, IsolatedCacheWorker) {
    let (requests, receiver) = sync_channel(capacity);
    let (control, control_receiver) = sync_channel(1);
    (
        BoundedCacheClient {
            requests,
            control,
            read_timeout,
            state: CacheState::Active,
        },
        IsolatedCacheWorker {
            requests: receiver,
            control: control_receiver,
        },
    )
}

impl BoundedCacheClient {
    fn fail(&mut self) -> CacheMutation {
        self.state = CacheState::Unavailable;
        CacheMutation::Failed
    }

    fn send_mutation(&mut self, request: IsolatedCacheRequest) -> CacheMutation {
        if self.state != CacheState::Active {
            return CacheMutation::Skipped;
        }
        match self.requests.try_send(request) {
            Ok(()) => CacheMutation::Accepted,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => self.fail(),
        }
    }
}

impl BestEffortCache for BoundedCacheClient {
    fn read(&mut self, offset: u64, destination: &mut [u8]) -> CacheRead {
        if self.state != CacheState::Active {
            return CacheRead::Miss;
        }
        let (reply, response) = sync_channel(1);
        let request = IsolatedCacheRequest::Read {
            offset,
            len: destination.len(),
            reply,
        };
        if self.requests.try_send(request).is_err() {
            self.fail();
            return CacheRead::Failed;
        }
        match response.recv_timeout(self.read_timeout) {
            Ok(Ok(Some(bytes))) if bytes.len() == destination.len() => {
                destination.copy_from_slice(&bytes);
                CacheRead::Hit
            }
            Ok(Ok(None)) => CacheRead::Miss,
            Ok(Ok(Some(_)))
            | Ok(Err(_))
            | Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.fail();
                CacheRead::Failed
            }
        }
    }

    fn update(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
        self.send_mutation(IsolatedCacheRequest::Update {
            offset,
            data: data.to_vec(),
        })
    }

    fn promote(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
        self.send_mutation(IsolatedCacheRequest::Promote {
            offset,
            data: data.to_vec(),
        })
    }

    fn disable(&mut self) -> CacheMutation {
        if self.state == CacheState::Off {
            return CacheMutation::Skipped;
        }
        let (reply, acknowledgement) = sync_channel(1);
        if self
            .control
            .try_send(IsolatedCacheControl::Disable { reply })
            .is_err()
        {
            self.state = CacheState::Stuck;
            return CacheMutation::Failed;
        }
        match acknowledgement.recv_timeout(self.read_timeout) {
            Ok(Ok(())) => {
                self.state = CacheState::Off;
                CacheMutation::Accepted
            }
            Ok(Err(_))
            | Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.state = CacheState::Stuck;
                CacheMutation::Failed
            }
        }
    }

    fn state(&self) -> CacheState {
        self.state
    }
}

pub struct AuthoritativeOriginBackend<O, C> {
    origin: O,
    cache: C,
    size: u64,
    block: u32,
    telemetry: CacheTelemetry,
    origin_state: OriginState,
    origin_probe_successes: u8,
    origin_dirty: bool,
}

impl<O: OriginStorage, C: BestEffortCache> AuthoritativeOriginBackend<O, C> {
    pub fn new(origin: O, cache: C, size: u64, block: u32) -> Result<Self, IoError> {
        if size == 0 || block == 0 || !size.is_multiple_of(block as u64) {
            return Err(IoError("invalid authoritative origin geometry".into()));
        }
        Ok(Self {
            origin,
            cache,
            size,
            block,
            telemetry: CacheTelemetry::default(),
            origin_state: OriginState::Ready,
            origin_probe_successes: 0,
            origin_dirty: false,
        })
    }

    pub fn origin_state(&self) -> OriginState {
        self.origin_state
    }

    pub fn cache_state(&self) -> CacheState {
        self.cache.state()
    }

    pub fn cached_bytes(&self) -> u64 {
        self.cache.cached_bytes()
    }

    pub fn refresh_cached_bytes(&mut self) -> Result<u64, &'static str> {
        self.cache.refresh_cached_bytes()
    }

    pub fn target_bytes(&self) -> u64 {
        self.cache.target_bytes()
    }

    pub fn gpu_budget_telemetry(&self) -> Option<&GpuBudgetTelemetry> {
        self.cache.gpu_budget_telemetry()
    }

    /// Cache occupancy and codec health from the underlying cache client.
    pub fn cache_telemetry(&self) -> Option<&WorkerCacheTelemetry> {
        self.cache.cache_telemetry()
    }

    pub fn telemetry(&self) -> CacheTelemetry {
        self.telemetry
    }

    pub fn release_cache(&mut self) -> Result<u64, IoError> {
        match self.revoke_cache() {
            CacheMutation::Accepted | CacheMutation::Skipped => {
                self.telemetry.releases = self.telemetry.releases.saturating_add(1);
                Ok(0)
            }
            CacheMutation::Failed => Err(IoError(
                "cache release acknowledgement was unavailable".into(),
            )),
        }
    }

    pub fn probe_origin(&mut self) -> Result<OriginState, IoError> {
        let mut block = vec![0; self.block as usize];
        let result = self
            .origin
            .read_exact_at(0, &mut block)
            .and_then(|()| self.origin.sync_data());
        if let Err(error) = result {
            self.mark_origin_failed();
            return Err(error);
        }
        self.origin_dirty = false;
        self.telemetry.origin_syncs = self.telemetry.origin_syncs.saturating_add(1);
        if matches!(
            self.origin_state,
            OriginState::Failed | OriginState::Degraded
        ) {
            self.origin_probe_successes = self.origin_probe_successes.saturating_add(1);
            if self.origin_probe_successes >= 3 {
                self.origin_state = OriginState::Ready;
                self.origin_probe_successes = 0;
            } else {
                self.origin_state = OriginState::Degraded;
            }
        }
        Ok(self.origin_state)
    }

    fn check_range(&self, offset: u64, len: usize) -> Result<(), IoError> {
        offset
            .checked_add(len as u64)
            .filter(|end| *end <= self.size)
            .map(|_| ())
            .ok_or_else(|| IoError("authoritative origin I/O is out of range".into()))
    }

    fn require_ready_origin(&self) -> Result<(), IoError> {
        if self.origin_state == OriginState::Ready {
            Ok(())
        } else {
            Err(IoError(
                "origin authority is unavailable pending three read+sync probes".into(),
            ))
        }
    }

    fn revoke_cache(&mut self) -> CacheMutation {
        if !matches!(
            self.cache.state(),
            CacheState::Off | CacheState::Unavailable
        ) {
            self.telemetry.invalidations = self.telemetry.invalidations.saturating_add(1);
        }
        self.cache.disable()
    }

    fn mark_origin_failed(&mut self) {
        self.origin_state = OriginState::Failed;
        self.origin_probe_successes = 0;
        let _ = self.revoke_cache();
    }

    fn write_origin(&mut self, offset: u64, data: &[u8]) -> Result<(), IoError> {
        if let Err(error) = self.origin.write_all_at(offset, data) {
            self.mark_origin_failed();
            return Err(error);
        }
        self.origin_dirty = true;
        self.telemetry.origin_written_bytes = self
            .telemetry
            .origin_written_bytes
            .saturating_add(data.len() as u64);
        Ok(())
    }

    fn sync_dirty_origin(&mut self) -> Result<(), IoError> {
        if !self.origin_dirty {
            return Ok(());
        }
        if let Err(error) = self.origin.sync_data() {
            self.mark_origin_failed();
            return Err(error);
        }
        self.origin_dirty = false;
        self.telemetry.origin_syncs = self.telemetry.origin_syncs.saturating_add(1);
        Ok(())
    }

    /// Mirror an already-authorized origin write into the cache, one
    /// `MAX_CACHE_MUTATION_BYTES` frame at a time. The origin is already
    /// committed, so a mid-write failure revokes the cache and leaves reads
    /// falling back to origin — correctness never depends on the mirror.
    fn update_cache(&mut self, offset: u64, data: &[u8]) {
        for (offset, slice) in mutation_frames(offset, data) {
            if self.cache.update(offset, slice) == CacheMutation::Failed {
                self.telemetry.cache_write_failures =
                    self.telemetry.cache_write_failures.saturating_add(1);
                let _ = self.revoke_cache();
                return;
            }
        }
    }
}

impl<O: OriginStorage, C: BestEffortCache> BlockBackend for AuthoritativeOriginBackend<O, C> {
    fn size_bytes(&self) -> u64 {
        self.size
    }

    fn block_size(&self) -> u32 {
        self.block
    }

    fn read_at(&mut self, offset: u64, destination: &mut [u8]) -> Result<(), IoError> {
        self.check_range(offset, destination.len())?;
        self.require_ready_origin()?;
        if destination.is_empty() {
            return Ok(());
        }
        match self.cache.read(offset, destination) {
            CacheRead::Hit => {
                self.telemetry.cache_read_bytes = self
                    .telemetry
                    .cache_read_bytes
                    .saturating_add(destination.len() as u64);
                return Ok(());
            }
            CacheRead::Miss => {}
            CacheRead::Failed => {
                self.telemetry.cache_read_failures =
                    self.telemetry.cache_read_failures.saturating_add(1);
                let _ = self.revoke_cache();
            }
        }
        if let Err(error) = self.origin.read_exact_at(offset, destination) {
            self.mark_origin_failed();
            return Err(error);
        }
        self.telemetry.fallback_reads = self.telemetry.fallback_reads.saturating_add(1);
        for (offset, slice) in mutation_frames(offset, destination) {
            if self.cache.promote(offset, slice) == CacheMutation::Failed {
                self.telemetry.promotion_refusals =
                    self.telemetry.promotion_refusals.saturating_add(1);
                let _ = self.revoke_cache();
                break;
            }
        }
        Ok(())
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<(), IoError> {
        self.check_range(offset, data.len())?;
        self.require_ready_origin()?;
        if data.is_empty() {
            return Ok(());
        }
        self.write_origin(offset, data)?;
        self.telemetry.batched_writes = self.telemetry.batched_writes.saturating_add(1);
        self.update_cache(offset, data);
        Ok(())
    }

    fn write_at_with_options(
        &mut self,
        offset: u64,
        data: &[u8],
        options: WriteOptions,
    ) -> Result<(), IoError> {
        if !options.fua {
            return self.write_at(offset, data);
        }
        self.check_range(offset, data.len())?;
        self.require_ready_origin()?;
        if data.is_empty() {
            return Ok(());
        }
        self.write_origin(offset, data)?;
        self.sync_dirty_origin()?;
        self.update_cache(offset, data);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        self.require_ready_origin()?;
        self.sync_dirty_origin()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    #[derive(Clone)]
    struct MemoryOrigin(Rc<RefCell<Vec<u8>>>);

    impl OriginStorage for MemoryOrigin {
        fn read_at(&mut self, offset: u64, destination: &mut [u8]) -> Result<usize, IoError> {
            let start = offset as usize;
            destination.copy_from_slice(&self.0.borrow()[start..start + destination.len()]);
            Ok(destination.len())
        }

        fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<usize, IoError> {
            let start = offset as usize;
            self.0.borrow_mut()[start..start + data.len()].copy_from_slice(data);
            Ok(data.len())
        }

        fn sync_data(&mut self) -> Result<(), IoError> {
            Ok(())
        }
    }

    #[test]
    fn cache_timeout_falls_back_to_origin() {
        let bytes = Rc::new(RefCell::new(b"origin!!".to_vec()));
        let (cache, _hung_worker) = isolated_cache_channel(1, Duration::ZERO);
        let mut backend =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), cache, 8, 4).unwrap();

        let mut read_back = [0; 8];
        backend.read_at(0, &mut read_back).unwrap();

        assert_eq!(&read_back, b"origin!!");
        assert_eq!(backend.cache_state(), CacheState::Stuck);
        assert_eq!(backend.telemetry().cache_read_failures, 1);
        assert_eq!(backend.telemetry().fallback_reads, 1);
    }

    #[test]
    fn cache_disconnect_falls_back_to_origin() {
        let bytes = Rc::new(RefCell::new(b"durable!".to_vec()));
        let (cache, worker) = isolated_cache_channel(1, Duration::from_millis(1));
        drop(worker);
        let mut backend =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), cache, 8, 4).unwrap();

        let mut read_back = [0; 8];
        backend.read_at(0, &mut read_back).unwrap();

        assert_eq!(&read_back, b"durable!");
        assert_eq!(backend.cache_state(), CacheState::Stuck);
        assert_eq!(backend.telemetry().cache_read_failures, 1);
        assert_eq!(backend.telemetry().fallback_reads, 1);
    }

    /// A codec/decode fault inside the compressed cache must serve the SSD
    /// origin bytes, never the refused payload and never a fabricated one
    /// (RF-1, DT-1, DT-7, DT-11).
    ///
    /// Unlike a transport timeout or a disconnect, a codec integrity failure
    /// surfaces as a cache **miss** with the corrupt payload left unread. The
    /// origin stays authoritative and the cache client is not revoked (DT-11).
    #[test]
    fn compressed_cache_fault_falls_back_to_origin() {
        let counters = Rc::new(CacheCounters::default());
        let mut codec_fault_cache = ScriptedCache::active(Rc::clone(&counters));
        // The worker refused this entry before decode (stored-checksum
        // mismatch), so the read is a miss. The payload the slab still holds
        // is deliberately wrong: it must never reach the reader.
        codec_fault_cache.read = CacheRead::Miss;
        codec_fault_cache.hit = b"corrupt!".to_vec();

        let bytes = Rc::new(RefCell::new(b"origin!!".to_vec()));
        let mut backend = AuthoritativeOriginBackend::new(
            MemoryOrigin(Rc::clone(&bytes)),
            codec_fault_cache,
            8,
            4,
        )
        .unwrap();

        let mut destination = [0; 8];
        backend.read_at(0, &mut destination).unwrap();

        // The origin is authoritative: the durable bytes are what the reader
        // gets, not the refused compressed payload.
        assert_eq!(&destination, b"origin!!");
        assert_ne!(&destination, b"corrupt!");
        assert_eq!(backend.telemetry().fallback_reads, 1);
        assert_eq!(
            backend.telemetry().cache_read_failures,
            0,
            "a codec integrity miss is not a transport failure"
        );
        assert_eq!(
            backend.cache_state(),
            CacheState::Active,
            "a codec fault must not revoke the cache client (DT-11)"
        );

        // A later real hit is still served from the cache, proving the client
        // survived the codec fault and the raw path keeps working.
        let mut hit_cache = ScriptedCache::active(counters);
        hit_cache.read = CacheRead::Hit;
        hit_cache.hit = b"cache!!!".to_vec();
        let mut after =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), hit_cache, 8, 4).unwrap();
        after.read_at(0, &mut destination).unwrap();
        assert_eq!(&destination, b"cache!!!");
        assert_eq!(after.telemetry().fallback_reads, 0);
    }

    #[test]
    fn disabled_cache_never_changes_origin_durability_order() {
        let bytes = Rc::new(RefCell::new(vec![0; 8]));
        let mut backend =
            AuthoritativeOriginBackend::new(MemoryOrigin(Rc::clone(&bytes)), DisabledCache, 8, 4)
                .unwrap();

        backend
            .write_at_with_options(0, b"safe", WriteOptions { fua: true })
            .unwrap();

        assert_eq!(&bytes.borrow()[..4], b"safe");
        assert_eq!(backend.telemetry().origin_syncs, 1);
        assert_eq!(backend.cache_state(), CacheState::Unavailable);
    }

    #[derive(Default)]
    struct CacheCounters {
        disables: Cell<u32>,
    }

    struct ScriptedCache {
        read: CacheRead,
        hit: Vec<u8>,
        update: CacheMutation,
        promote: CacheMutation,
        state: CacheState,
        counters: Rc<CacheCounters>,
        cached_bytes: u64,
        target_bytes: u64,
    }

    impl ScriptedCache {
        fn active(counters: Rc<CacheCounters>) -> Self {
            Self {
                read: CacheRead::Miss,
                hit: Vec::new(),
                update: CacheMutation::Accepted,
                promote: CacheMutation::Accepted,
                state: CacheState::Active,
                counters,
                cached_bytes: 4,
                target_bytes: 8,
            }
        }
    }

    impl BestEffortCache for ScriptedCache {
        fn read(&mut self, _offset: u64, destination: &mut [u8]) -> CacheRead {
            if self.read == CacheRead::Hit {
                destination.copy_from_slice(&self.hit);
            }
            self.read
        }

        fn update(&mut self, _offset: u64, _data: &[u8]) -> CacheMutation {
            self.update
        }

        fn promote(&mut self, _offset: u64, _data: &[u8]) -> CacheMutation {
            self.promote
        }

        fn disable(&mut self) -> CacheMutation {
            self.counters
                .disables
                .set(self.counters.disables.get().saturating_add(1));
            self.state = CacheState::Unavailable;
            CacheMutation::Accepted
        }

        fn state(&self) -> CacheState {
            self.state
        }

        fn cached_bytes(&self) -> u64 {
            self.cached_bytes
        }

        fn target_bytes(&self) -> u64 {
            self.target_bytes
        }
    }

    struct FaultOriginState {
        bytes: RefCell<Vec<u8>>,
        fail_read: Cell<bool>,
        fail_write: Cell<bool>,
        fail_sync: Cell<bool>,
        syncs: Cell<u32>,
    }

    #[derive(Clone)]
    struct FaultOrigin(Rc<FaultOriginState>);

    impl OriginStorage for FaultOrigin {
        fn read_at(&mut self, offset: u64, destination: &mut [u8]) -> Result<usize, IoError> {
            if self.0.fail_read.get() {
                return Err(IoError("fixture origin read failure".into()));
            }
            let start = offset as usize;
            destination.copy_from_slice(&self.0.bytes.borrow()[start..start + destination.len()]);
            Ok(destination.len())
        }

        fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<usize, IoError> {
            if self.0.fail_write.get() {
                return Err(IoError("fixture origin write failure".into()));
            }
            let start = offset as usize;
            self.0.bytes.borrow_mut()[start..start + data.len()].copy_from_slice(data);
            Ok(data.len())
        }

        fn sync_data(&mut self) -> Result<(), IoError> {
            if self.0.fail_sync.get() {
                return Err(IoError("fixture origin sync failure".into()));
            }
            self.0.syncs.set(self.0.syncs.get().saturating_add(1));
            Ok(())
        }
    }

    fn fault_origin(bytes: &[u8]) -> (FaultOrigin, Rc<FaultOriginState>) {
        let state = Rc::new(FaultOriginState {
            bytes: RefCell::new(bytes.to_vec()),
            fail_read: Cell::new(false),
            fail_write: Cell::new(false),
            fail_sync: Cell::new(false),
            syncs: Cell::new(0),
        });
        (FaultOrigin(Rc::clone(&state)), state)
    }

    #[test]
    fn bounded_cache_client_covers_hit_miss_mutation_and_disable_protocol() {
        let (mut cache, worker) = isolated_cache_channel(2, Duration::from_millis(100));
        let worker = std::thread::spawn(move || {
            if let Ok(IsolatedCacheRequest::Read { offset, len, reply }) = worker.requests.recv() {
                assert_eq!((offset, len), (4, 4));
                reply.send(Ok(Some(b"hit!".to_vec()))).unwrap();
            }
            worker
        });
        let mut destination = [0; 4];
        assert_eq!(cache.read(4, &mut destination), CacheRead::Hit);
        assert_eq!(&destination, b"hit!");
        let worker = worker.join().unwrap();

        assert_eq!(cache.update(8, b"new!"), CacheMutation::Accepted);
        if let Ok(IsolatedCacheRequest::Update { offset, data }) = worker.requests.recv() {
            assert_eq!(offset, 8);
            assert_eq!(data, b"new!");
        }
        assert_eq!(cache.promote(12, b"warm"), CacheMutation::Accepted);
        if let Ok(IsolatedCacheRequest::Promote { offset, data }) = worker.requests.recv() {
            assert_eq!(offset, 12);
            assert_eq!(data, b"warm");
        }
        let control = std::thread::spawn(move || {
            let IsolatedCacheControl::Disable { reply } = worker.control.recv().unwrap();
            reply.send(Ok(())).unwrap();
            worker
        });
        assert_eq!(cache.disable(), CacheMutation::Accepted);
        let worker = control.join().unwrap();
        assert_eq!(cache.state(), CacheState::Off);
        assert_eq!(cache.update(0, b"skip"), CacheMutation::Skipped);
        assert_eq!(cache.promote(0, b"skip"), CacheMutation::Skipped);
        assert_eq!(cache.read(0, &mut destination), CacheRead::Miss);
        assert_eq!(cache.disable(), CacheMutation::Skipped);
        drop(worker);
    }

    fn cache_read_with_reply(reply: Result<Option<Vec<u8>>, String>) -> (CacheRead, CacheState) {
        let (mut cache, worker) = isolated_cache_channel(1, Duration::from_millis(100));
        let worker = std::thread::spawn(move || {
            if let Ok(IsolatedCacheRequest::Read { reply: sender, .. }) = worker.requests.recv() {
                let _ = sender.send(reply);
            }
        });
        let result = cache.read(0, &mut [0; 4]);
        let _ = worker.join();
        (result, cache.state())
    }

    #[test]
    fn bounded_cache_client_revokes_on_protocol_queue_and_transport_faults() {
        assert_eq!(
            cache_read_with_reply(Ok(None)),
            (CacheRead::Miss, CacheState::Active)
        );
        for reply in [Ok(Some(vec![1; 3])), Err("fixture refusal".into())] {
            assert_eq!(
                cache_read_with_reply(reply),
                (CacheRead::Failed, CacheState::Unavailable)
            );
        }

        let (mut blocked, worker) = isolated_cache_channel(1, Duration::ZERO);
        assert_eq!(blocked.update(0, b"first"), CacheMutation::Accepted);
        assert_eq!(blocked.update(4, b"second"), CacheMutation::Failed);
        assert_eq!(blocked.state(), CacheState::Unavailable);
        assert_eq!(blocked.update(4, b"third"), CacheMutation::Skipped);
        drop(worker);

        let (mut disconnected, worker) = isolated_cache_channel(1, Duration::ZERO);
        drop(worker);
        assert_eq!(disconnected.update(0, b"data"), CacheMutation::Failed);

        let (mut no_queue, _requests) = isolated_cache_channel(0, Duration::ZERO);
        assert_eq!(no_queue.read(0, &mut [0; 4]), CacheRead::Failed);
        assert_eq!(no_queue.state(), CacheState::Unavailable);
    }

    #[test]
    // TestName: cache_disable_remains_deliverable_after_unavailable_full_data_queue
    fn cache_disable_remains_deliverable_after_unavailable_full_data_queue() {
        let (mut cache, worker) = isolated_cache_channel(1, Duration::from_millis(100));
        assert_eq!(cache.update(0, b"first"), CacheMutation::Accepted);
        assert_eq!(cache.update(4, b"full"), CacheMutation::Failed);
        assert_eq!(cache.state(), CacheState::Unavailable);

        let control = std::thread::spawn(move || {
            let IsolatedCacheControl::Disable { reply } = worker.control.recv().unwrap();
            reply.send(Ok(())).unwrap();
            worker
        });
        assert_eq!(cache.disable(), CacheMutation::Accepted);
        let worker = control.join().unwrap();
        assert!(matches!(
            worker
                .requests
                .recv_timeout(Duration::from_millis(100))
                .unwrap(),
            IsolatedCacheRequest::Update { offset: 0, .. }
        ));
        assert_eq!(cache.state(), CacheState::Off);
    }

    #[test]
    // TestName: release_cache_returns_zero_only_after_dedicated_control_acknowledgement
    fn release_cache_returns_zero_only_after_dedicated_control_acknowledgement() {
        let bytes = Rc::new(RefCell::new(b"durable!".to_vec()));
        let (cache, worker) = isolated_cache_channel(1, Duration::from_millis(100));
        let acknowledged = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_acknowledged = std::sync::Arc::clone(&acknowledged);
        let control = std::thread::spawn(move || {
            let IsolatedCacheControl::Disable { reply } = worker.control.recv().unwrap();
            worker_acknowledged.store(true, std::sync::atomic::Ordering::SeqCst);
            reply.send(Ok(())).unwrap();
        });
        let mut backend =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), cache, 8, 4).unwrap();

        assert_eq!(backend.release_cache().unwrap(), 0);
        assert!(acknowledged.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(backend.cache_state(), CacheState::Off);
        control.join().unwrap();

        let bytes = Rc::new(RefCell::new(b"durable!".to_vec()));
        let (cache, worker) = isolated_cache_channel(1, Duration::ZERO);
        drop(worker);
        let mut unacknowledged =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), cache, 8, 4).unwrap();
        assert!(unacknowledged.release_cache().is_err());
        assert_eq!(unacknowledged.cache_state(), CacheState::Stuck);
    }

    #[test]
    fn backend_geometry_range_and_empty_io_are_bounded() {
        let bytes = Rc::new(RefCell::new(vec![0; 8]));
        assert!(
            AuthoritativeOriginBackend::new(MemoryOrigin(Rc::clone(&bytes)), DisabledCache, 0, 4)
                .is_err()
        );
        assert!(
            AuthoritativeOriginBackend::new(MemoryOrigin(Rc::clone(&bytes)), DisabledCache, 8, 0)
                .is_err()
        );
        assert!(
            AuthoritativeOriginBackend::new(MemoryOrigin(Rc::clone(&bytes)), DisabledCache, 7, 4)
                .is_err()
        );

        let mut backend =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), DisabledCache, 8, 4).unwrap();
        assert_eq!(backend.size_bytes(), 8);
        assert_eq!(backend.block_size(), 4);
        assert_eq!(backend.origin_state(), OriginState::Ready);
        assert_eq!((backend.cached_bytes(), backend.target_bytes()), (0, 0));
        assert!(backend.read_at(8, &mut []).is_ok());
        assert!(backend.write_at(8, &[]).is_ok());
        assert!(
            backend
                .write_at_with_options(8, &[], WriteOptions { fua: true })
                .is_ok()
        );
        assert!(backend.flush().is_ok());
        assert!(backend.read_at(8, &mut [0]).is_err());
        assert!(backend.write_at(u64::MAX, b"x").is_err());
    }

    #[test]
    fn backend_cache_paths_preserve_origin_authority_and_telemetry() {
        let counters = Rc::new(CacheCounters::default());
        let mut hit_cache = ScriptedCache::active(Rc::clone(&counters));
        hit_cache.read = CacheRead::Hit;
        hit_cache.hit = b"cache!!!".to_vec();
        let bytes = Rc::new(RefCell::new(b"origin!!".to_vec()));
        let mut hit =
            AuthoritativeOriginBackend::new(MemoryOrigin(Rc::clone(&bytes)), hit_cache, 8, 4)
                .unwrap();
        let mut destination = [0; 8];
        hit.read_at(0, &mut destination).unwrap();
        assert_eq!(&destination, b"cache!!!");
        assert_eq!(hit.telemetry().cache_read_bytes, 8);
        assert_eq!(hit.telemetry().fallback_reads, 0);
        assert_eq!((hit.cached_bytes(), hit.target_bytes()), (4, 8));
        assert_eq!(hit.release_cache().unwrap(), 0);
        assert_eq!(hit.cache_state(), CacheState::Unavailable);
        assert_eq!(hit.telemetry().invalidations, 1);

        let mut failed_cache = ScriptedCache::active(Rc::clone(&counters));
        failed_cache.read = CacheRead::Failed;
        let mut fallback =
            AuthoritativeOriginBackend::new(MemoryOrigin(Rc::clone(&bytes)), failed_cache, 8, 4)
                .unwrap();
        fallback.read_at(0, &mut destination).unwrap();
        assert_eq!(&destination, b"origin!!");
        assert_eq!(fallback.telemetry().cache_read_failures, 1);
        assert_eq!(fallback.telemetry().fallback_reads, 1);

        let mut promotion_cache = ScriptedCache::active(Rc::clone(&counters));
        promotion_cache.promote = CacheMutation::Failed;
        let mut promotion =
            AuthoritativeOriginBackend::new(MemoryOrigin(Rc::clone(&bytes)), promotion_cache, 8, 4)
                .unwrap();
        promotion.read_at(0, &mut destination).unwrap();
        assert_eq!(promotion.telemetry().promotion_refusals, 1);
        assert_eq!(promotion.cache_state(), CacheState::Unavailable);

        let mut update_cache = ScriptedCache::active(counters);
        update_cache.update = CacheMutation::Failed;
        let mut update =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), update_cache, 8, 4).unwrap();
        update.write_at(0, b"safe").unwrap();
        assert_eq!(update.telemetry().origin_written_bytes, 4);
        assert_eq!(update.telemetry().batched_writes, 1);
        assert_eq!(update.telemetry().cache_write_failures, 1);
        assert_eq!(update.cache_state(), CacheState::Unavailable);
    }

    /// Records every cache mutation frame so tests can assert the framing.
    struct RecordingCache {
        updates: RefCell<Vec<(u64, usize)>>,
        promotes: RefCell<Vec<(u64, usize)>>,
    }

    impl RecordingCache {
        fn new() -> Self {
            Self {
                updates: RefCell::new(Vec::new()),
                promotes: RefCell::new(Vec::new()),
            }
        }
    }

    impl BestEffortCache for RecordingCache {
        fn read(&mut self, _offset: u64, destination: &mut [u8]) -> CacheRead {
            destination.fill(0);
            CacheRead::Miss
        }

        fn update(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
            self.updates.borrow_mut().push((offset, data.len()));
            CacheMutation::Accepted
        }

        fn promote(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
            self.promotes.borrow_mut().push((offset, data.len()));
            CacheMutation::Accepted
        }

        fn disable(&mut self) -> CacheMutation {
            CacheMutation::Accepted
        }

        fn state(&self) -> CacheState {
            CacheState::Active
        }
    }

    /// A block-layer write can be far larger than one mutation frame (the NBD
    /// queue on this surface admits up to 4 MiB). Each frame must stay within
    /// `MAX_CACHE_MUTATION_BYTES` or the transport fails closed and revokes the
    /// cache, which is what used to leave `cache_state: UNAVAILABLE`.
    #[test]
    fn large_write_is_framed_to_the_cache_mutation_limit() {
        let capacity = 4 * 1024 * 1024;
        let bytes = Rc::new(RefCell::new(vec![0u8; capacity]));
        let cache = RecordingCache::new();
        let mut backend =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), cache, capacity as u64, 512)
                .unwrap();

        let payload = vec![0xA5u8; capacity];
        backend.write_at(0, &payload).unwrap();

        let frames = backend.cache.updates.borrow();
        assert_eq!(
            frames.len(),
            capacity / MAX_CACHE_MUTATION_BYTES,
            "one frame per MAX_CACHE_MUTATION_BYTES slice"
        );
        let mut expected_offset = 0u64;
        for (offset, len) in frames.iter() {
            assert_eq!(*offset, expected_offset, "frames must be consecutive");
            assert!(
                *len <= MAX_CACHE_MUTATION_BYTES,
                "frame of {len} bytes exceeds the mutation limit"
            );
            expected_offset += *len as u64;
        }
        assert_eq!(
            expected_offset, capacity as u64,
            "frames must cover the write"
        );
        assert_eq!(backend.cache_state(), CacheState::Active);
        assert_eq!(backend.telemetry().cache_write_failures, 0);
    }

    /// The read-miss fallback promotes origin data back into the cache with the
    /// same framing constraint as `update`.
    #[test]
    fn large_promote_is_framed_to_the_cache_mutation_limit() {
        let capacity = MAX_CACHE_MUTATION_BYTES * 3 + 512;
        let bytes = Rc::new(RefCell::new(vec![0x5Au8; capacity]));
        let cache = RecordingCache::new();
        let mut backend =
            AuthoritativeOriginBackend::new(MemoryOrigin(bytes), cache, capacity as u64, 512)
                .unwrap();

        let mut destination = vec![0u8; capacity];
        backend.read_at(0, &mut destination).unwrap();

        let frames = backend.cache.promotes.borrow();
        assert_eq!(frames.len(), 4, "three full frames plus a 512-byte tail");
        assert_eq!(frames[0], (0, MAX_CACHE_MUTATION_BYTES));
        assert_eq!(
            frames[1],
            (MAX_CACHE_MUTATION_BYTES as u64, MAX_CACHE_MUTATION_BYTES)
        );
        assert_eq!(
            frames[2],
            (
                2 * MAX_CACHE_MUTATION_BYTES as u64,
                MAX_CACHE_MUTATION_BYTES
            )
        );
        assert_eq!(frames[3], (3 * MAX_CACHE_MUTATION_BYTES as u64, 512));
        assert_eq!(backend.telemetry().promotion_refusals, 0);
    }

    #[test]
    fn origin_failure_requires_three_successful_read_sync_probes() {
        let (origin, state) = fault_origin(b"durable!");
        state.fail_read.set(true);
        let mut backend = AuthoritativeOriginBackend::new(origin, DisabledCache, 8, 4).unwrap();
        assert!(backend.read_at(0, &mut [0; 4]).is_err());
        assert_eq!(backend.origin_state(), OriginState::Failed);
        assert!(backend.write_at(0, b"nope").is_err());
        assert!(backend.flush().is_err());

        state.fail_read.set(false);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Degraded);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Degraded);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Ready);
        assert_eq!(state.syncs.get(), 3);

        backend.write_at(0, b"safe").unwrap();
        backend.flush().unwrap();
        assert_eq!(state.syncs.get(), 4);
        state.fail_sync.set(true);
        backend.write_at(4, b"data").unwrap();
        assert!(backend.flush().is_err());
        assert_eq!(backend.origin_state(), OriginState::Failed);
        assert!(backend.probe_origin().is_err());

        let (origin, state) = fault_origin(b"durable!");
        state.fail_write.set(true);
        let mut write_failure =
            AuthoritativeOriginBackend::new(origin, DisabledCache, 8, 4).unwrap();
        assert!(
            write_failure
                .write_at_with_options(0, b"fail", WriteOptions { fua: true })
                .is_err()
        );
        assert_eq!(write_failure.origin_state(), OriginState::Failed);
    }
}
