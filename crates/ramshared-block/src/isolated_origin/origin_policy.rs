use crate::origin_cache::{CacheState, CacheTelemetry, OriginState, OriginStorage};
use crate::{BlockBackend, IoError, WriteOptions};
use super::origin_tracker::{BestEffortCache, CacheMutation, CacheRead};
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

    pub fn target_bytes(&self) -> u64 {
        self.cache.target_bytes()
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

    fn update_cache(&mut self, offset: u64, data: &[u8]) {
        if self.cache.update(offset, data) == CacheMutation::Failed {
            self.telemetry.cache_write_failures =
                self.telemetry.cache_write_failures.saturating_add(1);
            let _ = self.revoke_cache();
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
        if self.cache.promote(offset, destination) == CacheMutation::Failed {
            self.telemetry.promotion_refusals = self.telemetry.promotion_refusals.saturating_add(1);
            let _ = self.revoke_cache();
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
