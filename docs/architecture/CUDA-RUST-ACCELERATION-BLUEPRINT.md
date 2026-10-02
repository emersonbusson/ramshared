# GPU Cache Compression: Architecture and Qualification

## Status

RamShared currently stores raw copies in its isolated, revocable VRAM cache. The SSD origin remains authoritative. There is no GPU compression implementation, compressed cache format, or qualified compression result.

The proposed feature is cache-only and lossless. It does not change swap data, the SSD origin, the kernel interface, or persistent storage. Compression remains disabled by default and has not been implemented or qualified.

## Recommended design

- Keep compression inside the existing isolated GPU cache worker.
- Preserve origin-first writes, bounded cache reads, reserve checks, cache revocation, and SSD fallback.
- Add an optional provider-specific codec capability. The first NVIDIA candidate is nvCOMP LZ4, dynamically available only when its runtime and selected adapter qualify.
- Retain compressed output in variable-size extents within on-demand VRAM slabs. Count full slab allocations, allocator slack, and retained workspace against physical GPU budget.
- Store an extent raw when compression does not reduce allocator-rounded physical use. Use no CPU codec fallback. Providers without a qualified GPU codec remain raw-only.
- Report logical cached bytes separately from physical VRAM bytes, scratch, metadata, and raw bypasses. Never label logical cache capacity as WSL or host RAM.

## Hardware and software boundary

NVIDIA's current nvCOMP installation requirements list Volta sm70 or newer, CUDA Toolkit 12.0 or newer, and minimum driver versions. The local RTX 2060 is sm75 and therefore meets the documented architecture floor. The exact WSL package/runtime/driver path and end-to-end behavior still require a live test.

nvCOMP exposes lossless LZ4 and batched encode/decode APIs. The caller owns chunking, length metadata, output bounds, status checks, and device workspace. The API guide recommends similarly sized chunks for load balancing. Its decoder documentation warns that malformed data receives limited validation, so RamShared must validate bounds and verify a stored-payload CRC32 while it remains in VRAM before decode, then check status, exact output length, and the uncompressed CRC32 before returning bytes. nvCOMP's GPU CRC32 path is the NVIDIA candidate; providers without a safe pre-decode checksum stay raw-only.

cuTile-rs is a Rust DSL for writing tiled GPU kernels, not a ready-made memory-compression library. It is not the first codec choice. Vulkan, AMD, Intel, and other providers remain raw-only until they implement and qualify the same optional capability contract.

Primary references:

- [nvCOMP installation](https://docs.nvidia.com/cuda/nvcomp/installation.html)
- [nvCOMP overview and lossless algorithms](https://docs.nvidia.com/cuda/nvcomp/)
- [nvCOMP batched API guide](https://docs.nvidia.com/cuda/nvcomp/samples/lowlevel_c_quickstart.html)
- [nvCOMP C API reference](https://docs.nvidia.com/cuda/nvcomp/c_api.html)
- [cuTile-rs](https://github.com/NVlabs/cutile-rs)

## Safety contract

A cache entry is volatile and disposable. It becomes visible only after compression status, lengths, bounds, and checksums have passed. Updates evict every overlapping entry before replacement. Reads require complete range coverage and verify the full result in a private buffer before returning it.

Missing codec support, non-beneficial compression, workspace refusal, decoder error, bad checksum, timeout, worker loss, or GPU pressure produces a raw-cache bypass, a cache miss, or cache revocation. The authoritative origin continues independently.

All allocations use the current adapter-bound budget and reserve policy. Only one batch may be in flight; the proposal caps additional host codec staging at 4 MiB, GPU temporary allocations at min(64 MiB, 1% of latest available bytes after the existing reserve floor), metadata at the PRD's physical-target-derived ceiling, and a private cache response at 16 MiB. Transient GPU allocation is checked before every batch. No capacity multiplier is promised. An effective gain is measured only after slab allocation, workspace, metadata, allocator slack, and fragmentation are included.

## Qualification stages

1. Unit-test a fake codec and bounded allocator for exact bytes, corruption, partial updates, unsupported providers, pressure refusal, and idempotent teardown.
2. Implement the optional cache path with compression disabled by default; keep raw entries as the permanent fallback.
3. Test nvCOMP LZ4 on an exact NVIDIA adapter, including output bounds, per-item statuses, corruption refusal, context lifetime, and worker deadlines.
4. Compare the raw and compressed cache in three fixed synthetic-data runs. Report logical bytes, physical slabs, scratch, metadata/RSS, throughput, p50/p95/p99, CPU time, origin fallback, timeouts, and integrity.
5. Keep compression disabled if any integrity error or codec-induced timeout occurs, if the current headroom floor is crossed, if the declared workload mix gains less than 10% net logical capacity after all overhead, or if a paired foreground GPU run misses a deadline or regresses p95 latency by more than 5%.

A successful source test or codec microbenchmark does not qualify the WSL2 cache worker. Hardware, live-worker, and product evidence remain separate gates.

## Detailed requirements

See the current [PRD](../specs/no-milestone/cuda-rust-native-tiering/PRD.md), [SPEC](../specs/no-milestone/cuda-rust-native-tiering/SPEC.md), and [AUDIT-2.5](../specs/no-milestone/cuda-rust-native-tiering/AUDIT-2.5.md). The existing [IMPL tracker](../specs/no-milestone/cuda-rust-native-tiering/IMPL.md) must be reconciled with the cache-only scope before Step 3. No implementation or hardware qualification is recorded here.
