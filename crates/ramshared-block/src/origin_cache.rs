//! Durable origin with a revocable, best-effort VRAM block cache.
use std::cmp;
use std::fs::File;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::FileExt;
use std::path::Path;
use std::time::{Duration, Instant};

use ramshared_vram::{GpuAdapterIdentity, GpuBudgetSource, VramMemory, VramProvider};

use crate::{BlockBackend, IoError, WriteOptions};

pub const GIB: u64 = 1024 * 1024 * 1024;
pub const ORIGIN_CACHE_CHUNK_BYTES: u64 = 128 * 1024 * 1024;
const HEALTHY_SAMPLES_TO_GROW: u8 = 3;
const RESTRICTED_SAMPLES_TO_RECLAIM: u8 = 3;
const GROWTH_INTERVAL: Duration = Duration::from_secs(2);
const STUCK_AFTER: Duration = Duration::from_secs(2);

pub trait OriginStorage {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, IoError>;
    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<usize, IoError>;
    fn sync_data(&mut self) -> Result<(), IoError>;

    fn read_exact_at(&mut self, mut off: u64, mut buf: &mut [u8]) -> Result<(), IoError> {
        while !buf.is_empty() {
            let read = self.read_at(off, buf)?;
            if read == 0 {
                return Err(IoError("origin read made no progress".into()));
            }
            if read > buf.len() {
                return Err(IoError("origin read exceeded requested length".into()));
            }
            off = off
                .checked_add(read as u64)
                .ok_or_else(|| IoError("origin read offset overflow".into()))?;
            buf = &mut buf[read..];
        }
        Ok(())
    }

    fn write_all_at(&mut self, mut off: u64, mut data: &[u8]) -> Result<(), IoError> {
        while !data.is_empty() {
            let written = self.write_at(off, data)?;
            if written == 0 {
                return Err(IoError("origin write made no progress".into()));
            }
            if written > data.len() {
                return Err(IoError("origin write exceeded requested length".into()));
            }
            off = off
                .checked_add(written as u64)
                .ok_or_else(|| IoError("origin write offset overflow".into()))?;
            data = &data[written..];
        }
        Ok(())
    }
}

pub struct FileOrigin {
    file: File,
}
impl FileOrigin {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            file: File::options().read(true).write(true).open(path)?,
        })
    }
    pub fn from_file(file: File) -> Self {
        Self { file }
    }
}
impl OriginStorage for FileOrigin {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, IoError> {
        #[cfg(unix)]
        {
            self.file
                .read_at(buf, off)
                .map_err(|error| IoError(error.to_string()))
        }
        #[cfg(windows)]
        {
            self.file
                .seek_read(buf, off)
                .map_err(|error| IoError(error.to_string()))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (off, buf);
            Err(IoError("unsupported platform for FileOrigin".into()))
        }
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<usize, IoError> {
        #[cfg(unix)]
        {
            self.file
                .write_at(data, off)
                .map_err(|error| IoError(error.to_string()))
        }
        #[cfg(windows)]
        {
            self.file
                .seek_write(data, off)
                .map_err(|error| IoError(error.to_string()))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (off, data);
            Err(IoError("unsupported platform for FileOrigin".into()))
        }
    }

    fn sync_data(&mut self) -> Result<(), IoError> {
        self.file
            .sync_data()
            .map_err(|error| IoError(error.to_string()))
    }
}

/// Maximum age of a GPU sample before it is refused (DT-6).
///
/// Matches `ramshared_wsl2d::gpu_budget::WDDM_BUDGET_MAX_AGE`. Duplicated here
/// because `ramshared-block` does not depend on the daemon crate; the two are
/// pinned equal by `sample_max_age_matches_daemon_budget_window`.
pub const SAMPLE_MAX_AGE: Duration = Duration::from_secs(5);

/// One adapter-bound driver budget observation.
///
/// Carries its own provenance (DT-12). **Not `Copy`**: `GpuAdapterIdentity`
/// owns `String` fields. The producer builds this; `observe_gpu` only forwards
/// it; [`physical_target_bytes`] is the single validator (DT-6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuSample {
    /// Allocation budget for the adapter, in bytes.
    pub budget_bytes: u64,
    /// VRAM used by processes other than this worker, in bytes.
    pub external_usage_bytes: u64,
    /// Total physical device memory, in bytes.
    pub total_vram_bytes: u64,
    /// When the producer observed this sample (DT-12: `Instant`, one clock).
    pub sampled_at: Instant,
    /// Where the numbers came from. Only [`GpuBudgetSource::DriverReported`]
    /// is admitted.
    pub source: GpuBudgetSource,
    /// The adapter the numbers describe. Missing identity refuses admission.
    pub adapter: Option<GpuAdapterIdentity>,
}

/// Physical cache target for a logical size, or `0` when the sample must not
/// be trusted (DT-6).
///
/// One ownership chain, no second owner: the producer carries provenance,
/// `observe_gpu` forwards, and **this function validates and refuses**. It
/// never stamps provenance — never `Instant::now()`, never
/// `GpuBudgetSource::DriverReported` — so a staleness check can actually fail.
///
/// After the checks it returns
/// `safe_target_bytes(logical_bytes, configured_reserve_bytes,
/// runtime_headroom_bytes)` over the numeric mapping `budget_bytes` →
/// `budget_bytes`, `external_usage_bytes` → `used_bytes`, `total_vram_bytes` →
/// `total_bytes`. `safe_target_bytes` is arithmetic only and supplies the
/// arithmetic **after** these checks.
pub fn physical_target_bytes(
    logical_bytes: u64,
    sample: Option<&GpuSample>,
    configured_reserve_bytes: u64,
    runtime_headroom_bytes: u64,
    now: Instant,
) -> u64 {
    let Some(sample) = sample else {
        return 0;
    };
    // Provenance refusal. Each of these is a real, testable failure.
    if sample.source != GpuBudgetSource::DriverReported {
        return 0;
    }
    if sample.adapter.is_none() {
        return 0;
    }
    let age = match now.checked_duration_since(sample.sampled_at) {
        Some(age) => age,
        // A sample from the future is not trustworthy either.
        None => return 0,
    };
    if age > SAMPLE_MAX_AGE {
        return 0;
    }
    // Consistency refusal.
    if sample.external_usage_bytes > sample.budget_bytes {
        return 0;
    }
    if sample.budget_bytes > sample.total_vram_bytes {
        return 0;
    }

    // Numeric mapping onto the shared helper. No inline reserve math remains.
    let snapshot = ramshared_vram::GpuBudgetSnapshot {
        adapter: sample.adapter.clone(),
        total_bytes: Some(sample.total_vram_bytes),
        budget_bytes: sample.budget_bytes,
        used_bytes: sample.external_usage_bytes,
        source: sample.source,
        sampled_at: sample.sampled_at,
    };
    snapshot.safe_target_bytes(
        logical_bytes,
        configured_reserve_bytes,
        runtime_headroom_bytes,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OriginState {
    Off,
    Ready,
    Degraded,
    Failed,
}

impl OriginState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Ready => "READY",
            Self::Degraded => "DEGRADED",
            Self::Failed => "FAILED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheState {
    Off,
    Active,
    Restricted,
    Unavailable,
    Stuck,
}

impl CacheState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Active => "ACTIVE",
            Self::Restricted => "RESTRICTED",
            Self::Unavailable => "UNAVAILABLE",
            Self::Stuck => "STUCK",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheTelemetry {
    pub origin_written_bytes: u64,
    pub origin_syncs: u64,
    pub batched_writes: u64,
    pub cache_read_bytes: u64,
    pub fallback_reads: u64,
    pub invalidations: u64,
    pub promotion_refusals: u64,
    pub releases: u64,
    pub allocation_failures: u64,
    pub cache_read_failures: u64,
    pub cache_write_failures: u64,
    pub valid_blocks: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CachePolicyOutcome {
    pub target_bytes: u64,
    pub allocated_bytes: u64,
    pub released_bytes: u64,
}

struct CacheChunk<M> {
    mem: Option<M>,
    generation: u64,
    validity_generation: u64,
    valid: Vec<bool>,
    last_access: u64,
}

pub struct WriteThroughCacheBackend<'p, P: VramProvider + 'p, O> {
    provider: &'p P,
    origin: O,
    size: u64,
    block: u32,
    chunk_bytes: u64,
    chunks: Vec<CacheChunk<P::Mem<'p>>>,
    telemetry: CacheTelemetry,
    target_bytes: u64,
    physical_cap_bytes: u64,
    /// Configured reserve floor component (DT-2). Supplied by the sealed
    /// policy at startup; `0` means "no configured floor", so the 20%
    /// capacity share still binds inside `safe_target_bytes`.
    configured_reserve_bytes: u64,
    /// Runtime free buffer applied after the reserve (DT-4).
    runtime_headroom_bytes: u64,
    healthy_samples: u8,
    restricted_samples: u8,
    last_growth_at: Option<Duration>,
    over_target_since: Option<Duration>,
    access_clock: u64,
    origin_state: OriginState,
    origin_probe_successes: u8,
    origin_dirty: bool,
    cache_state: CacheState,
}

impl<'p, P: VramProvider + 'p, O: OriginStorage> WriteThroughCacheBackend<'p, P, O> {
    pub fn new(provider: &'p P, origin: O, size: u64, block: u32) -> Result<Self, IoError> {
        Self::with_chunk_bytes(provider, origin, size, block, ORIGIN_CACHE_CHUNK_BYTES)
    }

    #[doc(hidden)]
    pub fn with_chunk_bytes(
        provider: &'p P,
        origin: O,
        size: u64,
        block: u32,
        chunk_bytes: u64,
    ) -> Result<Self, IoError> {
        if size == 0
            || block == 0
            || chunk_bytes == 0
            || !size.is_multiple_of(block as u64)
            || !chunk_bytes.is_multiple_of(block as u64)
        {
            return Err(IoError("invalid origin cache geometry".into()));
        }
        let chunk_count = size.div_ceil(chunk_bytes);
        let mut chunks = Vec::with_capacity(chunk_count as usize);
        for index in 0..chunk_count {
            let start = index * chunk_bytes;
            let len = cmp::min(chunk_bytes, size - start);
            chunks.push(CacheChunk {
                mem: None,
                generation: 1,
                validity_generation: 0,
                valid: vec![false; (len / block as u64) as usize],
                last_access: 0,
            });
        }
        Ok(Self {
            provider,
            origin,
            size,
            block,
            chunk_bytes,
            chunks,
            telemetry: CacheTelemetry::default(),
            target_bytes: 0,
            physical_cap_bytes: size,
            configured_reserve_bytes: 0,
            runtime_headroom_bytes: crate::gpu_cache_worker::RUNTIME_FREE_BUFFER_BYTES,
            healthy_samples: 0,
            restricted_samples: 0,
            last_growth_at: None,
            over_target_since: None,
            access_clock: 0,
            origin_state: OriginState::Ready,
            origin_probe_successes: 0,
            origin_dirty: false,
            cache_state: CacheState::Off,
        })
    }

    pub fn chunk_bytes(&self) -> u64 {
        self.chunk_bytes
    }

    pub fn cached_bytes(&self) -> u64 {
        self.chunks.iter().fold(0, |total, chunk| {
            total + chunk.mem.as_ref().map_or(0, |_| self.chunk_bytes)
        })
    }

    pub fn target_bytes(&self) -> u64 {
        self.target_bytes
    }

    pub fn set_physical_cap_bytes(&mut self, cap_bytes: u64) {
        self.physical_cap_bytes = cap_bytes.min(self.size);
    }

    /// Binds the sealed reserve floor this cache must respect (DT-2/DT-4).
    ///
    /// Called once at startup with the resolved policy. The runtime headroom
    /// is taken from the caller so one authority supplies it.
    pub fn set_reserve_floor(
        &mut self,
        configured_reserve_bytes: u64,
        runtime_headroom_bytes: u64,
    ) {
        self.configured_reserve_bytes = configured_reserve_bytes;
        self.runtime_headroom_bytes = runtime_headroom_bytes;
    }

    pub fn configured_reserve_bytes(&self) -> u64 {
        self.configured_reserve_bytes
    }

    pub fn runtime_headroom_bytes(&self) -> u64 {
        self.runtime_headroom_bytes
    }

    pub fn origin_state(&self) -> OriginState {
        self.origin_state
    }

    pub fn cache_state(&self) -> CacheState {
        self.cache_state
    }

    pub fn telemetry(&self) -> CacheTelemetry {
        CacheTelemetry {
            valid_blocks: self.valid_block_count(),
            ..self.telemetry
        }
    }

    pub fn release_cache(&mut self) -> u64 {
        let released = self.release_lru_to(0);
        self.cache_state = CacheState::Off;
        released
    }

    /// Forwards the sample and the growth tick. Does **not** validate and does
    /// **not** stamp provenance (DT-6): [`physical_target_bytes`] owns both.
    pub fn observe_gpu(
        &mut self,
        sample: Option<&GpuSample>,
        tick: Duration,
        now: Instant,
    ) -> CachePolicyOutcome {
        let target = physical_target_bytes(
            self.physical_cap_bytes,
            sample,
            self.configured_reserve_bytes,
            self.runtime_headroom_bytes,
            now,
        );
        self.target_bytes = target;
        let cached_before = self.cached_bytes();
        let missing = sample.is_none();
        let restricted = missing || target < cached_before || target < self.chunk_bytes;

        if restricted {
            self.healthy_samples = 0;
            self.restricted_samples = self.restricted_samples.saturating_add(1);
            self.cache_state = if missing {
                CacheState::Unavailable
            } else {
                CacheState::Restricted
            };
        } else {
            self.restricted_samples = 0;
            self.healthy_samples = self.healthy_samples.saturating_add(1);
            self.cache_state = CacheState::Active;
        }

        let mut released_bytes = 0;
        if restricted && self.restricted_samples >= RESTRICTED_SAMPLES_TO_RECLAIM {
            released_bytes = self.release_lru_to(target);
            self.restricted_samples = 0;
        }

        let mut allocated_bytes = 0;
        if !restricted
            && self.healthy_samples >= HEALTHY_SAMPLES_TO_GROW
            && self.cached_bytes().saturating_add(self.chunk_bytes) <= target
            && self
                .last_growth_at
                .is_none_or(|previous| tick.saturating_sub(previous) >= GROWTH_INTERVAL)
        {
            self.last_growth_at = Some(tick);
            match self.allocate_one_chunk() {
                Ok(bytes) => allocated_bytes = bytes,
                Err(()) => self.cache_state = CacheState::Unavailable,
            }
        }

        let excess = self.cached_bytes().saturating_sub(target);
        if excess > self.chunk_bytes {
            let since = self.over_target_since.get_or_insert(tick);
            if tick.saturating_sub(*since) > STUCK_AFTER {
                self.cache_state = CacheState::Stuck;
            }
        } else {
            self.over_target_since = None;
        }

        CachePolicyOutcome {
            target_bytes: target,
            allocated_bytes,
            released_bytes,
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

    fn check_range(&self, off: u64, len: usize) -> Result<(), IoError> {
        off.checked_add(len as u64)
            .filter(|end| *end <= self.size)
            .map(|_| ())
            .ok_or_else(|| IoError("origin cache I/O is out of range".into()))
    }

    fn valid_block_count(&self) -> u64 {
        self.chunks
            .iter()
            .map(|chunk| {
                chunk
                    .valid
                    .iter()
                    .filter(|valid| {
                        **valid
                            && chunk.validity_generation == chunk.generation
                            && chunk.mem.is_some()
                    })
                    .count() as u64
            })
            .sum()
    }

    fn allocate_one_chunk(&mut self) -> Result<u64, ()> {
        let Some(index) = self.chunks.iter().position(|chunk| chunk.mem.is_none()) else {
            return Ok(0);
        };
        let bytes = self.chunk_bytes as usize;
        let mut mem = match self.provider.alloc(bytes) {
            Ok(mem) => mem,
            Err(_) => {
                self.telemetry.allocation_failures =
                    self.telemetry.allocation_failures.saturating_add(1);
                return Err(());
            }
        };
        if mem.zero().is_err() {
            self.telemetry.allocation_failures =
                self.telemetry.allocation_failures.saturating_add(1);
            return Err(());
        }
        self.access_clock = self.access_clock.saturating_add(1);
        self.chunks[index].last_access = self.access_clock;
        self.chunks[index].validity_generation = self.chunks[index].generation;
        self.chunks[index].valid.fill(false);
        self.chunks[index].mem = Some(mem);
        Ok(self.chunk_bytes)
    }

    fn release_lru_to(&mut self, target: u64) -> u64 {
        let mut released = 0u64;
        while self.cached_bytes() > target {
            let Some((index, _)) = self
                .chunks
                .iter()
                .enumerate()
                .filter(|(_, chunk)| chunk.mem.is_some())
                .min_by_key(|(_, chunk)| chunk.last_access)
            else {
                break;
            };
            self.invalidate_chunk(index);
            self.chunks[index].mem = None;
            released = released.saturating_add(self.chunk_bytes);
            self.telemetry.releases = self.telemetry.releases.saturating_add(1);
        }
        released
    }

    fn invalidate_chunk(&mut self, index: usize) {
        let chunk = &mut self.chunks[index];
        chunk.generation = chunk.generation.wrapping_add(1).max(1);
        chunk.valid.fill(false);
        self.telemetry.invalidations = self.telemetry.invalidations.saturating_add(1);
    }

    /// A failed origin write may have made partial progress. Invalidate every
    /// overlapping clean cache chunk before origin recovery can permit reads.
    fn invalidate_cached_range(&mut self, off: u64, len: usize) {
        if len == 0 {
            return;
        }
        let first = (off / self.chunk_bytes) as usize;
        let last = ((off + len as u64 - 1) / self.chunk_bytes) as usize;
        for index in first..=last {
            if self.chunks[index].mem.is_some() {
                self.invalidate_chunk(index);
            }
        }
    }

    fn invalidate_all_cached(&mut self) {
        for index in 0..self.chunks.len() {
            if self.chunks[index].mem.is_some() {
                self.invalidate_chunk(index);
            }
        }
    }

    fn mark_origin_failed(&mut self) {
        self.origin_state = OriginState::Failed;
        self.origin_probe_successes = 0;
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

    fn range_is_cached(&self, off: u64, len: usize) -> bool {
        let mut done = 0usize;
        while done < len {
            let absolute = off + done as u64;
            let index = (absolute / self.chunk_bytes) as usize;
            let relative = absolute % self.chunk_bytes;
            let count = (len - done).min((self.chunk_bytes - relative) as usize);
            let chunk = &self.chunks[index];
            if chunk.mem.is_none() {
                return false;
            }
            let first_block = relative / self.block as u64;
            let last_block = (relative + count as u64).div_ceil(self.block as u64);
            if chunk.validity_generation != chunk.generation
                || (first_block..last_block).any(|block| !chunk.valid[block as usize])
            {
                return false;
            }
            done += count;
        }
        true
    }

    fn cache_read(&mut self, off: u64, buf: &mut [u8]) -> Result<(), usize> {
        let mut done = 0usize;
        while done < buf.len() {
            let absolute = off + done as u64;
            let index = (absolute / self.chunk_bytes) as usize;
            let relative = absolute % self.chunk_bytes;
            let count = (buf.len() - done).min((self.chunk_bytes - relative) as usize);
            let result = self.chunks[index]
                .mem
                .as_ref()
                .ok_or(index)
                .and_then(|mem| {
                    mem.read_at(relative, &mut buf[done..done + count])
                        .map_err(|_| index)
                });
            result?;
            self.access_clock = self.access_clock.saturating_add(1);
            self.chunks[index].last_access = self.access_clock;
            done += count;
        }
        Ok(())
    }

    fn update_cached_chunks(&mut self, off: u64, data: &[u8], count_refusals: bool) {
        let mut done = 0usize;
        while done < data.len() {
            let absolute = off + done as u64;
            let index = (absolute / self.chunk_bytes) as usize;
            let relative = absolute % self.chunk_bytes;
            let count = (data.len() - done).min((self.chunk_bytes - relative) as usize);
            let Some(mem) = self.chunks[index].mem.as_mut() else {
                if count_refusals {
                    self.telemetry.promotion_refusals =
                        self.telemetry.promotion_refusals.saturating_add(1);
                }
                done += count;
                continue;
            };
            if mem.write_at(relative, &data[done..done + count]).is_err() {
                self.telemetry.cache_write_failures =
                    self.telemetry.cache_write_failures.saturating_add(1);
                self.invalidate_chunk(index);
                done += count;
                continue;
            }
            self.access_clock = self.access_clock.saturating_add(1);
            self.chunks[index].last_access = self.access_clock;
            self.mark_fully_covered_blocks(index, relative, count);
            done += count;
        }
    }

    fn mark_fully_covered_blocks(&mut self, index: usize, relative: u64, len: usize) {
        let block = self.block as u64;
        let first = relative.div_ceil(block);
        let last = (relative + len as u64) / block;
        let chunk = &mut self.chunks[index];
        if chunk.validity_generation != chunk.generation {
            chunk.valid.fill(false);
            chunk.validity_generation = chunk.generation;
        }
        for block_index in first..last {
            if let Some(valid) = chunk.valid.get_mut(block_index as usize) {
                *valid = true;
            }
        }
    }

    fn write_origin(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        if let Err(error) = self.origin.write_all_at(off, data) {
            self.invalidate_cached_range(off, data.len());
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
            self.invalidate_all_cached();
            self.mark_origin_failed();
            return Err(error);
        }
        self.origin_dirty = false;
        self.telemetry.origin_syncs = self.telemetry.origin_syncs.saturating_add(1);
        Ok(())
    }
}

impl<P: VramProvider, O: OriginStorage> BlockBackend for WriteThroughCacheBackend<'_, P, O> {
    fn size_bytes(&self) -> u64 {
        self.size
    }

    fn block_size(&self) -> u32 {
        self.block
    }

    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
        self.check_range(off, buf.len())?;
        self.require_ready_origin()?;
        if buf.is_empty() {
            return Ok(());
        }
        if self.range_is_cached(off, buf.len()) {
            match self.cache_read(off, buf) {
                Ok(()) => {
                    self.telemetry.cache_read_bytes = self
                        .telemetry
                        .cache_read_bytes
                        .saturating_add(buf.len() as u64);
                    return Ok(());
                }
                Err(index) => {
                    self.telemetry.cache_read_failures =
                        self.telemetry.cache_read_failures.saturating_add(1);
                    self.invalidate_chunk(index);
                }
            }
        }
        if let Err(error) = self.origin.read_exact_at(off, buf) {
            self.mark_origin_failed();
            return Err(error);
        }
        self.telemetry.fallback_reads = self.telemetry.fallback_reads.saturating_add(1);
        if self.cached_bytes() <= self.target_bytes {
            self.update_cached_chunks(off, buf, true);
        } else {
            self.telemetry.promotion_refusals = self.telemetry.promotion_refusals.saturating_add(1);
        }
        Ok(())
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        self.check_range(off, data.len())?;
        self.require_ready_origin()?;
        if data.is_empty() {
            return Ok(());
        }
        self.write_origin(off, data)?;
        self.telemetry.batched_writes = self.telemetry.batched_writes.saturating_add(1);
        self.update_cached_chunks(off, data, false);
        Ok(())
    }

    fn write_at_with_options(
        &mut self,
        off: u64,
        data: &[u8],
        options: WriteOptions,
    ) -> Result<(), IoError> {
        if !options.fua {
            return self.write_at(off, data);
        }
        self.check_range(off, data.len())?;
        self.require_ready_origin()?;
        if data.is_empty() {
            return Ok(());
        }
        self.write_origin(off, data)?;
        self.sync_dirty_origin()?;
        self.update_cached_chunks(off, data, false);
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
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use ramshared_vram::{
        GpuAdapterIdentity, GpuBudgetSnapshot, GpuBudgetSource, VramError, VramMemory, VramProvider,
    };

    #[derive(Clone)]
    struct ScriptedOrigin {
        bytes: Rc<RefCell<Vec<u8>>>,
        events: Rc<RefCell<Vec<&'static str>>>,
        max_write: usize,
        fail_read: Rc<Cell<bool>>,
        fail_write: Rc<Cell<bool>>,
        fail_sync: Rc<Cell<bool>>,
        zero_write: Rc<Cell<bool>>,
        writes_before_failure: Rc<Cell<usize>>,
    }

    impl ScriptedOrigin {
        fn new(size: usize, events: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                bytes: Rc::new(RefCell::new(vec![0; size])),
                events,
                max_write: usize::MAX,
                fail_read: Rc::new(Cell::new(false)),
                fail_write: Rc::new(Cell::new(false)),
                fail_sync: Rc::new(Cell::new(false)),
                zero_write: Rc::new(Cell::new(false)),
                writes_before_failure: Rc::new(Cell::new(usize::MAX)),
            }
        }
    }

    impl OriginStorage for ScriptedOrigin {
        fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, IoError> {
            self.events.borrow_mut().push("origin_read");
            if self.fail_read.get() {
                return Err(IoError("injected origin read failure".into()));
            }
            let bytes = self.bytes.borrow();
            let start = off as usize;
            let count = buf.len().min(bytes.len().saturating_sub(start));
            buf[..count].copy_from_slice(&bytes[start..start + count]);
            Ok(count)
        }

        fn write_at(&mut self, off: u64, data: &[u8]) -> Result<usize, IoError> {
            self.events.borrow_mut().push("origin_write");
            if self.fail_write.get() {
                return Err(IoError("injected origin write failure".into()));
            }
            let writes_before_failure = self.writes_before_failure.get();
            if writes_before_failure == 0 {
                return Err(IoError("injected partial origin write failure".into()));
            }
            if self.zero_write.get() {
                return Ok(0);
            }
            let count = data.len().min(self.max_write);
            let start = off as usize;
            self.bytes.borrow_mut()[start..start + count].copy_from_slice(&data[..count]);
            self.writes_before_failure
                .set(writes_before_failure.saturating_sub(1));
            Ok(count)
        }

        fn sync_data(&mut self) -> Result<(), IoError> {
            self.events.borrow_mut().push("origin_sync");
            if self.fail_sync.get() {
                Err(IoError("injected origin sync failure".into()))
            } else {
                Ok(())
            }
        }
    }

    #[derive(Clone)]
    struct FakeMem {
        bytes: Rc<RefCell<Vec<u8>>>,
        events: Rc<RefCell<Vec<&'static str>>>,
        fail_read: Rc<Cell<bool>>,
        fail_write: Rc<Cell<bool>>,
    }

    impl VramMemory for FakeMem {
        fn len(&self) -> usize {
            self.bytes.borrow().len()
        }

        fn zero(&mut self) -> Result<(), VramError> {
            self.bytes.borrow_mut().fill(0);
            Ok(())
        }

        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            self.events.borrow_mut().push("cache_read");
            if self.fail_read.get() {
                return Err(VramError::Provider("injected cache read failure".into()));
            }
            let start = off as usize;
            dst.copy_from_slice(&self.bytes.borrow()[start..start + dst.len()]);
            Ok(())
        }

        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            self.events.borrow_mut().push("cache_write");
            if self.fail_write.get() {
                return Err(VramError::Provider("injected cache write failure".into()));
            }
            let start = off as usize;
            self.bytes.borrow_mut()[start..start + src.len()].copy_from_slice(src);
            Ok(())
        }
    }

    struct FakeProvider {
        events: Rc<RefCell<Vec<&'static str>>>,
        fail_alloc: Cell<bool>,
        fail_read: Rc<Cell<bool>>,
        fail_write: Rc<Cell<bool>>,
    }

    impl FakeProvider {
        fn new(events: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                events,
                fail_alloc: Cell::new(false),
                fail_read: Rc::new(Cell::new(false)),
                fail_write: Rc::new(Cell::new(false)),
            }
        }
    }

    impl VramProvider for FakeProvider {
        type Mem<'a> = FakeMem;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            self.events.borrow_mut().push("cache_alloc");
            if self.fail_alloc.get() {
                return Err(VramError::Provider("injected allocation failure".into()));
            }
            Ok(FakeMem {
                bytes: Rc::new(RefCell::new(vec![0; bytes])),
                events: Rc::clone(&self.events),
                fail_read: Rc::clone(&self.fail_read),
                fail_write: Rc::clone(&self.fail_write),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((u64::MAX, u64::MAX))
        }
    }

    /// A fresh, driver-reported, adapter-bound sample at `t0`.
    fn sample_at(
        t0: Instant,
        budget_bytes: u64,
        external_usage_bytes: u64,
        total_vram_bytes: u64,
    ) -> GpuSample {
        GpuSample {
            budget_bytes,
            external_usage_bytes,
            total_vram_bytes,
            sampled_at: t0,
            source: GpuBudgetSource::DriverReported,
            adapter: Some(GpuAdapterIdentity {
                backend: "test".into(),
                key: "test-adapter".into(),
                luid: None,
            }),
        }
    }

    fn healthy_sample() -> GpuSample {
        sample_at(Instant::now(), 4 * GIB, 0, 8 * GIB)
    }

    /// The runtime headroom this crate's default backend applies (DT-4).
    fn runtime() -> u64 {
        crate::gpu_cache_worker::RUNTIME_FREE_BUFFER_BYTES
    }

    fn backend<'a>(
        provider: &'a FakeProvider,
        origin: ScriptedOrigin,
    ) -> WriteThroughCacheBackend<'a, FakeProvider, ScriptedOrigin> {
        WriteThroughCacheBackend::with_chunk_bytes(provider, origin, 32, 4, 8)
            .expect("valid test backend geometry")
    }

    fn grow_one<O: OriginStorage>(backend: &mut WriteThroughCacheBackend<'_, FakeProvider, O>) {
        backend.observe_gpu(
            Some(&healthy_sample()),
            Duration::from_secs(0),
            Instant::now(),
        );
        backend.observe_gpu(
            Some(&healthy_sample()),
            Duration::from_secs(1),
            Instant::now(),
        );
        let outcome = backend.observe_gpu(
            Some(&healthy_sample()),
            Duration::from_secs(2),
            Instant::now(),
        );
        assert_eq!(outcome.allocated_bytes, 8);
    }

    fn assert_write_release_vram_read_origin_hash_matches() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        events.borrow_mut().clear();

        let payload = *b"cache-origin-round-trip-proof-32";
        backend.write_at(0, &payload).unwrap();
        assert_eq!(backend.release_cache(), 8);

        let mut read_back = [0; 32];
        backend.read_at(0, &mut read_back).unwrap();
        assert_eq!(read_back, payload);
        assert_eq!(backend.telemetry().fallback_reads, 1);
        assert_eq!(
            events.borrow().as_slice(),
            ["origin_write", "cache_write", "origin_read"]
        );
    }

    #[test]
    fn origin_write_precedes_cache_update() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        events.borrow_mut().clear();

        backend.write_at(0, &[1, 2, 3, 4]).unwrap();

        assert_eq!(events.borrow().as_slice(), ["origin_write", "cache_write"]);
    }

    #[test]
    fn write_release_vram_read_origin_hash_matches() {
        assert_write_release_vram_read_origin_hash_matches();
    }

    #[test]
    // TestName: write_release_vram_read_origin_hash_parallel_fixtures_are_isolated
    fn write_release_vram_read_origin_hash_parallel_fixtures_are_isolated() {
        std::thread::scope(|scope| {
            let mut workers = Vec::with_capacity(4);
            for _ in 0..4 {
                workers.push(scope.spawn(assert_write_release_vram_read_origin_hash_matches));
            }
            for worker in workers.drain(..) {
                worker.join().unwrap();
            }
        });
    }

    #[test]
    fn gpu_allocation_failure_continues_on_origin() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        provider.fail_alloc.set(true);
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);

        backend.observe_gpu(
            Some(&healthy_sample()),
            Duration::from_secs(0),
            Instant::now(),
        );
        backend.observe_gpu(
            Some(&healthy_sample()),
            Duration::from_secs(1),
            Instant::now(),
        );
        let outcome = backend.observe_gpu(
            Some(&healthy_sample()),
            Duration::from_secs(2),
            Instant::now(),
        );
        assert_eq!(outcome.allocated_bytes, 0);
        backend.write_at(0, b"safe").unwrap();
        let mut read_back = [0; 4];
        backend.read_at(0, &mut read_back).unwrap();
        assert_eq!(&read_back, b"safe");
        assert_eq!(backend.telemetry().allocation_failures, 1);
    }

    #[test]
    fn cache_growth_and_reclaim_hysteresis_is_exact() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);

        assert_eq!(
            backend
                .observe_gpu(Some(&healthy_sample()), Duration::ZERO, Instant::now())
                .allocated_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(
                    Some(&healthy_sample()),
                    Duration::from_secs(1),
                    Instant::now()
                )
                .allocated_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(
                    Some(&healthy_sample()),
                    Duration::from_secs(2),
                    Instant::now()
                )
                .allocated_bytes,
            8
        );
        assert_eq!(
            backend
                .observe_gpu(
                    Some(&healthy_sample()),
                    Duration::from_secs(3),
                    Instant::now()
                )
                .allocated_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(
                    Some(&healthy_sample()),
                    Duration::from_secs(4),
                    Instant::now()
                )
                .allocated_bytes,
            8
        );
        assert_eq!(backend.cached_bytes(), 16);

        let restricted = sample_at(Instant::now(), 0, 0, 8 * GIB);
        assert_eq!(
            backend
                .observe_gpu(Some(&restricted), Duration::from_secs(5), Instant::now())
                .released_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(Some(&restricted), Duration::from_secs(6), Instant::now())
                .released_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(Some(&restricted), Duration::from_secs(7), Instant::now())
                .released_bytes,
            16
        );
        assert_eq!(backend.cached_bytes(), 0);
        assert_eq!(backend.cache_state(), CacheState::Restricted);
    }

    #[test]
    fn configured_physical_cap_bounds_an_ample_gpu_budget() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);
        backend.set_physical_cap_bytes(8);

        let outcome = backend.observe_gpu(Some(&healthy_sample()), Duration::ZERO, Instant::now());
        assert_eq!(outcome.target_bytes, 8);
    }

    #[test]
    fn origin_failure_returns_io_error() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, events);
        origin.fail_write.set(true);
        let mut backend = backend(&provider, origin);

        assert!(backend.write_at(0, b"fail").is_err());
        assert_eq!(backend.origin_state(), OriginState::Failed);
        assert_eq!(backend.telemetry().origin_written_bytes, 0);
        assert!(backend.read_at(0, &mut [0; 4]).is_err());
    }

    #[test]
    fn partial_origin_write_is_completed_before_ack() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let mut origin = ScriptedOrigin::new(32, Rc::clone(&events));
        origin.max_write = 2;
        let bytes = Rc::clone(&origin.bytes);
        let mut backend = backend(&provider, origin);

        backend.write_at(4, b"partial!").unwrap();

        assert_eq!(&bytes.borrow()[4..12], b"partial!");
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| **event == "origin_write")
                .count(),
            4
        );
        assert_eq!(events.borrow().last(), Some(&"origin_write"));
        assert!(!events.borrow().contains(&"origin_sync"));
    }

    #[test]
    fn zero_progress_origin_write_is_never_acknowledged() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, events);
        origin.zero_write.set(true);
        let mut backend = backend(&provider, origin);

        let error = backend.write_at(0, b"stop").unwrap_err();

        assert!(error.0.contains("no progress"));
        assert_eq!(backend.telemetry().origin_written_bytes, 0);
        assert_eq!(backend.telemetry().valid_blocks, 0);
    }

    #[test]
    fn origin_flush_failure_does_not_ack_or_validate_cache() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let fail_sync = Rc::clone(&origin.fail_sync);
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        events.borrow_mut().clear();

        backend.write_at(0, b"nope").unwrap();
        fail_sync.set(true);
        assert!(backend.flush().is_err());
        assert!(events.borrow().contains(&"cache_write"));
        assert_eq!(backend.telemetry().valid_blocks, 0);
        assert_eq!(backend.origin_state(), OriginState::Failed);
    }

    #[test]
    fn partial_origin_failure_invalidates_cached_data_before_recovery_read() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let mut origin = ScriptedOrigin::new(32, Rc::clone(&events));
        origin.max_write = 4;
        let bytes = Rc::clone(&origin.bytes);
        let writes_before_failure = Rc::clone(&origin.writes_before_failure);
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        backend.write_at(0, b"ABCDEFGH").unwrap();

        writes_before_failure.set(1);
        assert!(backend.write_at(0, b"ijklmnop").is_err());
        assert_eq!(&bytes.borrow()[..8], b"ijklEFGH");
        writes_before_failure.set(usize::MAX);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Degraded);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Degraded);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Ready);

        let mut recovered = [0; 8];
        backend.read_at(0, &mut recovered).unwrap();
        assert_eq!(&recovered, b"ijklEFGH");
    }

    #[test]
    fn sync_origin_failure_invalidates_cached_data_before_recovery_read() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let fail_sync = Rc::clone(&origin.fail_sync);
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        backend.write_at(0, b"old!").unwrap();
        backend.flush().unwrap();

        backend.write_at(0, b"new!").unwrap();
        fail_sync.set(true);
        assert!(backend.flush().is_err());
        fail_sync.set(false);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Degraded);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Degraded);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Ready);

        let mut recovered = [0; 4];
        backend.read_at(0, &mut recovered).unwrap();
        assert_eq!(&recovered, b"new!");
    }

    #[test]
    fn durable_origin_write_legitimate_path_passes() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let bytes = Rc::clone(&origin.bytes);
        let mut backend = backend(&provider, origin);

        backend.write_at(8, b"good").unwrap();
        backend.flush().unwrap();

        assert_eq!(&bytes.borrow()[8..12], b"good");
        assert_eq!(backend.telemetry().origin_written_bytes, 4);
        assert_eq!(backend.origin_state(), OriginState::Ready);
    }

    #[test]
    fn normal_writes_batch_until_flush() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);

        backend.write_at(0, b"one!").unwrap();
        backend.write_at(4, b"two!").unwrap();
        assert!(!events.borrow().contains(&"origin_sync"));
        assert_eq!(backend.telemetry().batched_writes, 2);

        backend.flush().unwrap();
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| **event == "origin_sync")
                .count(),
            1
        );
        assert_eq!(backend.telemetry().origin_syncs, 1);
    }

    #[test]
    fn fua_write_syncs_before_ack() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);

        backend
            .write_at_with_options(0, b"fua!", WriteOptions { fua: true })
            .unwrap();

        assert_eq!(events.borrow().as_slice(), ["origin_write", "origin_sync"]);
        assert_eq!(backend.telemetry().origin_syncs, 1);
        assert_eq!(backend.telemetry().batched_writes, 0);
    }

    #[test]
    fn flush_failure_invalidates_dirty_epoch() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let fail_sync = Rc::clone(&origin.fail_sync);
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        backend.write_at(0, b"data").unwrap();
        assert_eq!(backend.telemetry().valid_blocks, 1);

        fail_sync.set(true);
        assert!(backend.flush().is_err());

        assert_eq!(backend.telemetry().valid_blocks, 0);
        assert_eq!(backend.origin_state(), OriginState::Failed);
    }

    /// DT-4/DT-6: the origin target uses the shared reserve floor, not an
    /// independent reserve.
    #[test]
    fn origin_physical_target_uses_shared_reserve_floor() {
        let t0 = Instant::now();
        let sample = sample_at(t0, 10 * GIB, 2 * GIB, 20 * GIB);
        let snapshot = GpuBudgetSnapshot {
            adapter: sample.adapter.clone(),
            total_bytes: Some(sample.total_vram_bytes),
            budget_bytes: sample.budget_bytes,
            used_bytes: sample.external_usage_bytes,
            source: sample.source,
            sampled_at: sample.sampled_at,
        };
        let configured = 0;
        assert_eq!(
            physical_target_bytes(24 * GIB, Some(&sample), configured, runtime(), t0),
            snapshot.safe_target_bytes(24 * GIB, configured, runtime())
        );
    }

    /// DT-6: the `.max(2 * GIB)` term is deleted. At a capacity whose 20%
    /// share is well under 2 GiB the old formula reserved 2 GiB; the shared
    /// formula must produce a strictly larger target.
    #[test]
    fn origin_physical_target_has_no_hardcoded_two_gib_term() {
        let t0 = Instant::now();
        // budget 4 GiB, total 8 GiB → capacity = min(8, 4) = 4 GiB.
        // Shared reserve = ceil(4 GiB / 5) = 0.8 GiB.
        // Old reserve   = max(0.8 GiB, 2 GiB) = 2 GiB.
        let sample = sample_at(t0, 4 * GIB, 0, 8 * GIB);
        let target = physical_target_bytes(4 * GIB, Some(&sample), 0, runtime(), t0);
        let snapshot = GpuBudgetSnapshot {
            adapter: sample.adapter.clone(),
            total_bytes: Some(8 * GIB),
            budget_bytes: 4 * GIB,
            used_bytes: 0,
            source: GpuBudgetSource::DriverReported,
            sampled_at: t0,
        };
        let old_style = 4 * GIB.saturating_sub(0).saturating_sub(2 * GIB);
        assert_eq!(target, snapshot.safe_target_bytes(4 * GIB, 0, runtime()));
        assert!(target < 4 * GIB, "reserve and runtime still bind");
        // The shared floor is the 20% share, not 2 GiB.
        assert!(
            snapshot.required_free_bytes(0, runtime()) < 2 * GIB + runtime(),
            "no 2 GiB constant may remain in the reserve"
        );
        let _ = old_style;
    }

    /// DT-6: `external_usage_bytes` maps to `used_bytes`.
    #[test]
    fn origin_sample_maps_external_usage_to_used_bytes() {
        let t0 = Instant::now();
        let sample = sample_at(t0, 10 * GIB, 3 * GIB, 20 * GIB);
        let snapshot = GpuBudgetSnapshot {
            adapter: sample.adapter.clone(),
            total_bytes: Some(sample.total_vram_bytes),
            budget_bytes: sample.budget_bytes,
            used_bytes: sample.external_usage_bytes,
            source: sample.source,
            sampled_at: sample.sampled_at,
        };
        assert_eq!(snapshot.used_bytes, 3 * GIB);
        assert_eq!(
            physical_target_bytes(24 * GIB, Some(&sample), 0, runtime(), t0),
            snapshot.safe_target_bytes(24 * GIB, 0, runtime())
        );
    }

    /// DT-6: an unnormalizable sample yields zero, not a partial target.
    #[test]
    fn origin_unnormalizable_sample_returns_zero() {
        let t0 = Instant::now();
        // External use above the budget cannot be normalized.
        let over = sample_at(t0, 4 * GIB, 8 * GIB, 16 * GIB);
        assert_eq!(
            physical_target_bytes(4 * GIB, Some(&over), 0, runtime(), t0),
            0
        );
        // A missing sample is the same refusal.
        assert_eq!(physical_target_bytes(4 * GIB, None, 0, runtime(), t0), 0);
    }

    /// DT-6: the path never stamps provenance. `physical_target_bytes` takes
    /// the clock as an argument; it does not call `Instant::now()` and does
    /// not write `GpuBudgetSource::DriverReported`.
    #[test]
    fn origin_path_never_stamps_provenance() {
        let t0 = Instant::now();
        // A sample stamped in the future is refused: the function trusts the
        // sample's own `sampled_at`, it does not replace it with a fresh one.
        let mut future = sample_at(t0, 4 * GIB, 0, 8 * GIB);
        future.sampled_at = t0 + Duration::from_secs(60);
        assert_eq!(
            physical_target_bytes(4 * GIB, Some(&future), 0, runtime(), t0),
            0
        );
        // A non-driver source is never normalized to DriverReported.
        let mut lab = sample_at(t0, 4 * GIB, 0, 8 * GIB);
        lab.source = GpuBudgetSource::ProviderLocalEstimate;
        assert_eq!(
            physical_target_bytes(4 * GIB, Some(&lab), 0, runtime(), t0),
            0
        );
    }
    #[test]
    fn origin_stale_sample_refuses() {
        let t0 = Instant::now();
        let mut stale = sample_at(t0, 4 * GIB, 0, 8 * GIB);
        stale.sampled_at = t0 - (SAMPLE_MAX_AGE + Duration::from_secs(1));
        // A stale sample must yield no target at all, not a reduced one.
        assert_eq!(
            physical_target_bytes(4 * GIB, Some(&stale), 0, runtime(), t0),
            0
        );
        // The same sample inside the window is admitted. The value is the
        // shared formula, not the requested size: capacity = min(8G, 4G) =
        // 4G; reserve = ceil(4G/5); within_live_headroom = 4G - reserve -
        // runtime. Both terms bind below the requested 4G.
        let fresh = sample_at(t0, 4 * GIB, 0, 8 * GIB);
        let admitted = physical_target_bytes(4 * GIB, Some(&fresh), 0, runtime(), t0);
        assert!(admitted > 0, "fresh sample must be admitted");
        assert!(admitted < 4 * GIB, "reserve and runtime headroom must bind");
        let snapshot = GpuBudgetSnapshot {
            adapter: fresh.adapter.clone(),
            total_bytes: Some(fresh.total_vram_bytes),
            budget_bytes: fresh.budget_bytes,
            used_bytes: fresh.external_usage_bytes,
            source: fresh.source,
            sampled_at: fresh.sampled_at,
        };
        assert_eq!(admitted, snapshot.safe_target_bytes(4 * GIB, 0, runtime()));
    }

    #[test]
    fn origin_untrusted_sample_refuses() {
        let t0 = Instant::now();
        // Source other than DriverReported.
        let mut lab = sample_at(t0, 4 * GIB, 0, 8 * GIB);
        lab.source = GpuBudgetSource::ProviderLocalEstimate;
        assert_eq!(
            physical_target_bytes(4 * GIB, Some(&lab), 0, runtime(), t0),
            0
        );
        // Missing adapter identity.
        let mut anonymous = sample_at(t0, 4 * GIB, 0, 8 * GIB);
        anonymous.adapter = None;
        assert_eq!(
            physical_target_bytes(4 * GIB, Some(&anonymous), 0, runtime(), t0),
            0
        );
    }

    #[test]
    fn physical_target_bytes_refuses_inconsistent_sample() {
        let t0 = Instant::now();
        // External use above the budget.
        let over_used = sample_at(t0, 4 * GIB, 8 * GIB, 16 * GIB);
        assert_eq!(
            physical_target_bytes(4 * GIB, Some(&over_used), 0, runtime(), t0),
            0
        );
        // Budget above the physical total.
        let over_budget = sample_at(t0, 16 * GIB, 0, 8 * GIB);
        assert_eq!(
            physical_target_bytes(4 * GIB, Some(&over_budget), 0, runtime(), t0),
            0
        );
    }

    #[test]
    fn exact_target_formula_and_missing_measurement_fail_safe() {
        let t0 = Instant::now();
        assert_eq!(physical_target_bytes(4 * GIB, None, 0, runtime(), t0), 0);
        // capacity = min(total 20G, budget 10G) = 10G.
        // reserve = max(0, ceil(10G/5)) = 2G   (no 2 GiB constant).
        // within_capacity = 8G.
        // within_live_headroom = (10G-2G) - 2G - runtime = 6G - runtime.
        let ten = sample_at(t0, 10 * GIB, 2 * GIB, 20 * GIB);
        let expected = 6 * GIB - runtime();
        assert_eq!(
            physical_target_bytes(24 * GIB, Some(&ten), 0, runtime(), t0),
            expected
        );
        // The old formula reserved max(total/5, 2 GiB) = 4G here and returned
        // 4G. The shared formula reserves 2G, so the target is higher — the
        // hardcoded 2 GiB floor is gone. Prove the direction.
        assert!(expected > 4 * GIB);
        // Logical size still binds.
        let eight = sample_at(t0, 8 * GIB, 0, 8 * GIB);
        assert_eq!(
            physical_target_bytes(GIB, Some(&eight), 0, runtime(), t0),
            GIB
        );
    }

    #[test]
    fn cache_io_failures_invalidate_and_fall_back_without_eio() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        backend.write_at(0, b"data").unwrap();
        provider.fail_read.set(true);

        let mut read_back = [0; 4];
        backend.read_at(0, &mut read_back).unwrap();

        assert_eq!(&read_back, b"data");
        assert_eq!(backend.telemetry().cache_read_failures, 1);
        assert!(backend.telemetry().invalidations >= 1);

        provider.fail_read.set(false);
        provider.fail_write.set(true);
        backend.write_at(0, b"next").unwrap();
        assert_eq!(backend.telemetry().cache_write_failures, 1);
        let mut after_write_failure = [0; 4];
        backend.read_at(0, &mut after_write_failure).unwrap();
        assert_eq!(&after_write_failure, b"next");
    }

    #[test]
    fn restricted_reclaim_releases_least_recent_clean_chunk() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        assert_eq!(
            backend
                .observe_gpu(
                    Some(&healthy_sample()),
                    Duration::from_secs(4),
                    Instant::now()
                )
                .allocated_bytes,
            8
        );
        backend.write_at(0, b"zero").unwrap();
        backend.write_at(8, b"one!").unwrap();
        let mut make_first_recent = [0; 4];
        backend.read_at(0, &mut make_first_recent).unwrap();

        // Isolate LRU reclaim from the reserve formula: bind the reserve so
        // `safe_target_bytes` returns exactly one 8-byte chunk. capacity =
        // min(total 8G, budget 2G+8) = 2G+8; reserve = 2G; within_capacity = 8;
        // within_live_headroom = (2G+8) - 2G - 0 = 8. The configured reserve is
        // doing the work here, not any hardcoded floor.
        backend.set_reserve_floor(2 * GIB, 0);
        let one_chunk_target = sample_at(Instant::now(), 2 * GIB + 8, 0, 8 * GIB);
        backend.observe_gpu(
            Some(&one_chunk_target),
            Duration::from_secs(5),
            Instant::now(),
        );
        backend.observe_gpu(
            Some(&one_chunk_target),
            Duration::from_secs(6),
            Instant::now(),
        );
        let outcome = backend.observe_gpu(
            Some(&one_chunk_target),
            Duration::from_secs(7),
            Instant::now(),
        );
        assert_eq!(outcome.released_bytes, 8);

        events.borrow_mut().clear();
        backend.read_at(0, &mut [0; 4]).unwrap();
        backend.read_at(8, &mut [0; 4]).unwrap();
        assert_eq!(events.borrow().as_slice(), ["cache_read", "origin_read"]);
    }

    #[test]
    fn reallocated_chunk_requires_current_generation_validity() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, Rc::clone(&events));
        let mut backend = backend(&provider, origin);
        grow_one(&mut backend);
        backend.write_at(0, b"gen!").unwrap();
        backend.release_cache();
        assert_eq!(
            backend
                .observe_gpu(
                    Some(&healthy_sample()),
                    Duration::from_secs(4),
                    Instant::now()
                )
                .allocated_bytes,
            8
        );

        events.borrow_mut().clear();
        let mut read_back = [0; 4];
        backend.read_at(0, &mut read_back).unwrap();

        assert_eq!(&read_back, b"gen!");
        assert_eq!(events.borrow().as_slice(), ["origin_read", "cache_write"]);
    }

    #[test]
    fn origin_failure_is_sticky_until_three_read_sync_probes() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(32, events);
        let fail_write = Rc::clone(&origin.fail_write);
        let mut backend = backend(&provider, origin);
        fail_write.set(true);
        assert!(backend.write_at(0, b"fail").is_err());
        fail_write.set(false);

        assert_eq!(backend.probe_origin().unwrap(), OriginState::Degraded);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Degraded);
        assert_eq!(backend.probe_origin().unwrap(), OriginState::Ready);
    }

    #[test]
    fn production_constructor_seals_chunk_size_at_128_mib() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let provider = FakeProvider::new(Rc::clone(&events));
        let origin = ScriptedOrigin::new(8, events);
        let backend = WriteThroughCacheBackend::new(&provider, origin, 8, 4).unwrap();
        assert_eq!(backend.chunk_bytes(), ORIGIN_CACHE_CHUNK_BYTES);
    }
}
