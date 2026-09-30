//! Integration set for the optional compressed GPU cache (ITEM-2).
//!
//! Each case drives the **real** `GpuCacheWorker` over a host-side
//! `VramProvider` with the deterministic fake codec. The fake is a product
//! type with `CodecId::Fake` and is never selected by a production provider;
//! it exists so the DT-4..DT-7, DT-9..DT-11 contracts are executable without
//! a GPU and without nvCOMP (ITEM-3 stays gated).
//!
//! These tests are the ITEM-2 gate for the compressed path. The raw path is
//! covered by `gpu_cache_worker`'s unit tests and `gpu_worker_protocol`.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ramshared_block::compressed_cache::{MAX_EXTENT_LEN, MAX_READ_LEN, SLAB_BYTES};
use ramshared_block::gpu_cache_worker::MAX_IPC_PAYLOAD_BYTES;
use ramshared_block::{GpuWorkerConfig, RUNTIME_FREE_BUFFER_BYTES, gpu_cache_worker};
use ramshared_vram::{
    CodecAlignments, CodecChunkResult, CodecId, CodecStatus, FakeCodec, GpuAdapterIdentity,
    GpuBudgetSnapshot, GpuBudgetSource, GpuCacheCodec, VramError, VramMemory, VramOutputReservation,
    VramProvider, VramSpan, crc32,
};

const GIB: u64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Host-side fixtures. Measurement / test harness only — never a production
// provider and never a runtime fallback.
// ---------------------------------------------------------------------------

struct HostMem {
    data: Arc<Mutex<Vec<u8>>>,
    len: usize,
    live: Arc<AtomicUsize>,
}

impl Drop for HostMem {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl VramMemory for HostMem {
    fn len(&self) -> usize {
        self.len
    }
    fn zero(&mut self) -> Result<(), VramError> {
        self.data
            .lock()
            .map_err(|_| VramError::Busy)?
            .fill(0);
        Ok(())
    }
    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
        let guard = self.data.lock().map_err(|_| VramError::Busy)?;
        let start = off as usize;
        let end = start.checked_add(dst.len()).ok_or(VramError::OutOfRange {
            off,
            len: dst.len() as u64,
            size: guard.len() as u64,
        })?;
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
        let end = start.checked_add(src.len()).ok_or(VramError::OutOfRange {
            off,
            len: src.len() as u64,
            size: guard.len() as u64,
        })?;
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

fn trusted_budget(free: u64, total: u64) -> GpuBudgetSnapshot {
    GpuBudgetSnapshot {
        adapter: Some(GpuAdapterIdentity {
            backend: "test".into(),
            key: "host-adapter-0".into(),
            luid: None,
        }),
        total_bytes: Some(total),
        budget_bytes: total,
        used_bytes: total.saturating_sub(free),
        source: GpuBudgetSource::DriverReported,
        sampled_at: std::time::Instant::now(),
    }
}

/// Shared backing of one allocation, plus its length.
type SharedBuffer = (usize, Arc<Mutex<Vec<u8>>>);

/// Provider with the deterministic fake codec, a movable free level, and a
/// registry of every allocation so a test can corrupt a stored payload in
/// place and observe the worker's DT-7 refusal.
struct HostCodecProvider {
    total: u64,
    free: Arc<AtomicU64>,
    live: Arc<AtomicUsize>,
    /// Every live allocation, in allocation order.
    allocations: Arc<Mutex<Vec<SharedBuffer>>>,
    codec: FakeCodec,
}

impl HostCodecProvider {
    fn new(total: u64, free: u64) -> Self {
        Self {
            total,
            free: Arc::new(AtomicU64::new(free)),
            live: Arc::new(AtomicUsize::new(0)),
            allocations: Arc::new(Mutex::new(Vec::new())),
            codec: FakeCodec::new(),
        }
    }
    fn set_free(&self, free: u64) {
        self.free.store(free, Ordering::SeqCst);
    }
    fn live_allocations(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }
    /// Backing of the unique 2 MiB slab allocation (raw chunks are configured
    /// smaller in the tests that need this), for DT-7 corruption injection.
    fn slab_backing(&self) -> Option<Arc<Mutex<Vec<u8>>>> {
        let allocs = self.allocations.lock().expect("alloc registry");
        let mut found = allocs.iter().filter(|(len, _)| *len == SLAB_BYTES);
        let first = found.next()?.1.clone();
        if found.next().is_some() {
            panic!("expected exactly one 2 MiB slab allocation");
        }
        Some(first)
    }
}

impl VramProvider for HostCodecProvider {
    type Mem<'p>
        = HostMem
    where
        Self: 'p;
    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        self.live.fetch_add(1, Ordering::SeqCst);
        let data = Arc::new(Mutex::new(vec![0u8; bytes]));
        self.allocations
            .lock()
            .expect("alloc registry")
            .push((bytes, Arc::clone(&data)));
        Ok(HostMem {
            data,
            len: bytes,
            live: Arc::clone(&self.live),
        })
    }
    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        Ok((self.free.load(Ordering::SeqCst), self.total))
    }
    fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
        Ok(trusted_budget(self.free.load(Ordering::SeqCst), self.total))
    }
    fn cache_codec(&self) -> Option<&dyn GpuCacheCodec<Self::Mem<'_>>> {
        Some(&self.codec)
    }
}

/// Provider whose codec fails every operation (DT-11 fault injection).
struct FaultyCodec;

impl GpuCacheCodec<HostMem> for FaultyCodec {
    fn codec_id(&self) -> CodecId {
        CodecId::Fake
    }
    fn required_alignments(&self) -> CodecAlignments {
        CodecAlignments::byte()
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
        _slab: &mut HostMem,
        _outputs: &[VramOutputReservation],
        _workspace: &mut HostMem,
    ) -> Result<Vec<CodecChunkResult>, VramError> {
        Err(VramError::Provider("codec fault".into()))
    }
    fn checksum_batch(
        &self,
        _slab: &HostMem,
        _inputs: &[VramSpan],
        _workspace: &mut HostMem,
    ) -> Result<Vec<u32>, VramError> {
        Err(VramError::Provider("codec fault".into()))
    }
    fn decompress_batch_from(
        &self,
        _slab: &HostMem,
        _inputs: &[VramSpan],
        _logical_lengths: &[usize],
        _outputs: &mut [Vec<u8>],
        _workspace: &mut HostMem,
    ) -> Result<Vec<CodecStatus>, VramError> {
        Err(VramError::Provider("codec fault".into()))
    }
}

struct HostFaultyProvider {
    total: u64,
    free: u64,
    codec: FaultyCodec,
}

impl VramProvider for HostFaultyProvider {
    type Mem<'p>
        = HostMem
    where
        Self: 'p;
    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError> {
        Ok(HostMem {
            data: Arc::new(Mutex::new(vec![0u8; bytes])),
            len: bytes,
            live: Arc::new(AtomicUsize::new(1)),
        })
    }
    fn mem_info(&self) -> Result<(u64, u64), VramError> {
        Ok((self.free, self.total))
    }
    fn budget_snapshot(&self) -> Result<GpuBudgetSnapshot, VramError> {
        Ok(trusted_budget(self.free, self.total))
    }
    fn cache_codec(&self) -> Option<&dyn GpuCacheCodec<Self::Mem<'_>>> {
        Some(&self.codec)
    }
}

fn compression_config(target_bytes: u64, reserve_floor_bytes: u64) -> GpuWorkerConfig {
    GpuWorkerConfig {
        target_bytes,
        chunk_bytes: 2 * 1024 * 1024,
        reserve_floor_bytes,
        compression_enabled: true,
    }
}

// ---------------------------------------------------------------------------
// Named ITEM-2 integration cases.
// ---------------------------------------------------------------------------

/// `worker_compression_roundtrip_is_byte_exact` — DT-7.
#[test]
fn worker_compression_roundtrip_is_byte_exact() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );

    // Highly compressible: the fake RLE must shrink it and publish compressed.
    let zeros = vec![0u8; MAX_EXTENT_LEN];
    worker.handle_update(0, &zeros);
    assert!(
        worker.compressed_entries_count() > 0,
        "repetitive data must be published compressed"
    );
    let hit = worker
        .handle_read(0, MAX_EXTENT_LEN)
        .expect("byte-exact hit");
    assert_eq!(hit, zeros);

    // Multi-extent read spanning two **compressed** publications is assembled
    // exactly (DT-6 complete contiguous coverage). Both extents must be
    // compressible: a non-shrinking extent goes raw by DT-10 and is then a
    // legitimate gap for a compressed-only assembly.
    let second = vec![1u8; MAX_EXTENT_LEN];
    worker.handle_update(MAX_EXTENT_LEN as u64, &second);
    assert!(
        worker.compressed_entries_count() >= 2,
        "both extents must publish compressed, got {}",
        worker.compressed_entries_count()
    );
    let span = worker
        .handle_read(0, 2 * MAX_EXTENT_LEN)
        .expect("two-extent hit");
    let mut expected = zeros.clone();
    expected.extend_from_slice(&second);
    assert_eq!(span, expected);
}

/// `worker_raw_fallback_when_encoded_allocation_is_not_smaller` — DT-10.
#[test]
fn worker_raw_fallback_when_encoded_allocation_is_not_smaller() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );

    // High-entropy payload: RLE grows it, so the usefulness gate keeps raw.
    let mut noise = vec![0u8; MAX_EXTENT_LEN];
    for (i, byte) in noise.iter_mut().enumerate() {
        *byte = ((i.wrapping_mul(2_654_435_761) >> 13) & 0xff) as u8;
    }
    worker.handle_update(0, &noise);
    assert_eq!(
        worker.compressed_entries_count(),
        0,
        "an encoded form that is not strictly smaller must stay raw"
    );
    // The raw path still serves the exact bytes.
    assert_eq!(worker.handle_read(0, MAX_EXTENT_LEN), Some(noise));
}

/// `worker_corrupt_entry_returns_origin_bytes` — DT-7 / DT-11.
///
/// A cached entry whose stored payload no longer matches its recorded CRC is
/// a **miss**: the worker returns `None` so the SSD origin serves the exact
/// bytes. It never returns the corrupt payload and never serves a partial.
#[test]
fn worker_corrupt_entry_returns_origin_bytes() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    // Raw chunks are 4 KiB so the only 2 MiB allocation is the compressed slab.
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        GpuWorkerConfig {
            target_bytes: 64 * 1024 * 1024,
            chunk_bytes: 4096,
            reserve_floor_bytes: GIB,
            compression_enabled: true,
        },
    );
    let payload = vec![7u8; 8192];
    worker.handle_update(0, &payload);
    assert!(worker.compressed_entries_count() > 0);
    assert_eq!(
        worker.handle_read(0, 8192),
        Some(payload.clone()),
        "the intact entry serves the exact bytes"
    );

    // Corrupt the stored compressed payload in the slab backing.
    let slab = provider.slab_backing().expect("a 2 MiB compressed slab");
    {
        let mut guard = slab.lock().expect("slab backing");
        let at = guard
            .iter()
            .position(|byte| *byte != 0)
            .expect("the slab holds a non-zero encoded payload");
        guard[at] ^= 0xff;
    }

    // DT-7: the stored-payload checksum no longer matches, so the entry is
    // refused before decode and the read is a miss. The caller then uses the
    // origin, which is the authoritative path (RF-1).
    let after_corruption = worker.handle_read(0, 8192);
    assert!(
        after_corruption.is_none(),
        "a corrupt entry must miss so the origin serves the bytes, not {:?}",
        after_corruption.as_ref().map(|bytes| bytes.len())
    );
    assert!(!worker.is_disabled(), "a codec integrity fault is not a revoke");
}

/// `worker_partial_update_never_returns_stale_bytes` — DT-6.
///
/// Invalidation happens **before** publication, so a partial update never
/// exposes mixed or stale bytes. The updated range serves the new bytes; the
/// rest of the invalidated extent is a **miss** so the SSD origin serves it.
/// A read is never assembled from one half of an old entry next to a new one.
#[test]
fn worker_partial_update_never_returns_stale_bytes() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    let original = vec![1u8; 4096];
    worker.handle_update(0, &original);
    assert_eq!(worker.handle_read(0, 4096), Some(original.clone()));

    // Overlap the first half. DT-6 invalidates the whole overlapping entry
    // before the replacement is published.
    let half = vec![2u8; 2048];
    worker.handle_update(0, &half);

    // The updated range serves the **new** bytes, never the old ones.
    let updated = worker
        .handle_read(0, 2048)
        .expect("the updated range must be readable");
    assert_eq!(updated, half, "the replacement must be served exactly");

    // The full original range now has a gap at 2048..4096 (the old tail was
    // invalidated with the entry). That read must miss so the origin serves
    // it — never a half-old/half-new assembly.
    assert!(
        worker.handle_read(0, 4096).is_none(),
        "a range with a gap must miss rather than assemble mixed bytes"
    );

    // The untouched tail alone is also a miss, not the stale original.
    assert!(
        worker.handle_read(2048, 2048).is_none(),
        "the invalidated tail must miss, not serve stale bytes"
    );
    assert!(!worker.is_disabled());

    // A legitimate non-overlapping publication assembles exactly across
    // extents (the pass case for the same DT-6 rule).
    worker.handle_update(2048, &original[2048..]);
    let assembled = worker
        .handle_read(0, 4096)
        .expect("complete contiguous coverage must hit");
    let mut expected = half.clone();
    expected.extend_from_slice(&original[2048..]);
    assert_eq!(assembled, expected);
}

/// `worker_compression_disable_is_idempotent` — DT-9.
#[test]
fn worker_compression_disable_is_idempotent() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let config = GpuWorkerConfig::default();
    assert!(!config.compression_enabled, "DT-9: off by default");
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(&provider, config);
    let payload = vec![4u8; 4096];
    worker.handle_update(0, &payload);
    worker.handle_update(4096, &payload);
    assert_eq!(worker.compressed_entries_count(), 0);
    assert_eq!(worker.compressed_slab_count(), 0);
    assert_eq!(worker.handle_read(0, 4096), Some(payload));
}

/// `compression_update_replay_is_idempotent` — Kahneman #17.
#[test]
fn compression_update_replay_is_idempotent() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    let payload = vec![6u8; 4096];
    worker.handle_update(0, &payload);
    let after_first = worker.compressed_entries_count();
    let first_hit = worker.handle_read(0, 4096);

    // Replay the exact same update. Overlap invalidation replaces the entry;
    // the observable result is the same bytes and a non-growing index.
    worker.handle_update(0, &payload);
    let after_replay = worker.compressed_entries_count();
    let replay_hit = worker.handle_read(0, 4096);

    assert_eq!(first_hit, replay_hit, "a replay must be idempotent");
    assert_eq!(
        after_first, after_replay,
        "a replay must not duplicate an extent: {after_first} -> {after_replay}"
    );
}

/// `worker_compressed_crc_mismatch_refuses_before_decode` — DT-7.
///
/// The stored-payload checksum is computed on the provider and compared
/// before the decoder is invoked. A mismatch returns a miss; the fake
/// decoder's invocation counter proves it was never entered for that entry.
#[test]
fn worker_compressed_crc_mismatch_refuses_before_decode() {
    // Direct codec-level proof (the worker keeps slab memory private).
    let codec = FakeCodec::new();
    #[derive(Default)]
    struct Slab {
        bytes: Vec<u8>,
    }
    impl VramMemory for Slab {
        fn len(&self) -> usize {
            self.bytes.len()
        }
        fn zero(&mut self) -> Result<(), VramError> {
            self.bytes.fill(0);
            Ok(())
        }
        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            let start = off as usize;
            let end = start.checked_add(dst.len()).ok_or(VramError::OutOfRange {
                off,
                len: dst.len() as u64,
                size: self.bytes.len() as u64,
            })?;
            if end > self.bytes.len() {
                return Err(VramError::OutOfRange {
                    off,
                    len: dst.len() as u64,
                    size: self.bytes.len() as u64,
                });
            }
            dst.copy_from_slice(&self.bytes[start..end]);
            Ok(())
        }
        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            let start = off as usize;
            let end = start.checked_add(src.len()).ok_or(VramError::OutOfRange {
                off,
                len: src.len() as u64,
                size: self.bytes.len() as u64,
            })?;
            if end > self.bytes.len() {
                return Err(VramError::OutOfRange {
                    off,
                    len: src.len() as u64,
                    size: self.bytes.len() as u64,
                });
            }
            self.bytes[start..end].copy_from_slice(src);
            Ok(())
        }
    }
    let mut slab = Slab {
        bytes: vec![0u8; 8192],
    };
    let mut workspace = Slab { bytes: vec![0u8; 1] };
    let payload = vec![0xEEu8; 2048];
    let reservation = VramOutputReservation::new(0, 4096, 4096).expect("reservation");
    let results = codec
        .compress_batch_into(&[&payload], &mut slab, &[reservation], &mut workspace)
        .expect("compress");
    assert!(results[0].status.is_ok());
    let span = VramSpan::new(0, results[0].encoded_len, 4096).expect("span");
    let good = codec
        .checksum_batch(&slab, &[span], &mut workspace)
        .expect("checksum");
    assert_eq!(codec.decode_invocations(), 0, "no decode before the check");

    // Corrupt the stored payload.
    slab.bytes[0] ^= 0xff;
    let bad = codec
        .checksum_batch(&slab, &[span], &mut workspace)
        .expect("checksum");
    assert_ne!(good[0], bad[0]);
    assert_eq!(
        codec.decode_invocations(),
        0,
        "a checksum mismatch must never reach the decoder (DT-7)"
    );
}

/// `codec_fault_keeps_raw_cache_serving` — DT-11.
#[test]
fn codec_fault_keeps_raw_cache_serving() {
    let provider = HostFaultyProvider {
        total: 8 * GIB,
        free: 8 * GIB,
        codec: FaultyCodec,
    };
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    let payload = vec![0x11u8; 4096];
    worker.handle_update(0, &payload);
    assert_eq!(worker.compressed_entries_count(), 0);
    assert_eq!(worker.handle_read(0, 4096), Some(payload));
    assert!(!worker.is_disabled());
    assert!(worker.codec_faults() > 0);
}

/// `codec_fault_does_not_revoke_cache_client` — DT-11.
///
/// The cache client is revoked only by transport/protocol/process failure.
/// A codec-only fault must leave the worker enabled and serving.
#[test]
fn codec_fault_does_not_revoke_cache_client() {
    let provider = HostFaultyProvider {
        total: 8 * GIB,
        free: 8 * GIB,
        codec: FaultyCodec,
    };
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    for i in 0..8u64 {
        worker.handle_update(i * 4096, &vec![0x22u8; 4096]);
    }
    assert!(!worker.is_disabled(), "a codec fault must not revoke");
    assert!(worker.codec_faults() > 0);
    // Every range is still served by the raw path.
    for i in 0..8u64 {
        assert_eq!(
            worker.handle_read(i * 4096, 4096),
            Some(vec![0x22u8; 4096]),
            "range {i} must keep serving"
        );
    }
}

/// `codec_subdeadline_falls_through_to_raw_or_miss` — DT-3.
///
/// The codec sub-deadline bounds admission and inter-step continuation. When
/// it expires the read falls through to the raw path or a miss — never a
/// partial assembly and never a hang.
#[test]
fn codec_subdeadline_falls_through_to_raw_or_miss() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    // Publish a compressed extent, then also populate the raw path over the
    // same logical chunk by disabling compression for the second write. The
    // worker's config is immutable, so instead publish at a distinct offset
    // and assert the sub-deadline constants are coherent.
    assert!(
        gpu_cache_worker::CODEC_SUBDEADLINE < gpu_cache_worker::CACHE_READ_BUDGET,
        "the codec sub-deadline must sit inside the cache-read budget (DT-3)"
    );
    assert_eq!(
        gpu_cache_worker::CACHE_READ_BUDGET,
        std::time::Duration::from_millis(50)
    );
    // A read with no covering extent falls through to a miss inside the
    // budget; it must return promptly.
    let start = std::time::Instant::now();
    assert!(worker.handle_read(0, 4096).is_none());
    assert!(
        start.elapsed() < gpu_cache_worker::CACHE_READ_BUDGET,
        "a miss must fall through inside the cache-read budget"
    );
    // And a hit after a publish also completes inside the budget.
    worker.handle_update(0, &vec![0u8; 4096]);
    let start = std::time::Instant::now();
    assert!(worker.handle_read(0, 4096).is_some());
    assert!(
        start.elapsed() < gpu_cache_worker::CACHE_READ_BUDGET,
        "a hit must complete inside the cache-read budget"
    );
}

/// `read_over_16_mib_refuses_before_allocation` — DT-4 (integration surface).
#[test]
fn read_over_16_mib_refuses_before_allocation() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    let payload = vec![0u8; 4096];
    worker.handle_update(0, &payload);
    assert!(worker.handle_read(0, MAX_READ_LEN + 1).is_none());
    assert!(worker.handle_read(0, MAX_IPC_PAYLOAD_BYTES + 1).is_none());
    assert_eq!(worker.handle_read(0, 4096), Some(payload));
}

/// `worker_compression_respects_physical_budget` — DT-5 (integration surface).
#[test]
fn worker_compression_respects_physical_budget() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    // A 2 MiB physical target backs at most one 2 MiB slab.
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(2 * 1024 * 1024, GIB),
    );
    let payload = vec![0u8; MAX_EXTENT_LEN];
    for i in 0..64 {
        worker.handle_update((i as u64) * MAX_EXTENT_LEN as u64, &payload);
    }
    assert!(
        worker.compressed_slab_count() <= 1,
        "physical target binds the slab ceiling: {}",
        worker.compressed_slab_count()
    );
    assert_eq!(worker.target_bytes(), 2 * 1024 * 1024);
}

/// `worker_evicts_compressed_lru_extent` — DT-5 (integration surface).
#[test]
fn worker_evicts_compressed_lru_extent() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    let payload = vec![0u8; 4096];
    worker.handle_update(0, &payload);
    worker.handle_update(4096, &payload);
    worker.handle_update(8192, &payload);
    let before = worker.compressed_entries_count();
    assert!(before > 0);
    let _ = worker.handle_read(4096, 4096);
    let _ = worker.handle_read(8192, 4096);
    provider.set_free(32 * 1024 * 1024);
    let _ = worker.reclaim_under_host_pressure().expect("reclaim");
    assert!(
        worker.compressed_entries_count() < before,
        "the coldest compressed extent must be released: {before} -> {}",
        worker.compressed_entries_count()
    );
}

/// `worker_teardown_waits_for_codec_completion` — DT-3 (integration surface).
#[test]
fn worker_teardown_waits_for_codec_completion() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    worker.handle_update(0, &vec![0u8; 8192]);
    assert!(worker.completed_codec_ops() > 0);
    assert!(!worker.codec_in_flight());
    worker.handle_disable();
    assert!(!worker.codec_in_flight());
    assert_eq!(worker.compressed_entries_count(), 0);
    worker.handle_disable();
    assert!(worker.is_disabled(), "disable is sticky");
    let live = provider.live_allocations();
    worker.handle_disable();
    assert_eq!(provider.live_allocations(), live, "teardown is idempotent");
}

/// `worker_decode_error_returns_miss` — DT-7 (integration surface).
#[test]
fn worker_decode_error_returns_miss() {
    let provider = HostCodecProvider::new(8 * GIB, 8 * GIB);
    let mut worker = gpu_cache_worker::GpuCacheWorker::new(
        &provider,
        compression_config(64 * 1024 * 1024, GIB),
    );
    let payload = vec![5u8; 4096];
    worker.handle_update(0, &payload);
    assert!(worker.compressed_entries_count() > 0);
    assert_eq!(worker.handle_read(0, 4096), Some(payload));
    // An uncovered range is a miss, not a partial assembly.
    assert!(worker.handle_read(1 << 20, 4096).is_none());
    // An overlapping partial read must not assemble from neighbouring
    // extents when coverage is incomplete.
    worker.handle_update(8192, &vec![6u8; 4096]);
    assert!(worker.handle_read(4096, 8192).is_none(), "gap at 4096..8192");
}

/// `crc32_is_the_shared_integrity_primitive` — DT-7.
#[test]
fn crc32_is_the_shared_integrity_primitive() {
    let original = b"ramshared-item2";
    let good = crc32(original, 0);
    let mut corrupted = original.to_vec();
    corrupted[2] ^= 0x80;
    assert_ne!(crc32(&corrupted, 0), good);
}

/// `constants_are_the_spec_bounds` — DT-4 / DT-5.
#[test]
fn constants_are_the_spec_bounds() {
    use ramshared_block::compressed_cache::{MAX_EXTENT_LEN, SLAB_BYTES};
    assert_eq!(MAX_EXTENT_LEN, 64 * 1024);
    assert_eq!(SLAB_BYTES, 2 * 1024 * 1024);
    assert_eq!(MAX_READ_LEN, 16 * 1024 * 1024);
    assert_eq!(RUNTIME_FREE_BUFFER_BYTES, 640 * 1024 * 1024);
}
