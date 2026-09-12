# SPEC - Native CUDA-Rust Acceleration, In-GPU Page Compression, and Async Cancellation

## 1. Closed Scope

### In Now
- Architectural integration of `cuda-core` and `cuda-async` into `crates/ramshared-cuda`.
- Non-blocking asynchronous stream execution with Rust Future `.await` and cancellation token propagation.
- Dual-track GPU kernel design: `cuda-oxide` SIMT PTX for `sm_75` (RTX 2060) and `cutile-rs` Tile IR for `sm_80+` (Ampere/Blackwell).
- Verification of round-trip in-GPU page compression and decompression.

### Out Now
- Kernel-space LKM changes (userspace daemon and GPU runtime scope).
- Modification of Windows display driver internals.

### Assumed-Ready Dependencies
- `crates/ramshared-cuda` and `crates/ramshared-vram`.
- NVIDIA CUDA 13.x driver stack on Linux/WSL2.

---

## 2. Traceability

| PRD Requirement | Implementation / Decision Item | Covered By Test / Evidence |
| :--- | :--- | :--- |
| **RF-1** (`cuda-core` Context) | `ITEM-1`, `DT-1` | `test_cuda_core_context_lifecycle` |
| **RF-2** (Async Cancellation) | `ITEM-2`, `DT-2` | `test_async_dma_cancellation_token` |
| **RF-3** (In-GPU Compression) | `ITEM-3`, `DT-3` | `test_in_gpu_page_compression_roundtrip` |
| **RF-4** (Architecture Detection) | `ITEM-4`, `DT-4` | `test_gpu_compute_capability_dispatch` |
| **NFR-1** (Memory Amplification) | `ITEM-3` | Compression ratio $\ge 1.8\times$ assertion |
| **NFR-2** (Latency) | `ITEM-3` | Microbenchmark latency $\le 5\,\mu\text{s}$ |
| **NFR-3** (Zero Freeze) | `ITEM-2` | Watchdog timeout non-blocking abort proof |

---

## 3. Technical Decisions

| # | Decision | Why |
| :--- | :--- | :--- |
| **DT-1** | **Adopt `cuda-core` over Raw FFI**: Replace manual `dlopen` wrappers in `crates/ramshared-cuda` with NVIDIA's `cuda-core`. | Guarantees safe RAII resource lifetime management, correct CUDA context scoping, and type-safe device buffers. |
| **DT-2** | **Rust Future-Driven DMA**: Implement `DeviceOperation` with explicit cancellation tokens. | Prevents thread deadlocks when `/dev/dxg` experiences host GPU memory pressure or TDR events. |
| **DT-3** | **Dual-Track Kernel Compilation**: Pre-compile `cuda-oxide` device kernels to static PTX for `sm_75`, while using `cutile-rs` JIT for `sm_80+`. | Accommodates the hardware reality: workstation RTX 2060 is `sm_75` (unsupported by Tile IR), while datacenter GPUs are `sm_80+`. |
| **DT-4** | **Page-Level Chunk Layout**: Store compressed pages in a variable-sized sub-allocated slab within the VRAM slice. | Maximizes VRAM storage density without incurring page fragmentation. |

---

## 4. Atomicity and Rollback

- **Atomicity Frontier**:
  - GPU context creation and buffer allocation are transactional; any failure during device initialization cleanly releases all resources and falls back to the RAM backend.
  - Page compression is verified via a header CRC32; corrupt or uncompressible pages fallback to uncompressed raw storage.
- **Rollback**:
  - Purely userspace in `crates/ramshared-cuda`; git revert cleanly restores legacy driver API wrappers.

---

## 5. Kahneman Map (Critical Steps)

| ITEM / Stage | # | Question | Min Evidence | Abort |
| :--- | :--- | :--- | :--- | :--- |
| **ITEM-1** (Core) | **#13** (Refusal + Legitimate) | Does context creation fail gracefully on non-CUDA systems while succeeding on valid hardware? | `cargo test -p ramshared-cuda test_cuda_core_context` | Unhandled panic or SIGSEGV |
| **ITEM-2** (Cancellation) | **#15** (Transient Retry / Failover) | Does a cancelled GPU operation abort within 50ms without hanging the executor thread? | `cargo test -p ramshared-cuda test_async_dma_cancellation` | Thread blocks $> 100\text{ ms}$ |
| **ITEM-3** (Compression) | **#17** (Idempotency & Integrity) | Does decompression of compressed swap pages produce byte-for-byte identical data? | `cargo test -p ramshared-cuda test_in_gpu_compression_integrity` | Checksum mismatch or memory corruption |

---

## 6. Security Checklist (Pre-Impl)

- [x] **Privilege**: Standard user/daemon permissions; no elevated Windows privileges required.
- [x] **User/Host Copy**: Device buffers strictly bounded; no out-of-bounds DMA transfers.
- [x] **Flags/IOCTL Codes**: Validated through `cuda-core`.
- [x] **Info-Leak**: No GPU memory contents leaked uninitialized; buffers explicitly cleared.
- [x] **IRQ / IRQL**: Runs in userspace async runtime; no illegal sleeping in atomic context.
- [x] **Lifetime**: RAII device memory drops automatically unmap and free GPU memory.
- [x] **Shared-Hardware Cushion**: Inherits the host reserve floor ($\ge 2,048\text{ MB}$) from Principle 11.
- [x] **Bounded DMA**: All GPU streams bound to cancellation tokens and timeout watchdogs.

---

## 7. Files to CREATE / MODIFY / DELETE

### CREATE
**`crates/ramshared-cuda/src/async_backend.rs`**
- **Purpose**: Composable async GPU I/O operations with cancellation support.
- **Required Tests**: `test_async_dma_cancellation_token`

### MODIFY
**`crates/ramshared-cuda/Cargo.toml`**
- **Purpose**: Add `cuda-core` and `cuda-async` dependencies.

---

## 8. Observability

| Signal | Where | Level / Type |
| :--- | :--- | :--- |
| `gpu_compression_ratio` | `telemetry.jsonl` | INFO / Float metric |
| `gpu_async_cancellation` | `stderr` + `telemetry.jsonl` | WARN / Structured JSON |

---

## 9. Implementation Order

- **ITEM-1**: Add `cuda-core` and `cuda-async` to `crates/ramshared-cuda/Cargo.toml` and implement safe context initialization and device discovery in `crates/ramshared-cuda/src/context.rs`.
- **ITEM-2**: Implement `crates/ramshared-cuda/src/async_backend.rs` with `CudaAsyncStream`, non-blocking DMA execution, and `CancellationToken` support.
- **ITEM-3**: Implement page-level compression kernel dispatch (using `cuda-oxide` PTX for `sm_75` and `cutile` tile abstractions for `sm_80+`) with CRC32 verification and uncompressed fallback.
- **ITEM-4**: Connect async driver operations to broker worker loop with bounded 50ms timeout watchdog.

---

## 10. Required Tests Matrix

| Production Path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| :--- | :--- | :--- | :--- | :--- |
| `crates/ramshared-cuda/src/context.rs` | `context` :: `test_cuda_core_context_lifecycle` | unit | #13 | ≥80% |
| `crates/ramshared-cuda/src/async_backend.rs` | `async_backend` :: `test_async_dma_cancellation_token` | unit | #15 | ≥80% |
| `crates/ramshared-cuda/src/async_backend.rs` | `async_backend` :: `test_in_gpu_page_compression_roundtrip` | unit | #17 | ≥80% |
| `crates/ramshared-cuda/src/async_backend.rs` | `async_backend` :: `test_gpu_compute_capability_dispatch` | unit | #13 | ≥80% |

---

## 11. Validation Checklist

- [ ] `cargo fmt` / `cargo clippy -p ramshared-cuda -- -D warnings` / `cargo test -p ramshared-cuda`
- [ ] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cuda --files crates/ramshared-cuda/src/async_backend.rs --min 80`
- [ ] Live path for this product surface (CUDA 13 Driver API on WSL2)
- [ ] Every matrix row has a real test name
- [ ] Kahneman critical rows have executable evidence

