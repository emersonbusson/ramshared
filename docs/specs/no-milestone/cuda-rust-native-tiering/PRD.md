---
slug: cuda-rust-native-tiering
title: Lossless compression for the revocable VRAM cache
milestone: —
issues: []
---

# PRD — Lossless Compression for the Revocable VRAM Cache

> Scope correction: this proposal replaces the earlier idea of changing swap-page storage into a variable-sized compressed format. It stores optional compressed representations only inside RamShared's disposable VRAM cache. This is a proposal, not an implemented feature.

## 1. Summary

RamShared keeps clean copies of authoritative SSD-origin data in VRAM. The isolated GPU cache worker currently stores raw fixed-size allocations. This proposal studies whether independently compressed, lossless cache entries can retain more logical data within the same safe physical VRAM budget.

The SSD origin remains the only authority. A compressed entry is disposable: a cache miss, checksum failure, GPU error, timeout, or worker exit returns the read to the origin. Writes continue to reach the origin before the cache is updated. Compression is opt-in and disabled by default until the exact backend passes correctness, latency, budget, and hardware gates.

The initial NVIDIA candidate is nvCOMP LZ4 through a provider-specific GPU codec capability. Unsupported providers remain raw-only. The design does not promise a fixed ratio or that 2 GiB of VRAM will hold 4 GiB of data.

## 2. Technical context

Facts confirmed in the codebase:

- The worker at crates/ramshared-block/src/gpu_cache_worker.rs allocates raw VRAM chunks, tracks valid ranges, updates/promotes data, and evicts the least recently used chunk. The default allocation chunk is 2 MiB.
- crates/ramshared-vram/src/lib.rs exposes VramProvider and VramMemory. The current memory contract provides allocation, zeroing, and byte reads/writes; it has no compression or GPU-compute interface.
- crates/ramshared-block/src/isolated_origin.rs implements the authoritative-origin boundary. A write reaches the origin before a cache mutation. A failed cache read falls through to the origin, and a failed cache operation can revoke the cache.
- crates/ramshared-block/src/ipc_cache_client.rs uses bounded worker I/O; the default cache-read deadline is 50 ms. The worker runs in the isolated WSL2 daemon process.
- crates/ramshared-wsl2d/src/main.rs selects an adapter using fresh adapter-bound GPU budget data and can run CUDA or Vulkan providers. This is an allocation policy, not proof that either provider can run a compression codec.
- No GPU compression implementation or compressed cache-entry format exists in the current code.

Facts confirmed in project and primary vendor documentation:

- The repository's CUDA wrapper is an existing uncompressed path. docs/architecture/CUDA-RUST-ACCELERATION-BLUEPRINT.md records that cuTile-rs and cuda-oxide are not integrated RamShared codecs.
- NVIDIA describes nvCOMP as a GPU lossless compression library, including LZ4. Its batched API requires the application to split chunks, manage compressed and uncompressed sizes, allocate output buffers, and provide GPU temporary workspace. The guide recommends similarly sized chunks for load balancing.
- The current nvCOMP installation page lists Volta sm70 or newer, CUDA Toolkit 12.0 or newer, and minimum driver versions. The RTX 2060 is sm75, so it meets the documented architecture floor; that does not qualify the local WSL runtime, package, driver, or end-to-end performance.
- NVIDIA warns that malformed compressed input receives limited validation and may produce undefined behavior or out-of-bounds errors. The design validates internal metadata and the stored-payload checksum before decoding, uses per-item codec statuses, bounds output, and checks decoded bytes before returning them.
- cuTile-rs describes itself as a Rust tiled-kernel programming DSL, not a ready-made general-purpose lossless memory codec.

Inference and open empirical questions:

- Some runtime-memory contents will compress; random, encrypted, and already-compressed data may not. The effective gain after allocator rounding, metadata, workspace, and fragmentation is unknown.
- GPU codec execution may reduce CPU work, but the CPU still manages entries, buffers, queues, and transfers. No CPU-free claim is made.
- Whether compression improves a RamShared workload depends on the complete cache-hit path and must be measured against the existing raw cache.

Primary references:

- [nvCOMP overview](https://docs.nvidia.com/cuda/nvcomp/)
- [nvCOMP installation requirements](https://docs.nvidia.com/cuda/nvcomp/installation.html)
- [nvCOMP batched C API guide](https://docs.nvidia.com/cuda/nvcomp/samples/lowlevel_c_quickstart.html)
- [nvCOMP C API reference](https://docs.nvidia.com/cuda/nvcomp/c_api.html)
- [cuTile-rs project](https://github.com/NVlabs/cutile-rs)

## 3. Recommended option

Add an optional, lossless representation to the existing isolated GPU cache worker. Keep the SSD-authoritative origin and its write/read ordering unchanged. The worker may store a cache extent raw or compressed; it publishes a compressed entry only after codec completion, metadata validation, and integrity checks.

Use an optional provider-specific GPU codec interface. The first NVIDIA implementation candidate is nvCOMP LZ4. Load it only when its runtime library and adapter are supported. CUDA without a qualified codec and Vulkan remain raw-only. Do not silently run a CPU compressor when the GPU codec is absent or fails.

Compressed output must occupy less physical VRAM than the raw representation after allocator alignment. Store output in dynamically allocated, bounded VRAM slabs with suballocated extents so small compressed entries can share a slab. Count the entire allocated slab and retained workspace against the physical cache budget. Fixed full-size allocations per compressed item are rejected because they retain the raw physical footprint.

### Discarded alternatives

- Compressing the swap/origin format is rejected because it changes the authoritative storage and recovery contract without helping a disposable VRAM cache.
- Using cuTile-rs or a handwritten kernel as the first codec is rejected because a kernel authoring tool is not a production codec, and implementing a codec creates unnecessary correctness and maintenance risk.
- Using a CPU compressor as an automatic fallback is rejected because it silently changes compute and host-memory costs. The safe fallback is the current raw GPU cache or no cache admission.
- Keeping compressed bytes in host RAM is rejected because that does not increase useful VRAM cache capacity and may add guest-memory pressure.
- Reserving a full raw-sized VRAM allocation per compressed extent is rejected because it cannot produce physical VRAM savings.
- Promising a fixed expansion ratio is rejected because exact, workload-specific compression and allocator overhead are unknown.

## 4. Functional requirements

| ID | Requirement | Verifiable acceptance |
| --- | --- | --- |
| RF-1 | Compression is confined to the disposable VRAM cache. The SSD origin remains authoritative and its write-before-cache order does not change. | Existing origin tests pass; a cache codec fault or worker loss returns exact origin bytes and never acknowledges a write missing from the origin. |
| RF-2 | Every encoded extent is lossless and independently bounded. An extent has logical offset and length, representation, stored length, allocated length, codec/version, and checksums. | Deterministic round-trip tests compare every output byte. Length, offset, codec, and allocation bounds are rejected before decode. |
| RF-3 | Compressed extents are retained only when their allocator-rounded physical allocation is smaller than the raw allocation. Otherwise the worker stores raw data if budget permits, or skips cache admission. | Tests cover compressible, incompressible, expanding, and allocator-rounded-no-gain inputs. |
| RF-4 | Cache entries cannot return stale bytes after an origin update. An update invalidates every overlapping entry before publishing replacement cache data. | Partial, overlapping, repeated, and out-of-order update tests return either a complete current cache hit or a miss; they never return stale or mixed data. |
| RF-5 | A cache hit containing compressed extents is returned only after complete range coverage, successful per-item decode status, a matching compressed-payload checksum before decode, exact output lengths, and matching uncompressed checksums. Raw-only hits retain the existing coverage checks. | Corrupt metadata, payload, status, holes, and short/long decode results become cache misses; no partial buffer is exposed. |
| RF-6 | Codec scratch, input/output staging, cache slabs, allocator slack, and metadata are bounded and included in admission. Every new batch uses a fresh adapter-bound budget sample and preserves existing reserve/runtime-free rules. | Tests from zero budget, stale budget, allocation failure, maximum scratch, and fragmentation admit no work that crosses the current policy floor. |
| RF-7 | Codec support is optional per provider. Missing library, unsupported adapter, initialization error, or codec failure does not prevent the raw cache path from starting. There is no CPU compression fallback. | NVIDIA codec refusal and raw-only CUDA/Vulkan tests pass alongside an eligible-provider compression test. |
| RF-8 | Physical VRAM, logical cached bytes, compressed payload, raw bypass, scratch, metadata, decode errors, and timeouts are separate signals. Logical cache bytes are never reported as Linux/WSL RAM usage. | Versioned telemetry tests validate units, bounds, freshness, and labels; status rendering distinguishes logical cache contents from physical VRAM use. |
| RF-9 | Compression is disabled by default. No CLI, kernel ABI, swap format, or public configuration change is introduced in this slice. | Default configuration uses the current raw cache; explicit test configuration is required to exercise compression. |

## 5. Non-functional requirements

| ID | Category | Requirement |
| --- | --- | --- |
| NFR-1 | Exactness | The codec is lossless. Any byte mismatch, checksum mismatch, or invalid codec status disables compressed entries for that worker and falls back to the authoritative origin. |
| NFR-2 | Host/device safety | Allocate persistent metadata only on demand; its budget is zero when physical_target_bytes is zero, otherwise capped at min(16 MiB, max(64 KiB, physical_target_bytes / 256)). Keep at most one codec batch in flight and cap additional host codec staging at 4 MiB. Cap all GPU codec temporary allocations at min(64 MiB, floor(fresh_admissible_headroom / 100)), where fresh_admissible_headroom = available_bytes.saturating_sub(reserve_floor_bytes) from the latest adapter-bound snapshot; include workspace, decoded output, pointer/status arrays, and temporary codec buffers. Allocate on demand only after a fresh adapter-bound budget check. If queried workspace does not fit, reduce the batch; if one bounded extent still does not fit, use raw admission or skip it. The existing private cache-read response remains capped at 16 MiB and is accounted separately from codec staging. |
| NFR-3 | Work bounds | A worker frame retains the existing 16 MiB mutation-payload limit. Compression uses extents no larger than 64 KiB and batches no larger than 4 MiB of logical input. A cache read over 16 MiB is a miss, checked before allocating its response buffer; the authoritative origin can still serve the original request. |
| NFR-4 | Latency | Preserve the existing cache read deadline; the default is 50 ms. Hardware qualification requires zero codec-induced read timeouts in each of three fixed runs. Do not increase the deadline to make compression appear successful. |
| NFR-5 | Capacity evidence | Report the effective ratio as logical bytes divided by all cache-owned VRAM slabs plus retained codec workspace. A release claim requires at least 10% more logical bytes resident than the raw-cache baseline on a predeclared workload mix, after allocator slack and workspace. This is a proposed minimum usefulness gate, not a promised result. |
| NFR-6 | Measurement and GPU co-load | Compare raw and compressed paths on the same adapter, driver, transport, budget, workload, and run duration. Report n=3, throughput, p50/p95/p99, cache hit and origin fallback bytes, host CPU time, logical bytes, physical VRAM, scratch, allocator slack, worker RSS, adapter utilization, and timeout/integrity counts. Repeat with a predeclared foreground GPU workload; the proposed promotion gate is no missed foreground deadlines and at most 5% p95 latency regression across paired runs. If the workload has no objective deadline/latency measure, keep compression experimental and disabled by default. |
| NFR-7 | Privacy and operations | Use deterministic synthetic data; do not dump or persist user RAM, swap, or application payloads for codec tuning. Do not enable cache stress or alter swap on the daily host as part of this proposal. |

## 6. Flows

### Cache promotion or update

1. AuthoritativeOriginBackend completes the origin read or durable origin write before sending a best-effort cache mutation.
2. The isolated worker validates the frame size and splits the supplied range into extents of at most 64 KiB.
3. For an update, the worker invalidates every old entry overlapping the updated logical range before publishing replacement data.
4. If compression is enabled and a GPU codec is available, the worker validates adapter-bound budget, queries maximum output and scratch sizes, and reserves bounded provisional slab ranges.
5. The codec compresses a batch losslessly and returns one status and output length per extent. The worker checks output bounds and allocator-rounded size, records a CRC32 of the original host input, and computes the compressed-payload CRC32 over VRAM before publishing the entry.
6. If compression does not reduce physical allocation, the worker stores bytes raw when existing budget policy admits them. If codec setup or compression fails, it uses the same raw-or-skip policy.
7. Only after all per-entry checks succeed does the worker publish new index records. Any failed provisional operation releases its provisional ranges.
8. The origin path does not wait for a cache hit. Cache mutation remains within the existing bounded worker IPC contract.

### Cache read

1. The worker validates the requested range against the request limit and locates non-overlapping entries that completely cover it.
2. Raw entries are copied into a private response buffer. For compressed entries, the worker validates metadata bounds and asks the provider to compute the compressed-payload CRC32 in VRAM. A mismatch refuses the entry before any decoder call.
3. The worker decodes at most one bounded batch into private host output, requires successful per-entry status and exact logical length, and checks the uncompressed CRC32 against the value captured before compression.
4. The worker sends a response only after the entire requested range is verified. A hole or any entry failure returns a miss; partial output is discarded.
5. The origin backend serves a miss from SSD. A cache read timeout or protocol fault revokes the cache client under the existing policy.

### Errors and alternate paths

| Trigger | Origin/worker result | Log and state |
| --- | --- | --- |
| Codec library absent or adapter unsupported | Keep raw cache if admitted; otherwise skip cache admission. Origin read/write remains successful. | Bounded codec raw-only reason; worker stays available. |
| Compression output expands or allocator rounding removes the gain | Store raw if admitted; otherwise skip. | Increment raw-bypass reason; no origin error. |
| Scratch, slab, or workspace allocation denied | Release provisional state; raw-store if admitted, otherwise skip. | Increment admission refusal; no target or reserve override. |
| Compressed checksum, metadata, codec status, output length, or decoded checksum fails | Invalidate affected entry, disable compressed entries for this worker, and return a miss. Origin supplies exact bytes. | Increment integrity failure; worker cache becomes raw-only or unavailable if the underlying GPU is unhealthy. |
| Cache worker exceeds its read deadline, crashes, or disconnects | Existing client marks cache unavailable and reads from origin. | Existing fail-closed worker state; no retry of a possibly in-flight codec operation. |
| Origin read/write/sync fails | Return existing block I/O error; cache data cannot mask origin failure. | Existing origin failed/degraded state. |

## 7. Data and state model

The cache remains volatile and non-authoritative. No compressed format is written to the SSD origin or exchanged across worker restarts.

Each cache record contains:

- Logical start and logical byte length.
- A worker-local generation assigned when the record is published.
- Representation: raw or compressed.
- Codec identifier and format version for compressed entries.
- Stored byte length and allocator-rounded physical allocation length.
- VRAM slab identifier and byte range.
- CRC32 of stored compressed payload and decoded logical bytes.
- LRU access time.

The worker owns a bounded ordered range index and dynamically allocated VRAM slabs. A slab uses coalescing free ranges for entries up to 64 KiB. Slabs are allocated only on demand and freed when empty. The metadata budget is zero when the physical target is zero; otherwise it is min(16 MiB, max(64 KiB, physical_target_bytes / 256)). The first implementation does not compact live entries; fragmentation may refuse admission, evict least-recently-used entries, or leave the cache raw-only. Physical accounting uses whole allocated slabs, not the sum of compressed lengths.

The worker separately accounts for:

- logical_cached_bytes: original bytes represented by valid cache entries.
- physical_cache_slab_bytes: all allocated VRAM slabs, including free space and fragmentation.
- codec_workspace_bytes: retained scratch and staging capacity.
- compressed_payload_bytes and raw_payload_bytes.
- metadata_bytes and raw-bypass/integrity/timeout counters.

Existing cached_bytes and target semantics remain physical. New logical telemetry is additive and must never be labelled as host RAM, WSL RAM, or Linux MemTotal.

## 8. Interfaces

- Add an internal-only optional GpuCacheCodec capability associated with a concrete VramProvider. It reports codec identity, maximum output bounds, required alignments, queried workspace, batch compression statuses/lengths, and bounded decompression statuses.
- Provider methods operate on provider-owned VRAM slabs and bounded caller-owned host slices. The codec implementation owns CUDA/Vulkan-specific handles and waits for completion before returning. If completion is uncertain, it retains in-flight resources and faults the compressed cache; it does not free memory early.
- ramshared-cuda may load the nvCOMP runtime dynamically. Missing runtime or unsupported device reports no compression capability and leaves the current raw path active.
- The initial NVIDIA codec is nvCOMP LZ4, lossless mode only. Per-item status reporting must remain enabled; decompression bounds/status checks must not be disabled to chase throughput.
- The existing worker protocol remains bounded and versioned. Add a bounded worker-cache telemetry envelope instead of overloading GPU budget fields. Preserve current physical cached_bytes meaning in existing frame fields.
- No public CLI, kernel uAPI, sysfs, ioctl, swap metadata, or disk-origin format is added. The existing CLI may render new telemetry with distinct logical-cache and physical-VRAM labels.

## 9. Dependencies and risks

- **Prerequisites:** bounded slab allocator and metadata index; optional codec interface; exact codec capability and hardware identity; tests with a deterministic fake codec; versioned cache telemetry.
- **Main risks:** range invalidation and fragmentation complexity; transient peak GPU memory; decoder latency; driver/library availability; worker timeout while GPU work is in flight; host RSS growth from per-entry metadata.
- **Mitigations:** cap entries, metadata, batches, host staging, and GPU scratch; account actual slab allocations; keep origin authoritative; keep raw fallback; preserve worker isolation and existing deadlines; verify compressed bytes before decode and decoded bytes before return; test co-load impact on a foreground GPU workload; use no CPU compression fallback.
- **Initial enablement:** default off. First hardware target is NVIDIA CUDA with nvCOMP LZ4 after the installed runtime and exact adapter pass. Vulkan, AMD, Intel, and every other GPU remain raw-only until a codec backend passes the same contract.
- **Numeric rollback trigger:** disable compressed-cache admission after any exact-byte or checksum mismatch, any cache-read timeout caused by codec work, or one live budget sample below the existing reserve/runtime-free floor. Keep it disabled if three qualification runs fail to show at least 10% net logical-capacity gain on the declared workload mix. Raw cache and SSD origin remain available.
- **Security and resource abuse:** malformed lengths, excessive frame/read sizes, stale adapter telemetry, compressed-payload corruption, repeated partial writes, and codec hangs all refuse or invalidate cache entries. They must not allocate outside the measured budget or delay the authoritative origin path.

## 10. Implementation strategy

1. Add the codec/entry contract and bounded fake-provider tests. Lock down exact bytes, range coverage, overlap invalidation, checksums, and failure behavior before any GPU kernel call.
2. Add a bounded slab/free-range allocator and raw/compressed entry handling to the isolated worker, with compression disabled by default. Test exhaustion, fragmentation, repeated disable, and release.
3. Add the NVIDIA nvCOMP LZ4 backend behind optional runtime loading. Test CUDA API/status/length/error handling with a mock and then the exact supported GPU. Do not add cuTile-rs or a hand-written codec.
4. Add versioned telemetry while preserving physical cached_bytes and GPU budget semantics. Ensure ramshared top never presents logical cache bytes as RAM.
5. Run the exact isolated-worker hardware drill and fixed baseline comparison. Enable no default setting until every acceptance gate passes. No full swap pressure, host installation, or release claim is part of this specification.

## 11. Documents to update

| Document | Action |
| --- | --- |
| docs/architecture/CUDA-RUST-ACCELERATION-BLUEPRINT.md | Replace the swap-page compression proposal with the cache-only contract and nvCOMP qualification boundary. |
| docs/specs/README.md and generated docs/INDEX.md | Update the existing spec title/scope; keep this feature in its current folder to avoid a duplicate compression spec. |
| docs/reliability/DEGRADATION-MATRIX.md | Add cache corruption, codec timeout, and workspace-pressure behavior before implementation merge. |
| validation.md | Append only after a real exact-adapter worker drill; this doc-only planning step is not validation evidence. |
| docs/BENCHMARKS.md and docs/benchmarks/results.jsonl | Add a claim only after the four-category comparison and Tier 3 origin qualification are measured. |
| Existing IMPL.md in this folder | Reconcile the prior swap-page proposal with this SPEC before Step 3. No implementation record is changed by this planning task. |

## 12. Out of scope

- Lossy encodings, quantization, NVFP4, or any transformation that changes bytes.
- Compressing the SSD origin, Linux swap format, a block-device ABI, Windows pagefile, or persistent data.
- GPU compression as a universal capability across all vendors.
- CPU codec fallback, automatic batch-size tuning from private memory contents, or user-data capture.
- Handwritten CUDA kernels, cuTile-rs, cuda-oxide, or replacement of the existing CUDA Driver API wrapper.
- Increasing the current cache deadline, relaxing VRAM reserve/headroom, changing worker teardown policy, or introducing a new host pressure campaign.
- Enabling compression by default, host install, swap activation, or product qualification in this planning step.

## 13. Acceptance criteria

1. The design stores lossless compressed data only in the revocable VRAM cache and keeps the SSD origin authoritative.
2. Raw fallback, range invalidation, bounded metadata/workspace, checksums, decode status, timeout, worker loss, and physical accounting have named tests in SPEC.
3. Unsupported codec/runtime leaves raw-only cache behavior intact.
4. The default remains off, the current worker deadline and GPU reserve policy remain in force, and no data representation is persisted.
5. Step 3 may implement only the isolated opt-in path. Production enablement requires the exact hardware drill, all integrity/refusal tests, three paired runs, at least 10% net effective capacity gain, zero codec-induced read timeouts, zero missed foreground GPU deadlines, and at most 5% foreground p95 latency regression.
6. No 2:1 ratio, universal GPU support, or host RAM increase claim is accepted without measured evidence.

## 14. Validation plan

- **Unit:** bounded metadata, extent splitting, exact lossless round trips, checksums, raw bypass, overlapping updates, incomplete reads, allocator fragmentation, scratch admission, disable/release idempotency, stale/malformed budgets, and unsupported codec refusal.
- **Origin integration:** exercise AuthoritativeOriginBackend with a memory origin and injected codec faults; prove writes remain origin-first and reads fall back to exact origin bytes.
- **Worker integration:** child worker with a fake provider; prove 50 ms client deadline behavior, worker crash, and disable acknowledgement without live swap.
- **Hardware:** ignored/opt-in exact-adapter nvCOMP LZ4 tests on NVIDIA sm75 or newer, with library/toolkit/driver hashes and fresh adapter-bound budget. This is required only for the NVIDIA backend and is not claimed by unit/CI tests.
- **Live product surface:** isolated WSL2 worker before/action/after drill on a non-pressure canary origin, exact adapter identity, zero timeout/integrity errors, worker binary identity where ramsharedd is involved. Do not force LKM, Windows WDK, cascade activation, or swap stress gates onto this pure userspace cache feature.
- **Performance:** three paired runs against the raw worker using seeded synthetic compressible, mixed, random, encrypted-like, sequential, random-read, and partial-update data, with the same predeclared foreground GPU workload in each raw/compressed pair. Report the four canonical categories: Workload & Capacity; Speed & Transfer Latency; Pressure & Stalls; Integrity & Stability. Include physical slabs, scratch, metadata/RSS, adapter utilization, cache p50/p95/p99, foreground p95/deadlines, origin fallback bytes, CPU time, and zero-panic/integrity verdict. Do not capture actual RAM or swap contents.
- **Environment-bound:** no codec implementation, hardware run, host install, pressure test, or performance result was produced in this planning task. Until the hardware and live-worker gates run, status remains proposal-only.
