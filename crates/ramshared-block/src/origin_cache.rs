//! Durable origin with a revocable, best-effort VRAM block cache.
use std::cmp;
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::time::Duration;

use ramshared_vram::{VramMemory, VramProvider};

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
        self.file
            .read_at(buf, off)
            .map_err(|error| IoError(error.to_string()))
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<usize, IoError> {
        self.file
            .write_at(data, off)
            .map_err(|error| IoError(error.to_string()))
    }

    fn sync_data(&mut self) -> Result<(), IoError> {
        self.file
            .sync_data()
            .map_err(|error| IoError(error.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuSample {
    pub budget_bytes: u64,
    pub external_usage_bytes: u64,
    pub total_vram_bytes: u64,
}

pub fn physical_target_bytes(logical_bytes: u64, sample: Option<GpuSample>) -> u64 {
    let Some(sample) = sample else {
        return 0;
    };
    let reserve = (sample.total_vram_bytes.div_ceil(5)).max(2 * GIB);
    sample
        .budget_bytes
        .saturating_sub(sample.external_usage_bytes)
        .saturating_sub(reserve)
        .min(logical_bytes)
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

    pub fn observe_gpu(&mut self, sample: Option<GpuSample>, now: Duration) -> CachePolicyOutcome {
        let target = physical_target_bytes(self.physical_cap_bytes, sample);
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
                .is_none_or(|previous| now.saturating_sub(previous) >= GROWTH_INTERVAL)
        {
            self.last_growth_at = Some(now);
            match self.allocate_one_chunk() {
                Ok(bytes) => allocated_bytes = bytes,
                Err(()) => self.cache_state = CacheState::Unavailable,
            }
        }

        let excess = self.cached_bytes().saturating_sub(target);
        if excess > self.chunk_bytes {
            let since = self.over_target_since.get_or_insert(now);
            if now.saturating_sub(*since) > STUCK_AFTER {
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

    use ramshared_vram::{VramError, VramMemory, VramProvider};

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

    fn healthy_sample() -> GpuSample {
        GpuSample {
            budget_bytes: 4 * GIB,
            external_usage_bytes: 0,
            total_vram_bytes: 8 * GIB,
        }
    }

    fn backend<'a>(
        provider: &'a FakeProvider,
        origin: ScriptedOrigin,
    ) -> WriteThroughCacheBackend<'a, FakeProvider, ScriptedOrigin> {
        WriteThroughCacheBackend::with_chunk_bytes(provider, origin, 32, 4, 8).unwrap()
    }

    fn grow_one<O: OriginStorage>(backend: &mut WriteThroughCacheBackend<'_, FakeProvider, O>) {
        backend.observe_gpu(Some(healthy_sample()), Duration::from_secs(0));
        backend.observe_gpu(Some(healthy_sample()), Duration::from_secs(1));
        let outcome = backend.observe_gpu(Some(healthy_sample()), Duration::from_secs(2));
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
            let workers = (0..4)
                .map(|_| scope.spawn(assert_write_release_vram_read_origin_hash_matches))
                .collect::<Vec<_>>();
            for worker in workers {
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

        backend.observe_gpu(Some(healthy_sample()), Duration::from_secs(0));
        backend.observe_gpu(Some(healthy_sample()), Duration::from_secs(1));
        let outcome = backend.observe_gpu(Some(healthy_sample()), Duration::from_secs(2));
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
                .observe_gpu(Some(healthy_sample()), Duration::ZERO)
                .allocated_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(Some(healthy_sample()), Duration::from_secs(1))
                .allocated_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(Some(healthy_sample()), Duration::from_secs(2))
                .allocated_bytes,
            8
        );
        assert_eq!(
            backend
                .observe_gpu(Some(healthy_sample()), Duration::from_secs(3))
                .allocated_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(Some(healthy_sample()), Duration::from_secs(4))
                .allocated_bytes,
            8
        );
        assert_eq!(backend.cached_bytes(), 16);

        let restricted = GpuSample {
            budget_bytes: 0,
            external_usage_bytes: 0,
            total_vram_bytes: 8 * GIB,
        };
        assert_eq!(
            backend
                .observe_gpu(Some(restricted), Duration::from_secs(5))
                .released_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(Some(restricted), Duration::from_secs(6))
                .released_bytes,
            0
        );
        assert_eq!(
            backend
                .observe_gpu(Some(restricted), Duration::from_secs(7))
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

        let outcome = backend.observe_gpu(Some(healthy_sample()), Duration::ZERO);
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

    #[test]
    fn exact_target_formula_and_missing_measurement_fail_safe() {
        assert_eq!(physical_target_bytes(4 * GIB, None), 0);
        assert_eq!(
            physical_target_bytes(
                24 * GIB,
                Some(GpuSample {
                    budget_bytes: 10 * GIB,
                    external_usage_bytes: 2 * GIB,
                    total_vram_bytes: 20 * GIB,
                })
            ),
            4 * GIB
        );
        assert_eq!(
            physical_target_bytes(
                GIB,
                Some(GpuSample {
                    budget_bytes: 8 * GIB,
                    external_usage_bytes: 0,
                    total_vram_bytes: 8 * GIB,
                })
            ),
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
                .observe_gpu(Some(healthy_sample()), Duration::from_secs(4))
                .allocated_bytes,
            8
        );
        backend.write_at(0, b"zero").unwrap();
        backend.write_at(8, b"one!").unwrap();
        let mut make_first_recent = [0; 4];
        backend.read_at(0, &mut make_first_recent).unwrap();

        let one_chunk_target = GpuSample {
            budget_bytes: 2 * GIB + 8,
            external_usage_bytes: 0,
            total_vram_bytes: 8 * GIB,
        };
        backend.observe_gpu(Some(one_chunk_target), Duration::from_secs(5));
        backend.observe_gpu(Some(one_chunk_target), Duration::from_secs(6));
        let outcome = backend.observe_gpu(Some(one_chunk_target), Duration::from_secs(7));
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
                .observe_gpu(Some(healthy_sample()), Duration::from_secs(4))
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
