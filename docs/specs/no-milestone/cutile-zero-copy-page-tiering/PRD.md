---
slug: cutile-zero-copy-page-tiering
title: Zero-copy host memory streaming and byte-level page tile operations in CUDA-Rust
milestone: —
issues: []
---

# PRD — Zero-Copy Host Memory Streaming and Byte-Level Page Tile Operations in CUDA-Rust

## 1. Summary

In RamShared's VRAM tiering architecture (`crates/ramshared-cuda` and `crates/ramshared-vram`), the daemon orchestrates high-throughput memory exchange between Linux swap/kernel pages and GPU device memory. Under the baseline implementation, transferring a 4KB host page into GPU memory requires an intermediate bounce buffer or a host-to-device synchronous `cuMemcpyHtoD`, incurring a CPU copy penalty and doubling bus latency.

Furthermore, applying in-GPU data transformations (such as LZ4 compression, bit-packing, or CRC32/parity verification) requires byte-level primitive manipulations and fast bitwise reductions (`reduce_xor`, `reduce_and`, `reduce_or`). While NVIDIA's recently open-sourced `NVlabs/cutile-rs` provides state-of-the-art Tile IR abstractions for tensor mathematics, its runtime currently lacks:
1. **Zero-Copy Host Memory Registration**: Wrapping existing host-allocated pages (`cuMemHostRegister`) into safe, RAII-managed device views (`DeviceAllocation`).
2. **Byte-Level Systems Operations**: First-class 4KB byte tiles (`Tile<u8, 4096>`), bitwise reduction intrinsics (`reduce_xor`, `reduce_and`, `reduce_or`), and bit-manipulation primitives for GPU-side data integrity and compression.

This PRD defines the requirements to co-evolve `cutile-rs` and `ramshared-cuda` to enable direct, zero-copy host memory streaming and ultra-fast in-GPU byte page manipulation.

---

## 2. Technical Context & Codebase Anchors

### 2.1 Hardware and Runtime Topology
- **Host Target**: Linux (WSL2 Ubuntu 24.04 and bare-metal Ubuntu/Debian x86_64).
- **GPU Targets**: NVIDIA Turing (`sm_75` via PTX fallback) and Ampere/Hopper/Blackwell (`sm_80`–`sm_100+` via native `cutile` Tile IR).
- **Driver**: NVIDIA Linux/WSL2 Display Driver supporting CUDA 12.x / 13.x with Unified Virtual Addressing (UVA).

### 2.2 Anchors in Codebase and Toolchain
- **Confirmed in codebase (`crates/ramshared-cuda/src/lib.rs`, `src/driver.rs`)**: `ramshared-cuda` currently uses manual `libcuda.so.1` symbol resolution (`loader_unix.rs`) for allocation and transfers, operating strictly on raw device pointers without zero-copy host registration or async cancellation tokens.
- **Confirmed in codebase (`scratch/cutile-rs/cuda-core/src/simt/pinned_host_buffer.rs`)**: `cuda-core` exposes `PinnedHostBuffer<T>` which allocates newly pinned memory using `cuMemAllocHost` and frees it with `cuMemFreeHost`. It does not support registering pre-existing caller memory.
- **Confirmed in codebase (`scratch/cutile-rs/cuda-async/src/device_buffer.rs:40-60`)**: Defines `unsafe trait DeviceAllocation: Send + Sync + 'static` and `DeviceBuffer::foreign(Arc<dyn DeviceAllocation>)` as a liveness token, enabling non-owning GPU views of externally managed allocations.
- **Confirmed in codebase (`scratch/cutile-rs/cutile/src/tensor.rs:961`)**: `Tensor::from_foreign(owner: Arc<dyn DeviceAllocation>, shape: Vec<i32>, strides: Vec<i32>)` constructs tensor views directly from foreign device allocations.
- **Confirmed in codebase (`scratch/cutile-rs/cutile-compiler/src/compiler/compile_intrinsic.rs:1675-1698`)**: The Tile JIT compiler supports reductions via `reduce_min`, `reduce_max`, `reduce_sum`, `reduce_prod`, but rejects any bitwise reduction (`reduce_xor`, `reduce_and`, `reduce_or`).
- **Confirmed in docs (CUDA Driver API Specification)**: `cuMemHostRegister(pHost, bytesize, Flags)` registers host memory with `CU_MEMHOSTREGISTER_DEVICEMAP` and `CU_MEMHOSTREGISTER_PORTABLE`, and `cuMemHostGetDevicePointer(&dptr, pHost, 0)` returns the mapped GPU virtual address.
- **Inference**: Wrapping `cuMemHostRegister` inside a `PinnedHostMapping` struct that implements `DeviceAllocation` allows `ramsharedd` and `cutile-rs` to zero-copy map existing 4KB swap pages with zero allocation overhead and under $1\,\mu\text{s}$ setup latency.

---

## 3. Recommended Option & Trade-offs

### Recommended Design: Dual-Sided Integration Bridge

1. **Host Memory Streaming Layer (`cuda-core` & `cuda-async`)**:
   - Introduce `PinnedHostMapping` in `cuda-core`: Safe RAII registration of arbitrary user/kernel-provided host buffers via `cuMemHostRegister` and `cuMemHostUnregister`.
   - Implement `unsafe trait DeviceAllocation` for `PinnedHostMapping`.
   - Expose `Tensor::<u8>::from_pinned_host_slice` or `DeviceBuffer::from_pinned_host_mapping` in `cuda-async` and `cutile`.
2. **Byte Tile & Bitwise Systems Intrinsics (`cutile-compiler` & `cutile`)**:
   - Add `reduce_xor`, `reduce_and`, and `reduce_or` to `cutile-compiler/src/compiler/compile_intrinsic.rs`.
   - Expose typed reduction functions in `cutile/src/_core.rs`.
   - Support 1D page tiles (`Tile<u8, 4096>`) for fast page comparison, zero-page detection, parity generation, and bitwise transforms.

### Discarded Alternatives
- **Continuous Host-to-Device Bounce Buffers**: Discarded. Requires copying 4KB pages twice (Host Buffer -> Staging Buffer -> GPU VRAM), consuming CPU cycles and saturating PCIe bandwidth.
- **Pure Userspace CPU Compression / Hashing**: Discarded. Consumes host CPU cores during swap-out storms. Offloading to GPU compute cores utilizes internal VRAM bus bandwidth (336 GB/s on RTX 2060, >2 TB/s on H100).
- **Modifying Raw Driver C Structs in `ramshared-cuda` without Upstream Alignment**: Discarded. Re-inventing custom CUDA abstractions increases maintenance fragmentation; leveraging and enhancing upstream `cutile-rs` guarantees long-term support.

---

## 4. Functional Requirements (RF-N)

| ID | Description | Verifiable Acceptance |
| :--- | :--- | :--- |
| **RF-1** | **`PinnedHostMapping` in `cuda-core`** | Provide a safe RAII struct `PinnedHostMapping` wrapping `cuMemHostRegister` and `cuMemHostUnregister`. Validates pointer alignment and page-size multiples, rejecting invalid or zero-length ranges. |
| **RF-2** | **Zero-Copy `DeviceAllocation` Bridge** | `PinnedHostMapping` implements `DeviceAllocation`. Passing an `Arc<PinnedHostMapping>` into `DeviceBuffer::foreign` yields a valid device pointer readable/writable by CUDA streams. |
| **RF-3** | **Bitwise Tile Reductions in Compiler** | `cutile-compiler` compiles `reduce_xor`, `reduce_and`, and `reduce_or` into bytecode `Opcode::Reduce` with appropriate neutral identities (`0` for XOR/OR, `!0` for AND) and bitwise block operations (`^`, `&`, `\|`). |
| **RF-4** | **4KB Page Tile Integrity & Zero-Detection Kernel** | Author a verified Tile kernel operating on `Tile<u8, 4096>` that executes page zero-detection and XOR parity computation on GPU in $<2\,\mu\text{s}$ per 4KB page. |

---

## 5. Non-Functional Requirements (NFR-N)

| ID | Category | Target Metric |
| :--- | :--- | :--- |
| **NFR-1** | **Registration Latency** | `cuMemHostRegister` + `cuMemHostGetDevicePointer` setup latency $\le 5\,\mu\text{s}$ for 4KB–2MB buffers. |
| **NFR-2** | **Transfer & Kernel Speed** | GPU zero-copy read throughput across PCIe $\ge 11.5\text{ GB/s}$ (saturating PCIe Gen3 x16 / Gen4 x8). |
| **NFR-3** | **Zero Memory Leaks & RAII Soundness** | Dropping `PinnedHostMapping` unregisters memory immediately via `cuMemHostUnregister`. No memory pinned in the driver after context teardown. |
| **NFR-4** | **Host Safety & Bounded Resource Usage** | Registered host memory per process bounded by a configurable cap (default: $\le 512\text{ MB}$) to prevent exhausting host OS unevictable kernel memory. |

---

## 6. Execution Flows

### 6.1 Zero-Copy Host-to-Device Stream Flow
1. Host process (e.g. `ramsharedd` swap daemon) receives a 4KB dirty memory page.
2. Daemon registers or references a registered pool page via `PinnedHostMapping::register(ctx, ptr, 4096, Flags)`.
3. Daemon wraps the mapping into `DeviceBuffer::foreign(Arc::new(mapping))` and instantiates `Tensor::<u8>::from_foreign(...)`.
4. GPU Tile kernel executes directly over the registered host memory, or issues a stream-ordered asynchronous DMA into GPU VRAM tier without CPU involvement.
5. When the operation completes, the token is released; on final drop, `cuMemHostUnregister` cleanly frees the OS lock.

### 6.2 Bitwise Verification & Parity Flow
1. 4KB page in GPU memory is loaded into a `Tile<u8, 4096>`.
2. Kernel executes `let parity = reduce_xor(tile, 0);`.
3. Parity byte is compared with recorded checksum; if mismatch detected, hardware error is signaled immediately without corrupting swap cache.

---

## 7. Data and State Model

```text
┌──────────────────────────────────────────────────────────┐
│                   Host Process Memory                     │
│  [ Page 0 (4KB) ]  [ Page 1 (4KB) ]  [ Page 2 (4KB) ]    │
└───────────────┬──────────────────────────────────────────┘
                │ cuMemHostRegister(DEVICEMAP | PORTABLE)
                ▼
┌──────────────────────────────────────────────────────────┐
│             cuda_core::PinnedHostMapping                 │
│  - host_ptr: NonNull<u8>                                 │
│  - dev_ptr: CUdeviceptr (from cuMemHostGetDevicePointer) │
│  - len_bytes: usize                                      │
│  - ctx: Arc<CudaContext>                                 │
└───────────────┬──────────────────────────────────────────┘
                │ impl DeviceAllocation
                ▼
┌──────────────────────────────────────────────────────────┐
│             cuda_async::DeviceBuffer                     │
│  - Owner::Foreign(Arc<dyn DeviceAllocation>)             │
└───────────────┬──────────────────────────────────────────┘
                │ cutile::Tensor::<u8>::from_foreign
                ▼
┌──────────────────────────────────────────────────────────┐
│             cutile GPU Tile Kernels                      │
│  - Tile<u8, 4096>                                        │
│  - reduce_xor / reduce_and / reduce_or                   │
│  - Bitwise page transformations (compression / hashing)   │
└──────────────────────────────────────────────────────────┘
```

---

## 8. Dependencies and Risks

- **Dependencies**:
  - `NVlabs/cutile-rs` (forked in `scratch/cutile-rs/`).
  - NVIDIA Driver $\ge 550.0$ with Unified Virtual Addressing.
  - Rust 1.89+ stable.
- **Risks**:
  - Pinned memory exhaustion: Registering too much host memory locks physical RAM, causing host OS starvation.
  - *Mitigation*: Enforce a strict buffer pool limit and check available system memory before registration.
  - Unaligned pointers passed from userspace.
  - *Mitigation*: Enforce `host_ptr as usize % page_size == 0` in `PinnedHostMapping::register`.
- **Numeric Rollback Trigger**:
  - Any registration failure with `CUDA_ERROR_OUT_OF_MEMORY` or `CUDA_ERROR_HOST_MEMORY_ALREADY_REGISTERED` triggers immediate fallback to standard staged DMA transfer (`cuMemcpyHtoDAsync`).

---

## 9. Implementation Strategy

1. **Slice 1 (`cuda-core`)**: Add `PinnedHostMapping` to `cuda-core/src/simt/pinned_host_buffer.rs` or dedicated module, implementing `cuMemHostRegister` / `cuMemHostUnregister`.
2. **Slice 2 (`cuda-async`)**: Implement `DeviceAllocation` for `PinnedHostMapping`. Add tests verifying lifetime tokens and device pointer stability.
3. **Slice 3 (`cutile-compiler` & `cutile`)**: Add `reduce_xor`, `reduce_and`, `reduce_or` intrinsics in compiler and public `cutile::core` API.
4. **Slice 4 (`ramshared-cuda` downstream)**: Integrate zero-copy streaming into `ramshared-cuda` page transfer benchmarks.

---

## 10. Documents to Update

- `docs/architecture/CUDA-RUST-ACCELERATION-BLUEPRINT.md`
- `docs/specs/no-milestone/cutile-zero-copy-page-tiering/PRD.md` (this file)
- `docs/specs/no-milestone/cutile-zero-copy-page-tiering/SPEC.md`
- `docs/specs/no-milestone/cutile-zero-copy-page-tiering/IMPL.md`

---

## 11. Out of Scope

- Modifying the proprietary NVIDIA CUDA Driver (`libcuda.so.1`).
- Custom hardware FPGA / CXL fabric controllers.
- Multi-GPU NVLink zero-copy mesh routing (single-node PCIe topology is prioritized).

---

## 12. Acceptance Criteria

1. `PinnedHostMapping` successfully registers an allocated host slice, obtains a valid device pointer, and unregisters upon drop.
2. `cutile-compiler` successfully compiles a kernel using `reduce_xor` on a `Tile<u8, 4096>` without compile or JIT errors.
3. Host memory can be read by a GPU kernel directly via zero-copy with verified numeric integrity.
4. All new unit tests pass in `cuda-core`, `cuda-async`, and `cutile`.

---

## 13. Validation Plan

- **Unit**:
  - `cargo test -p cuda-core test_pinned_host_mapping_lifecycle`
  - `cargo test -p cuda-async test_pinned_host_mapping_device_allocation`
  - `cargo test -p cutile-compiler test_compile_reduce_bitwise`
- **GPU Integration**:
  - Execution of 4KB zero-copy kernel with verified parity reduction on local GPU or simulated test harness.
- **Negative / Abuse Cases**:
  - Registering misaligned host pointer returns error.
  - Zero-length slice registration returns error.
  - Double unregister prevention verified by RAII drop design.
