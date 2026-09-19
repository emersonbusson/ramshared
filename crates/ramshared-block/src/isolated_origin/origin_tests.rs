use std::time::Duration;
use crate::origin_cache::{CacheState, OriginState, OriginStorage};
use super::origin_tracker::*;
use super::origin_policy::*;
use crate::{BlockBackend, IoError, WriteOptions};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

use std::rc::Rc;
    use std::cell::{Cell, RefCell};

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
