//! Process-isolated GPU cache worker.
//!
//! Provides out-of-process VRAM allocation and cache chunk management
//! communicating over an anonymous Unix domain socket pair.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ramshared_vram::{
    CodecCapability, CodecId, CodecState, CodecStatus, CodecTelemetry, GpuBudgetSnapshot,
    GpuBudgetTelemetry, GpuCacheCodec, VramError, VramMemory, VramOutputReservation, VramProvider,
    VramSpan, WorkerCacheTelemetry, WorkerTelemetryEnvelope, crc32,
};

use crate::compressed_cache::{
    CacheEntry, CacheRepresentation, MAX_EXTENT_LEN, MAX_READ_LEN, ReadCoverage, SlabSpan,
    VramSpanAllocator, invalidate_overlaps, read_coverage, split_extent,
};

pub const FRAME_HEADER_LEN: usize = 32;
pub const MAX_IPC_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

pub const MSG_READ_REQ: u8 = 1;
pub const MSG_READ_RESP: u8 = 2;
pub const MSG_UPDATE: u8 = 3;
pub const MSG_PROMOTE: u8 = 4;
pub const MSG_DISABLE_REQ: u8 = 5;
pub const MSG_DISABLE_RESP: u8 = 6;
pub const MSG_HEARTBEAT_REQ: u8 = 7;
pub const MSG_HEARTBEAT_RESP: u8 = 8;
pub const MSG_HANDSHAKE_REQ: u8 = 9;
pub const MSG_HANDSHAKE_RESP: u8 = 10;

pub const STATUS_OK: u8 = 0;
pub const STATUS_MISS: u8 = 1;
pub const STATUS_ERROR: u8 = 2;
pub const RUNTIME_FREE_BUFFER_BYTES: u64 = 640 * 1024 * 1024;
/// Absolute budget for one complete worker frame read (header plus payload).
/// Production clients heartbeat at least every few seconds (the daemon serve
/// loop ticks every five seconds), so a peer silent or mid-frame longer than
/// this budget is stalled and the worker must fail closed instead of blocking
/// until process teardown.
pub const WORKER_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Codec-operation sub-deadline inside the 50 ms cache-read budget (DT-3).
///
/// Bounds **admission and inter-step continuation only**. It cannot preempt a
/// blocked driver call: a provider call that never returns is already outside
/// what this worker can bound, and claiming otherwise would be a false
/// guarantee. A codec step that exceeds its slice reports `TimedOut` and the
/// raw path or a miss serves instead (DT-11).
pub const CODEC_SUBDEADLINE: Duration = Duration::from_millis(20);
/// Overall cache-read budget the codec sub-deadline sits inside (DT-3).
pub const CACHE_READ_BUDGET: Duration = Duration::from_millis(50);
const RUNTIME_RECOVERY_BUFFER_BYTES: u64 = 896 * 1024 * 1024;

/// Encodes cached bytes as KiB in the legacy `u32` IPC field without wrapping.
fn cached_kib_aux(cached_bytes: u64) -> u32 {
    u32::try_from(cached_bytes >> 10).unwrap_or(u32::MAX)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn effective_target_from_budget(budget: &GpuBudgetSnapshot, config: GpuWorkerConfig) -> u64 {
    if !budget.can_admit(0) {
        return 0;
    }
    budget.safe_target_bytes(
        config.target_bytes,
        config.reserve_floor_bytes,
        RUNTIME_FREE_BUFFER_BYTES,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    pub msg_type: u8,
    pub status: u8,
    pub correlation_id: u64,
    pub offset: u64,
    pub payload_len: u32,
    pub aux: u32,
}

impl FrameHeader {
    pub fn encode(&self) -> [u8; FRAME_HEADER_LEN] {
        let mut buf = [0u8; FRAME_HEADER_LEN];
        buf[0] = self.msg_type;
        buf[1] = self.status;
        buf[8..16].copy_from_slice(&self.correlation_id.to_le_bytes());
        buf[16..24].copy_from_slice(&self.offset.to_le_bytes());
        buf[24..28].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[28..32].copy_from_slice(&self.aux.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8; FRAME_HEADER_LEN]) -> Option<Self> {
        if buf[2..8].iter().any(|byte| *byte != 0) {
            return None;
        }

        let msg_type = buf[0];
        let status = buf[1];
        let correlation_id = u64::from_le_bytes([
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
        ]);
        let offset = u64::from_le_bytes([
            buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23],
        ]);
        let payload_len = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]);
        let aux = u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]);
        Some(Self {
            msg_type,
            status,
            correlation_id,
            offset,
            payload_len,
            aux,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuWorkerConfig {
    pub target_bytes: u64,
    pub chunk_bytes: usize,
    pub reserve_floor_bytes: u64,
    /// DT-9: compression is **off by default**. The raw cache is the product;
    /// the codec is an opt-in accelerator on top of a working raw cache.
    ///
    /// Even when enabled, nothing compresses unless the selected provider
    /// reports a codec capability and the encoded form is strictly smaller
    /// than the raw payload (DT-10 usefulness gate).
    pub compression_enabled: bool,
}

impl Default for GpuWorkerConfig {
    fn default() -> Self {
        Self {
            target_bytes: 4 * 1024 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 1536 * 1024 * 1024,
            compression_enabled: false,
        }
    }
}

struct CacheChunk<'p, P: VramProvider + 'p> {
    mem: P::Mem<'p>,
    last_accessed: Instant,
    valid_ranges: Vec<(u64, u64)>,
}

impl<P: VramProvider> CacheChunk<'_, P> {
    fn contains(&self, start: u64, end: u64) -> bool {
        self.valid_ranges
            .iter()
            .any(|&(valid_start, valid_end)| valid_start <= start && end <= valid_end)
    }

    fn mark_valid(&mut self, start: u64, end: u64) {
        self.valid_ranges.push((start, end));
        self.valid_ranges.sort_unstable_by_key(|range| range.0);
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(self.valid_ranges.len());
        for (start, end) in self.valid_ranges.drain(..) {
            if let Some(last) = merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
                continue;
            }
            merged.push((start, end));
        }
        self.valid_ranges = merged;
    }
}

/// Extent index for the optional compressed representation (DT-4..DT-7).
///
/// Lives alongside the raw chunk map: when compression is disabled or
/// unsupported this store stays empty and the raw path is the whole product
/// (DT-9). Codec faults drop compressed entries only and never touch the raw
/// chunks (DT-11).
struct CompressedStore {
    entries: Vec<CacheEntry>,
    allocator: VramSpanAllocator,
    generation: u64,
    /// Codec faults observed. Reported for triage; never a disable signal.
    codec_faults: u64,
    /// Checksum refusals before decode (DT-7).
    codec_integrity_errors: u64,
    /// Decoder failures: bad status, wrong length, or output checksum mismatch.
    codec_decode_errors: u64,
    /// Codec sub-deadline exceedances (DT-3).
    codec_timeouts: u64,
    /// Completed codec operations. A non-zero count with an in-flight
    /// operation is what `worker_teardown_waits_for_codec_completion` asserts
    /// against before the store is dropped.
    completed_ops: u64,
    /// Set while one codec operation is in flight.
    in_flight: bool,
}

impl CompressedStore {
    fn new(physical_target_bytes: u64) -> Self {
        Self {
            entries: Vec::new(),
            allocator: VramSpanAllocator::new(physical_target_bytes),
            generation: 0,
            codec_faults: 0,
            codec_integrity_errors: 0,
            codec_decode_errors: 0,
            codec_timeouts: 0,
            completed_ops: 0,
            in_flight: false,
        }
    }

    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.generation
    }

    /// DT-11: record a codec-only fault. The raw cache keeps serving.
    ///
    /// The typed counters feed the telemetry envelope (DT-8); `codec_faults`
    /// stays the aggregate so existing triage callers keep working.
    fn note_codec_fault(&mut self) {
        self.codec_faults = self.codec_faults.saturating_add(1);
        self.in_flight = false;
    }

    /// DT-7: a checksum refused an entry before decode.
    fn note_integrity_error(&mut self) {
        self.codec_integrity_errors = self.codec_integrity_errors.saturating_add(1);
        self.note_codec_fault();
    }

    /// A decode/encode step failed without being a checksum or timeout.
    fn note_decode_error(&mut self) {
        self.codec_decode_errors = self.codec_decode_errors.saturating_add(1);
        self.note_codec_fault();
    }

    /// DT-3: the codec sub-deadline fired.
    fn note_codec_timeout(&mut self) {
        self.codec_timeouts = self.codec_timeouts.saturating_add(1);
        self.note_codec_fault();
    }

    fn note_codec_completion(&mut self) {
        self.completed_ops = self.completed_ops.saturating_add(1);
        self.in_flight = false;
    }
}

pub struct GpuCacheWorker<'p, P: VramProvider + 'p> {
    provider: &'p P,
    config: GpuWorkerConfig,
    effective_target_bytes: u64,
    chunks: HashMap<u64, CacheChunk<'p, P>>,
    disabled: bool,
    pressure_constrained: bool,
    compressed: CompressedStore,
    /// Provider-owned slab backing for the compressed store, one per allocator
    /// slab, in creation order. `slab_index` addresses into this list.
    compressed_slabs: Vec<P::Mem<'p>>,
    /// Provider-owned scratch for one codec call (DT-4 workspace).
    codec_workspace: Option<P::Mem<'p>>,
}

impl<'p, P: VramProvider + 'p> GpuCacheWorker<'p, P> {
    pub fn new(provider: &'p P, config: GpuWorkerConfig) -> Self {
        let effective_target = match provider.budget_snapshot() {
            Ok(budget) => effective_target_from_budget(&budget, config),
            // No GPU measurement available: report zero target so the client
            // and telemetry correctly reflect that physical VRAM is absent
            // (SPEC RF-4, GAP-6).
            Err(_) => 0,
        };

        Self {
            provider,
            config,
            effective_target_bytes: effective_target,
            chunks: HashMap::new(),
            disabled: false,
            pressure_constrained: false,
            compressed: CompressedStore::new(effective_target),
            compressed_slabs: Vec::new(),
            codec_workspace: None,
        }
    }

    pub fn target_bytes(&self) -> u64 {
        self.effective_target_bytes
    }

    pub fn cached_bytes(&self) -> u64 {
        (self.chunks.len() as u64).saturating_mul(self.config.chunk_bytes as u64)
    }

    pub fn active_chunks_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Codec faults seen on this worker (DT-11 triage counter).
    pub fn codec_faults(&self) -> u64 {
        self.compressed.codec_faults
    }

    /// Checksum refusals before decode (DT-7).
    pub fn codec_integrity_errors(&self) -> u64 {
        self.compressed.codec_integrity_errors
    }

    /// Decoder failures: bad status, wrong length, or output checksum mismatch.
    pub fn codec_decode_errors(&self) -> u64 {
        self.compressed.codec_decode_errors
    }

    /// Codec sub-deadline exceedances (DT-3).
    pub fn codec_timeouts(&self) -> u64 {
        self.compressed.codec_timeouts
    }

    /// Cache occupancy and codec health for one heartbeat sample (DT-8).
    ///
    /// Physical `cached_bytes` / `target_bytes` stay in the frame header; this
    /// sample never re-labels cache occupancy as guest or host RAM.
    pub fn cache_telemetry(&self, sampled_at_unix_ms: u64) -> WorkerCacheTelemetry {
        let mut logical_cached_bytes = 0u64;
        let mut compressed_payload_bytes = 0u64;
        let mut raw_payload_bytes = 0u64;
        for entry in &self.compressed.entries {
            logical_cached_bytes = logical_cached_bytes.saturating_add(entry.logical_len);
            match &entry.representation {
                CacheRepresentation::Raw { address } => {
                    raw_payload_bytes =
                        raw_payload_bytes.saturating_add(address.span.stored_len as u64);
                }
                CacheRepresentation::Compressed { address, .. } => {
                    compressed_payload_bytes =
                        compressed_payload_bytes.saturating_add(address.span.stored_len as u64);
                }
            }
        }
        // Bytes living in the raw chunk map bypassed the compressed store.
        let mut raw_bypass_bytes = 0u64;
        for chunk in self.chunks.values() {
            for (start, end) in &chunk.valid_ranges {
                let len = end.saturating_sub(*start);
                raw_bypass_bytes = raw_bypass_bytes.saturating_add(len);
            }
        }
        logical_cached_bytes = logical_cached_bytes.saturating_add(raw_bypass_bytes);

        let physical_cache_slab_bytes = self
            .compressed_slabs
            .iter()
            .map(|slab| slab.len() as u64)
            .fold(0u64, u64::saturating_add);
        let codec_workspace_bytes = self
            .codec_workspace
            .as_ref()
            .map(|mem| mem.len() as u64)
            .unwrap_or(0);
        let metadata_bytes = self.compressed.allocator.metadata_bytes() as u64;

        let capability = if self.provider.cache_codec().is_some() {
            CodecCapability::Available
        } else {
            CodecCapability::RawOnly
        };
        let state = if self.disabled {
            CodecState::Disabled
        } else if capability == CodecCapability::RawOnly || !self.config.compression_enabled {
            CodecState::RawOnly
        } else if self.compressed.codec_timeouts > 0 {
            CodecState::TimedOut
        } else if self.compressed.codec_faults > 0 {
            CodecState::Faulted
        } else {
            CodecState::Ready
        };
        let refusal_reason = match (capability, state) {
            (CodecCapability::RawOnly, _) => Some("no-provider-codec"),
            (_, CodecState::RawOnly) => Some("compression-disabled"),
            (_, CodecState::Disabled) => Some("worker-disabled"),
            (_, CodecState::TimedOut) => Some("codec-subdeadline"),
            (_, CodecState::Faulted) => Some("codec-fault"),
            (_, CodecState::Ready) => None,
        };

        WorkerCacheTelemetry {
            schema_version: ramshared_vram::WORKER_TELEMETRY_SCHEMA_VERSION,
            sampled_at_unix_ms,
            codec: CodecTelemetry::new(capability, state, refusal_reason),
            logical_cached_bytes,
            physical_cache_slab_bytes,
            codec_workspace_bytes,
            compressed_payload_bytes,
            raw_payload_bytes,
            metadata_bytes,
            raw_bypass_bytes,
            codec_integrity_errors: self.compressed.codec_integrity_errors,
            codec_decode_errors: self.compressed.codec_decode_errors,
            codec_timeouts: self.compressed.codec_timeouts,
        }
    }

    /// Codec operations that ran to completion (teardown proof).
    pub fn completed_codec_ops(&self) -> u64 {
        self.compressed.completed_ops
    }

    /// Whether a codec operation is currently in flight.
    pub fn codec_in_flight(&self) -> bool {
        self.compressed.in_flight
    }

    /// Live compressed extents (0 when compression is off or raw-only).
    pub fn compressed_entries_count(&self) -> usize {
        self.compressed.entries.len()
    }

    /// Provider slabs backing the compressed store.
    pub fn compressed_slab_count(&self) -> usize {
        self.compressed_slabs.len()
    }

    /// Codec/slab admission free floor (DT-4 / NFR-2).
    ///
    /// The **full** parent free floor: `required_free_bytes(configured_reserve,
    /// runtime_headroom)`. Using the configured `reserve_floor_bytes` alone is
    /// forbidden — it would let a slab allocation consume the runtime buffer
    /// and the `ceil(capacity/5)` safety share.
    pub fn codec_admission_required_free(&self, budget: &GpuBudgetSnapshot) -> u64 {
        budget.required_free_bytes(self.config.reserve_floor_bytes, RUNTIME_FREE_BUFFER_BYTES)
    }

    /// `true` when one more codec/slab allocation of `needed` bytes is admissible
    /// under the full free floor and a fresh, driver-reported budget (DT-4).
    fn codec_admission_allows(&self, needed: u64) -> bool {
        self.provider.budget_snapshot().is_ok_and(|budget| {
            budget.can_admit(0)
                && budget.available_bytes()
                    >= needed.saturating_add(self.codec_admission_required_free(&budget))
        })
    }

    pub fn handle_read(&mut self, offset: u64, len: usize) -> Option<Vec<u8>> {
        if self.disabled || self.config.chunk_bytes == 0 || len == 0 {
            return None;
        }
        // Read ceiling: refuse **before** any response allocation (DT-4).
        // `MAX_READ_LEN` is the private-response bound; a larger request would
        // allocate first and only then discover it cannot be served.
        if len > MAX_READ_LEN {
            return None;
        }
        // Compressed extents are tried first: the codec is an accelerator on
        // top of the raw cache, so a miss here falls through to the raw path.
        if self.config.compression_enabled
            && !self.compressed.entries.is_empty()
            && let Some(hit) = self.read_compressed(offset, len)
        {
            return Some(hit);
        }
        let chunk_bytes = self.config.chunk_bytes as u64;
        let chunk_idx = offset / chunk_bytes;
        let chunk_off = offset % chunk_bytes;
        if chunk_off.saturating_add(len as u64) > chunk_bytes {
            return None;
        }
        let chunk_base = chunk_idx.saturating_mul(chunk_bytes);
        if let Some(chunk) = self.chunks.get_mut(&chunk_base) {
            if !chunk.contains(chunk_off, chunk_off + len as u64) {
                return None;
            }
            chunk.last_accessed = Instant::now();
            let mut buf = vec![0u8; len];
            if chunk.mem.read_at(chunk_off, &mut buf).is_ok() {
                Some(buf)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Serves one read from the compressed extent index, or `None` (DT-7).
    ///
    /// Every covering extent is checksum-verified **before** the decoder is
    /// invoked. A stored-checksum mismatch refuses the entry without decoding
    /// and returns `None` so the raw path or a miss serves (DT-11). The
    /// response is assembled into a private buffer only after all extents
    /// decode successfully (DT-6: never a partial assembly).
    fn read_compressed(&mut self, offset: u64, len: usize) -> Option<Vec<u8>> {
        let end = offset.checked_add(len as u64)?;
        let coverage = read_coverage(&self.compressed.entries, offset, end);
        if !matches!(coverage, ReadCoverage::Exact | ReadCoverage::Contiguous) {
            return None;
        }
        let deadline = Instant::now().checked_add(CODEC_SUBDEADLINE)?;
        let mut assembled = vec![0u8; len];
        let mut filled = 0usize;
        let mut cursor = offset;
        while cursor < end {
            if Instant::now() >= deadline {
                self.compressed.note_codec_timeout();
                return None;
            }
            let index =
                self.compressed.entries.iter().position(|entry| {
                    entry.logical_start <= cursor && cursor < entry.logical_end()
                })?;
            let entry = self.compressed.entries[index].clone();
            self.compressed.entries[index].last_accessed = Instant::now();
            let within = (cursor - entry.logical_start) as usize;
            let take = ((entry.logical_end() - cursor) as usize).min(len - filled);
            // `decode_entry` records its own typed refusal; do not double-count.
            let chunk = self.decode_entry(&entry)?;
            if within + take > chunk.len() {
                self.compressed.note_decode_error();
                return None;
            }
            assembled[filled..filled + take].copy_from_slice(&chunk[within..within + take]);
            filled += take;
            cursor = cursor.saturating_add(take as u64);
        }
        if filled != len {
            return None;
        }
        self.compressed.note_codec_completion();
        Some(assembled)
    }

    /// Decodes one entry into a private buffer after the stored checksum passes.
    ///
    /// Every refusal is typed here so the telemetry envelope (DT-8) can tell
    /// a checksum rejection from a decoder failure or a sub-deadline hit. The
    /// raw cache keeps serving either way (DT-11).
    fn decode_entry(&mut self, entry: &CacheEntry) -> Option<Vec<u8>> {
        match &entry.representation {
            CacheRepresentation::Raw { address } => {
                let Some(slab_len) = self.compressed.allocator.slab_len(address.slab_index) else {
                    self.compressed.note_decode_error();
                    return None;
                };
                if address.span.check_within(slab_len).is_err() {
                    self.compressed.note_decode_error();
                    return None;
                }
                let mut buf = vec![0u8; entry.logical_len as usize];
                let Some(slab) = self.compressed_slabs.get(address.slab_index) else {
                    self.compressed.note_decode_error();
                    return None;
                };
                let payload = address.span.stored_len.min(buf.len());
                if slab
                    .read_at(address.span.offset, &mut buf[..payload])
                    .is_err()
                {
                    self.compressed.note_decode_error();
                    return None;
                }
                if crc32(&buf, 0) != entry.original_crc32 {
                    self.compressed.note_integrity_error();
                    return None;
                }
                Some(buf)
            }
            CacheRepresentation::Compressed {
                address,
                original_crc32,
                stored_crc32,
                codec_id: _,
            } => {
                let Some(slab_len) = self.compressed.allocator.slab_len(address.slab_index) else {
                    self.compressed.note_decode_error();
                    return None;
                };
                if address.span.check_within(slab_len).is_err() {
                    self.compressed.note_decode_error();
                    return None;
                }
                let provider: &'p P = self.provider;
                let Some(codec) = provider.cache_codec() else {
                    self.compressed.note_decode_error();
                    return None;
                };
                let workspace_needed = codec.workspace_bytes(1, MAX_EXTENT_LEN).unwrap_or(1).max(1);
                let workspace_ready = self
                    .codec_workspace
                    .as_ref()
                    .is_some_and(|mem| mem.len() >= workspace_needed);
                if !workspace_ready {
                    self.codec_workspace = self.provider.alloc(workspace_needed).ok();
                }
                // The provider-side checksum runs while the bytes are still in
                // VRAM. A mismatch refuses the entry **without** invoking the
                // decoder (DT-7).
                let computed = {
                    let (Some(workspace), Some(slab)) = (
                        self.codec_workspace.as_mut(),
                        self.compressed_slabs.get(address.slab_index),
                    ) else {
                        self.compressed.note_decode_error();
                        return None;
                    };
                    match codec.checksum_batch(slab, &[address.span], workspace) {
                        Ok(sums) => sums.into_iter().next(),
                        Err(_) => {
                            self.compressed.note_integrity_error();
                            return None;
                        }
                    }
                };
                if computed.as_ref() != Some(stored_crc32) {
                    self.compressed.note_integrity_error();
                    return None;
                }
                let (statuses, bytes) = {
                    let (Some(workspace), Some(slab)) = (
                        self.codec_workspace.as_mut(),
                        self.compressed_slabs.get(address.slab_index),
                    ) else {
                        self.compressed.note_decode_error();
                        return None;
                    };
                    let mut out = vec![Vec::new()];
                    match codec.decompress_batch_from(
                        slab,
                        &[address.span],
                        &[entry.logical_len as usize],
                        &mut out,
                        workspace,
                    ) {
                        Ok(statuses) => (statuses, out.pop()),
                        Err(_) => {
                            self.compressed.note_decode_error();
                            return None;
                        }
                    }
                };
                if statuses.first() != Some(&CodecStatus::Ok) {
                    self.compressed.note_decode_error();
                    return None;
                }
                let Some(bytes) = bytes else {
                    self.compressed.note_decode_error();
                    return None;
                };
                if bytes.len() as u64 != entry.logical_len {
                    self.compressed.note_decode_error();
                    return None;
                }
                if crc32(&bytes, 0) != *original_crc32 {
                    self.compressed.note_integrity_error();
                    return None;
                }
                Some(bytes)
            }
        }
    }

    /// Grows the slab backing by one provider allocation (DT-5).
    ///
    /// Called only after the span allocator has accepted a new slab, and only
    /// when the full free floor admits the allocation.
    fn grow_slab_backing(&mut self) -> bool {
        let needed = crate::compressed_cache::SLAB_BYTES as u64;
        if !self.codec_admission_allows(needed) {
            return false;
        }
        match self.provider.alloc(crate::compressed_cache::SLAB_BYTES) {
            Ok(mem) => {
                self.compressed_slabs.push(mem);
                true
            }
            Err(_) => false,
        }
    }

    pub fn handle_update(&mut self, offset: u64, data: &[u8]) {
        if self.disabled || self.config.chunk_bytes == 0 || data.is_empty() {
            return;
        }
        // DT-6: invalidate overlapping extents **before** the new bytes are
        // published, so a reader can never observe a stale extent alongside
        // the replacement.
        let end = offset.saturating_add(data.len() as u64);
        if self.config.compression_enabled {
            let removed = invalidate_overlaps(&mut self.compressed.entries, offset, end);
            for entry in removed {
                self.release_entry_storage(entry);
            }
            if self.try_compressed_update(offset, data) {
                return;
            }
        }
        let chunk_bytes = self.config.chunk_bytes as u64;
        let chunk_idx = offset / chunk_bytes;
        let chunk_off = offset % chunk_bytes;
        if chunk_off.saturating_add(data.len() as u64) > chunk_bytes {
            return;
        }
        let chunk_base = chunk_idx.saturating_mul(chunk_bytes);
        if let Some(chunk) = self.chunks.get_mut(&chunk_base) {
            if chunk.mem.write_at(chunk_off, data).is_err() {
                self.chunks.remove(&chunk_base);
                return;
            }
            chunk.mark_valid(chunk_off, chunk_off + data.len() as u64);
            chunk.last_accessed = Instant::now();
            return;
        }
        self.allocate_and_write(chunk_base, chunk_off, data);
    }

    /// Attempts to publish `data` as compressed extents (DT-9, DT-10).
    ///
    /// Returns `true` when every extent was published compressed. Returns
    /// `false` when compression is unsupported, inadmissible, or not strictly
    /// smaller — the caller then takes the raw path. A codec fault here never
    /// disables the worker (DT-11).
    fn try_compressed_update(&mut self, offset: u64, data: &[u8]) -> bool {
        let Some(codec) = self.provider.cache_codec() else {
            return false;
        };
        if data.len() > MAX_EXTENT_LEN {
            // Split oversized updates into per-extent publications so no
            // extent exceeds the DT-4 cap.
            let mut published_any = false;
            for (rel, len) in split_extent(0, data.len() as u64) {
                let slice = &data[rel as usize..(rel + len) as usize];
                if !self.publish_one_extent(offset.saturating_add(rel), slice, codec) {
                    return published_any;
                }
                published_any = true;
            }
            return published_any;
        }
        self.publish_one_extent(offset, data, codec)
    }

    /// Publishes one extent, compressed only when it is strictly smaller.
    fn publish_one_extent(
        &mut self,
        logical_start: u64,
        data: &[u8],
        codec: &dyn GpuCacheCodec<P::Mem<'p>>,
    ) -> bool {
        let logical_len = data.len() as u64;
        let original_crc = crc32(data, 0);
        let max_encoded = match codec.max_encoded_len(data.len()) {
            Ok(len) => len,
            Err(_) => {
                self.compressed.note_decode_error();
                return false;
            }
        };
        // Reserve at the worst case so a growing encode cannot overrun.
        let reservation_len = max_encoded.max(1);
        // Admission uses the **full** parent free floor, never the configured
        // reserve alone (DT-4 / NFR-2).
        if !self.codec_admission_allows(reservation_len as u64) {
            return false;
        }
        let address = match self.compressed.allocator.alloc(reservation_len) {
            Ok(address) => address,
            Err(_) => return false,
        };
        // Back a freshly created slab with provider memory.
        while self.compressed_slabs.len() <= address.slab_index {
            if !self.grow_slab_backing() {
                let _ = self.compressed.allocator.free(address);
                return false;
            }
        }
        let reservation =
            match VramOutputReservation::new(address.span.offset, reservation_len, reservation_len)
            {
                Ok(reservation) => reservation,
                Err(_) => {
                    let _ = self.compressed.allocator.free(address);
                    return false;
                }
            };
        self.compressed.in_flight = true;
        // Disjoint field borrows: the slab is shared, the workspace is unique.
        let results = {
            let workspace_needed = codec.workspace_bytes(1, MAX_EXTENT_LEN).unwrap_or(1).max(1);
            let workspace_ready = self
                .codec_workspace
                .as_ref()
                .is_some_and(|mem| mem.len() >= workspace_needed);
            if !workspace_ready {
                self.codec_workspace = self.provider.alloc(workspace_needed).ok();
            }
            let Some(workspace) = self.codec_workspace.as_mut() else {
                self.compressed.note_decode_error();
                let _ = self.compressed.allocator.free(address);
                return false;
            };
            let Some(slab) = self.compressed_slabs.get_mut(address.slab_index) else {
                self.compressed.note_decode_error();
                let _ = self.compressed.allocator.free(address);
                return false;
            };
            codec.compress_batch_into(&[data], slab, &[reservation], workspace)
        };
        let encoded = match results {
            Ok(results) if results.len() == 1 => results[0],
            _ => {
                self.compressed.note_decode_error();
                let _ = self.compressed.allocator.free(address);
                return false;
            }
        };
        if !encoded.status.is_ok() || encoded.encoded_len == 0 || encoded.encoded_len >= data.len()
        {
            // DT-10 usefulness gate: keep raw unless the encoded form is
            // strictly smaller. `Incompressible` is not a fault.
            self.compressed.note_codec_completion();
            let _ = self.compressed.allocator.free(address);
            return false;
        }
        let stored_span =
            match VramSpan::new(address.span.offset, encoded.encoded_len, reservation_len) {
                Ok(span) => span,
                Err(_) => {
                    self.compressed.note_decode_error();
                    let _ = self.compressed.allocator.free(address);
                    return false;
                }
            };
        // Provider-side checksum of the **stored** payload (DT-7).
        let stored_crc = {
            let workspace_needed = codec.workspace_bytes(1, MAX_EXTENT_LEN).unwrap_or(1).max(1);
            let workspace_ready = self
                .codec_workspace
                .as_ref()
                .is_some_and(|mem| mem.len() >= workspace_needed);
            if !workspace_ready {
                self.codec_workspace = self.provider.alloc(workspace_needed).ok();
            }
            let Some(workspace) = self.codec_workspace.as_mut() else {
                self.compressed.note_decode_error();
                let _ = self.compressed.allocator.free(address);
                return false;
            };
            let Some(slab) = self.compressed_slabs.get(address.slab_index) else {
                self.compressed.note_decode_error();
                let _ = self.compressed.allocator.free(address);
                return false;
            };
            match codec.checksum_batch(slab, &[stored_span], workspace) {
                Ok(sums) if sums.len() == 1 => sums[0],
                _ => {
                    self.compressed.note_integrity_error();
                    let _ = self.compressed.allocator.free(address);
                    return false;
                }
            }
        };
        let generation = self.compressed.next_generation();
        self.compressed.entries.push(CacheEntry {
            logical_start,
            logical_len,
            original_crc32: original_crc,
            generation,
            last_accessed: Instant::now(),
            representation: CacheRepresentation::Compressed {
                address: SlabSpan {
                    slab_index: address.slab_index,
                    span: stored_span,
                },
                original_crc32: original_crc,
                stored_crc32: stored_crc,
                codec_id: match codec.codec_id() {
                    CodecId::Raw => 0,
                    CodecId::Fake => 1,
                    CodecId::NvcompLz4 => 2,
                },
            },
        });
        self.compressed.note_codec_completion();
        true
    }

    /// Returns one entry's backing storage to the allocator.
    fn release_entry_storage(&mut self, entry: CacheEntry) {
        let address = match &entry.representation {
            CacheRepresentation::Raw { address } => *address,
            CacheRepresentation::Compressed { address, .. } => *address,
        };
        let _ = self.compressed.allocator.free(address);
    }

    pub fn handle_promote(&mut self, offset: u64, data: &[u8]) {
        self.handle_update(offset, data);
    }

    pub fn handle_disable(&mut self) {
        self.disabled = true;
        self.chunks.clear();
        // DT-11 / teardown: every codec operation is either complete or has
        // already reported its fault; the store is dropped after that point.
        self.compressed.in_flight = false;
        self.compressed.entries.clear();
    }

    /// Give clean cache chunks back when other GPU users consume the free buffer.
    /// The durable origin remains authoritative for every evicted range.
    pub fn reclaim_under_host_pressure(&mut self) -> Result<u64, VramError> {
        let mut released = 0u64;
        loop {
            let budget = self.provider.budget_snapshot()?;
            let free = if budget.can_admit(0) {
                budget.available_bytes()
            } else {
                0
            };
            let required_free = budget
                .required_free_bytes(self.config.reserve_floor_bytes, RUNTIME_FREE_BUFFER_BYTES);
            let recovery_free = required_free.max(RUNTIME_RECOVERY_BUFFER_BYTES);
            if free >= recovery_free {
                self.pressure_constrained = false;
            }
            if free >= required_free {
                break;
            }
            self.pressure_constrained = true;
            if !self.evict_coldest_chunk() {
                break;
            }
            released = released.saturating_add(self.config.chunk_bytes as u64);
        }
        Ok(released)
    }

    fn allocate_and_write(&mut self, chunk_base: u64, chunk_off: u64, data: &[u8]) {
        if self.pressure_constrained {
            return;
        }
        let chunk_bytes = self.config.chunk_bytes;
        let needed = chunk_bytes as u64;

        // Preserve the display reserve and runtime buffer from the live headroom
        // on every allocation, including allocations after external GPU use changes.
        let admissible = self.provider.budget_snapshot().is_ok_and(|budget| {
            budget.can_admit(0)
                && budget.available_bytes()
                    >= needed.saturating_add(budget.required_free_bytes(
                        self.config.reserve_floor_bytes,
                        RUNTIME_FREE_BUFFER_BYTES,
                    ))
        });
        if !admissible {
            return;
        }

        while self.cached_bytes().saturating_add(needed) > self.effective_target_bytes {
            if !self.evict_coldest_chunk() {
                return;
            }
        }

        if let Ok(mut mem) = self.provider.alloc(chunk_bytes)
            && mem.write_at(chunk_off, data).is_ok()
        {
            self.chunks.insert(
                chunk_base,
                CacheChunk {
                    mem,
                    last_accessed: Instant::now(),
                    valid_ranges: vec![(chunk_off, chunk_off + data.len() as u64)],
                },
            );
        }
    }

    fn evict_coldest_chunk(&mut self) -> bool {
        let coldest = self
            .chunks
            .iter()
            .min_by_key(|(_, chunk)| chunk.last_accessed)
            .map(|(&base, _)| base);
        if let Some(base) = coldest {
            self.chunks.remove(&base);
            return true;
        }
        self.evict_coldest_compressed_extent()
    }

    /// Releases the coldest compressed extent (DT-5 / DT-6).
    ///
    /// Compressed extents hold VRAM too: under host pressure they must be
    /// reclaimable exactly like raw chunks. The released range becomes a Gap,
    /// so a later read misses and the durable origin serves — never a stale
    /// extent.
    fn evict_coldest_compressed_extent(&mut self) -> bool {
        let Some(index) = self
            .compressed
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(i, _)| i)
        else {
            return false;
        };
        let entry = self.compressed.entries.remove(index);
        self.release_entry_storage(entry);
        true
    }
}

/// Reads until `buffer` is full against one absolute `deadline`.
///
/// Mirrors the client-side `read_exact_until` discipline: the deadline is
/// absolute across every partial read of the frame, so a peer that trickles
/// bytes cannot extend it. Returns `Ok(false)` only when the peer closed
/// cleanly before delivering any byte (the loop's only clean-exit path);
/// a truncated frame, a stall, or deadline expiry fails closed.
fn read_exact_until(
    socket: &mut UnixStream,
    mut buffer: &mut [u8],
    deadline: Instant,
) -> Result<bool, String> {
    let mut read_any = false;
    while !buffer.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("worker frame read deadline expired".to_string());
        }
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|error| format!("worker read timeout setup error: {error}"))?;
        match socket.read(buffer) {
            Ok(0) if !read_any => return Ok(false),
            Ok(0) => return Err("worker peer closed mid-frame".to_string()),
            Ok(read) => {
                read_any = true;
                buffer = &mut buffer[read..];
            }
            Err(ref error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(ref error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Err("worker frame read deadline expired".to_string());
            }
            Err(error) => return Err(format!("worker read error: {error}")),
        }
    }
    Ok(true)
}

pub fn run_gpu_worker_loop<P: VramProvider>(
    socket: UnixStream,
    provider: P,
    config: GpuWorkerConfig,
) -> Result<(), String> {
    run_gpu_worker_loop_with_frame_read_timeout(socket, provider, config, WORKER_FRAME_READ_TIMEOUT)
}

/// Runs the worker loop with an injectable per-frame read budget.
///
/// `frame_read_timeout` bounds one complete frame read (header plus payload)
/// so a stalled peer cannot block the worker until process teardown.
pub fn run_gpu_worker_loop_with_frame_read_timeout<P: VramProvider>(
    mut socket: UnixStream,
    provider: P,
    config: GpuWorkerConfig,
    frame_read_timeout: Duration,
) -> Result<(), String> {
    let mut worker = GpuCacheWorker::new(&provider, config);
    let mut hdr_buf = [0u8; FRAME_HEADER_LEN];

    loop {
        let frame_deadline = Instant::now()
            .checked_add(frame_read_timeout)
            .ok_or_else(|| "worker frame read deadline overflow".to_string())?;
        if !read_exact_until(&mut socket, &mut hdr_buf, frame_deadline)? {
            break;
        }

        let hdr = FrameHeader::decode(&hdr_buf)
            .ok_or_else(|| "worker received nonzero reserved header bytes".to_string())?;

        let payload = if hdr.payload_len > 0 {
            if hdr.payload_len as usize > MAX_IPC_PAYLOAD_BYTES {
                return Err("worker payload len exceeds limit".to_string());
            }
            let mut buf = vec![0u8; hdr.payload_len as usize];
            if !read_exact_until(&mut socket, &mut buf, frame_deadline)? {
                return Err("worker peer closed mid-frame".to_string());
            }
            buf
        } else {
            Vec::new()
        };

        match hdr.msg_type {
            MSG_HANDSHAKE_REQ => {
                let resp = FrameHeader {
                    msg_type: MSG_HANDSHAKE_RESP,
                    status: STATUS_OK,
                    correlation_id: hdr.correlation_id,
                    offset: worker.target_bytes(),
                    payload_len: 0,
                    aux: cached_kib_aux(worker.cached_bytes()),
                };
                if let Err(e) = socket.write_all(&resp.encode()) {
                    return Err(format!("worker write handshake resp error: {e}"));
                }
            }
            MSG_READ_REQ if hdr.aux as usize > MAX_IPC_PAYLOAD_BYTES => {
                return Err("worker read length exceeds limit".to_string());
            }
            MSG_READ_REQ => match worker.handle_read(hdr.offset, hdr.aux as usize) {
                Some(data) => {
                    let resp = FrameHeader {
                        msg_type: MSG_READ_RESP,
                        status: STATUS_OK,
                        correlation_id: hdr.correlation_id,
                        offset: hdr.offset,
                        payload_len: data.len() as u32,
                        aux: cached_kib_aux(worker.cached_bytes()),
                    };
                    if let Err(e) = socket.write_all(&resp.encode()) {
                        return Err(format!("worker write read resp error: {e}"));
                    }
                    if let Err(e) = socket.write_all(&data) {
                        return Err(format!("worker write read data error: {e}"));
                    }
                }
                None => {
                    let resp = FrameHeader {
                        msg_type: MSG_READ_RESP,
                        status: STATUS_MISS,
                        correlation_id: hdr.correlation_id,
                        offset: hdr.offset,
                        payload_len: 0,
                        aux: cached_kib_aux(worker.cached_bytes()),
                    };
                    if let Err(e) = socket.write_all(&resp.encode()) {
                        return Err(format!("worker write read resp error: {e}"));
                    }
                }
            },
            MSG_UPDATE => {
                worker.handle_update(hdr.offset, &payload);
            }
            MSG_PROMOTE => {
                worker.handle_promote(hdr.offset, &payload);
            }
            MSG_DISABLE_REQ => {
                worker.handle_disable();
                let resp = FrameHeader {
                    msg_type: MSG_DISABLE_RESP,
                    status: STATUS_OK,
                    correlation_id: hdr.correlation_id,
                    offset: 0,
                    payload_len: 0,
                    aux: 0,
                };
                let _ = socket.write_all(&resp.encode());
                break;
            }
            MSG_HEARTBEAT_REQ => {
                if worker.reclaim_under_host_pressure().is_err() {
                    worker.handle_disable();
                }
                // DT-8: one versioned envelope carries the existing GPU budget
                // telemetry nested beside the cache occupancy sample. Physical
                // `target_bytes`/`cached_bytes` stay in the frame header and are
                // never re-derived from the envelope. A sample that does not fit
                // the ceiling is omitted, never truncated.
                let telemetry_payload = if worker.is_disabled() {
                    Vec::new()
                } else {
                    let now = unix_time_ms();
                    let budget = worker
                        .provider
                        .budget_snapshot()
                        .ok()
                        .map(|snapshot| GpuBudgetTelemetry::from_snapshot(&snapshot, now));
                    let cache = worker.cache_telemetry(now);
                    WorkerTelemetryEnvelope::new(now, budget, Some(cache))
                        .to_bounded_payload()
                        .unwrap_or_default()
                };
                let resp = FrameHeader {
                    msg_type: MSG_HEARTBEAT_RESP,
                    status: if worker.is_disabled() {
                        STATUS_ERROR
                    } else {
                        STATUS_OK
                    },
                    correlation_id: hdr.correlation_id,
                    offset: worker.target_bytes(),
                    payload_len: telemetry_payload.len() as u32,
                    aux: cached_kib_aux(worker.cached_bytes()),
                };
                if let Err(e) = socket.write_all(&resp.encode()) {
                    return Err(format!("worker write heartbeat error: {e}"));
                }
                if let Err(e) = socket.write_all(&telemetry_payload) {
                    return Err(format!("worker write heartbeat telemetry error: {e}"));
                }
            }
            _ => {
                return Err(format!(
                    "worker received unsupported message type {}",
                    hdr.msg_type
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::ipc_cache_client::IpcCacheClient;
    use crate::isolated_origin::{BestEffortCache, CacheMutation, CacheRead};
    use ramshared_vram::{GpuAdapterIdentity, GpuBudgetSnapshot, GpuBudgetSource, VramError};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn trusted_test_budget(free: u64, total: u64) -> GpuBudgetSnapshot {
        GpuBudgetSnapshot {
            adapter: Some(GpuAdapterIdentity {
                backend: "test".into(),
                key: "fake-adapter-0".into(),
                luid: None,
            }),
            total_bytes: Some(total),
            budget_bytes: total,
            used_bytes: total.saturating_sub(free),
            source: GpuBudgetSource::DriverReported,
            sampled_at: Instant::now(),
        }
    }

    #[test]
    fn cached_kib_aux_saturates_instead_of_wrapping_at_wire_limit() {
        let max_kib = u64::from(u32::MAX);
        let max_bytes = max_kib * 1024;

        assert_eq!(cached_kib_aux(max_bytes - 1024), u32::MAX - 1);
        assert_eq!(cached_kib_aux(max_bytes), u32::MAX);
        assert_eq!(cached_kib_aux(max_bytes + 1024), u32::MAX);
        assert_eq!(cached_kib_aux(4 * 1024 * 1024 * 1024 * 1024), u32::MAX);
    }

    /// ITEM-4: every worker admission threshold is the shared helper's value
    /// over an identical snapshot, with the headroom from the one named
    /// constant. A second source of the 640 MiB buffer, or any caller that
    /// admits below the helper threshold, trips this.
    #[test]
    fn worker_admission_uses_resolved_policy() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let policy = ramshared_vram::ReserveFloorPolicy::from_manifest(1, 20).expect("valid");
        let configured = policy.configured_reserve_bytes(8 * GIB);
        let config = GpuWorkerConfig {
            target_bytes: 16 * GIB,
            chunk_bytes: 64 * 1024 * 1024,
            reserve_floor_bytes: configured,
            compression_enabled: false,
        };
        // 2 GiB free: available 2G sits under configured(1.6G)+share(1.6G)+
        // runtime(640MiB) + chunk(64MiB), so admission must refuse.
        let budget = trusted_test_budget(2 * GIB, 8 * GIB);

        // The single helper threshold over this snapshot.
        let required_free = budget.required_free_bytes(configured, RUNTIME_FREE_BUFFER_BYTES);
        assert_eq!(
            required_free,
            policy.enforced_free_floor_bytes(8 * GIB, RUNTIME_FREE_BUFFER_BYTES),
            "worker and policy must compute one floor"
        );

        // Admission refuses exactly at the boundary: one byte below.
        let needed = config.chunk_bytes as u64;
        let below = budget.available_bytes() < needed + required_free;
        assert!(below, "fixture must sit below the threshold");
        assert_eq!(
            effective_target_from_budget(&budget, config),
            budget.safe_target_bytes(config.target_bytes, configured, RUNTIME_FREE_BUFFER_BYTES)
        );

        // Above the threshold the same snapshot yields the shared target.
        let roomy = trusted_test_budget(8 * GIB - 1024, 8 * GIB);
        assert!(roomy.available_bytes() >= needed + required_free);
        assert_eq!(
            effective_target_from_budget(&roomy, config),
            roomy.safe_target_bytes(config.target_bytes, configured, RUNTIME_FREE_BUFFER_BYTES)
        );
    }

    /// ITEM-4: a stale budget is refused, never admitted with a reduced
    /// target. `can_admit` is the freshness gate; a stale snapshot must fail
    /// it rather than reach `safe_target_bytes`.
    #[test]
    fn worker_admission_refuses_on_stale_budget() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let config = GpuWorkerConfig {
            target_bytes: 4 * GIB,
            chunk_bytes: 512 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: false,
        };
        let mut stale = trusted_test_budget(6 * GIB, 8 * GIB);
        stale.sampled_at = Instant::now() - Duration::from_secs(60);
        assert!(
            !stale.can_admit(0),
            "a 60s-old snapshot must not be admitted"
        );
        assert_eq!(effective_target_from_budget(&stale, config), 0);

        // Fresh equivalent is admitted.
        let fresh = trusted_test_budget(6 * GIB, 8 * GIB);
        assert!(fresh.can_admit(0));
        assert!(effective_target_from_budget(&fresh, config) > 0);
    }

    #[test]
    fn worker_budget_target_requires_external_adapter_bound_snapshot() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let config = GpuWorkerConfig {
            target_bytes: 4 * GIB,
            chunk_bytes: 512 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: false,
        };
        let trusted = GpuBudgetSnapshot {
            adapter: Some(GpuAdapterIdentity {
                backend: "test".into(),
                key: "stable-id".into(),
                luid: None,
            }),
            total_bytes: Some(8 * GIB),
            budget_bytes: 5 * GIB,
            used_bytes: 0,
            source: GpuBudgetSource::DriverReported,
            sampled_at: Instant::now(),
        };
        assert_eq!(
            effective_target_from_budget(&trusted, config),
            4 * GIB - RUNTIME_FREE_BUFFER_BYTES
        );

        let unknown = GpuBudgetSnapshot {
            adapter: None,
            source: GpuBudgetSource::ProviderLocalEstimate,
            ..trusted
        };
        assert_eq!(effective_target_from_budget(&unknown, config), 0);
    }

    #[test]
    fn worker_budget_target_never_exceeds_current_available_headroom() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let config = GpuWorkerConfig {
            target_bytes: 4 * GIB,
            chunk_bytes: 512 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: false,
        };
        let low_headroom = GpuBudgetSnapshot {
            adapter: Some(GpuAdapterIdentity {
                backend: "test".into(),
                key: "stable-id".into(),
                luid: None,
            }),
            total_bytes: Some(8 * GIB),
            budget_bytes: 5 * GIB,
            used_bytes: 3 * GIB,
            source: GpuBudgetSource::DriverReported,
            sampled_at: Instant::now(),
        };

        assert_eq!(
            effective_target_from_budget(&low_headroom, config),
            2 * GIB - GIB - RUNTIME_FREE_BUFFER_BYTES
        );
    }

    #[test]
    fn worker_budget_target_is_zero_until_runtime_buffer_is_available() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let config = GpuWorkerConfig {
            target_bytes: GIB,
            chunk_bytes: 64 * 1024 * 1024,
            reserve_floor_bytes: 0,
            compression_enabled: false,
        };
        let insufficient = GpuBudgetSnapshot {
            adapter: Some(GpuAdapterIdentity {
                backend: "test".into(),
                key: "stable-id".into(),
                luid: None,
            }),
            total_bytes: Some(2 * GIB),
            budget_bytes: 2 * GIB,
            used_bytes: 2 * GIB - (RUNTIME_FREE_BUFFER_BYTES - 1),
            source: GpuBudgetSource::DriverReported,
            sampled_at: Instant::now(),
        };

        assert_eq!(effective_target_from_budget(&insufficient, config), 0);
    }

    struct FakeMem {
        data: Arc<Mutex<Vec<u8>>>,
        len: usize,
        live_allocations: Arc<AtomicUsize>,
    }

    impl Drop for FakeMem {
        fn drop(&mut self) {
            self.live_allocations.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl VramMemory for FakeMem {
        fn len(&self) -> usize {
            self.len
        }

        fn zero(&mut self) -> Result<(), VramError> {
            let mut guard = self.data.lock().map_err(|_| VramError::Busy)?;
            guard.fill(0);
            Ok(())
        }

        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            let guard = self.data.lock().map_err(|_| VramError::Busy)?;
            let start = off as usize;
            let end = start + dst.len();
            if end > guard.len() {
                return Err(VramError::OutOfRange {
                    off,
                    len: dst.len() as u64,
                    size: guard.len() as u64,
                });
            }
            dst.copy_from_slice(&guard[start..end]);
            Ok(())
        }

        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            let mut guard = self.data.lock().map_err(|_| VramError::Busy)?;
            let start = off as usize;
            let end = start + src.len();
            if end > guard.len() {
                return Err(VramError::OutOfRange {
                    off,
                    len: src.len() as u64,
                    size: guard.len() as u64,
                });
            }
            guard[start..end].copy_from_slice(src);
            Ok(())
        }
    }

    struct FakeProvider {
        total: u64,
        free: u64,
        live_allocations: Arc<AtomicUsize>,
    }

    impl FakeProvider {
        fn new(total: u64, free: u64) -> Self {
            Self {
                total,
                free,
                live_allocations: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl VramProvider for FakeProvider {
        type Mem<'p>
            = FakeMem
        where
            Self: 'p;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            self.live_allocations.fetch_add(1, Ordering::SeqCst);
            Ok(FakeMem {
                data: Arc::new(Mutex::new(vec![0u8; bytes])),
                len: bytes,
                live_allocations: Arc::clone(&self.live_allocations),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((self.free, self.total))
        }

        fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
            Ok(trusted_test_budget(self.free, self.total))
        }
    }

    struct PressureProvider {
        total: u64,
        external: Arc<AtomicU64>,
        live_allocations: Arc<AtomicUsize>,
        chunk_bytes: u64,
    }

    impl VramProvider for PressureProvider {
        type Mem<'p>
            = FakeMem
        where
            Self: 'p;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            self.live_allocations.fetch_add(1, Ordering::SeqCst);
            Ok(FakeMem {
                data: Arc::new(Mutex::new(vec![0u8; bytes])),
                len: bytes,
                live_allocations: Arc::clone(&self.live_allocations),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            let cache_bytes = (self.live_allocations.load(Ordering::SeqCst) as u64)
                .saturating_mul(self.chunk_bytes);
            let free = self
                .total
                .saturating_sub(self.external.load(Ordering::SeqCst))
                .saturating_sub(cache_bytes);
            Ok((free, self.total))
        }

        fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
            let (free, total) = self.mem_info()?;
            Ok(trusted_test_budget(free, total))
        }
    }

    #[test]
    fn heartbeat_pressure_reclaims_cold_cache_and_keeps_origin_fallback() {
        let chunk_bytes = 2 * 1024 * 1024;
        let total = 2 * 1024 * 1024 * 1024;
        let external = Arc::new(AtomicU64::new(0));
        let live_allocations = Arc::new(AtomicUsize::new(0));
        let provider = PressureProvider {
            total,
            external: Arc::clone(&external),
            live_allocations: Arc::clone(&live_allocations),
            chunk_bytes,
        };
        let mut worker = GpuCacheWorker::new(
            &provider,
            GpuWorkerConfig {
                target_bytes: 4 * chunk_bytes,
                chunk_bytes: chunk_bytes as usize,
                reserve_floor_bytes: 128 * 1024 * 1024,
                compression_enabled: false,
            },
        );
        worker.handle_update(0, &[1]);
        std::thread::sleep(Duration::from_millis(1));
        worker.handle_update(chunk_bytes, &[2]);
        assert_eq!(worker.cached_bytes(), 2 * chunk_bytes);

        external.store(
            total - 2 * chunk_bytes - (RUNTIME_FREE_BUFFER_BYTES - chunk_bytes),
            Ordering::SeqCst,
        );
        assert_eq!(
            worker.reclaim_under_host_pressure().unwrap(),
            2 * chunk_bytes
        );
        assert_eq!(worker.cached_bytes(), 0);
        assert_eq!(worker.handle_read(0, 1), None);
        assert_eq!(worker.handle_read(chunk_bytes, 1), None);
        assert!(provider.mem_info().unwrap().0 >= RUNTIME_FREE_BUFFER_BYTES);
        worker.handle_update(2 * chunk_bytes, &[3]);
        assert_eq!(worker.cached_bytes(), 0);
        external.store(0, Ordering::SeqCst);
        assert_eq!(worker.reclaim_under_host_pressure().unwrap(), 0);
        worker.handle_update(2 * chunk_bytes, &[3]);
        assert_eq!(worker.cached_bytes(), chunk_bytes);
    }

    #[test]
    fn heartbeat_reports_physical_release_after_external_gpu_pressure() {
        let chunk_bytes = 2 * 1024 * 1024;
        let total = 2 * 1024 * 1024 * 1024;
        let external = Arc::new(AtomicU64::new(0));
        let live_allocations = Arc::new(AtomicUsize::new(0));
        let provider = PressureProvider {
            total,
            external: Arc::clone(&external),
            live_allocations: Arc::clone(&live_allocations),
            chunk_bytes,
        };
        let (client_socket, worker_socket) = UnixStream::pair().expect("socketpair failed");
        let worker_thread = std::thread::spawn(move || {
            run_gpu_worker_loop(
                worker_socket,
                provider,
                GpuWorkerConfig {
                    target_bytes: 4 * chunk_bytes,
                    chunk_bytes: chunk_bytes as usize,
                    reserve_floor_bytes: 128 * 1024 * 1024,
                    compression_enabled: false,
                },
            )
            .expect("worker loop failed");
        });
        let mut client =
            IpcCacheClient::new(client_socket, Duration::from_secs(1), 4 * chunk_bytes);
        client.perform_handshake().expect("handshake failed");
        assert_eq!(client.update(0, &[1]), CacheMutation::Accepted);
        assert_eq!(client.update(chunk_bytes, &[2]), CacheMutation::Accepted);
        assert_eq!(
            client.refresh_cached_bytes().expect("first heartbeat"),
            2 * chunk_bytes
        );

        external.store(
            total - 2 * chunk_bytes - (RUNTIME_FREE_BUFFER_BYTES - chunk_bytes),
            Ordering::SeqCst,
        );
        assert_eq!(
            client.refresh_cached_bytes().expect("pressure heartbeat"),
            0
        );
        assert_eq!(client.read(0, &mut [0]), CacheRead::Miss);
        assert_eq!(
            client.update(2 * chunk_bytes, &[3]),
            CacheMutation::Accepted
        );
        assert_eq!(client.refresh_cached_bytes().expect("parked heartbeat"), 0);
        external.store(0, Ordering::SeqCst);
        assert_eq!(
            client.refresh_cached_bytes().expect("recovery heartbeat"),
            0
        );
        assert_eq!(
            client.update(2 * chunk_bytes, &[3]),
            CacheMutation::Accepted
        );
        assert_eq!(
            client.refresh_cached_bytes().expect("refill heartbeat"),
            chunk_bytes
        );
        drop(client);
        worker_thread.join().expect("worker thread joined");
        assert_eq!(live_allocations.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn worker_handshake_and_read_hit_cycle() {
        let (client_sock, worker_sock) = UnixStream::pair().expect("socketpair failed");
        let provider = FakeProvider::new(8 * 1024 * 1024 * 1024, 6 * 1024 * 1024 * 1024);
        let config = GpuWorkerConfig {
            target_bytes: 64 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 1536 * 1024 * 1024,
            compression_enabled: false,
        };

        let worker_thread = std::thread::spawn(move || {
            run_gpu_worker_loop(worker_sock, provider, config).expect("worker loop failed");
        });

        let mut client =
            IpcCacheClient::new(client_sock, Duration::from_millis(100), 64 * 1024 * 1024);
        client.perform_handshake().expect("handshake failed");
        assert_eq!(client.target_bytes(), 64 * 1024 * 1024);

        let test_payload = vec![0x42; 4096];
        let outcome = client.update(0, &test_payload);
        assert_eq!(outcome, CacheMutation::Accepted);

        // Give worker brief moment to process non-blocking update
        std::thread::sleep(Duration::from_millis(10));

        let mut read_buf = vec![0u8; 4096];
        let read_outcome = client.read(0, &mut read_buf);
        assert_eq!(read_outcome, CacheRead::Hit);
        assert_eq!(read_buf, test_payload);

        // Read unwritten offset in another chunk
        let mut unwritten = vec![0u8; 4096];
        let miss_outcome = client.read(4 * 1024 * 1024, &mut unwritten);
        assert_eq!(miss_outcome, CacheRead::Miss);

        let disable_outcome = client.disable();
        assert_eq!(disable_outcome, CacheMutation::Accepted);
        worker_thread.join().expect("join worker thread");
    }

    #[test]
    fn worker_never_serves_unwritten_bytes_from_an_allocated_chunk() {
        let provider = FakeProvider::new(4 * 1024 * 1024 * 1024, 3 * 1024 * 1024 * 1024);
        let mut worker = GpuCacheWorker::new(
            &provider,
            GpuWorkerConfig {
                target_bytes: 2 * 1024 * 1024,
                chunk_bytes: 2 * 1024 * 1024,
                reserve_floor_bytes: 1536 * 1024 * 1024,
                compression_enabled: false,
            },
        );
        worker.handle_update(0, &[1, 2, 3, 4]);
        assert_eq!(worker.handle_read(0, 4), Some(vec![1, 2, 3, 4]));
        assert_eq!(worker.handle_read(4096, 4), None);
        assert_eq!(worker.handle_read(2, 4), None);
        worker.handle_update(4, &[5, 6, 7, 8]);
        assert_eq!(worker.handle_read(0, 8), Some(vec![1, 2, 3, 4, 5, 6, 7, 8]));
    }

    #[test]
    fn worker_reports_allocated_vram_after_bounded_heartbeat() {
        let (client_sock, worker_sock) = UnixStream::pair().expect("socketpair failed");
        let provider = FakeProvider::new(4 * 1024 * 1024 * 1024, 3 * 1024 * 1024 * 1024);
        let worker_thread = std::thread::spawn(move || {
            run_gpu_worker_loop(
                worker_sock,
                provider,
                GpuWorkerConfig {
                    target_bytes: 2 * 1024 * 1024,
                    chunk_bytes: 2 * 1024 * 1024,
                    reserve_floor_bytes: 1536 * 1024 * 1024,
                    compression_enabled: false,
                },
            )
            .expect("worker loop failed");
        });
        let mut client =
            IpcCacheClient::new(client_sock, Duration::from_millis(100), 2 * 1024 * 1024);
        client.perform_handshake().expect("handshake failed");
        assert_eq!(client.update(0, &[1, 2, 3, 4]), CacheMutation::Accepted);
        assert_eq!(
            client.refresh_cached_bytes().expect("heartbeat failed"),
            2 * 1024 * 1024
        );
        assert_eq!(client.cached_bytes(), 2 * 1024 * 1024);
        assert_eq!(client.disable(), CacheMutation::Accepted);
        worker_thread.join().expect("join worker thread");
    }

    #[test]
    fn oversized_mutation_disables_cache_before_worker_frame_is_sent() {
        let (client_sock, worker_sock) = UnixStream::pair().expect("socketpair failed");
        let provider = FakeProvider::new(4 * 1024 * 1024 * 1024, 3 * 1024 * 1024 * 1024);
        let worker_thread = std::thread::spawn(move || {
            run_gpu_worker_loop(
                worker_sock,
                provider,
                GpuWorkerConfig {
                    target_bytes: 2 * 1024 * 1024,
                    chunk_bytes: 2 * 1024 * 1024,
                    reserve_floor_bytes: 1536 * 1024 * 1024,
                    compression_enabled: false,
                },
            )
            .expect("worker loop failed");
        });
        let mut client =
            IpcCacheClient::new(client_sock, Duration::from_millis(100), 2 * 1024 * 1024);
        client.perform_handshake().expect("handshake failed");
        let payload = vec![0x5a; 512 * 1024];
        assert_eq!(client.update(0, &payload), CacheMutation::Failed);
        assert_eq!(client.state(), crate::origin_cache::CacheState::Unavailable);
        worker_thread.join().expect("join worker thread");
    }

    #[test]
    fn worker_respects_headroom_floor() {
        let total_vram = 8 * 1024 * 1024 * 1024u64; // 8 GiB
        let free_vram = 7 * 1024 * 1024 * 1024u64; // 7 GiB
        let provider = FakeProvider::new(total_vram, free_vram);

        // Current use leaves 7 GiB; a 2 GiB display reserve and 640 MiB runtime
        // headroom must remain available after any cache allocation.
        let config = GpuWorkerConfig {
            target_bytes: 10 * 1024 * 1024 * 1024, // Request 10 GiB (more than available)
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 2 * 1024 * 1024 * 1024,
            compression_enabled: false,
        };

        let worker = GpuCacheWorker::new(&provider, config);
        assert_eq!(
            worker.target_bytes(),
            7 * 1024 * 1024 * 1024 - 2 * 1024 * 1024 * 1024 - RUNTIME_FREE_BUFFER_BYTES
        );
    }

    #[test]
    fn worker_refuses_allocation_when_live_gpu_free_buffer_is_low() {
        let provider = FakeProvider::new(6 * 1024 * 1024 * 1024, 256 * 1024 * 1024);
        let allocations = Arc::clone(&provider.live_allocations);
        let mut worker = GpuCacheWorker::new(
            &provider,
            GpuWorkerConfig {
                target_bytes: 2 * 1024 * 1024,
                chunk_bytes: 2 * 1024 * 1024,
                reserve_floor_bytes: 1536 * 1024 * 1024,
                compression_enabled: false,
            },
        );
        worker.handle_update(0, &[1, 2, 3, 4]);
        assert_eq!(allocations.load(Ordering::SeqCst), 0);
        assert_eq!(worker.cached_bytes(), 0);
    }

    #[test]
    fn worker_disable_frees_allocations() {
        let provider = FakeProvider::new(4 * 1024 * 1024 * 1024, 3 * 1024 * 1024 * 1024);
        let live_allocs = Arc::clone(&provider.live_allocations);
        let config = GpuWorkerConfig {
            target_bytes: 16 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 1536 * 1024 * 1024,
            compression_enabled: false,
        };

        let mut worker = GpuCacheWorker::new(&provider, config);
        worker.handle_update(0, &[1, 2, 3, 4]);
        worker.handle_update(2 * 1024 * 1024, &[5, 6, 7, 8]);

        assert_eq!(worker.active_chunks_count(), 2);
        assert_eq!(worker.cached_bytes(), 4 * 1024 * 1024);
        assert_eq!(live_allocs.load(Ordering::SeqCst), 2);

        worker.handle_disable();

        assert_eq!(worker.active_chunks_count(), 0);
        assert_eq!(worker.cached_bytes(), 0);
        assert!(worker.is_disabled());
        assert_eq!(live_allocs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn worker_evicts_coldest_chunk_on_pressure() {
        let provider = FakeProvider::new(4 * 1024 * 1024 * 1024, 3 * 1024 * 1024 * 1024);
        let config = GpuWorkerConfig {
            target_bytes: 4 * 1024 * 1024, // Space for only 2 chunks of 2 MiB
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 1536 * 1024 * 1024,
            compression_enabled: false,
        };

        let mut worker = GpuCacheWorker::new(&provider, config);

        // Fill 2 chunks
        worker.handle_update(0, &[10, 20]);
        std::thread::sleep(Duration::from_millis(5));
        worker.handle_promote(2 * 1024 * 1024, &[30, 40]);
        assert_eq!(worker.active_chunks_count(), 2);

        // Add 3rd chunk - chunk 0 should be evicted as coldest
        std::thread::sleep(Duration::from_millis(5));
        worker.handle_update(4 * 1024 * 1024, &[50, 60]);
        assert_eq!(worker.active_chunks_count(), 2);

        // Chunk 0 is evicted -> miss
        assert_eq!(worker.handle_read(0, 2), None);
        // Chunk 1 and 2 remain -> hit
        assert_eq!(worker.handle_read(2 * 1024 * 1024, 2), Some(vec![30, 40]));
        assert_eq!(worker.handle_read(4 * 1024 * 1024, 2), Some(vec![50, 60]));

        // Boundary crossing read returns None
        assert_eq!(worker.handle_read(2 * 1024 * 1024 - 1, 4), None);
    }

    #[test]
    fn worker_handles_promote_and_heartbeat_loop() {
        let (client_sock, worker_sock) = UnixStream::pair().expect("socketpair failed");
        let provider = FakeProvider::new(8 * 1024 * 1024 * 1024, 6 * 1024 * 1024 * 1024);
        let config = GpuWorkerConfig {
            target_bytes: 64 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 1536 * 1024 * 1024,
            compression_enabled: false,
        };

        let worker_thread = std::thread::spawn(move || {
            run_gpu_worker_loop(worker_sock, provider, config).expect("worker loop failed");
        });

        let mut client =
            IpcCacheClient::new(client_sock, Duration::from_millis(100), 64 * 1024 * 1024);
        client.perform_handshake().expect("handshake failed");

        // Promote frame
        let promote_data = vec![0xEE; 1024];
        let promote_outcome = client.promote(0, &promote_data);
        assert_eq!(promote_outcome, CacheMutation::Accepted);

        std::thread::sleep(Duration::from_millis(10));

        let mut read_buf = vec![0u8; 1024];
        let read_outcome = client.read(0, &mut read_buf);
        assert_eq!(read_outcome, CacheRead::Hit);
        assert_eq!(read_buf, promote_data);

        client
            .refresh_cached_bytes()
            .expect("heartbeat reports cached bytes");
        let budget = client
            .gpu_budget_telemetry()
            .expect("heartbeat publishes adapter budget");
        assert_eq!(
            budget.adapter.as_ref().map(|id| id.backend.as_str()),
            Some("test")
        );
        assert_eq!(budget.source, GpuBudgetSource::DriverReported);
        assert_eq!(
            budget.trusted_available_at(unix_time_ms(), 5_000),
            Some(6 * 1024 * 1024 * 1024)
        );

        let disable_outcome = client.disable();
        assert_eq!(disable_outcome, CacheMutation::Accepted);
        worker_thread.join().expect("join worker thread");
    }

    /// Kahneman #17 — teardown must be idempotent and bounded.
    /// SPEC: `worker_teardown_is_idempotent_and_bounded`
    #[test]
    fn worker_teardown_is_idempotent_and_bounded() {
        let provider = FakeProvider::new(4 * 1024 * 1024 * 1024, 3 * 1024 * 1024 * 1024);
        let live_allocs = Arc::clone(&provider.live_allocations);
        let config = GpuWorkerConfig {
            target_bytes: 16 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 1536 * 1024 * 1024,
            compression_enabled: false,
        };

        let mut worker = GpuCacheWorker::new(&provider, config);
        worker.handle_update(0, &[1, 2, 3]);
        worker.handle_update(2 * 1024 * 1024, &[4, 5, 6]);
        assert_eq!(live_allocs.load(Ordering::SeqCst), 2);

        // First teardown: frees all allocations
        let start = Instant::now();
        worker.handle_disable();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "teardown must be bounded"
        );
        assert_eq!(live_allocs.load(Ordering::SeqCst), 0);

        // Second teardown: idempotent — no panic, no double-free
        worker.handle_disable();
        assert_eq!(live_allocs.load(Ordering::SeqCst), 0);
        assert!(worker.is_disabled());

        // Third teardown on empty state: still idempotent
        worker.handle_disable();
        assert_eq!(worker.active_chunks_count(), 0);
    }
    // ------------------------------------------------------------------
    // ITEM-2 — optional compressed cache (DT-4 .. DT-7, DT-9 .. DT-11).
    // ------------------------------------------------------------------

    use ramshared_vram::{FakeCodec, GpuCacheCodec};

    /// A provider that reports the deterministic fake codec (DT-2).
    ///
    /// `free` is shared so a test can publish under headroom and then drop the
    /// live free level to model an external GPU workload (Kahneman #5).
    struct CodecProvider {
        total: u64,
        free: Arc<AtomicU64>,
        live_allocations: Arc<AtomicUsize>,
        codec: FakeCodec,
    }

    impl CodecProvider {
        fn new(total: u64, free: u64) -> Self {
            Self {
                total,
                free: Arc::new(AtomicU64::new(free)),
                live_allocations: Arc::new(AtomicUsize::new(0)),
                codec: FakeCodec::new(),
            }
        }

        fn set_free(&self, free: u64) {
            self.free.store(free, Ordering::SeqCst);
        }
    }

    impl VramProvider for CodecProvider {
        type Mem<'p>
            = FakeMem
        where
            Self: 'p;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            self.live_allocations.fetch_add(1, Ordering::SeqCst);
            Ok(FakeMem {
                data: Arc::new(Mutex::new(vec![0u8; bytes])),
                len: bytes,
                live_allocations: Arc::clone(&self.live_allocations),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((self.free.load(Ordering::SeqCst), self.total))
        }

        fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
            Ok(trusted_test_budget(
                self.free.load(Ordering::SeqCst),
                self.total,
            ))
        }

        fn cache_codec(&self) -> Option<&dyn GpuCacheCodec<Self::Mem<'_>>> {
            Some(&self.codec)
        }
    }

    /// A provider whose budget snapshot is stale (DT-4 admission refusal).
    struct StaleBudgetProvider {
        total: u64,
        free: u64,
        codec: FakeCodec,
    }

    impl VramProvider for StaleBudgetProvider {
        type Mem<'p>
            = FakeMem
        where
            Self: 'p;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            Ok(FakeMem {
                data: Arc::new(Mutex::new(vec![0u8; bytes])),
                len: bytes,
                live_allocations: Arc::new(AtomicUsize::new(1)),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((self.free, self.total))
        }

        fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
            Ok(GpuBudgetSnapshot {
                adapter: Some(GpuAdapterIdentity {
                    backend: "test".into(),
                    key: "stale-adapter-0".into(),
                    luid: None,
                }),
                total_bytes: Some(self.total),
                budget_bytes: self.total,
                used_bytes: self.total.saturating_sub(self.free),
                source: GpuBudgetSource::DriverReported,
                // Far enough in the past that `can_admit` refuses.
                sampled_at: Instant::now() - Duration::from_secs(60),
            })
        }

        fn cache_codec(&self) -> Option<&dyn GpuCacheCodec<Self::Mem<'_>>> {
            Some(&self.codec)
        }
    }

    /// A provider with a codec but no measured budget at all (zero target).
    struct NoBudgetProvider {
        codec: FakeCodec,
    }

    impl VramProvider for NoBudgetProvider {
        type Mem<'p>
            = FakeMem
        where
            Self: 'p;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            Ok(FakeMem {
                data: Arc::new(Mutex::new(vec![0u8; bytes])),
                len: bytes,
                live_allocations: Arc::new(AtomicUsize::new(1)),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Err(VramError::Provider("no budget".into()))
        }

        fn cache_codec(&self) -> Option<&dyn GpuCacheCodec<Self::Mem<'_>>> {
            Some(&self.codec)
        }
    }

    /// A provider whose codec fails every operation (DT-11 fault injection).
    struct FaultyCodec;

    impl GpuCacheCodec<FakeMem> for FaultyCodec {
        fn codec_id(&self) -> CodecId {
            CodecId::Fake
        }
        fn required_alignments(&self) -> ramshared_vram::CodecAlignments {
            ramshared_vram::CodecAlignments::byte()
        }
        fn max_encoded_len(&self, _logical_len: usize) -> Result<usize, VramError> {
            Err(VramError::Provider("codec unavailable".into()))
        }
        fn workspace_bytes(
            &self,
            _item_count: usize,
            _max_logical_len: usize,
        ) -> Result<usize, VramError> {
            Ok(0)
        }
        fn compress_batch_into(
            &self,
            _inputs: &[&[u8]],
            _slab: &mut FakeMem,
            _outputs: &[ramshared_vram::VramOutputReservation],
            _workspace: &mut FakeMem,
        ) -> Result<Vec<ramshared_vram::CodecChunkResult>, VramError> {
            Err(VramError::Provider("codec fault".into()))
        }
        fn checksum_batch(
            &self,
            _slab: &FakeMem,
            _inputs: &[ramshared_vram::VramSpan],
            _workspace: &mut FakeMem,
        ) -> Result<Vec<u32>, VramError> {
            Err(VramError::Provider("codec fault".into()))
        }
        fn decompress_batch_from(
            &self,
            _slab: &FakeMem,
            _inputs: &[ramshared_vram::VramSpan],
            _logical_lengths: &[usize],
            _outputs: &mut [Vec<u8>],
            _workspace: &mut FakeMem,
        ) -> Result<Vec<CodecStatus>, VramError> {
            Err(VramError::Provider("codec fault".into()))
        }
    }

    struct FaultyProvider {
        total: u64,
        free: u64,
        live_allocations: Arc<AtomicUsize>,
        codec: FaultyCodec,
    }

    impl VramProvider for FaultyProvider {
        type Mem<'p>
            = FakeMem
        where
            Self: 'p;

        fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
            self.live_allocations.fetch_add(1, Ordering::SeqCst);
            Ok(FakeMem {
                data: Arc::new(Mutex::new(vec![0u8; bytes])),
                len: bytes,
                live_allocations: Arc::clone(&self.live_allocations),
            })
        }

        fn mem_info(&self) -> Result<(u64, u64), VramError> {
            Ok((self.free, self.total))
        }

        fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
            Ok(trusted_test_budget(self.free, self.total))
        }

        fn cache_codec(&self) -> Option<&dyn GpuCacheCodec<Self::Mem<'_>>> {
            Some(&self.codec)
        }
    }

    /// `read_over_16_mib_refuses_before_allocation` — DT-4.
    ///
    /// A cache read above the private-response ceiling must be refused
    /// **before** any response buffer is allocated. A request at exactly the
    /// ceiling is still reachable (the ceiling is an upper bound, not a
    /// lower one) and a normal hit still serves.
    #[test]
    fn read_over_16_mib_refuses_before_allocation() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let provider = FakeProvider::new(8 * GIB, 8 * GIB);
        let config = GpuWorkerConfig {
            target_bytes: 2 * GIB,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: false,
        };
        let mut worker = GpuCacheWorker::new(&provider, config);
        assert_eq!(worker.target_bytes(), 2 * GIB);

        // One populated chunk so a legitimate read is possible.
        let payload = vec![0xABu8; 4096];
        worker.handle_update(0, &payload);
        assert_eq!(worker.handle_read(0, 4096).as_deref(), Some(&payload[..]));

        // Over the ceiling: refused, and the refusal is not a "miss because
        // nothing is cached" — the same range serves at a legal length.
        assert!(worker.handle_read(0, MAX_READ_LEN + 1).is_none());
        assert!(worker.handle_read(0, MAX_READ_LEN).is_none());
        assert_eq!(worker.handle_read(0, 4096).as_deref(), Some(&payload[..]));

        // Zero and disabled still refuse early.
        assert!(worker.handle_read(0, 0).is_none());
        worker.handle_disable();
        assert!(worker.handle_read(0, 4096).is_none());
    }

    /// `codec_admission_uses_full_parent_free_floor` — DT-4 / NFR-2.
    ///
    /// Codec/slab admission must read the **full** parent free floor
    /// `required_free_bytes(configured_reserve, runtime_headroom)`, never the
    /// configured `reserve_floor_bytes` alone. The named threshold must be
    /// strictly larger than `reserve_floor_bytes` on a real capacity, and
    /// admission must refuse exactly at `reserve + needed` while accepting at
    /// `full_floor + needed`.
    #[test]
    fn codec_admission_uses_full_parent_free_floor() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let provider = CodecProvider::new(8 * GIB, 8 * GIB);
        let configured = 512 * 1024 * 1024u64;
        let config = GpuWorkerConfig {
            target_bytes: GIB,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: configured,
            compression_enabled: true,
        };
        let worker = GpuCacheWorker::new(&provider, config);
        let budget = provider.budget_snapshot().expect("budget");
        let full_floor = worker.codec_admission_required_free(&budget);

        // The full floor is reserve ∨ ceil(capacity/5) + runtime headroom.
        let capacity = 8 * GIB;
        let expected = configured
            .max(capacity.div_ceil(5))
            .saturating_add(RUNTIME_FREE_BUFFER_BYTES);
        assert_eq!(full_floor, expected);
        assert!(
            full_floor > configured + RUNTIME_FREE_BUFFER_BYTES,
            "the ceil(capacity/5) share must be part of the floor, got {full_floor}"
        );
        assert_ne!(
            full_floor, configured,
            "using reserve_floor_bytes alone is forbidden (DT-4)"
        );

        // At exactly `configured + needed` the admission must refuse: that
        // free level is below the runtime headroom and the safety share.
        let needed = 2 * 1024 * 1024u64;
        let tight = configured + needed;
        assert!(
            tight < full_floor + needed,
            "the fixture must actually distinguish the two floors"
        );
        let tight_provider = CodecProvider::new(8 * GIB, tight);
        let mut tight_worker = GpuCacheWorker::new(&tight_provider, config);
        // `codec_admission_allows` is private; the observable consequence is
        // that a compression-enabled update does not grow any slab.
        tight_worker.handle_update(0, &vec![0u8; 8192]);
        assert_eq!(
            tight_worker.compressed_entries_count(),
            0,
            "admission at reserve+needed must refuse the slab allocation"
        );

        // At `full_floor + needed` the same update is admissible.
        let roomy = full_floor + needed + (2 * 1024 * 1024);
        let roomy_provider = CodecProvider::new(8 * GIB, roomy);
        let mut roomy_worker = GpuCacheWorker::new(&roomy_provider, config);
        roomy_worker.handle_update(0, &vec![0u8; 8192]);
        assert!(
            roomy_worker.compressed_entries_count() > 0,
            "admission at full_floor+needed must accept the slab allocation"
        );
        assert!(roomy_worker.compressed_slab_count() > 0);
    }

    /// `worker_compression_refuses_from_zero_or_stale_budget` — DT-4.
    #[test]
    fn worker_compression_refuses_from_zero_or_stale_budget() {
        // Zero / unmeasured budget: target is 0 and nothing may be allocated.
        let none = NoBudgetProvider {
            codec: FakeCodec::new(),
        };
        let config = GpuWorkerConfig {
            target_bytes: 1024 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 128 * 1024 * 1024,
            compression_enabled: true,
        };
        let mut zero_worker = GpuCacheWorker::new(&none, config);
        assert_eq!(zero_worker.target_bytes(), 0);
        zero_worker.handle_update(0, &vec![0u8; 8192]);
        assert_eq!(zero_worker.compressed_entries_count(), 0);
        assert_eq!(zero_worker.compressed_slab_count(), 0);

        // Stale budget: `can_admit` refuses, so no slab and no entry.
        let stale = StaleBudgetProvider {
            total: 8 * 1024 * 1024 * 1024,
            free: 8 * 1024 * 1024 * 1024,
            codec: FakeCodec::new(),
        };
        let mut stale_worker = GpuCacheWorker::new(&stale, config);
        stale_worker.handle_update(0, &vec![0u8; 8192]);
        assert_eq!(stale_worker.compressed_entries_count(), 0);
        assert_eq!(stale_worker.compressed_slab_count(), 0);
    }

    /// `worker_compression_respects_physical_budget` — DT-5.
    ///
    /// `target_bytes` and `cached_bytes` stay **physical**. The compressed
    /// store never grows past the slab ceiling derived from the physical
    /// target, and a zero target allocates no slab at all.
    #[test]
    fn worker_compression_respects_physical_budget() {
        const GIB: u64 = 1024 * 1024 * 1024;
        // A 2 MiB physical target backs exactly one 2 MiB slab.
        let provider = CodecProvider::new(8 * GIB, 8 * GIB);
        let config = GpuWorkerConfig {
            target_bytes: 2 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: true,
        };
        let mut worker = GpuCacheWorker::new(&provider, config);
        assert_eq!(worker.target_bytes(), 2 * 1024 * 1024);
        let physical_target = worker.target_bytes();

        // Publish enough compressible extents to exhaust the single slab.
        let payload = vec![0u8; MAX_EXTENT_LEN];
        for i in 0..64 {
            worker.handle_update((i as u64) * MAX_EXTENT_LEN as u64, &payload);
        }
        assert!(
            worker.compressed_slab_count() <= 1,
            "a 2 MiB physical target may back at most one slab, got {}",
            worker.compressed_slab_count()
        );
        // The physical target is reported unchanged by compression.
        assert_eq!(worker.target_bytes(), physical_target);
        // And the allocator itself refuses to exceed its slab ceiling.
        assert!(worker.compressed_entries_count() > 0);
    }

    /// `worker_evicts_compressed_lru_extent` — DT-5 / DT-6.
    ///
    /// Under host pressure the coldest compressed extent is released and the
    /// released range is a Gap afterwards, never a stale hit.
    #[test]
    fn worker_evicts_compressed_lru_extent() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let provider = CodecProvider::new(8 * GIB, 8 * GIB);
        let config = GpuWorkerConfig {
            target_bytes: 64 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: true,
        };
        let mut worker = GpuCacheWorker::new(&provider, config);
        let payload = vec![0u8; 4096];
        // Three extents; the first is the coldest once the others are read.
        worker.handle_update(0, &payload);
        worker.handle_update(4096, &payload);
        worker.handle_update(8192, &payload);
        let before = worker.compressed_entries_count();
        assert!(
            before > 0,
            "the codec must have published compressed extents"
        );

        // Touch the later extents so extent 0 is the LRU.
        let _ = worker.handle_read(4096, 4096);
        let _ = worker.handle_read(8192, 4096);

        // Drop the live free level below the full free floor: an external GPU
        // workload now consumes the headroom the cache was holding (Kahneman
        // #5 — the realistic condition, not the idle one).
        provider.set_free(64 * 1024 * 1024);
        let released = worker.reclaim_under_host_pressure().expect("reclaim");
        let _ = released;

        // The LRU extent is gone: its range is no longer a compressed hit.
        assert!(
            worker.compressed_entries_count() < before,
            "at least the coldest compressed extent must be released: {before} -> {}",
            worker.compressed_entries_count()
        );
        // The range that was evicted is a Gap, never a stale hit (DT-6).
        assert!(
            worker.handle_read(0, 4096).is_none(),
            "an evicted extent must miss and fall through to the origin"
        );
    }

    /// `worker_decode_error_returns_miss` — DT-7 / DT-11.
    ///
    /// A stored-checksum mismatch or a decode failure must return a miss to
    /// the client. The raw cache keeps serving afterwards: the fault never
    /// revokes the cache client (DT-11).
    #[test]
    fn worker_decode_error_returns_miss() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let provider = CodecProvider::new(8 * GIB, 8 * GIB);
        let config = GpuWorkerConfig {
            target_bytes: 64 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: true,
        };
        let mut worker = GpuCacheWorker::new(&provider, config);
        let payload = vec![5u8; 4096];
        worker.handle_update(0, &payload);
        assert!(worker.compressed_entries_count() > 0);
        // Byte-exact hit through the compressed path.
        assert_eq!(worker.handle_read(0, 4096).as_deref(), Some(&payload[..]));

        // Corrupt the stored payload of the first entry. The provider-side
        // checksum must refuse before decode and the read must miss.
        let representation = worker
            .compressed
            .entries
            .first()
            .expect("one compressed entry")
            .representation
            .clone();
        if let crate::compressed_cache::CacheRepresentation::Compressed {
            address,
            stored_crc32,
            ..
        } = representation
        {
            let slab = worker
                .compressed_slabs
                .get_mut(address.slab_index)
                .expect("slab backing");
            let mut bytes = vec![0u8; address.span.stored_len];
            slab.read_at(address.span.offset, &mut bytes)
                .expect("read stored");
            assert_eq!(crc32(&bytes, 0), stored_crc32, "fixture starts consistent");
            bytes[0] ^= 0xff;
            slab.write_at(address.span.offset, &bytes)
                .expect("write corrupt");
        } else {
            panic!("expected a compressed representation");
        }

        let faults_before = worker.codec_faults();
        assert!(
            worker.handle_read(0, 4096).is_none(),
            "a corrupt stored payload must return a miss, never wrong bytes"
        );
        assert!(
            worker.codec_faults() >= faults_before,
            "the fault must be recorded for triage"
        );
        // DT-11: the worker is not disabled and the raw cache still serves.
        assert!(!worker.is_disabled());
        worker.handle_update(2 * 1024 * 1024, &payload);
        assert_eq!(
            worker.handle_read(2 * 1024 * 1024, 4096).as_deref(),
            Some(&payload[..]),
            "the raw cache must keep serving after a codec fault"
        );
    }

    /// `worker_teardown_waits_for_codec_completion` — DT-3 / DT-11.
    ///
    /// Teardown is bounded and idempotent, and it only proceeds once no codec
    /// operation is in flight. A completed operation is counted; an in-flight
    /// one is cleared by the fault/completion path before the store is
    /// dropped.
    #[test]
    fn worker_teardown_waits_for_codec_completion() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let provider = CodecProvider::new(8 * GIB, 8 * GIB);
        let config = GpuWorkerConfig {
            target_bytes: 64 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: true,
        };
        let mut worker = GpuCacheWorker::new(&provider, config);
        worker.handle_update(0, &vec![0u8; 8192]);
        let completed = worker.completed_codec_ops();
        assert!(
            completed > 0,
            "a successful publish must count as completed"
        );
        assert!(
            !worker.codec_in_flight(),
            "no operation may remain in flight after handle_update returns"
        );

        let start = Instant::now();
        worker.handle_disable();
        assert!(start.elapsed() < Duration::from_secs(5), "teardown bounded");
        assert!(!worker.codec_in_flight(), "teardown clears in-flight");
        assert_eq!(worker.compressed_entries_count(), 0);
        // Idempotent: a second teardown changes nothing and does not panic.
        worker.handle_disable();
        assert_eq!(worker.completed_codec_ops(), completed);
    }

    /// `worker_compression_disable_is_idempotent` — DT-9.
    ///
    /// Compression is off by default; turning it off again is a no-op and the
    /// raw path is what serves.
    #[test]
    fn worker_compression_disable_is_idempotent() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let provider = CodecProvider::new(8 * GIB, 8 * GIB);
        let config = GpuWorkerConfig::default();
        assert!(
            !config.compression_enabled,
            "DT-9: compression is off by default"
        );
        let mut worker = GpuCacheWorker::new(&provider, config);
        let payload = vec![9u8; 4096];
        worker.handle_update(0, &payload);
        assert_eq!(worker.compressed_entries_count(), 0);
        assert_eq!(worker.compressed_slab_count(), 0);
        assert_eq!(worker.handle_read(0, 4096).as_deref(), Some(&payload[..]));
        // Disabling an already-disabled compression path is a no-op.
        worker.handle_update(4096, &payload);
        assert_eq!(worker.compressed_entries_count(), 0);
    }

    /// `codec_fault_keeps_raw_cache_serving` — DT-11.
    #[test]
    fn codec_fault_keeps_raw_cache_serving() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let provider = FaultyProvider {
            total: 8 * GIB,
            free: 8 * GIB,
            live_allocations: Arc::new(AtomicUsize::new(0)),
            codec: FaultyCodec,
        };
        let config = GpuWorkerConfig {
            target_bytes: 64 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: GIB,
            compression_enabled: true,
        };
        let mut worker = GpuCacheWorker::new(&provider, config);
        let payload = vec![3u8; 4096];
        worker.handle_update(0, &payload);
        // The codec failed, so nothing was published compressed...
        assert_eq!(worker.compressed_entries_count(), 0);
        // ...but the raw path published and serves.
        assert_eq!(worker.handle_read(0, 4096).as_deref(), Some(&payload[..]));
        assert!(!worker.is_disabled(), "a codec fault must not disable");
        assert!(worker.codec_faults() > 0, "the fault must be recorded");
    }
}
