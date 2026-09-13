# SPEC — Zero-Copy Host Memory Streaming and Byte-Level Page Tile Operations in CUDA-Rust

> SSDV3 Step 2 · PRD: docs/specs/no-milestone/cutile-zero-copy-page-tiering/PRD.md

## Closed Scope

### In Now
- **`cuda-core` Host Memory Registration**: Safe RAII wrapper `PinnedHostMapping` managing `cuMemHostRegister_v2`, `cuMemHostGetDevicePointer_v2`, and `cuMemHostUnregister`.
- **`cuda-async` DeviceAllocation Integration**: Implement `unsafe trait DeviceAllocation` for `PinnedHostMapping`, allowing zero-copy `DeviceBuffer::foreign` wrapped in `Arc`.
- **`cutile-compiler` Bitwise Reductions**: Implement `reduce_xor`, `reduce_and`, and `reduce_or` in `compile_intrinsic.rs` using `Opcode::Reduce` with type-checked neutral identities.
- **`cutile` Public Surface**: Expose `reduce_xor`, `reduce_and`, `reduce_or` in `cutile::core` and `_core.rs` for `Tile<E, D>`.
- **Downstream Tiering Synergy (`crates/ramshared-cuda`)**: Architectural trait alignment for zero-copy streaming of 4KB memory swap pages directly into GPU pipelines.

### Out Now
- Dynamic in-flight re-registration of resized memory spans (buffers must have a fixed length for their mapping lifetime).
- Multi-GPU P2P UVA memory registration across multiple NVLink nodes.
- Linux HMM (Heterogeneous Memory Management) driver-managed page faulting.

### Assumed-Ready Dependencies
- `NVlabs/cutile-rs` repository tree (checked out in `scratch/cutile-rs/`).
- CUDA Driver $\ge 550.0$ supporting 64-bit Unified Virtual Addressing (UVA).
- Linux kernel memory locking capabilities (`RLIMIT_MEMLOCK` or `CAP_IPC_LOCK`).

---

## Traceability

| PRD Requirement | SPEC ITEMs / Decisions | Verifiable Target |
| :--- | :--- | :--- |
| **RF-1** (`PinnedHostMapping`) | ITEM-1, ITEM-2, DT-1, DT-2 | `cuda-core::PinnedHostMapping::register` |
| **RF-2** (`DeviceAllocation` Bridge) | ITEM-3, DT-4 | `cuda-async::DeviceBuffer::foreign` with `Arc<PinnedHostMapping>` |
| **RF-3** (Bitwise Reductions) | ITEM-4, ITEM-5, DT-3 | `cutile-compiler` compiles `reduce_xor` / `reduce_and` / `reduce_or` |
| **RF-4** (4KB Page Tile Kernels) | ITEM-6 | 4KB Page zero-detect & XOR parity kernel in `cutile` |
| **NFR-1** (Registration Latency) | ITEM-1, ITEM-2 | Registration benchmark $\le 5\,\mu\text{s}$ |
| **NFR-2** (Transfer Speed) | ITEM-3, ITEM-6 | Zero-copy throughput $\ge 11.5\text{ GB/s}$ |
| **NFR-3** (RAII Soundness) | ITEM-1, ITEM-3 | Zero leaked mappings upon drop in sanitizer |
| **NFR-4** (Host Safety Bounds) | ITEM-1, DT-2 | Max registered size cap enforced |

---

## Technical Decisions

| # | Decision | Why |
| :--- | :--- | :--- |
| **DT-1** | Default Registration Flags | Use `CU_MEMHOSTREGISTER_DEVICEMAP \| CU_MEMHOSTREGISTER_PORTABLE`. `DEVICEMAP` creates an addressable GPU virtual mapping; `PORTABLE` ensures all CUDA contexts in multi-threaded daemons share access without secondary registration. |
| **DT-2** | Memory Boundary & Alignment Invariants | Enforce page alignment (`ptr as usize % 4096 == 0`) and page-multiple length (`len % 4096 == 0`). Reject misaligned addresses or zero length immediately with `DriverError(CUDA_ERROR_INVALID_VALUE)` before foreign driver invocation. |
| **DT-3** | Bitwise Reduction Identity Tokens | Map `reduce_xor` to neutral identity `0` with `{ curr ^ prev }`; map `reduce_and` to neutral identity `!0` (all bits 1: `0xFF` for u8, `0xFFFFFFFF` for u32) with `{ curr & prev }`; map `reduce_or` to neutral identity `0` with `{ curr \| prev }`. |
| **DT-4** | Arc Refcount as Liveness Barrier | `PinnedHostMapping` implements `DeviceAllocation`. Wrapping it in `Arc<PinnedHostMapping>` prevents `cuMemHostUnregister` from executing while any downstream `DeviceBuffer` or `Tensor` is alive. |

---

## Atomicity and Rollback

### Atomicity Frontier
1. **Registration Flow**:
   - Step A: Verify host pointer alignment, length bounds, and non-zero size.
   - Step B: Bind context to thread (`ctx.bind_to_thread()`).
   - Step C: Invoke `cuMemHostRegister_v2(host_ptr, len_bytes, flags)`.
   - Step D: Invoke `cuMemHostGetDevicePointer_v2(&mut dev_ptr, host_ptr, 0)`. If step D fails, step C is undone immediately via `cuMemHostUnregister(host_ptr)` before returning error.
2. **Deallocation Flow**:
   - `PinnedHostMapping::drop` executes atomically on final refcount decrement. Binds context and calls `cuMemHostUnregister(host_ptr)`.
   - Any driver error during drop is passed to `ctx.record_err(...)` without panicking in destructor.

### Rollback Split
- **Userspace / Daemon**: If `PinnedHostMapping::register` fails (e.g. `RLIMIT_MEMLOCK` exceeded), the caller catches the error and falls back to standard staged bounce-buffer DMA (`cuMemAllocHost` + `cuMemcpyHtoDAsync`).
- **Driver / Persistent**: RAII ensures no lingering registered host memory remains in `libcuda.so.1` upon process exit or error unwind.

---

## Kahneman Map (Critical Steps)

| ITEM / Stage | # | Question | Min Evidence | Abort |
| :--- | :--- | :--- | :--- | :--- |
| **ITEM-1** (`PinnedHostMapping` Validation) | #13 | Does the boundary strictly reject invalid pointers while accepting valid 4KB pages without false alarms? | `cargo test -p cuda-core test_pinned_host_mapping_validation` (verifying rejection of misaligned and zero-length slices + acceptance of 4KB page) | Refusal test fails or accepts misaligned pointer |
| **ITEM-3** (`DeviceAllocation` Soundness) | #16 | Can a fast thread drop the host buffer while a GPU stream is executing a zero-copy read? | `cargo test -p cuda-async test_pinned_mapping_liveness_token` (ensuring `Arc<dyn DeviceAllocation>` prevents drop until GPU fence/stream completes) | UAF detected under ASAN/Compute-Sanitizer |
| **ITEM-4** (Bitwise Reduction Identity) | #17 | Does `reduce_xor` remain idempotent and mathematically consistent across multi-warp reductions? | `cargo test -p cutile-compiler test_compile_reduce_xor_identity` | Output diverges from CPU reference bitwise reduction |

---

## Security Checklist (Pre-Impl)

- [x] **Privilege**: Host memory registration respects process `RLIMIT_MEMLOCK`; operations fail cleanly with `CUDA_ERROR_OUT_OF_MEMORY` if exceeded.
- [x] **User/Host Copy & Bounds**: Checks `len_bytes > 0` and `host_ptr as usize % 4096 == 0`. No out-of-bounds pointer arithmetic.
- [x] **Flags / IOCTL Codes**: Flags validated against allowed mask `(CU_MEMHOSTREGISTER_PORTABLE | CU_MEMHOSTREGISTER_DEVICEMAP | CU_MEMHOSTREGISTER_READ_ONLY)`. Unknown flags rejected.
- [x] **Info-Leak**: No raw host/kernel virtual memory addresses logged to production output or panic messages.
- [x] **IRQ/Atomic / Sleep**: CUDA Driver API calls only executed in preemptible thread context; never called in interrupt or atomic handlers.
- [x] **Lifetime**: RAII ownership prevents double-unregister; unregister occurs strictly when all `Arc` references to `PinnedHostMapping` hit zero.
- [x] **Hot-Unplug / Device-Gone**: If GPU resets or driver crashes, calls return `CUDA_ERROR_DEINITIALIZED` without SIGSEGV.
- [x] **Host Safety**: Unsupervised swap thrash strictly prohibited; memory registered per process capped to $\le 512\text{ MB}$.
- [x] **Shared-Hardware Cushion**: Host physical RAM is not locked beyond the safe memory reserve floor ($20\%$ host RAM preserved).
- [x] **Bounded DMA**: All zero-copy transfers executed stream-ordered with timeout guards.
- [x] **Cooperative Cascade Spillover**: If host registration quota is exhausted, memory pipeline spills over gracefully into staged DMA or SSD tier.
- [x] **Replayable Ops**: Idempotent mapping registration and teardown (#17).

---

## Files to CREATE / MODIFY / DELETE

### CREATE

**`scratch/cutile-rs/cuda-core/src/simt/pinned_host_mapping.rs`**
- Purpose: RAII safe wrapper for registering caller-owned host memory via `cuMemHostRegister`.
- RF / DT: RF-1, DT-1, DT-2.
- Types / fns:
  ```rust
  pub struct PinnedHostMapping {
      host_ptr: NonNull<u8>,
      dev_ptr: cuda_bindings::CUdeviceptr,
      len_bytes: usize,
      device_id: usize,
      ctx: Arc<CudaContext>,
  }
  impl PinnedHostMapping {
      pub fn register(ctx: &Arc<CudaContext>, host_ptr: NonNull<u8>, len_bytes: usize, flags: u32) -> Result<Self, DriverError>;
      pub fn dev_ptr(&self) -> cuda_bindings::CUdeviceptr;
      pub fn len_bytes(&self) -> usize;
      pub fn as_slice(&self) -> &[u8];
  }
  ```
- Reference pattern: `cuda-core/src/simt/pinned_host_buffer.rs`.
- Required tests: `cuda-core` :: `test_pinned_host_mapping_validation`, `test_pinned_host_mapping_lifecycle`.
- Cover target: $\ge 80\%$.
- Kahneman: #13 (refusal of misaligned/empty pointers).

**`scratch/cutile-rs/cuda-core/tests/pinned_host_mapping.rs`**
- Purpose: Integration tests for `PinnedHostMapping` on real or mock CUDA context.
- RF / DT: RF-1, NFR-3.
- Required tests: `pinned_host_mapping` :: `test_registration_and_unregister_cycle`.
- Cover target: N/A — test suite.

**`scratch/cutile-rs/cuda-async/tests/foreign_host_mapping.rs`**
- Purpose: Test `DeviceAllocation` bridge on `PinnedHostMapping` with `DeviceBuffer::foreign`.
- RF / DT: RF-2, DT-4.
- Required tests: `foreign_host_mapping` :: `test_pinned_mapping_as_foreign_device_buffer`.
- Cover target: N/A — test suite.

### MODIFY

**`scratch/cutile-rs/cuda-core/src/simt/memory.rs`**
- What: Add `host_register`, `host_unregister`, and `host_get_device_pointer` safe FFI helpers.
- RF / DT: RF-1.
- Before: Only `malloc_host` and `free_host`.
- After: Includes `host_register`, `host_unregister`, and `host_get_device_pointer`.
- Required tests: `cuda-core` :: `test_host_register_ffi`.
- Cover target: $\ge 80\%$.

**`scratch/cutile-rs/cuda-core/src/simt/mod.rs` & `src/lib.rs`**
- What: Re-export `PinnedHostMapping`.
- RF / DT: RF-1.

**`scratch/cutile-rs/cuda-async/src/device_buffer.rs`**
- What: Implement `unsafe trait DeviceAllocation for cuda_core::PinnedHostMapping`.
- RF / DT: RF-2, DT-4.
- Required tests: `cuda-async` :: `test_pinned_mapping_liveness_token`.
- Cover target: $\ge 80\%$.

**`scratch/cutile-rs/cutile-compiler/src/compiler/compile_intrinsic.rs`**
- What: Add `"reduce_xor"`, `"reduce_and"`, `"reduce_or"` match cases to `compile_reduce_op`.
- RF / DT: RF-3, DT-3.
- Symbols: `compile_reduce_op`, identity tokens (`0`, `!0`), closure syntax blocks.
- Required tests: `cutile-compiler` :: `test_compile_reduce_xor_intrinsic`.
- Cover target: $\ge 80\%$.

**`scratch/cutile-rs/cutile/src/_core.rs`**
- What: Expose `reduce_xor`, `reduce_and`, `reduce_or` functions on `Tile<E, D>`.
- RF / DT: RF-3.
- Required tests: `cutile` :: `test_reduce_xor_tile_api`.
- Cover target: $\ge 80\%$.

---

## Observability

| Signal | Where | Level / Type | Description |
| :--- | :--- | :--- | :--- |
| `host_mapping_bytes_total` | `crates/ramshared-cuda` metrics | Gauge (bytes) | Total host memory registered via `cuMemHostRegister` |
| `host_mapping_reg_fail` | Daemon telemetry / logs | Counter / WARN | Count of host registration failures leading to staged DMA fallback |
| `gpu_page_parity_mismatch` | Kernel / driver monitor | Counter / ERROR | Page parity verification failures indicating data corruption |

---

## Living Docs

| Document | Action |
| :--- | :--- |
| `docs/architecture/CUDA-RUST-ACCELERATION-BLUEPRINT.md` | Update with Section on Zero-Copy Page Streaming and Bitwise Reductions |
| `ARCHITECTURE.md` | Append note on zero-copy host registration in Tier 2 VRAM pipeline |
| `docs/specs/no-milestone/cutile-zero-copy-page-tiering/PRD.md` | Sourced |
| `docs/specs/no-milestone/cutile-zero-copy-page-tiering/IMPL.md` | To create in Step 3 |

---

## Implementation Order

- **ITEM-1**: Add `host_register`, `host_unregister`, `host_get_device_pointer` to `cuda-core/src/simt/memory.rs`.
- **ITEM-2**: Author `cuda-core/src/simt/pinned_host_mapping.rs` with strict pointer alignment and size validation; re-export in `cuda-core/src/lib.rs`.
- **ITEM-3**: Implement `unsafe trait DeviceAllocation for PinnedHostMapping` in `cuda-async/src/device_buffer.rs`.
- **ITEM-4**: Add `"reduce_xor"`, `"reduce_and"`, `"reduce_or"` compiler support in `cutile-compiler/src/compiler/compile_intrinsic.rs`.
- **ITEM-5**: Expose `reduce_xor`, `reduce_and`, `reduce_or` in `cutile/src/_core.rs`.
- **ITEM-6**: Author comprehensive integration tests verifying zero-copy mapping and bitwise 4KB page parity kernels.

---

## Required Tests Matrix

| Production Path | Test (`File` :: `Name`) | Kind | Kahneman | Cover Target |
| :--- | :--- | :--- | :--- | :--- |
| `cuda-core/src/simt/pinned_host_mapping.rs` | `pinned_host_mapping.rs` :: `test_pinned_host_mapping_validation` | unit | #13 | $\ge 80\%$ |
| `cuda-core/src/simt/pinned_host_mapping.rs` | `pinned_host_mapping.rs` :: `test_pinned_host_mapping_lifecycle` | integration | #16 | $\ge 80\%$ |
| `cuda-async/src/device_buffer.rs` | `device_buffer.rs` :: `test_pinned_mapping_liveness_token` | integration | #16 | $\ge 80\%$ |
| `cutile-compiler/src/compiler/compile_intrinsic.rs` | `compile_intrinsic.rs` :: `test_compile_reduce_xor_intrinsic` | unit | #17 | $\ge 80\%$ |
| `cutile/src/_core.rs` | `bitwise_page.rs` :: `test_4kb_page_parity_reduce_xor` | GPU integration | #17 | $\ge 80\%$ |

---

## Validation Checklist

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Every matrix row has an explicit executable test name.
- [ ] Kahneman critical rows (#13, #16, #17) have verifiable proof.
- [ ] RAII clean unregister verified with zero resource leak.
- [ ] Documentation index updated via `node tools/generate-docs-index.mjs`.
- [ ] `./scripts/docs-check.sh` passes 100% green.
