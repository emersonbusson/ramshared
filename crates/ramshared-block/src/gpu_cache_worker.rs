//! Process-isolated GPU cache worker.
//!
//! Provides out-of-process VRAM allocation and cache chunk management
//! communicating over an anonymous Unix domain socket pair.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ramshared_vram::{GpuBudgetSnapshot, GpuBudgetTelemetry, VramError, VramMemory, VramProvider};

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
const RUNTIME_RECOVERY_BUFFER_BYTES: u64 = 896 * 1024 * 1024;
const MAX_GPU_BUDGET_PAYLOAD_BYTES: usize = 4096;

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

    pub fn decode(buf: &[u8; FRAME_HEADER_LEN]) -> Self {
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
        Self {
            msg_type,
            status,
            correlation_id,
            offset,
            payload_len,
            aux,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuWorkerConfig {
    pub target_bytes: u64,
    pub chunk_bytes: usize,
    pub reserve_floor_bytes: u64,
}

impl Default for GpuWorkerConfig {
    fn default() -> Self {
        Self {
            target_bytes: 4 * 1024 * 1024 * 1024,
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 1536 * 1024 * 1024,
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

pub struct GpuCacheWorker<'p, P: VramProvider + 'p> {
    provider: &'p P,
    config: GpuWorkerConfig,
    effective_target_bytes: u64,
    chunks: HashMap<u64, CacheChunk<'p, P>>,
    disabled: bool,
    pressure_constrained: bool,
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

    pub fn handle_read(&mut self, offset: u64, len: usize) -> Option<Vec<u8>> {
        if self.disabled || self.config.chunk_bytes == 0 || len == 0 {
            return None;
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

    pub fn handle_update(&mut self, offset: u64, data: &[u8]) {
        if self.disabled || self.config.chunk_bytes == 0 || data.is_empty() {
            return;
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

    pub fn handle_promote(&mut self, offset: u64, data: &[u8]) {
        self.handle_update(offset, data);
    }

    pub fn handle_disable(&mut self) {
        self.disabled = true;
        self.chunks.clear();
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
            true
        } else {
            false
        }
    }
}

pub fn run_gpu_worker_loop<P: VramProvider>(
    mut socket: UnixStream,
    provider: P,
    config: GpuWorkerConfig,
) -> Result<(), String> {
    let mut worker = GpuCacheWorker::new(&provider, config);
    let mut hdr_buf = [0u8; FRAME_HEADER_LEN];

    loop {
        match socket.read_exact(&mut hdr_buf) {
            Ok(()) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(format!("worker read header error: {e}")),
        }

        let hdr = FrameHeader::decode(&hdr_buf);

        let payload = if hdr.payload_len > 0 {
            if hdr.payload_len as usize > MAX_IPC_PAYLOAD_BYTES {
                return Err("worker payload len exceeds limit".to_string());
            }
            let mut buf = vec![0u8; hdr.payload_len as usize];
            if let Err(e) = socket.read_exact(&mut buf) {
                return Err(format!("worker read payload error: {e}"));
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
                    aux: (worker.cached_bytes() >> 10) as u32,
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
                        aux: (worker.cached_bytes() >> 10) as u32,
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
                        aux: (worker.cached_bytes() >> 10) as u32,
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
                let budget_payload = if worker.is_disabled() {
                    Vec::new()
                } else {
                    worker
                        .provider
                        .budget_snapshot()
                        .ok()
                        .map(|snapshot| {
                            GpuBudgetTelemetry::from_snapshot(&snapshot, unix_time_ms())
                        })
                        .and_then(|telemetry| serde_json::to_vec(&telemetry).ok())
                        .filter(|payload| payload.len() <= MAX_GPU_BUDGET_PAYLOAD_BYTES)
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
                    payload_len: budget_payload.len() as u32,
                    aux: (worker.cached_bytes() >> 10) as u32,
                };
                if let Err(e) = socket.write_all(&resp.encode()) {
                    return Err(format!("worker write heartbeat error: {e}"));
                }
                if let Err(e) = socket.write_all(&budget_payload) {
                    return Err(format!(
                        "worker write heartbeat budget telemetry error: {e}"
                    ));
                }
            }
            _ => {}
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
    fn worker_budget_target_requires_external_adapter_bound_snapshot() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let config = GpuWorkerConfig {
            target_bytes: 4 * GIB,
            chunk_bytes: 512 * 1024 * 1024,
            reserve_floor_bytes: GIB,
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
}
