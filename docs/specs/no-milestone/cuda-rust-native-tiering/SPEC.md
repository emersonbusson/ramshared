# SPEC — Lossless compression for the revocable VRAM cache

> SSDV3 Step 2 · PRD: docs/specs/no-milestone/cuda-rust-native-tiering/PRD.md
>
> Step 2.5 changelog (2026-09-30): completed the required tests matrix (4 missing rows);
> added codec-operation sub-deadline and codec-fault isolation (DT-3, DT-11); added the
> compressed-hit self-latency gate and the CPU-codec measurement control arm (DT-10);
> added the Day-0 sunset rule for a failed usefulness gate (DT-9, Rollback); recorded the
> inherited reserve-floor dependency. See AUDIT-2.5.md.
>
> Step 2.5 pass 2 (2026-09-30): declared the numeric self-latency bound (1.25× raw-hit p95);
> added the named codec sub-deadline test; restated DT-11 as worker-side codec-fault
> isolation so it no longer contradicts the closed scope, PRD flow, or the security
> checklist; moved the CPU-codec control arm to ITEM-2 exit to remove the ITEM-3↔ITEM-5
> ordering cycle; limited the DT-3 sub-deadline claim to what it can actually bound.
>
> Step 2.5 pass 3 (2026-09-30): corrected the codec admission formula to consume the
> parent's full free floor (`required_free_bytes(configured, runtime_headroom)`) instead of
> the configured `reserve_floor_bytes` alone (DT-4, NFR-2); defined the CPU-codec control
> arm as a host-side measurement harness outside `GpuCacheCodec`, which cannot host a
> CPU implementation (NFR-6b, DT-10); added `isolated_origin.rs` to MODIFY so its matrix
> row is executable; pinned self-latency comparison to identical logical read ranges.

## Closed scope

### In now

- Optional, lossless compression of cache entries inside the existing isolated GPU worker.
- A provider-specific codec capability; first implementation candidate is NVIDIA nvCOMP LZ4.
- Bounded VRAM slabs and variable-size extents, raw fallback, metadata bounds, integrity checks, and separate logical/physical telemetry.
- Preservation of the SSD-authoritative origin, current write-before-cache ordering, current GPU reserve policy, existing worker isolation, IPC frame bounds, and cache-read deadline.
- Compression is disabled by default. The first implementation is experimental and can only be enabled by test configuration.

### Out now

- Any compressed representation in the SSD origin, swap format, kernel block interface, persistent storage, Windows pagefile, or public ABI.
- CPU codec as a production runtime fallback (the measurement-only CPU-codec control arm in NFR-6b is in scope), lossy encoding, a custom codec kernel, cuTile-rs, cuda-oxide, and universal GPU codec claims.
- Any alteration to the current worker deadline value, headroom formulas, transport/protocol revocation policy, worker teardown policy, or host stress/install procedure. Codec-only fault handling inside the worker is in scope (DT-11).

### Assumed-ready dependencies

- AuthoritativeOriginBackend and BoundedCacheClient in crates/ramshared-block/src/isolated_origin.rs.
- GpuCacheWorker, its existing adapter-bound allocation checks, and its isolated socket loop in crates/ramshared-block/src/gpu_cache_worker.rs.
- VramProvider, VramMemory, and adapter-bound budget snapshots in crates/ramshared-vram/src/lib.rs.
- CUDA and Vulkan provider selection in crates/ramshared-wsl2d/src/main.rs.
- NVIDIA nvCOMP LZ4 runtime availability is not assumed; missing runtime means raw-only.
- Codec and slab admission consume the parent worker's existing reserve contract as a
  single free-floor value: `GpuBudgetSnapshot::required_free_bytes(configured_reserve_bytes,
  runtime_headroom_bytes)`, i.e. `max(configured, ceil(min(total, budget)/5)) +
  runtime_headroom`. The configured `reserve_floor_bytes` alone is **not** the floor. This
  SPEC does not redefine that floor. The parent reserve default is currently unreconciled
  (`wsl2-isolated-gpu-cache-worker` validation checklist) and must be closed before any
  compression admission is qualified on hardware; the shape of the floor is fixed here
  regardless of which default value the parent settles on.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 origin remains authoritative | ITEM-1, ITEM-2, DT-1, DT-6 |
| RF-2 bounded lossless entry | ITEM-1, ITEM-2, DT-4, DT-7 |
| RF-3 physical-size admission and raw bypass | ITEM-2, ITEM-3, DT-4, DT-5 |
| RF-4 invalidation on overlapping writes | ITEM-2, DT-6 |
| RF-5 verify before returning bytes | ITEM-1, ITEM-2, DT-7 |
| RF-6 bounded scratch/slabs/metadata | ITEM-2, ITEM-3, DT-4, DT-5 |
| RF-7 optional provider capability | ITEM-1, ITEM-3, DT-2 |
| RF-8 separated telemetry | ITEM-4, DT-8 |
| RF-9 default off and no public ABI | ITEM-2, ITEM-4, DT-9 |
| NFR-1 exactness | ITEM-1, ITEM-2 |
| NFR-2 host/device resource ceilings | ITEM-2, ITEM-3, DT-4, DT-5 |
| NFR-3 bounded operations | ITEM-2, DT-4 |
| NFR-4 existing read deadline | ITEM-2, ITEM-3, DT-3 |
| NFR-5 measured capacity gate | ITEM-5, DT-10 |
| NFR-5b compressed-hit self-latency gate | ITEM-5, DT-10 |
| NFR-6 comparable measurements | ITEM-5, DT-10 |
| NFR-6b CPU-codec measurement control arm | ITEM-5, DT-10 |
| Codec fault isolation | DT-3, DT-11, ITEM-2 |
| NFR-7 synthetic-only data | ITEM-1, ITEM-5 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Compression is a cache representation, never an origin or swap representation. A successful origin write precedes cache mutation; every cache miss or fault uses the origin. | Keeps compression outside the durable data contract and preserves existing recovery semantics. |
| DT-2 | Add an optional GpuCacheCodec capability tied to the selected provider. The first implementation is dynamically loaded nvCOMP LZ4 for NVIDIA CUDA; unsupported or unqualified providers use the existing raw cache. | Reuses a production lossless codec, allows backend-specific implementations, and avoids a CPU fallback. |
| DT-3 | Preserve the existing cache read deadline, 50 ms by default. Every codec operation carries its own internal sub-deadline strictly inside that budget (encode, pre-decode checksum, and decode each reserve a bounded slice of it); when the remaining budget cannot fit the next codec step, skip compressed handling and fall through to raw or to a miss. Do not retry or extend a possibly in-flight GPU operation after timeout. The sub-deadline bounds **admission and continuation between codec steps**; it cannot preempt a driver call already blocked in the kernel, which remains handled by worker isolation and supervised teardown. | The cache is optional and must not hold the origin path behind an unbounded GPU/driver operation, and a single slow codec step must not be allowed to consume the whole read deadline. Stating the preemption limit keeps the design from implying a control the kernel does not give us. |
| DT-4 | Split incoming cache mutations into exact logical extents no larger than 64 KiB. Keep at most one batch in flight, with at most 4 MiB logical input and at most 4 MiB of additional host codec staging. Cap all GPU codec temporary allocations at min(64 MiB, floor(fresh_admissible_headroom / 100)), where fresh_admissible_headroom = available_bytes.saturating_sub(required_free_bytes(configured_reserve_bytes, runtime_headroom_bytes)) from the latest adapter-bound snapshot, with required_free_bytes being the parent worker's full free floor `max(configured_reserve_bytes, ceil(min(total_bytes, budget_bytes) / 5)) + runtime_headroom_bytes` (the same value that gates parent allocation). Using the configured `reserve_floor_bytes` alone is forbidden: it omits the 20% capacity share and the runtime buffer and would admit codec scratch the parent policy refuses. Include workspace, decoded output, pointer/status arrays, and temporary codec buffers in the cap. Query required workspace per batch and reduce a batch or refuse compression if it does not fit. A cache read over 16 MiB returns a miss before allocating its response buffer; an allowed private response is capped at 16 MiB separately from codec staging. | Bounds transient GPU and host use while allowing batch parallelism. These are engineering ceilings, not performance claims; the origin may serve a larger request. The full parent free floor is required so codec admission cannot overcommit shared host memory relative to the policy that already protects it. |
| DT-5 | Use dynamically allocated 2 MiB backing slabs with a coalescing free-range allocator. Allocate no slab or metadata at startup when the physical target is zero. Otherwise cap host metadata at min(16 MiB, max(64 KiB, physical_target_bytes / 256)); include index and allocator overhead in the cap. Do not compact live extents in the first slice. | Shares compressed outputs without one device allocation per small item, preserves on-demand allocation, and bounds WSL metadata cost. Fragmentation may refuse cache admission. |
| DT-6 | Cache records are non-overlapping logical extents. On update, invalidate every overlapping record before publishing updated bytes. On read, require complete contiguous coverage; assemble and verify the entire response privately before sending it. | Makes partial updates and range reads safe without in-place compressed mutation or exposing mixed/stale bytes. |
| DT-7 | Every compressed entry records logical length, stored length, allocator-rounded allocation length, codec/format id, generation, CRC32 of the original bytes, and CRC32 of the stored compressed payload. Compute the original checksum over the already-present host input. Before decode, compute the stored-payload checksum on the provider while bytes remain in VRAM; a mismatch must refuse the entry without invoking the decoder. Then require per-item decoder status, exact output length, and a matching checksum over the host output before returning bytes. CRC32 detects accidental corruption, not malicious modification. | Prevents known-corrupt bytes from reaching nvCOMP, whose C API documents limited validation for malformed compressed input, and prevents silent wrong output from reaching a reader. The stream is worker-generated; no external compressed stream is accepted. |
| DT-8 | Keep existing cached_bytes and target_bytes as physical accounting. Add a versioned telemetry envelope for logical bytes, slab bytes, workspace, payloads, metadata, bypasses, integrity failures, and timeouts. | Prevents logical cache capacity from being mistaken for WSL or host RAM and avoids overloading the GPU budget contract. |
| DT-9 | Add an internal compression_enabled field to GpuWorkerConfig with default false. When disabled or when codec capability is absent, use the existing raw worker path. When enabled and supported, raw-bypass entries use the bounded slab allocator. Keep the raw path permanently as the unsupported-provider and rollback path; no public command or ABI is added. If the DT-10 usefulness gate fails, the codec path, its staging, and its metadata are removed from the tree; only the raw cache remains. | Preserves Day-0 behavior and makes unsupported hardware safe. Keeping the existing raw path is the documented Day-0 exception: reason is permanent fallback/rollback for providers with no codec; removal date is none; rollback sets compression_enabled=false; evidence is the existing raw worker suite plus the new raw-only refusal tests. Leaving a never-enabled codec in the tree would be an undocumented dual-path, so a failed gate sunsets it (Kahneman #18). |
| DT-10 | Keep compression only when actual allocated bytes are lower than raw allocation. Promote beyond experimental mode only if three paired runs show at least 10% net logical-capacity gain on the declared workload mix, no codec-induced read timeout, no missed foreground GPU deadlines, at most 5% foreground p95 latency regression, and compressed cache-hit p95 at most **1.25× raw cache-hit p95** on the same paired runs. All three latency figures — compressed cache-hit p95, raw cache-hit p95, and origin-read p95 — are measured over **identical logical read ranges** (same offset/length set served by each path), so the comparison is apples-to-apples despite the raw path serving 2 MiB chunks and the compressed path serving 64 KiB extents. Publish the raw/compressed/origin hit-p95 triple so a compressed hit slower than the origin read it replaces is visible. The same paired runs must include a measurement-only CPU-codec control arm over identical extents; the GPU codec is promoted only if it meets the gates at least as well as that arm. The 1.25× bound is a proposed prototype constant: ITEM-5 must publish the triple so the bound can be tightened or rejected with numbers, never loosened to make compression pass. | Requires a measured benefit after slabs, workspace, and fragmentation while bounding shared-GPU interference and while keeping the compressed hit path acceptably close to the raw hit path it replaces. Pinning the comparison to identical logical ranges prevents a shaped-read artifact from flattering either path. The CPU arm is a baseline, never a production fallback, so the choice of GPU compute can be falsified. The bound is numeric so the gate can fail. No universal ratio is inferred. |
| DT-11 | Codec faults are isolated from the raw cache **inside the worker**. A codec sub-deadline expiry, codec integrity failure, or uncertain codec completion disables compressed admission and leaves the worker raw-only or returns a miss; it does not revoke the cache client. The existing client deadline value, transport/protocol revocation, and worker teardown policy are unchanged: a read that still exceeds the 50 ms client deadline, a protocol fault, or worker-process loss revokes the cache client under the existing policy exactly as before. | A codec is an optional accelerator on top of a working raw cache; letting its failures destroy the fallback path inverts the fail-safe order and turns one slow GPU step into a full cache outage. Keeping revocation rules for transport and process failure unchanged preserves the closed scope. |

### Provider codec contract

The optional codec contract is implemented per provider and receives provider-owned memory, bounded host inputs/outputs, and queried scratch. It must report a maximum encoded length and required alignments before allocation. The following proposed shape defines the minimum operations; concrete CUDA/Vulkan types remain private to their provider.

    pub struct VramSpan {
        pub offset: u64,
        pub stored_len: usize,
        pub allocation_len: usize,
    }

    pub struct VramOutputReservation {
        pub offset: u64,
        pub capacity: usize,
        pub allocation_len: usize,
    }

    pub struct CodecAlignments {
        pub compression_input: usize,
        pub compression_output: usize,
        pub decompression_input: usize,
        pub decompression_output: usize,
        pub workspace: usize,
    }

    pub struct CodecChunkResult {
        pub status: CodecStatus,
        pub encoded_len: usize,
    }

    pub trait GpuCacheCodec<M: VramMemory> {
        fn codec_id(&self) -> CodecId;
        fn required_alignments(&self) -> CodecAlignments;
        fn max_encoded_len(&self, logical_len: usize) -> Result<usize, VramError>;
        fn workspace_bytes(
            &self,
            item_count: usize,
            max_logical_len: usize,
        ) -> Result<usize, VramError>;
        fn compress_batch_into(
            &self,
            inputs: &[&[u8]],
            slab: &mut M,
            outputs: &[VramOutputReservation],
            workspace: &mut M,
        ) -> Result<Vec<CodecChunkResult>, VramError>;
        fn checksum_batch(
            &self,
            slab: &M,
            inputs: &[VramSpan],
            workspace: &mut M,
        ) -> Result<Vec<u32>, VramError>;
        fn decompress_batch_from(
            &self,
            slab: &M,
            inputs: &[VramSpan],
            logical_lengths: &[usize],
            outputs: &mut [Vec<u8>],
            workspace: &mut M,
        ) -> Result<Vec<CodecStatus>, VramError>;
    }

VramProvider gains a default cache_codec method returning None. A provider may return its codec only when the optional runtime is loaded and the selected adapter supports it. The worker calls this method only when compression is explicitly enabled; an absent capability means raw-only operation.

    fn cache_codec(&self) -> Option<&dyn GpuCacheCodec<Self::Mem<'_>>> {
        None
    }

The span carries both the exact stored length and its allocator-owned physical length; every offset-plus-length calculation is checked before provider calls. Output reservations carry a writable capacity and allocator-owned length, and the worker accepts only a reported encoded length within that capacity. The codec supplies the compressed-payload checksum before decode using a provider-side checksum operation over the exact stored length while bytes remain in VRAM; the worker checks the uncompressed checksum over the bounded host output after decode. For the NVIDIA candidate, the provider must use nvCOMP's documented GPU CRC32 operation or another documented safe mechanism; it must not read compressed bytes back merely to checksum them. If the provider cannot verify a checksum before decode, it must report no codec capability and remain raw-only. CRC32 detects accidental corruption, not malicious modification. Compressed input is created only by this worker and is never accepted as an externally supplied stream. Metadata and decoder sizes are validated at every read boundary. GPU operations return only after completion/status is observed. If completion is uncertain, the worker retains the associated memory and faults the codec path; it must not free an in-flight span. Decode batches are grouped by backing slab so each span is paired with the correct provider-owned allocation.

### Codec operation lifecycle

The worker uses a serialized lifecycle for each provider operation: ready, in flight, completed, or faulted. A cache-client deadline expiry returns a cache miss to the origin immediately; it is not treated as proof that the GPU operation was cancelled. If provider completion is uncertain, the worker stops accepting cache operations and does not free, recycle, or reuse involved memory. The isolated worker is terminated through its existing supervisor. Compression stays unavailable until that worker process is confirmed gone, a new provider is initialized for the same adapter, and a fresh budget snapshot passes. If process exit or provider recovery cannot be confirmed, the cache remains unavailable. No retry or new worker may race an operation whose ownership is unresolved.

### CPU-codec measurement control arm surface

`GpuCacheCodec<M: VramMemory>` compresses into provider-owned VRAM slabs and workspace (`slab: &mut M`, `workspace: &mut M`). A host-side codec cannot implement that trait and is never asked to. The NFR-6b measurement-only CPU-codec control arm is produced by a **host-side measurement harness outside `GpuCacheCodec`**: it takes the same logical extents the codec would receive, encodes and decodes them in host memory, and reports the same metric envelope and integrity checks (byte-exact round trip, checksum refusal before decode, encoded vs raw allocated bytes, encode/decode wall time, CPU time). The harness is measurement-only: it is never a trait implementation, never selected as a runtime provider, and never a production fallback. Its output is a comparable capacity/latency figure for the same extents, so the GPU-compute choice can be falsified.

## Atomicity and rollback

### Atomicity frontier

- **Origin:** the existing origin write/sync completes before a cache update is sent. Compression never acknowledges origin data.
- **Cache entry:** reserve provisional allocator spans; compress and validate output; calculate final allocated length; then publish the entry and range-index state in one worker-thread operation. On failure, return provisional spans and do not expose the record.
- **Update:** because the origin has already accepted the new bytes, evict overlapping cache records first. If replacement compression/raw storage fails, the range remains a cache miss and the origin serves it.
- **Read:** collect all needed extents into a private response buffer. No bytes are sent until every extent has passed bounds, codec status, length, and checksum checks.
- **Restart/worker loss:** all records and slabs are volatile. Worker loss drops the cache only; there is no on-disk format or migration.

### Rollback

- **Userspace/daemon:** set compression mode off, release codec workspace and compressed slabs after observed completion, and retain the existing raw worker path. Any uncertain codec completion uses existing worker isolation/reap behavior and marks the worker unavailable; do not reuse possibly in-flight memory.
- **Kernel/module:** N/A — no kernel code or ABI is changed.
- **Host/persistent:** N/A — no host configuration, swap mapping, or origin format changes. If a later release has enabled this cache, disable/restart through the existing supervised RamShared lifecycle; never bypass its swapoff-first contract.
- **Numeric rollback trigger:** one byte/checksum mismatch, one codec-induced read timeout, one fresh budget observation below the parent full free floor (`required_free_bytes(configured_reserve_bytes, runtime_headroom_bytes)`), or one paired run over identical logical ranges where compressed cache-hit p95 exceeds 1.25× raw cache-hit p95 disables compressed admission. Three runs below 10% net capacity gain, or a GPU codec that fails to meet the gates at least as well as the CPU-codec control arm, remove the codec path from the tree (Day-0 sunset, Kahneman #18) rather than leaving it disabled in place.

## Kahneman map (critical only)

| ITEM / stage | Discipline | Question | Minimum executable evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-1 / decode and publication | #17 — idempotency of replayable effects | Does replaying the same encoded entry or update twice leave one current exact cache representation, and does a bad compressed checksum refuse before decode? | gpu_cache_compression::compression_update_replay_is_idempotent; gpu_cache_compression::worker_compressed_crc_mismatch_refuses_before_decode; compressed round-trip tests | Any stale generation, duplicate visible extent, decoder call on a checksum mismatch, or byte mismatch. |
| ITEM-2 / admission and reclaim | #16 — fail-safe default from exhaustion | At zero or stale GPU headroom, can any scratch, slab, or metadata allocation cross the existing reserve? | gpu_cache_worker::tests::compression_refuses_from_zero_or_stale_budget and scratch exhaustion test | Any allocation after refusal or free headroom below the existing floor. |
| ITEM-2 / partial updates | #13 — refusal plus legitimate pass | Are overlaps invalidated while an unaffected exact cache read still succeeds? | gpu_cache_worker::tests::partial_update_invalidates_overlapping_compressed_entry plus legitimate non-overlap hit | Stale/mixed bytes or a false hit across a gap. |
| ITEM-2 / cache read size | #13 — refusal plus legitimate pass | Is an over-limit request refused before response allocation while a normal request still hits? | gpu_cache_worker::tests::read_over_16_mib_refuses_before_allocation and a legitimate bounded read | Any oversized allocation or false refusal of a bounded request. |
| ITEM-3 / worker timeout | #16 — fail-safe default | Does a stalled decoder fall back to the origin without extending the cache read deadline? | ipc_cache_client::tests::codec_timeout_falls_back_to_origin | Read exceeds the configured deadline or the origin is blocked. |
| ITEM-2 / codec fault isolation | #16 — fail-safe default | Does a codec sub-deadline expiry, timeout, or integrity failure leave the raw cache serving instead of revoking the cache client? | gpu_cache_compression::tests::codec_fault_keeps_raw_cache_serving; gpu_cache_compression::tests::codec_fault_does_not_revoke_cache_client; gpu_cache_compression::tests::codec_subdeadline_falls_through_to_raw_or_miss | Any codec fault that disables the raw path, any transport revoke triggered by a codec-only failure, or a sub-deadline that fails to fall through to raw/miss. |
| ITEM-5 / usefulness and co-load | #9 — number, not adjective | Does the complete compressed cache path add useful logical capacity without harming a representative foreground GPU workload, and is its hit path acceptably close to the raw hit path it replaces? | Three paired raw/compressed runs on the same adapter, including a predeclared foreground GPU workload, complete metric envelope, foreground deadline/p95 measurements, and the raw/compressed/origin hit-p95 triple measured over identical logical read ranges | Less than 10% net gain, any timeout/integrity error, a missed foreground deadline, more than 5% foreground p95 regression, or compressed-hit p95 above 1.25× raw-hit p95 on identical logical ranges. |
| ITEM-5 / codec resource choice | #11 — anti-halo; #3 — number | Does the GPU codec meet the gates at least as well as a measurement-only CPU-codec control arm over the same extents, or is host-side compression the better resource? | Three paired runs including the CPU-codec control arm produced by the host-side harness outside `GpuCacheCodec`, same integrity checks and metric envelope, never used as a production fallback | GPU codec worse than the CPU arm on any promotion gate, or the CPU arm alone already meeting the usefulness gate without GPU contention. |

## Security checklist (pre-impl)

- [x] Privilege: N/A — no new privilege, public device node, or user-facing capability.
- [x] User/host copy: all IPC frame, requested read, input, output, and decoder lengths are checked and bounded; only one codec batch may be staged at once (4 MiB); the verified private response is capped at 16 MiB.
- [x] Flags/IOCTL codes: N/A — no new ioctl or public flags.
- [x] Information flow: telemetry contains lengths/counters only; never payloads, kernel pointers, host addresses, or user RAM samples.
- [x] IRQ/atomic or IRQL: N/A — userspace worker only. The provider must not block the origin-serving thread on codec completion.
- [x] Lifetime: provider retains slabs, scratch, contexts, and input buffers until GPU completion is observed. A timeout cannot free in-flight memory.
- [x] Hot-unplug/device-gone: adapter loss faults compression, invalidates compressed entries, and produces a cache miss or raw-only worker state.
- [x] Host safety: no unsupervised live WSL2 pressure, swap stress, install, or host activation in this planning task.
- [x] Shared-hardware cushion: reuse the existing adapter-bound budget, reserve, runtime-free buffer, and per-allocation freshness checks; include scratch and slab allocations in the same accounting.
- [x] Bounded DMA/foreign calls: the existing client deadline value and transport/protocol revocation remain unchanged; no retry of uncertain in-flight work. A codec sub-deadline expiry, codec integrity failure, or uncertain codec completion leaves the worker raw-only and does not revoke the client (DT-11); a read that still exceeds the client deadline, a protocol fault, or worker-process loss revokes the client through its current supervisor as before. The client falls back to origin independently of codec cancellation; memory is never recycled until provider completion is observed or the isolated worker exits.
- [x] Cooperative spillover: cache miss, codec refusal, corruption, and allocation failure all use the authoritative SSD origin.
- [x] Replayable ops: overlapping update, eviction, and disable are idempotent; apply an update twice and observe one current range.

## Files to CREATE / MODIFY / DELETE

### CREATE

**crates/ramshared-vram/src/codec.rs**
- Purpose: Define optional codec identity, bounded spans, result/status types, and provider codec contract.
- RF / DT: RF-2, RF-6, RF-7; DT-2, DT-4, DT-7.
- Types / functions: CodecId, CodecStatus, VramSpan, VramOutputReservation, CodecAlignments, CodecChunkResult, GpuCacheCodec<M: VramMemory>.
- Reference pattern: adapter-bound VramProvider contract in crates/ramshared-vram/src/lib.rs.
- Required tests: codec::tests::codec_bounds_reject_overflow; codec::tests::unsupported_provider_is_raw_only.
- Cover target: at least 80% on business logic.

**crates/ramshared-block/src/compressed_cache.rs**
- Purpose: Bounded extent index, slab/free-range allocation, metadata, overlap invalidation, admission, and raw/compressed publication.
- RF / DT: RF-2 through RF-6; DT-4 through DT-7.
- Types / functions: CacheEntry, CacheRepresentation, VramSlab, VramSpanAllocator, split_extent, invalidate_overlaps, read_coverage.
- Reference pattern: existing LRU and valid-range logic in crates/ramshared-block/src/gpu_cache_worker.rs.
- Required tests: compressed_cache::tests::extent_split_respects_maximum; compressed_cache::tests::allocator_coalesces_and_refuses_fragmented_request; compressed_cache::tests::overlap_invalidation_removes_only_affected_entries; compressed_cache::tests::metadata_budget_caps_entry_count; compressed_cache::tests::zero_physical_target_allocates_no_metadata.
- Cover target: at least 80% on business logic.

**crates/ramshared-block/tests/gpu_cache_compression.rs**
- Purpose: Exercise the worker through its public cache behavior using a deterministic fake codec/provider.
- RF / DT: RF-1 through RF-9; DT-1, DT-6 through DT-9.
- Required tests: worker_compression_roundtrip_is_byte_exact; worker_raw_fallback_when_encoded_allocation_is_not_smaller; worker_corrupt_entry_returns_origin_bytes; worker_partial_update_never_returns_stale_bytes; worker_compression_disable_is_idempotent; compression_update_replay_is_idempotent.
- Required test: worker_compressed_crc_mismatch_refuses_before_decode; assert the fake decoder invocation count remains zero and the exact origin bytes are returned.
- Required tests: codec_fault_keeps_raw_cache_serving; codec_fault_does_not_revoke_cache_client; codec_subdeadline_falls_through_to_raw_or_miss.
- Cover target: N/A — integration tests; production business logic is covered per source file.

**crates/ramshared-cuda/src/nvcomp.rs**
- Purpose: Optional dynamically loaded nvCOMP LZ4 adapter; no change to the existing raw CUDA path when library/capability is unavailable.
- RF / DT: RF-2, RF-5, RF-7; DT-2, DT-7.
- Types / functions: NvcompLz4Codec, bounded library loader, queried workspace/output bounds, per-item status conversion.
- Reference pattern: existing dynamic CUDA Driver API loader in crates/ramshared-cuda/src/lib.rs.
- Required tests: nvcomp::tests::missing_runtime_returns_unsupported; nvcomp::tests::reported_bounds_reject_truncation; nvcomp::tests::status_and_lengths_are_checked.
- Cover target: at least 80% on non-hardware business logic.

**crates/ramshared-cuda/tests/nvcomp_cache_codec.rs**
- Purpose: Optional exact-adapter integration test for nvCOMP LZ4; ignored unless the runtime, supported adapter, and fresh budget are present.
- RF / DT: RF-2, RF-5, RF-7; DT-2, DT-7.
- Required tests: nvcomp_lz4_sm75_roundtrip_and_corruption_refusal.
- Cover target: N/A — hardware integration.

**crates/ramshared-block/tests/cpu_codec_control_arm.rs**
- Purpose: Host-side measurement harness for the NFR-6b CPU-codec control arm. Runs outside `GpuCacheCodec` (which requires provider-owned `VramMemory` slabs and cannot host a CPU implementation). Encodes and decodes the same logical extents the GPU codec would receive, entirely in host memory, and reports the same metric envelope and integrity checks: byte-exact round trip, checksum refusal before decode, encoded vs raw allocated bytes, encode/decode wall time, and CPU time. Measurement-only: never a trait implementation, never a runtime provider, never a production fallback. Its numbers are the ITEM-2 exit record and the ITEM-5 comparison baseline.
- RF / DT: NFR-6b; DT-9, DT-10.
- Required tests: cpu_codec_arm_roundtrip_is_byte_exact.
- Cover target: N/A — measurement harness; the harness's own lossless round trip is the gate before its numbers are trusted.

### MODIFY

**crates/ramshared-vram/src/lib.rs**
- What/how/why: export the optional codec contract and default cache_codec method without adding compression methods to raw VramMemory. Preserve existing providers that do not implement the capability. Required refusal and budget tests remain.
- RF / DT: RF-6, RF-7; DT-2, DT-4.
- Required tests: codec capability absent on raw-only provider; fresh adapter budget still gates allocation.
- Cover: existing business logic stays at or above 80%.

**crates/ramshared-block/src/gpu_cache_worker.rs**
- What/how/why: add compression_enabled=false to GpuWorkerConfig by default; retain the existing raw behavior when compression is disabled or unsupported; optionally use the codec and bounded extent allocator; keep target/cached_bytes physical; invalidate overlap before update publication; reject cache reads above 16 MiB before allocating; decode only into private response buffers. Codec/slab admission reads the parent's full free floor via `required_free_bytes(configured_reserve_bytes, runtime_headroom_bytes)`, never the configured `reserve_floor_bytes` alone.
- RF / DT: RF-1 through RF-9; DT-1, DT-3 through DT-9.
- Required tests: worker_compression_respects_physical_budget; worker_decode_error_returns_miss; worker_compression_refuses_from_zero_or_stale_budget; worker_evicts_compressed_lru_extent; worker_teardown_waits_for_codec_completion; read_over_16_mib_refuses_before_allocation; codec_admission_uses_full_parent_free_floor.
- Cover target: at least 80%.

**crates/ramshared-block/src/isolated_origin.rs**
- What/how/why: retain the origin-authority fallback path so a codec fault or decode failure serves origin bytes rather than stalling or fabricating data. No compression format or origin-side representation is added here.
- RF / DT: RF-1, RF-2; DT-1, DT-6, DT-11.
- Required tests: compressed_cache_fault_falls_back_to_origin.
- Cover target: existing source coverage gate (this file is modified only to keep the named test real; no new business logic).

**crates/ramshared-block/src/ipc_cache_client.rs**
- What/how/why: parse a bounded, versioned worker-cache telemetry envelope while retaining the existing budget validation, physical cached_bytes field, and timeout behavior. Return a cache miss without sending a read request above 16 MiB.
- RF / DT: RF-8, RF-9; DT-3, DT-8, DT-9.
- Required tests: telemetry_envelope_rejects_unknown_version_or_oversize; codec_timeout_falls_back_to_origin; oversized_cache_read_is_miss_before_frame_send; logical_bytes_never_replace_physical_cached_bytes.
- Cover target: at least 80%.

**crates/ramshared-wsl2d/src/main.rs**
- What/how/why: pass an optional codec only for a provider that explicitly advertises it; keep missing nvCOMP and unsupported adapters in raw-only mode. Continue exact adapter identity and fresh budget revalidation.
- RF / DT: RF-6 through RF-9; DT-2, DT-5, DT-8, DT-9.
- Required tests: selected_provider_without_codec_starts_raw_only; selected_codec_provider_revalidates_exact_adapter.
- Cover target: at least 80% on extracted business logic; do not widen main.rs coverage by unrelated lines.

**crates/ramshared-cli/src/monitor.rs**
- What/how/why: display logical cache bytes, physical VRAM slab bytes, workspace, and codec status with explicit VRAM/cache labels. Do not merge these counters with guest/host RAM.
- RF / DT: RF-8; DT-8.
- Required tests: monitor_labels_logical_cache_separately_from_ram; monitor_omits_stale_codec_telemetry.
- Cover target: at least 80% on touched business logic.

**docs/architecture/CUDA-RUST-ACCELERATION-BLUEPRINT.md; docs/specs/README.md; docs/reliability/DEGRADATION-MATRIX.md**
- What/how/why: reflect cache-only authority, optional codec, corruption/timeout degradation, and backend qualification before merge.
- RF / DT: RF-1, RF-7, RF-8; DT-1, DT-2, DT-7 through DT-10.
- Required tests: docs-check and generated-index check.
- Cover target: N/A — documentation.

### DELETE

None. The raw cache and existing uncompressed CUDA/Vulkan providers remain supported fallbacks.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| codec capability/state and bounded refusal reason | worker telemetry envelope and status JSON | enum/string, no payload |
| logical_cached_bytes | worker telemetry and status | bytes |
| physical_cache_slab_bytes | worker telemetry and status | bytes |
| codec_workspace_bytes | worker telemetry and status | bytes |
| compressed_payload_bytes/raw_payload_bytes | worker telemetry and status | bytes |
| metadata_bytes/raw_bypass_bytes | worker telemetry and status | bytes |
| codec_integrity_errors/decode_errors/timeouts | worker telemetry and status | counters |
| adapter identity, budget, available bytes, sample time | existing GPU budget telemetry | existing schema; do not conflate with cache occupancy |

The telemetry envelope is versioned, no larger than the existing 4 KiB GPU-budget payload limit, fresh at each worker heartbeat, and omitted when malformed or stale. No input/output bytes, file contents, addresses, or private workload labels are recorded.

## Living docs

| Document | Action |
| --- | --- |
| docs/architecture/CUDA-RUST-ACCELERATION-BLUEPRINT.md | Alter in this Step 2.5 update. |
| docs/specs/README.md and generated docs/INDEX.md | Alter in this Step 2.5 update. |
| docs/reliability/DEGRADATION-MATRIX.md | Update before implementation merge; this planning-only task does not alter the already-dirty matrix. |
| validation.md | Append only after exact-adapter worker validation. |
| docs/BENCHMARKS.md and docs/benchmarks/results.jsonl | Update only after qualified measurements. |
| Existing IMPL.md | Reconcile its prior swap-page task list with this SPEC before Step 3. Do not infer implementation from this planning update. |

## Implementation order

- **ITEM-1:** Define codec contract, span/result bounds, fake codec, exact-byte/checksum tests, and refusal behavior. No CUDA runtime call yet.
- **ITEM-2:** Implement extent splitting, bounded metadata, slab allocator, publication/invalidation ordering, raw bypass, private full-range reads, and the codec sub-deadline/fault-isolation behavior (DT-3, DT-11). The current raw path remains unchanged when compression is disabled or unsupported; the bounded allocator is used only in explicit compressed mode. Compression remains off by default. **ITEM-2 exit also records the measurement-only CPU-codec control arm** (NFR-6b) over identical extents with the same integrity checks — that record is the entry gate for ITEM-3, and ITEM-5 re-runs it inside the full promotion campaign. This is a measurement task, not a production CPU fallback.
- **ITEM-3 (gated on the ITEM-2 exit CPU-codec control arm):** Implement optional NVIDIA nvCOMP LZ4 loading and bounded batch encode/decode. Prove the exact adapter/toolkit/driver combination; keep Vulkan and other unsupported providers raw-only. If the CPU arm already meets the usefulness gate without GPU contention, do not start this item — sunset the codec path (DT-9).
- **ITEM-4:** Add the versioned telemetry envelope and explicit UI labels; preserve physical meanings in existing status fields.
- **ITEM-5:** Run fixed synthetic-data comparisons and exact isolated-worker E2E with a predeclared foreground GPU workload **and a measurement-only CPU-codec control arm over identical extents** (re-running the ITEM-2 exit arm); publish the raw/compressed/origin hit-p95 triple. Decide whether the measured result passes the 10% usefulness gate, the 1.25× self-latency bound, the foreground deadline/5% p95-regression gate, and whether the GPU codec meets those gates at least as well as the CPU arm. If the CPU arm alone meets the usefulness gate without GPU contention, sunset the codec path (DT-9) instead of keeping it. Production enablement remains a separate decision.

## Required tests matrix

All names marked “to add” are planned tests, not tests already present.

| Production path | Test (file :: name) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| Codec bounds | crates/ramshared-vram/src/codec.rs :: codec::tests::codec_bounds_reject_overflow | unit | #13 | at least 80% |
| Raw-only refusal | crates/ramshared-vram/src/codec.rs :: codec::tests::unsupported_provider_is_raw_only | unit | #13 | at least 80% |
| Extent splitting | crates/ramshared-block/src/compressed_cache.rs :: compressed_cache::tests::extent_split_respects_maximum | unit | #9 | at least 80% |
| Slab allocator | crates/ramshared-block/src/compressed_cache.rs :: compressed_cache::tests::allocator_coalesces_and_refuses_fragmented_request | unit | #16 | at least 80% |
| Overlap invalidation | crates/ramshared-block/src/compressed_cache.rs :: compressed_cache::tests::overlap_invalidation_removes_only_affected_entries | unit | #13 | at least 80% |
| Metadata ceiling | crates/ramshared-block/src/compressed_cache.rs :: compressed_cache::tests::metadata_budget_caps_entry_count | unit | #16 | at least 80% |
| Zero-target metadata | crates/ramshared-block/src/compressed_cache.rs :: compressed_cache::tests::zero_physical_target_allocates_no_metadata | unit | #16 | at least 80% |
| Lossless cache read | crates/ramshared-block/tests/gpu_cache_compression.rs :: worker_compression_roundtrip_is_byte_exact | integration | #17 | N/A — integration |
| No physical gain | crates/ramshared-block/tests/gpu_cache_compression.rs :: worker_raw_fallback_when_encoded_allocation_is_not_smaller | integration | #9 | N/A — integration |
| Corrupt entry | crates/ramshared-block/tests/gpu_cache_compression.rs :: worker_corrupt_entry_returns_origin_bytes | integration | #13/#16 | N/A — integration |
| Compressed CRC refusal | crates/ramshared-block/tests/gpu_cache_compression.rs :: worker_compressed_crc_mismatch_refuses_before_decode | integration | #13/#16 | N/A — integration |
| Partial write | crates/ramshared-block/tests/gpu_cache_compression.rs :: worker_partial_update_never_returns_stale_bytes | integration | #13 | N/A — integration |
| Repeated disable | crates/ramshared-block/tests/gpu_cache_compression.rs :: worker_compression_disable_is_idempotent | integration | #17 | N/A — integration |
| Update replay | crates/ramshared-block/tests/gpu_cache_compression.rs :: compression_update_replay_is_idempotent | integration | #17 | N/A — integration |
| Oversized cache read | crates/ramshared-block/src/gpu_cache_worker.rs :: read_over_16_mib_refuses_before_allocation | unit | #13 | at least 80% |
| Physical budget | crates/ramshared-block/src/gpu_cache_worker.rs :: worker_compression_respects_physical_budget | unit | #16 | at least 80% |
| Compressed LRU eviction | crates/ramshared-block/src/gpu_cache_worker.rs :: worker_evicts_compressed_lru_extent | unit | #16 | at least 80% |
| Zero/stale budget | crates/ramshared-block/src/gpu_cache_worker.rs :: worker_compression_refuses_from_zero_or_stale_budget | unit | #16 | at least 80% |
| Full parent free floor | crates/ramshared-block/src/gpu_cache_worker.rs :: codec_admission_uses_full_parent_free_floor | unit | #16 | at least 80% |
| Decode error | crates/ramshared-block/src/gpu_cache_worker.rs :: worker_decode_error_returns_miss | unit | #16 | at least 80% |
| In-flight cleanup | crates/ramshared-block/src/gpu_cache_worker.rs :: worker_teardown_waits_for_codec_completion | unit + worker drill | #17 | at least 80% |
| Codec fault keeps raw cache | crates/ramshared-block/tests/gpu_cache_compression.rs :: codec_fault_keeps_raw_cache_serving | integration | #16 | N/A — integration |
| Codec fault does not revoke client | crates/ramshared-block/tests/gpu_cache_compression.rs :: codec_fault_does_not_revoke_cache_client | integration | #16 | N/A — integration |
| Codec sub-deadline fall-through | crates/ramshared-block/tests/gpu_cache_compression.rs :: codec_subdeadline_falls_through_to_raw_or_miss | integration | #16 | N/A — integration |
| IPC timeout | crates/ramshared-block/src/ipc_cache_client.rs :: codec_timeout_falls_back_to_origin | unit | #16 | at least 80% |
| Oversized client read | crates/ramshared-block/src/ipc_cache_client.rs :: oversized_cache_read_is_miss_before_frame_send | unit | #13 | at least 80% |
| Telemetry bounds | crates/ramshared-block/src/ipc_cache_client.rs :: telemetry_envelope_rejects_unknown_version_or_oversize | unit | #13 | at least 80% |
| Telemetry semantics | crates/ramshared-block/src/ipc_cache_client.rs :: logical_bytes_never_replace_physical_cached_bytes | unit | #9 | at least 80% |
| Missing nvCOMP | crates/ramshared-cuda/src/nvcomp.rs :: nvcomp::tests::missing_runtime_returns_unsupported | unit | #13 | at least 80% |
| nvCOMP output bounds | crates/ramshared-cuda/src/nvcomp.rs :: nvcomp::tests::reported_bounds_reject_truncation | unit | #13 | at least 80% |
| nvCOMP status | crates/ramshared-cuda/src/nvcomp.rs :: nvcomp::tests::status_and_lengths_are_checked | unit | #16 | at least 80% |
| Exact NVIDIA hardware | crates/ramshared-cuda/tests/nvcomp_cache_codec.rs :: nvcomp_lz4_sm75_roundtrip_and_corruption_refusal | ignored hardware | #17 | hardware evidence |
| Raw provider integration | crates/ramshared-wsl2d/src/main.rs :: selected_provider_without_codec_starts_raw_only | integration | #13 | at least 80% |
| Codec adapter revalidation | crates/ramshared-wsl2d/src/main.rs :: selected_codec_provider_revalidates_exact_adapter | integration | #13 | at least 80% |
| UI labels | crates/ramshared-cli/src/monitor.rs :: monitor_labels_logical_cache_separately_from_ram | unit | #9 | at least 80% |
| Stale codec telemetry | crates/ramshared-cli/src/monitor.rs :: monitor_omits_stale_codec_telemetry | unit | #13 | at least 80% |
| Origin authority | crates/ramshared-block/src/isolated_origin.rs :: compressed_cache_fault_falls_back_to_origin | unit | #16 | existing source coverage gate |
| CPU-codec control arm | crates/ramshared-block/tests/cpu_codec_control_arm.rs :: cpu_codec_arm_roundtrip_is_byte_exact | measurement harness | #9 | N/A — harness |

## Coverage commands

Each business-logic source file declared above is gated by its own exact
invocation. The command text below is the contract consumed by
`docs/governance/rust-slice-coverage.json`; do not reformat it.

**crates/ramshared-block/src/compressed_cache.rs**

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block --files crates/ramshared-block/src/compressed_cache.rs --min 80 --report-json tmp/compressed-cache-cov.json
```

## Validation checklist

- [ ] cargo fmt --all -- --check
- [ ] cargo clippy -p ramshared-vram -p ramshared-block -p ramshared-cuda -p ramshared-wsl2d -p ramshared-cli --all-targets -- -D warnings
- [ ] cargo test -p ramshared-vram -p ramshared-block -p ramshared-cuda -p ramshared-wsl2d -p ramshared-cli
- [x] Coverage for `crates/ramshared-block/src/compressed_cache.rs`
  (2026-09-30: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block --files crates/ramshared-block/src/compressed_cache.rs --min 80 --report-json tmp/compressed-cache-cov.json`
  — 90.0% lines (224/249), gate PASSED. All five named unit tests in
  `compressed_cache::tests` are present and green: `extent_split_respects_maximum`,
  `allocator_coalesces_and_refuses_fragmented_request`,
  `overlap_invalidation_removes_only_affected_entries`,
  `metadata_budget_caps_entry_count`,
  `zero_physical_target_allocates_no_metadata`.)
- [ ] Coverage for `crates/ramshared-vram/src/codec.rs` — not yet gated. The
  file is declared above with "Cover target: at least 80% on business logic",
  but no `docs/governance/rust-slice-coverage.json` entry owns it and no
  measured invocation is recorded here. Do not claim this row until the exact
  command is bound in this SPEC, registered in the map, and measured on a tree
  without uncommitted work in that file.
- [ ] Coverage for `crates/ramshared-cuda/src/nvcomp.rs` — N/A yet. The file is
  not created; ITEM-3 is gated on the exact nvCOMP pre-decode checksum
  mechanism (DT-7). Its cover target is not applicable before the file exists.
- [ ] Every test matrix name exists and passes; hardware tests stay ignored unless exact prerequisites and fresh budget pass.
- [ ] Exact NVIDIA test checks sm75+, nvCOMP runtime/toolkit/driver versions, adapter identity, codec statuses, corrupt-entry refusal, and exact bytes.
- [ ] Live userspace path proves before/action/after on an isolated non-pressure canary origin; no forced cascade, kernel-module, WDK, or swap-stress test.
- [ ] If ramsharedd is exercised, verify the running executable matches the tested binary. Do not claim a live product result from unit tests.
- [ ] Before a performance claim, publish the four-category table plus Tier 3 origin metrics and PASS_ZERO_PANIC from a qualified lab campaign; include a paired foreground GPU workload with zero missed deadlines and at most 5% p95 regression.
- [ ] Compressed cache-hit p95 is at most 1.25× raw cache-hit p95 on the same paired runs over identical logical read ranges, and the raw/compressed/origin hit-p95 triple is published; failure keeps the feature disabled (DT-10).
- [ ] Codec and slab admission refuses when only the configured `reserve_floor_bytes` is satisfied but the parent full free floor (`required_free_bytes(configured, runtime_headroom)`) is not; `codec_admission_uses_full_parent_free_floor` passes.
- [ ] A measurement-only CPU-codec control arm is recorded through the host-side harness outside `GpuCacheCodec` over identical extents; the GPU codec meets every promotion gate at least as well as that arm. If the CPU arm alone meets the usefulness gate without GPU contention, the codec path is sunset (DT-9) rather than kept disabled.
- [ ] ITEM-3 begins only after the CPU-codec control arm is recorded and the exact nvCOMP pre-decode checksum mechanism is resolved (DT-7). No safe pre-decode verification means stop the slice; do not weaken DT-7 to host readback.
- [ ] Until hardware and live-worker gates pass, record the result as partial/proposal-only.
