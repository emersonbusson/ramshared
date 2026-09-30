# IMPL — cuda-rust-native-tiering

## Tracking Record
- **SPEC**: [SPEC.md](SPEC.md) — lossless compression for the revocable VRAM cache (cache-only).
- **PRD**: [PRD.md](PRD.md)
- **AUDIT-2.5**: [AUDIT-2.5.md](AUDIT-2.5.md) (2026-09-30 pass 3: conditional go for ITEM-1/ITEM-2 only after same-turn fixes).
- **Current state**: ITEM-1 (codec contract + deterministic fake codec),
  ITEM-2 (extent index, slab allocator, worker wiring, CPU-codec control arm),
  and ITEM-4 (versioned worker-cache telemetry envelope and monitor labels)
  are implemented, covered, and green. ITEM-3 is **gated** (no nvCOMP runtime
  on this host; entry gate 1 — the CPU-codec control arm record — is now
  closed). ITEM-5 is not started and requires real paired GPU runs. There is
  still no `cutile` / `cuda-oxide` backend and no nvCOMP loader.

## Superseded plan (do not implement)

The earlier tracking list below described **swap-page compression** (variable-sized
compressed swap pages, LZ4 PTX kernels, crash-consistent raw/compressed swap metadata) and
an async CUDA backend migration. That scope is **explicitly rejected** by the current
PRD/SPEC: compression is confined to the disposable VRAM cache, and the SSD origin, Linux
swap format, block ABI, and Windows pagefile are out of scope.

| Superseded item | Why it is gone |
| --- | --- |
| ITEM-1 Evaluate Context & Device Discovery (`cuda-core`/`cuda-async` migration) | Backend migration is out of scope. The existing Driver API path stays. |
| ITEM-2 Async Backend & Cancellation Token | Out of scope. Worker isolation and observed-completion retention replace cancellation promises. |
| ITEM-3 In-GPU Page Compression Kernel Dispatch (LZ4 PTX, swap metadata) | Swap-format compression is rejected (PRD §3 discarded alternatives). A hand-written codec kernel is also rejected. |
| ITEM-4 Broker Worker Wiring (`gpu_compression_ratio`, `gpu_async_cancellation`) | Replaced by the cache telemetry envelope (SPEC Observability). |

Historical text is retained in git history, not here.

## Step 3 checklist (derived from SPEC.md)

Implement `SPEC.md` only.

### ITEM-1 — Codec contract and fake codec (no GPU calls) — ✅ implemented

- [x] `crates/ramshared-vram/src/codec.rs`: `CodecId`, `CodecStatus`, `VramSpan`,
      `VramOutputReservation`, `CodecAlignments`, `CodecChunkResult`,
      `GpuCacheCodec<M: VramMemory>`. Every offset-plus-length calculation is
      checked before any provider call (RF-6); `CodecAlignments::validate`
      refuses zero or non-power-of-two alignments (DT-4).
- [x] `VramProvider::cache_codec()` default returning `None` (DT-2 / RF-7).
      No compression methods were added to raw `VramMemory`.
- [x] `crc32` (IEEE 802.3) for both the original-bytes and stored-payload
      checksums (DT-7). Detects accidental corruption, not malicious
      modification.
- [x] Deterministic `FakeCodec` (RLE wire format `0x00, n, v` = run of n,
      `0x01, n, b0..bn-1` = literal) with `compress_invocations` /
      `decode_invocations` counters, `CodecId::Fake`, byte alignments, zero
      workspace. Exact-byte round trips; checksum refusal **before** decode
      (the fake decoder invocation count stays zero on a mismatch).
- [x] Tests: `codec::tests::codec_bounds_reject_overflow`,
      `codec::tests::unsupported_provider_is_raw_only`,
      `codec_id_and_status_are_labelled`, `crc32_detects_single_bit_corruption`,
      `span_and_reservation_reject_inverted_ranges`,
      `fake_codec_roundtrip_is_byte_exact`,
      `fake_codec_checksum_mismatch_refuses_before_decode`.

**Cover:** `crates/ramshared-vram/src/codec.rs` — **90.1%** (317/352 lines), gate PASSED.
(Re-measured 2026-09-30 with the full fake codec and both integration
suites in the run; the earlier 132/135 figure predated `FakeCodec`.)

### ITEM-2 — Extent index, slab allocator, raw bypass — ✅ implemented

- [x] `crates/ramshared-block/src/compressed_cache.rs`: `CacheEntry`,
      `CacheRepresentation`, `VramSlab`, `VramSpanAllocator`, `split_extent`,
      `invalidate_overlaps`, `read_coverage`. Non-overlapping logical extents
      (DT-6); 2 MiB coalescing free-range slabs (DT-5); 64 KiB extent cap
      (DT-4); metadata cap `min(16 MiB, max(64 KiB, physical_target/256))`
      (DT-5); zero physical target allocates no slab and no metadata (DT-5);
      live extents are not compacted — fragmentation refuses admission.
- [x] **`SlabSpan` addressing (fragility closed).** `VramSpan.offset` is
      slab-local, so `free()` was previously slab-ambiguous: with ≥2 slabs a
      free of offset 0 always landed in slab 0. `SlabSpan { slab_index, span }`
      is now the full address and `free()` is index-authoritative — an unknown
      index is refused (`OutOfRange`), never applied to slab 0. Proof:
      `free_addresses_the_slab_not_the_offset`,
      `slab_span_refuses_a_span_outside_its_slab`.
- [x] **LRU on compressed extents (gap closed).** `CacheEntry.last_accessed`
      is bumped on serve and drives `evict_coldest_compressed_extent()`.
      `evict_coldest_chunk()` falls back to it, so host-pressure reclaim
      releases compressed VRAM exactly like raw chunks (it previously only
      ever released raw chunks). Proof: `worker_evicts_compressed_lru_extent`.
- [x] `gpu_cache_worker.rs`: `compression_enabled = false` by default (DT-9);
      `invalidate_overlaps` **before** update publication (DT-6); read ceiling
      `MAX_READ_LEN = 16 MiB` checked **before** any response allocation
      (DT-4); private full-range assembly with complete contiguous coverage
      (DT-6); provider-side `checksum_batch` before `decompress_batch_from`,
      mismatch refuses without invoking the decoder (DT-7); strictly-smaller
      usefulness gate, raw otherwise (DT-10).
- [x] Codec/slab admission consumes the parent full free floor
      `required_free_bytes(configured_reserve_bytes, runtime_headroom_bytes)` =
      `max(configured, ceil(min(total, budget)/5)) + runtime_headroom`
      (`RUNTIME_FREE_BUFFER_BYTES = 640 MiB`). Using the configured
      `reserve_floor_bytes` alone is forbidden (DT-4, NFR-2).
- [x] Codec-operation sub-deadline `CODEC_SUBDEADLINE = 20 ms` inside the
      `CACHE_READ_BUDGET = 50 ms` (DT-3). Codec faults leave the raw cache
      serving and do not revoke the client (DT-11).
- [x] **ITEM-2 exit:** measurement-only CPU-codec control arm (NFR-6b) recorded
      through the host-side harness outside `GpuCacheCodec`
      (`crates/ramshared-block/tests/cpu_codec_control_arm.rs`). The harness
      takes identical DT-4 extents, encodes/decodes them in host memory with
      the same lossless RLE, and reports the same metric envelope and
      integrity checks (byte-exact round trip, checksum refusal before decode,
      encoded vs raw allocated bytes, encode/decode wall and process-CPU time
      from `/proc/self/stat`). It is never a trait implementation, never a
      runtime provider, never a production fallback. This record is the
      ITEM-3 entry gate; ITEM-5 re-runs it.
- [x] Tests: `compressed_cache::tests::extent_split_respects_maximum`,
      `allocator_coalesces_and_refuses_fragmented_request`,
      `overlap_invalidation_removes_only_affected_entries`,
      `metadata_budget_caps_entry_count`,
      `zero_physical_target_allocates_no_metadata`,
      `read_coverage_distinguishes_exact_contiguous_gap_and_overlap`,
      `free_addresses_the_slab_not_the_offset`,
      `slab_span_refuses_a_span_outside_its_slab`.
- [x] Worker tests: `read_over_16_mib_refuses_before_allocation`,
      `worker_compression_respects_physical_budget`,
      `worker_evicts_compressed_lru_extent`,
      `worker_compression_refuses_from_zero_or_stale_budget`,
      `codec_admission_uses_full_parent_free_floor`,
      `worker_decode_error_returns_miss`,
      `worker_teardown_waits_for_codec_completion`,
      `worker_compression_disable_is_idempotent`,
      `codec_fault_keeps_raw_cache_serving`.
- [x] Integration set `crates/ramshared-block/tests/gpu_cache_compression.rs`
      (17 tests): `worker_compression_roundtrip_is_byte_exact`,
      `worker_raw_fallback_when_encoded_allocation_is_not_smaller`,
      `worker_corrupt_entry_returns_origin_bytes`,
      `worker_partial_update_never_returns_stale_bytes`,
      `worker_compression_disable_is_idempotent`,
      `compression_update_replay_is_idempotent`,
      `worker_compressed_crc_mismatch_refuses_before_decode`,
      `codec_fault_keeps_raw_cache_serving`,
      `codec_fault_does_not_revoke_cache_client`,
      `codec_subdeadline_falls_through_to_raw_or_miss`.
- [x] `compressed_cache_fault_falls_back_to_origin` in
      `crates/ramshared-block/src/isolated_origin.rs`: a codec integrity miss
      serves the SSD origin bytes, never the refused payload; the cache client
      is not revoked (DT-11) and a later real hit is still served.
- [x] CPU-codec control arm tests (7): `cpu_codec_arm_roundtrip_is_byte_exact`,
      `cpu_codec_arm_is_not_a_gpu_cache_codec`,
      `cpu_codec_arm_checksum_refusal_never_invokes_decode`,
      `cpu_codec_arm_capacity_gate_is_strictly_smaller`,
      `cpu_codec_arm_report_carries_the_metric_envelope`,
      `cpu_codec_arm_refuses_oversize_extents`,
      `cpu_codec_arm_max_encoded_len_is_a_bound`.

**Cover (gate 2026-09-30, `--min 80`, packages `ramshared-vram,ramshared-block`):**

| File | Lines | % |
| --- | --- | --- |
| `crates/ramshared-vram/src/codec.rs` | 317/352 | **90.1%** |
| `crates/ramshared-block/src/compressed_cache.rs` | 222/247 | **89.9%** |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | 623/722 | **86.3%** |

Gate: **PASSED**. Tests in the run: 223 passed, 0 failed (159 block lib +
17 `gpu_cache_compression` + 7 `cpu_codec_control_arm` + 6
`gpu_worker_protocol` + 34 vram lib). `cargo clippy --all-targets` on both
crates: 0 errors, 0 warnings.

#### Contract notes (not defects — do not "fix")

1. **DT-10 and mixed ranges.** An extent whose encoded form is not strictly
   smaller is published raw. A read that spans a compressed extent and a raw
   extent is therefore a **miss** for the compressed assembly and falls
   through to the raw path or the origin. This is the intended fail-safe order,
   not a coverage bug.
2. **DT-6 and partial updates.** Overlap invalidation removes the **whole**
   overlapping entry before the replacement is published, so the untouched
   tail of that entry becomes a gap until it is rewritten. A read over the gap
   misses and the origin serves. This is what makes "never returns stale
   bytes" true; it is not a lost-data bug.

### ITEM-3 — GATED: nvCOMP LZ4 backend
- **Entry gates (all must be closed first):**
  - [x] CPU-codec measurement control arm recorded at ITEM-2 exit (NFR-6b) —
        `crates/ramshared-block/tests/cpu_codec_control_arm.rs`, 7 tests green.
  - [ ] Exact nvCOMP release and its pre-decode checksum mechanism resolved (DT-7). No safe pre-decode verification means stop the slice; do not weaken DT-7 to host readback. **Env-bound:** no nvCOMP / `libnvcomp` on this host.
  - [ ] If the CPU arm alone meets the usefulness gate without GPU contention, sunset this path (DT-9) instead of implementing it. **Gated on ITEM-5 paired runs.**
- [ ] `crates/ramshared-cuda/src/nvcomp.rs`: bounded dynamic loader, queried workspace/output bounds, per-item status conversion.
- [ ] Tests: `nvcomp::tests::missing_runtime_returns_unsupported`, `reported_bounds_reject_truncation`, `status_and_lengths_are_checked`.
- [ ] Hardware test (ignored): `nvcomp_lz4_sm75_roundtrip_and_corruption_refusal`.

### ITEM-4 — Telemetry envelope and labels
- [x] Versioned worker-cache telemetry envelope within the existing 4 KiB payload limit. `WorkerTelemetryEnvelope` (schema version 1) carries a separate `budget` field (existing `GpuBudgetTelemetry`, its own schema unchanged) and a `cache` field (`WorkerCacheTelemetry`) so adapter budget and cache occupancy can never be read as each other. `to_bounded_payload()` returns `None` above `MAX_WORKER_TELEMETRY_PAYLOAD_BYTES` (4096) and never truncates; `from_bounded_payload()` refuses empty, oversize, and unknown versions.
- [x] Preserve physical `cached_bytes`/`target_bytes` meanings; logical cache bytes never labelled as RAM. `cached_bytes` and `target_bytes` remain frame-header fields (`aux`/`offset`) and are never re-derived from the envelope. `logical_cached_bytes` is documented as cache occupancy in the worker's logical address space and is rendered under the `VRAM cache` label only — `monitor_labels_logical_cache_separately_from_ram` asserts the rendered line contains no bare `RAM` token.
- [x] Tests: `telemetry_envelope_rejects_unknown_version_or_oversize`, `logical_bytes_never_replace_physical_cached_bytes`, `monitor_labels_logical_cache_separately_from_ram`, `monitor_omits_stale_codec_telemetry`, `selected_provider_without_codec_starts_raw_only`, `selected_codec_provider_revalidates_exact_adapter`. All six are present and green. Two further fail-closed paths are covered by `heartbeat_telemetry_truncation_fails_closed` and `heartbeat_mismatched_correlation_fails_closed`, and the `BestEffortCache` forwarding surface the daemon calls through is exercised explicitly in `logical_bytes_never_replace_physical_cached_bytes`.

Delivery chain (DT-8): worker heartbeat → `IpcCacheClient` (`WorkerTelemetryEnvelope::from_bounded_payload`) → `BestEffortCache::cache_telemetry()` → `AuthoritativeOriginBackend::cache_telemetry()` → wsl2d `OriginCacheStatus.gpu_cache` → `monitor::cache_telemetry_from_value` / `format_cache_telemetry_labels`. Samples are dropped on schema mismatch or staleness (`TELEMETRY_MAX_AGE_MS` = 5000 ms) rather than displayed; the monitor prints `omitted (stale or malformed)` instead of a stale figure. Codec refusals are typed at every site — `codec_integrity_errors` (checksum refusals before decode, DT-7), `codec_decode_errors` (bad status/length/workspace/slab), `codec_timeouts` (`CODEC_SUBDEADLINE` exceedances, DT-3) — and each still increments the aggregate `codec_faults` so existing triage callers are unchanged (DT-11). The refusal reason is bounded at 64 bytes and truncated on construction.

### ITEM-5 — Paired measurements and gates
- [ ] Three paired raw/compressed runs on the same adapter, plus the CPU-codec control arm (re-running the ITEM-2 exit harness), with a predeclared foreground GPU workload.
- [ ] Report the four canonical categories plus Tier 3 origin metrics and `PASS_ZERO_PANIC`, and publish the raw/compressed/origin hit-p95 triple measured over **identical logical read ranges**.
- [ ] Gates: ≥10% net logical capacity; compressed-hit p95 ≤ 1.25× raw-hit p95 on identical logical ranges; zero codec-induced read timeouts; zero missed foreground deadlines; ≤5% foreground p95 regression; GPU codec at least as good as the CPU arm.
- [ ] On gate failure: sunset the codec path (DT-9). Do not leave it disabled in the tree.

## Status
ITEM-1, ITEM-2, and ITEM-4 **implemented** (code, named tests, cover gate).
ITEM-3 **gated** on the nvCOMP runtime and its pre-decode checksum mechanism
(env-bound). ITEM-5 is **not started** and needs real paired GPU runs.
Compression is experimental, `compression_enabled = false` by default, and
cannot be promoted past experimental without the ITEM-5 paired-run gates.

## Validation (numbers)

Host-side validation 2026-09-30 (no GPU, no nvCOMP; the deterministic
`FakeCodec` and the CPU-codec control arm stand in for hardware):

| Metric | Value |
| --- | --- |
| Tests (`ramshared-vram`, `ramshared-block`, `ramshared-wsl2d`, `ramshared-cli`) | **1003 passed / 0 failed** (19 hardware-gated ignored) |
| `cargo clippy --all-targets -p ramshared-vram -p ramshared-block -p ramshared-cli -- -D warnings` | 0 errors, 0 warnings |
| Cover `crates/ramshared-vram/src/codec.rs` | **90.1%** (317/352) |
| Cover `crates/ramshared-vram/src/worker_telemetry.rs` | **86.7%** (52/60) |
| Cover `crates/ramshared-block/src/compressed_cache.rs` | **89.9%** (222/247) |
| Cover `crates/ramshared-block/src/gpu_cache_worker.rs` | **80.1%** (672/839) |
| Cover `crates/ramshared-block/src/ipc_cache_client.rs` | **84.0%** (305/363) |
| Cover `crates/ramshared-cli/src/monitor.rs` | **84.6%** (2171/2566) |
| Cover gate (`--min 80`, `tools/ci/check-rust-slice-coverage.mjs`) | **PASSED** |

Not measured here and still required before any promotion or DONE claim:
ITEM-5 three paired raw/compressed runs on a real adapter, the
raw/compressed/origin hit-p95 triple over identical logical read ranges, the
foreground GPU co-load p95, and the nvCOMP pre-decode checksum mechanism.
No performance number in this file is a hardware result.

## Gaps
- **Reserve-floor contract value drift** in the parent worker SPEC must be
  reconciled before any compression admission is qualified on hardware. The
  *shape* of the floor is fixed here (full `required_free_bytes`, not the
  configured floor alone); the default value (512 MiB env vs
  `max(1536 MiB, 20%)` in the parent PRD mitigation) is still unreconciled
  upstream.
- **nvCOMP pre-decode checksum capability is unresolved** and no `libnvcomp`
  is present on this host. ITEM-3 stays gated; DT-7 must not be weakened to a
  host readback to work around it.
- **ITEM-5 paired measurements** do not exist. The CPU-codec control arm
  record produced at ITEM-2 exit is its baseline input, not its result.
- Multi-vendor GPU (AMD/Intel), multi-adapter, and CoCo (SEV-SNP / TDX /
  Arm CCA) are **honestly unresolved** — never claimed.

## Rollback trigger
One byte/checksum mismatch, one codec-induced read timeout, one budget sample below the parent full free floor (`required_free_bytes(configured_reserve_bytes, runtime_headroom_bytes)`), or one paired run over identical logical ranges where compressed-hit p95 exceeds 1.25× raw-hit p95 disables compressed admission. A failed usefulness gate removes the codec path from the tree.

## Traceability
See SPEC.md traceability table. Every Step 3 item above maps to named SPEC matrix rows.

## Exit criteria
All SPEC validation-checklist items, per-file coverage ≥80% on the listed business-logic files, live before→action→after on the isolated WSL2 worker surface, and `BINARY_MATCH` where `ramsharedd` is involved. Env-bound gaps produce **partial**, never DONE.
