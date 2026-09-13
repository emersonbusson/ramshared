# IMPL — cuda-rust-native-tiering

## Tracking Record
- **SPEC**: [SPEC.md](SPEC.md)
- **AUDIT-2.5**: [AUDIT-2.5.md](AUDIT-2.5.md) (Verdict: **`go`**)

---

## ITEM Execution Order

### `[x]` ITEM-1: Modern Context & Device Discovery (`crates/ramshared-cuda`)
- Add `cuda-core = "0.3.1"` and `cuda-async = "0.3.1"` to `crates/ramshared-cuda/Cargo.toml`.
- Implement safe context initialization with architecture capability detection (`sm_75` vs `sm_80+`).
- Tests: `test_cuda_core_context_lifecycle`, `test_gpu_compute_capability_dispatch`.

### `[ ]` ITEM-2: Async Backend & Cancellation Token (`crates/ramshared-cuda`)
- Implement `CudaAsyncStream` and `DeviceOperation` with `tokio` / `futures` compatible cancellation.
- Guarantee non-blocking abort if DMA stalls $> 50\text{ms}$.
- Tests: `test_async_dma_cancellation_token`.

### `[ ]` ITEM-3: In-GPU Page Compression Kernel Dispatch
- Implement quantized 4KB slab sub-allocator for VRAM pages.
- Provide LZ4 PTX kernel integration for `sm_75` and Tile abstractions for `sm_80+`.
- Implement CRC32 integrity verification and raw uncompressed fallback.
- Tests: `test_in_gpu_page_compression_roundtrip`.

### `[ ]` ITEM-4: Broker Worker Wiring & Telemetry
- Wire async CUDA backend into `crates/ramshared-wsl2d` worker loop.
- Emit `gpu_compression_ratio` and `gpu_async_cancellation` metrics.
- Deploy binary and verify `BINARY_MATCH`.
