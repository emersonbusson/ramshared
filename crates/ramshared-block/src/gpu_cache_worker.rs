//! Process-isolated GPU cache worker.
//!
//! Provides out-of-process VRAM allocation and cache chunk management
//! communicating over an anonymous Unix domain socket pair.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use ramshared_vram::{VramMemory, VramProvider};

pub const FRAME_HEADER_LEN: usize = 32;

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
}

pub struct GpuCacheWorker<'p, P: VramProvider + 'p> {
    provider: &'p P,
    config: GpuWorkerConfig,
    effective_target_bytes: u64,
    chunks: HashMap<u64, CacheChunk<'p, P>>,
    disabled: bool,
}

impl<'p, P: VramProvider + 'p> GpuCacheWorker<'p, P> {
    pub fn new(provider: &'p P, config: GpuWorkerConfig) -> Self {
        let effective_target = match provider.mem_info() {
            Ok((_free, total)) => {
                let twenty_percent = total.div_ceil(5);
                let effective_reserve = config.reserve_floor_bytes.max(twenty_percent);
                total
                    .saturating_sub(effective_reserve)
                    .min(config.target_bytes)
            }
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
            chunk.last_accessed = Instant::now();
            let _ = chunk.mem.write_at(chunk_off, data);
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

    fn allocate_and_write(&mut self, chunk_base: u64, chunk_off: u64, data: &[u8]) {
        let chunk_bytes = self.config.chunk_bytes;
        let needed = chunk_bytes as u64;

        while self.cached_bytes().saturating_add(needed) > self.effective_target_bytes {
            if !self.evict_coldest_chunk() {
                return;
            }
        }

        if let Ok(mut mem) = self.provider.alloc(chunk_bytes) {
            let _ = mem.write_at(chunk_off, data);
            self.chunks.insert(
                chunk_base,
                CacheChunk {
                    mem,
                    last_accessed: Instant::now(),
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
            if hdr.payload_len > 16 * 1024 * 1024 {
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
                let resp = FrameHeader {
                    msg_type: MSG_HEARTBEAT_RESP,
                    status: STATUS_OK,
                    correlation_id: hdr.correlation_id,
                    offset: worker.target_bytes(),
                    payload_len: 0,
                    aux: (worker.cached_bytes() >> 10) as u32,
                };
                if let Err(e) = socket.write_all(&resp.encode()) {
                    return Err(format!("worker write heartbeat error: {e}"));
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
    use ramshared_vram::VramError;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

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
    fn worker_respects_headroom_floor() {
        let total_vram = 8 * 1024 * 1024 * 1024u64; // 8 GiB
        let free_vram = 7 * 1024 * 1024 * 1024u64; // 7 GiB
        let provider = FakeProvider::new(total_vram, free_vram);

        // 20% of 8 GiB = 1.6 GiB (1717986919 bytes)
        // reserve floor = 2 GiB (2147483648 bytes)
        // effective reserve = max(2 GiB, 1.6 GiB) = 2 GiB
        // max allocation = 8 GiB - 2 GiB = 6 GiB
        let config = GpuWorkerConfig {
            target_bytes: 10 * 1024 * 1024 * 1024, // Request 10 GiB (more than available)
            chunk_bytes: 2 * 1024 * 1024,
            reserve_floor_bytes: 2 * 1024 * 1024 * 1024,
        };

        let worker = GpuCacheWorker::new(&provider, config);
        assert_eq!(worker.target_bytes(), 6 * 1024 * 1024 * 1024);
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
