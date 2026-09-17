//! Elastic Cooperative VRAM Cache with Non-Blocking SSD Spillover.
//!
//! SPEC: docs/specs/no-milestone/elastic-vram-cooperative-tier/SPEC.md §DT-2, §DT-3, §DT-4, §DT-5

use std::time::{Duration, Instant};

use ramshared_vram::{VramMemory, VramProvider};

use crate::isolated_origin::{BestEffortCache, CacheMutation, CacheRead};
use crate::origin_cache::CacheState;

/// Default chunk size for elastic tiering (64 MiB).
pub const ELASTIC_CHUNK_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum bounded DMA duration before triggering spillover (50 ms).
pub const DMA_TIMEOUT_LIMIT: Duration = Duration::from_millis(50);

/// Status of an individual chunk extent.
pub struct ChunkSlot<'p, P: VramProvider + 'p> {
    pub mem: Option<P::Mem<'p>>,
    pub written: bool,
    pub last_accessed: Instant,
    pub spilled_to_ssd: bool,
}

/// Table of sparse chunk extents mapping logical offsets to physical storage.
pub struct ElasticExtentTable<'p, P: VramProvider + 'p> {
    pub chunk_bytes: u64,
    pub logical_capacity: u64,
    pub slots: Vec<ChunkSlot<'p, P>>,
}

impl<'p, P: VramProvider + 'p> ElasticExtentTable<'p, P> {
    /// Creates an empty extent table for the given logical capacity.
    pub fn new(logical_capacity: u64, chunk_bytes: u64) -> Self {
        let chunk_count = logical_capacity
            .saturating_add(chunk_bytes.saturating_sub(1))
            .checked_div(chunk_bytes)
            .unwrap_or(0) as usize;

        let mut slots = Vec::with_capacity(chunk_count);
        let now = Instant::now();
        for _ in 0..chunk_count {
            slots.push(ChunkSlot {
                mem: None,
                written: false,
                last_accessed: now,
                spilled_to_ssd: false,
            });
        }

        Self {
            chunk_bytes,
            logical_capacity,
            slots,
        }
    }

    /// Number of chunks currently committed to physical VRAM.
    pub fn resident_chunks_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.mem.is_some() && !s.spilled_to_ssd)
            .count()
    }
}

/// Configuration for Elastic VRAM Cache.
#[derive(Clone, Debug)]
pub struct ElasticCacheConfig {
    pub chunk_bytes: u64,
    pub logical_capacity: u64,
    pub max_vram_bytes: u64,
    pub dma_timeout: Duration,
}

impl Default for ElasticCacheConfig {
    fn default() -> Self {
        Self {
            chunk_bytes: ELASTIC_CHUNK_BYTES,
            logical_capacity: 4 * 1024 * 1024 * 1024, // 4 GiB
            max_vram_bytes: 4 * 1024 * 1024 * 1024,
            dma_timeout: DMA_TIMEOUT_LIMIT,
        }
    }
}

/// Elastic cooperative VRAM cache implementing BestEffortCache.
pub struct ElasticVramCache<'p, P: VramProvider + 'p> {
    provider: &'p P,
    config: ElasticCacheConfig,
    table: ElasticExtentTable<'p, P>,
    state: CacheState,
    watchdog_tripped: bool,
}

impl<'p, P: VramProvider + 'p> ElasticVramCache<'p, P> {
    /// Creates a new elastic cache with the given provider reference and configuration.
    pub fn new(provider: &'p P, config: ElasticCacheConfig) -> Self {
        let table = ElasticExtentTable::new(config.logical_capacity, config.chunk_bytes);
        Self {
            provider,
            config,
            table,
            state: CacheState::Active,
            watchdog_tripped: false,
        }
    }

    /// Releases the oldest written VRAM chunks to immediately return memory to host.
    pub fn evict_coldest_chunks(&mut self, target_bytes_to_free: u64) -> usize {
        let chunk_bytes = self.config.chunk_bytes;
        if chunk_bytes == 0 || target_bytes_to_free == 0 {
            return 0;
        }

        let mut eligible: Vec<(usize, Instant)> = self
            .table
            .slots
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| {
                if slot.mem.is_some() && !slot.spilled_to_ssd {
                    Some((idx, slot.last_accessed))
                } else {
                    None
                }
            })
            .collect();

        // Sort by oldest accessed first
        eligible.sort_by_key(|&(_, accessed)| accessed);

        let mut freed_bytes: u64 = 0;
        let mut evicted_count: usize = 0;

        for (idx, _) in eligible {
            if freed_bytes >= target_bytes_to_free {
                break;
            }
            let slot = &mut self.table.slots[idx];
            // Release GPU allocation (drop calls cudaFree)
            slot.mem = None;
            slot.spilled_to_ssd = true;
            freed_bytes = freed_bytes.saturating_add(chunk_bytes);
            evicted_count = evicted_count.saturating_add(1);
        }

        evicted_count
    }

    /// Trips the DMA watchdog, forcing non-blocking spillover to SSD.
    pub fn trip_watchdog(&mut self) {
        self.watchdog_tripped = true;
    }

    /// Resets the DMA watchdog after transient recovery.
    pub fn reset_watchdog(&mut self) {
        self.watchdog_tripped = false;
    }

    /// Number of active physical VRAM bytes held.
    pub fn active_vram_bytes(&self) -> u64 {
        (self.table.resident_chunks_count() as u64).saturating_mul(self.config.chunk_bytes)
    }

    /// Reference to internal extent table.
    pub fn table(&self) -> &ElasticExtentTable<'p, P> {
        &self.table
    }

    /// Mutable reference to internal extent table.
    pub fn table_mut(&mut self) -> &mut ElasticExtentTable<'p, P> {
        &mut self.table
    }

    /// Whether the watchdog is currently tripped.
    pub fn is_watchdog_tripped(&self) -> bool {
        self.watchdog_tripped
    }
}

impl<'p, P: VramProvider + 'p> BestEffortCache for ElasticVramCache<'p, P> {
    fn read(&mut self, offset: u64, destination: &mut [u8]) -> CacheRead {
        if self.state != CacheState::Active || self.watchdog_tripped {
            return CacheRead::Miss;
        }

        let chunk_bytes = self.config.chunk_bytes;
        if chunk_bytes == 0 || offset >= self.config.logical_capacity {
            return CacheRead::Failed;
        }

        let chunk_idx = (offset / chunk_bytes) as usize;
        let chunk_off = offset % chunk_bytes;

        if chunk_idx >= self.table.slots.len() {
            return CacheRead::Failed;
        }

        if self.table.slots[chunk_idx].spilled_to_ssd || self.table.slots[chunk_idx].mem.is_none() {
            return CacheRead::Miss;
        }

        let (result, timeout, failed) = if let Some(ref mem) = self.table.slots[chunk_idx].mem {
            let start = Instant::now();
            match mem.read_at(chunk_off, destination) {
                Ok(()) => {
                    if start.elapsed() > self.config.dma_timeout {
                        (CacheRead::Miss, true, false)
                    } else {
                        (CacheRead::Hit, false, false)
                    }
                }
                Err(_) => (CacheRead::Miss, false, true),
            }
        } else {
            (CacheRead::Miss, false, false)
        };

        if timeout || failed {
            self.watchdog_tripped = true;
            self.table.slots[chunk_idx].spilled_to_ssd = true;
        } else if result == CacheRead::Hit {
            self.table.slots[chunk_idx].last_accessed = Instant::now();
        }

        result
    }

    fn update(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
        if self.state != CacheState::Active || self.watchdog_tripped {
            return CacheMutation::Skipped;
        }

        let chunk_bytes = self.config.chunk_bytes;
        if chunk_bytes == 0 || offset >= self.config.logical_capacity {
            return CacheMutation::Failed;
        }

        let chunk_idx = (offset / chunk_bytes) as usize;
        let chunk_off = offset % chunk_bytes;

        if chunk_idx >= self.table.slots.len() {
            return CacheMutation::Failed;
        }

        if self.table.slots[chunk_idx].spilled_to_ssd {
            return CacheMutation::Skipped;
        }

        // On-demand chunk allocation on first write
        if self.table.slots[chunk_idx].mem.is_none() {
            let current_vram = self.active_vram_bytes();
            if current_vram.saturating_add(chunk_bytes) > self.config.max_vram_bytes {
                self.table.slots[chunk_idx].spilled_to_ssd = true;
                return CacheMutation::Skipped;
            }

            match self.provider.alloc(chunk_bytes as usize) {
                Ok(mut mem) => {
                    let _ = mem.zero();
                    self.table.slots[chunk_idx].mem = Some(mem);
                }
                Err(_) => {
                    self.table.slots[chunk_idx].spilled_to_ssd = true;
                    return CacheMutation::Skipped;
                }
            }
        }

        let (result, timeout, failed) = if let Some(ref mut mem) = self.table.slots[chunk_idx].mem {
            let start = Instant::now();
            match mem.write_at(chunk_off, data) {
                Ok(()) => {
                    if start.elapsed() > self.config.dma_timeout {
                        (CacheMutation::Failed, true, false)
                    } else {
                        (CacheMutation::Accepted, false, false)
                    }
                }
                Err(_) => (CacheMutation::Failed, false, true),
            }
        } else {
            (CacheMutation::Skipped, false, false)
        };

        if timeout || failed {
            self.watchdog_tripped = true;
            self.table.slots[chunk_idx].spilled_to_ssd = true;
        } else if result == CacheMutation::Accepted {
            self.table.slots[chunk_idx].written = true;
            self.table.slots[chunk_idx].last_accessed = Instant::now();
        }

        result
    }

    fn promote(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
        if self.state != CacheState::Active || self.watchdog_tripped {
            return CacheMutation::Skipped;
        }
        self.update(offset, data)
    }

    fn disable(&mut self) -> CacheMutation {
        self.state = CacheState::Unavailable;
        for slot in &mut self.table.slots {
            slot.mem = None;
            slot.spilled_to_ssd = true;
        }
        CacheMutation::Accepted
    }

    fn state(&self) -> CacheState {
        self.state
    }

    fn cached_bytes(&self) -> u64 {
        self.active_vram_bytes()
    }

    fn target_bytes(&self) -> u64 {
        self.config.max_vram_bytes
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use ramshared_vram::VramError;
    use std::cell::Cell;

    pub struct TestMem {
        pub buf: Vec<u8>,
        pub fail_io: Cell<bool>,
        pub artificial_delay: Cell<Duration>,
    }

    impl VramMemory for TestMem {
        fn len(&self) -> usize {
            self.buf.len()
        }
        fn zero(&mut self) -> Result<(), VramError> {
            self.buf.fill(0);
            Ok(())
        }
        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            if self.fail_io.get() {
                return Err(VramError::OutOfRange {
                    off,
                    len: dst.len() as u64,
                    size: self.buf.len() as u64,
                });
            }
            if self.artificial_delay.get() > Duration::ZERO {
                std::thread::sleep(self.artificial_delay.get());
            }
            let off = off as usize;
            let end = off + dst.len();
            if end > self.buf.len() {
                return Err(VramError::OutOfRange {
                    off: off as u64,
                    len: dst.len() as u64,
                    size: self.buf.len() as u64,
                });
            }
            dst.copy_from_slice(&self.buf[off..end]);
            Ok(())
        }
        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            if self.fail_io.get() {
                return Err(VramError::OutOfRange {
                    off,
                    len: src.len() as u64,
                    size: self.buf.len() as u64,
                });
            }
            if self.artificial_delay.get() > Duration::ZERO {
                std::thread::sleep(self.artificial_delay.get());
            }
            let off = off as usize;
            let end = off + src.len();
            if end > self.buf.len() {
                return Err(VramError::OutOfRange {
                    off: off as u64,
                    len: src.len() as u64,
                    size: self.buf.len() as u64,
                });
            }
            self.buf[off..end].copy_from_slice(src);
            Ok(())
        }
    }

    pub struct TestProvider {
        pub fail_alloc: Cell<bool>,
        pub alloc_count: Cell<usize>,
    }

    impl Default for TestProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl TestProvider {
        pub fn new() -> Self {
            Self {
                fail_alloc: Cell::new(false),
                alloc_count: Cell::new(0),
            }
        }
    }

    impl VramProvider for TestProvider {
        type Mem<'a> = TestMem;

        fn alloc(&self, size: usize) -> Result<Self::Mem<'_>, VramError> {
            if self.fail_alloc.get() {
                return Err(VramError::OutOfMemory);
            }
            self.alloc_count.set(self.alloc_count.get() + 1);
            Ok(TestMem {
                buf: vec![0u8; size],
                fail_io: Cell::new(false),
                artificial_delay: Cell::new(Duration::ZERO),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((4096 * 1024 * 1024, 6144 * 1024 * 1024))
        }
    }

    #[test]
    fn test_sparse_extent_allocation_on_demand() {
        let provider = TestProvider::new();
        let chunk_size = 64 * 1024 * 1024; // 64 MiB
        let config = ElasticCacheConfig {
            chunk_bytes: chunk_size,
            logical_capacity: 512 * 1024 * 1024, // 512 MiB
            max_vram_bytes: 256 * 1024 * 1024,
            dma_timeout: Duration::from_millis(50),
        };
        let mut cache = ElasticVramCache::new(&provider, config);

        // Before any writes: zero chunks allocated
        assert_eq!(cache.cached_bytes(), 0);

        // Write to chunk 0
        let data = vec![0xAB; 4096];
        let mut_res = cache.update(0, &data);
        assert_eq!(mut_res, CacheMutation::Accepted);
        assert_eq!(cache.cached_bytes(), chunk_size);

        // Write to chunk 2 (sparse)
        let off_chunk2 = 2 * chunk_size;
        let mut_res2 = cache.update(off_chunk2, &data);
        assert_eq!(mut_res2, CacheMutation::Accepted);
        assert_eq!(cache.cached_bytes(), 2 * chunk_size);

        // Read chunk 0
        let mut read_buf = vec![0u8; 4096];
        let read_res = cache.read(0, &mut read_buf);
        assert_eq!(read_res, CacheRead::Hit);
        assert_eq!(read_buf, data);

        // Read unwritten chunk 1 -> CacheRead::Miss
        let read_res_unwritten = cache.read(chunk_size, &mut read_buf);
        assert_eq!(read_res_unwritten, CacheRead::Miss);
    }

    #[test]
    fn test_spillover_on_dma_timeout() {
        let provider = TestProvider::new();
        let chunk_size = 64 * 1024 * 1024;
        let config = ElasticCacheConfig {
            chunk_bytes: chunk_size,
            logical_capacity: 256 * 1024 * 1024,
            max_vram_bytes: 256 * 1024 * 1024,
            dma_timeout: Duration::from_millis(20), // strict 20ms timeout
        };
        let mut cache = ElasticVramCache::new(&provider, config);

        // First write initializes chunk 0
        let data = vec![0xCC; 4096];
        assert_eq!(cache.update(0, &data), CacheMutation::Accepted);

        // Inject 50ms artificial delay into chunk 0
        if let Some(ref mem) = cache.table.slots[0].mem {
            mem.artificial_delay.set(Duration::from_millis(50));
        }

        // Subsequent write exceeds 20ms deadline -> fails and trips watchdog!
        let mut_res = cache.update(4096, &data);
        assert_eq!(mut_res, CacheMutation::Failed);
        assert!(cache.is_watchdog_tripped());

        // Further writes automatically spill to SSD (CacheMutation::Skipped)
        assert_eq!(cache.update(8192, &data), CacheMutation::Skipped);
    }

    #[test]
    fn test_eviction_worker_flushes_cold_chunks_under_deadline() {
        let provider = TestProvider::new();
        let chunk_size = 64 * 1024 * 1024;
        let config = ElasticCacheConfig {
            chunk_bytes: chunk_size,
            logical_capacity: 512 * 1024 * 1024,
            max_vram_bytes: 512 * 1024 * 1024,
            dma_timeout: Duration::from_millis(50),
        };
        let mut cache = ElasticVramCache::new(&provider, config);

        // Commit 4 chunks (256 MiB)
        let data = vec![0xEE; 4096];
        for i in 0..4 {
            assert_eq!(cache.update(i * chunk_size, &data), CacheMutation::Accepted);
        }
        assert_eq!(cache.cached_bytes(), 4 * chunk_size);

        // Evict 2 chunks (128 MiB)
        let start = Instant::now();
        let evicted = cache.evict_coldest_chunks(128 * 1024 * 1024);
        let elapsed = start.elapsed();

        assert_eq!(evicted, 2);
        assert_eq!(cache.cached_bytes(), 2 * chunk_size);
        // Freeing in userspace must complete in < 100ms
        assert!(elapsed < Duration::from_millis(100));

        // Evicted chunks are now marked spilled_to_ssd -> reads report Miss
        let mut buf = vec![0u8; 4096];
        assert_eq!(cache.read(0, &mut buf), CacheRead::Miss);
    }

    #[test]
    fn test_sparse_extent_idempotent_read_write() {
        let provider = TestProvider::new();
        let chunk_size = 64 * 1024 * 1024;
        let config = ElasticCacheConfig {
            chunk_bytes: chunk_size,
            logical_capacity: 128 * 1024 * 1024,
            max_vram_bytes: 128 * 1024 * 1024,
            dma_timeout: Duration::from_millis(50),
        };
        let mut cache = ElasticVramCache::new(&provider, config);

        let data1 = vec![0x11; 4096];
        let data2 = vec![0x22; 4096];

        assert_eq!(cache.update(0, &data1), CacheMutation::Accepted);
        let mut read_buf = vec![0u8; 4096];
        assert_eq!(cache.read(0, &mut read_buf), CacheRead::Hit);
        assert_eq!(read_buf, data1);

        // Overwrite same offset
        assert_eq!(cache.update(0, &data2), CacheMutation::Accepted);
        assert_eq!(cache.read(0, &mut read_buf), CacheRead::Hit);
        assert_eq!(read_buf, data2);
    }

    #[test]
    fn test_elastic_cache_disable_and_state() {
        let provider = TestProvider::new();
        let config = ElasticCacheConfig::default();
        let mut cache = ElasticVramCache::new(&provider, config);

        assert_eq!(cache.state(), CacheState::Active);
        assert_eq!(cache.target_bytes(), 4 * 1024 * 1024 * 1024);

        let data = vec![0x33; 4096];
        assert_eq!(cache.update(0, &data), CacheMutation::Accepted);
        assert!(cache.cached_bytes() > 0);

        // Disable cache
        assert_eq!(cache.disable(), CacheMutation::Accepted);
        assert_eq!(cache.state(), CacheState::Unavailable);
        assert_eq!(cache.cached_bytes(), 0);

        // Reads and writes now fail-safe to SSD
        let mut buf = vec![0u8; 4096];
        assert_eq!(cache.read(0, &mut buf), CacheRead::Miss);
        assert_eq!(cache.update(0, &data), CacheMutation::Skipped);
        assert_eq!(cache.promote(0, &data), CacheMutation::Skipped);
    }

    #[test]
    fn test_elastic_cache_out_of_bounds_and_spillover_edges() {
        let provider = TestProvider::new();
        let config = ElasticCacheConfig {
            chunk_bytes: 64 * 1024 * 1024,
            logical_capacity: 64 * 1024 * 1024,
            max_vram_bytes: 64 * 1024 * 1024,
            dma_timeout: Duration::from_millis(50),
        };
        let mut cache = ElasticVramCache::new(&provider, config);

        let data = vec![0x44; 4096];
        let mut buf = vec![0u8; 4096];

        // Out of bounds
        assert_eq!(cache.read(128 * 1024 * 1024, &mut buf), CacheRead::Failed);
        assert_eq!(
            cache.update(128 * 1024 * 1024, &data),
            CacheMutation::Failed
        );

        // Trip and reset watchdog
        cache.trip_watchdog();
        assert!(cache.is_watchdog_tripped());
        assert_eq!(cache.update(0, &data), CacheMutation::Skipped);
        assert_eq!(cache.read(0, &mut buf), CacheRead::Miss);
        cache.reset_watchdog();
        assert!(!cache.is_watchdog_tripped());

        // Promote
        assert_eq!(cache.promote(0, &data), CacheMutation::Accepted);
        assert_eq!(cache.read(0, &mut buf), CacheRead::Hit);
        assert_eq!(buf, data);

        // Coverage for table accessors
        assert_eq!(cache.table().resident_chunks_count(), 1);
        assert_eq!(cache.table_mut().resident_chunks_count(), 1);
    }

    #[test]
    fn test_elastic_cache_allocation_failure_spills_cleanly() {
        let provider = TestProvider::new();
        provider.fail_alloc.set(true); // Provider fails allocation
        let config = ElasticCacheConfig {
            chunk_bytes: 64 * 1024 * 1024,
            logical_capacity: 64 * 1024 * 1024,
            max_vram_bytes: 64 * 1024 * 1024,
            dma_timeout: Duration::from_millis(50),
        };
        let mut cache = ElasticVramCache::new(&provider, config);

        let data = vec![0x55; 4096];
        // Allocation failure -> cleanly marks spilled_to_ssd and returns Skipped (to let SSD handle write)
        assert_eq!(cache.update(0, &data), CacheMutation::Skipped);
        assert_eq!(cache.cached_bytes(), 0);
    }
}
