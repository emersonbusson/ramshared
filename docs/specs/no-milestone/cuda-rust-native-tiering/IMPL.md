# IMPL — cuda-rust-native-tiering

## Tracking Record
- **SPEC**: [SPEC.md](SPEC.md)
- **AUDIT-2.5**: [AUDIT-2.5.md](AUDIT-2.5.md) (historical verdict superseded by the September 2026 status correction)
- **Current state**: the existing CUDA Driver API path is working; `cuda-core`/`cuda-async` are optional manifest entries only; no `cutile`/`cuda-oxide` backend or compression implementation is present.

---

## ITEM Execution Order

### `[ ]` ITEM-1: Evaluate Context & Device Discovery (`crates/ramshared-cuda`)
- Optional `cuda-core = "0.3.1"` and `cuda-async = "0.3.1"` entries exist in `Cargo.toml`; production code does not call them.
- Prototype an isolated backend and compare it with the existing RAII Driver API implementation before migration.
- Implement and run the proposed context-lifecycle and capability-dispatch tests; the named tests below do not yet exist.

### `[ ]` ITEM-2: Async Backend & Cancellation Token (`crates/ramshared-cuda`)
- Implement `CudaAsyncStream` and `DeviceOperation` with `tokio` / `futures` compatible cancellation.
- Define bounded queueing and timeout reporting without promising that an in-flight foreign driver operation can be aborted. Retain all DMA memory until observed completion.
- Tests: `test_async_dma_cancellation_token`.

### `[ ]` ITEM-3: In-GPU Page Compression Kernel Dispatch
- Design crash-consistent raw/compressed metadata and bounded allocation before changing swap mappings.
- Provide LZ4 PTX kernel integration for `sm_75` and Tile abstractions for `sm_80+`.
- Implement CRC32 integrity verification and raw uncompressed fallback.
- Tests: `test_in_gpu_page_compression_roundtrip`.

### `[ ]` ITEM-4: Broker Worker Wiring & Telemetry
- Wire async CUDA backend into `crates/ramshared-wsl2d` worker loop.
- Emit `gpu_compression_ratio` and `gpu_async_cancellation` metrics.
- Qualification remains pending: live pressure/recovery evidence, swapoff-first, and `BINARY_MATCH` are required before host replacement. Tile tests require an `sm_80+` host; the local RTX 2060 cannot run them.
