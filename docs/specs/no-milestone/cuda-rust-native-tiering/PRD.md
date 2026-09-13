---
slug: cuda-rust-native-tiering
title: Native CUDA-Rust acceleration, in-GPU page compression, and async cancellation
milestone: —
issues: []
---

# PRD - Native CUDA-Rust Acceleration, In-GPU Page Compression, and Async Cancellation

## 1. Summary

Currently, RamShared's GPU backend (`crates/ramshared-cuda`) interacts with NVIDIA graphics hardware through raw, dynamic C Driver API bindings (`libcuda.so.1` / `nvcuda.dll`). While functional, this model limits the GPU to a passive, uncompressed DMA byte buffer and relies on synchronous blocking ioctls over `/dev/dxg` that can deadlock under memory pressure.

In September 2026, NVIDIA released the **CUDA-Rust** toolchain (`NVlabs/cutile-rs` on stable Rust 1.89+ and `NVlabs/cuda-oxide` on nightly rustc), enabling type-safe GPU kernel execution in pure Rust. This PRD establishes the long-term architectural transformation of RamShared's Tier 2 engine:
1. **Modernization of Core CUDA Bindings**: Migrate from raw C FFI to NVIDIA's idiomatic `cuda-core` and `cuda-async` crates, introducing Rust Future-based asynchronous dispatch with native cancellation tokens.
2. **In-GPU Page Compression**: Execute pure Rust compression kernels (LZ4 / bit-packing) directly on GPU compute cores at internal VRAM bandwidth (336 GB/s), increasing effective Tier 2 capacity by $2.5\times$ to $3\times$.
3. **Dual-Track Hardware Architecture**:
   - **Track A (`cuda-oxide` SIMT)**: Targets Turing architecture (`sm_75`, such as the workstation RTX 2060) and broader GPU generations via LLVM PTX generation.
   - **Track B (`cutile-rs` Tile IR)**: Targets Ampere, Hopper, and Blackwell architectures (`sm_80` to `sm_100+`) on stable Rust, utilizing hardware tensor tiles to achieve up to 7 TB/s memory throughput.

---

## 2. Technical Context & Topology

### 2.1 Hardware Topology & Compute Capabilities
- **Local Host Workstation**: NVIDIA GeForce RTX 2060 with 6,144 MB VRAM, Compute Capability **`sm_75` (Turing)**.
- **Modern Datacenter Targets**: NVIDIA A100 (`sm_80`), H100 (`sm_90`), and B200 (`sm_100+`).
- **Hardware Constraint (Audit Finding)**: `cutile-rs` strictly requires compute capability `sm_80` or higher (`Architectures below sm_80 are out of scope`). Therefore, `cuda-oxide` (which compiles pure Rust SIMT to PTX via LLVM) serves as the primary acceleration path for `sm_75`, while `cutile-rs` provides state-of-the-art Tile acceleration for `sm_80+`.

### 2.2 Codebase Anchors
- **Confirmed in codebase (`crates/ramshared-cuda/src/lib.rs`)**: Uses manual `loader_unix` and `loader_win` to resolve `cuMemAlloc_v2`, `cuMemcpyHtoD_v2`, and `cuMemcpyDtoH_v2`.
- **Confirmed in codebase (`crates/ramshared-vram/src/lib.rs`)**: Defines `VramProvider` and `VramMemory` traits that abstract memory allocation but lack asynchronous cancellation or compute dispatch.
- **Inference**: By compiling a Rust LZ4 compressor into GPU PTX, RamShared can compress 4KB swap pages inside VRAM in $<2\,\mu\text{s}$, completely bypassing CPU compression overhead.

---

## 3. Recommended Option

Adopt an **Adaptive Dual-Track CUDA-Rust Architecture**:

1. **Adopt `cuda-core` and `cuda-async`**:
   - Refactor `crates/ramshared-cuda` to build on `cuda-core` (safe context and buffer management) and `cuda-async` (composable asynchronous GPU operations).
   - Wire cancellation tokens into all DMA operations to prevent thread lockups during GPU stalls.

2. **Develop In-VRAM GPU Page Compression**:
   - Author a pure Rust page-compression kernel.
   - For `sm_75` workstations: compile via `cuda-oxide` and embed the PTX artifact.
   - For `sm_80+` systems: author tile-based kernels using `#[cutile::module]` on stable Rust.

### Discarded Alternatives
- **Continue with Raw C Driver API**: Rejected. Lacks memory safety, prevents in-GPU kernel compute without an external C++ `nvcc` build step, and cannot cleanly cancel stalled ioctls.
- **Force `cutile-rs` on `sm_75`**: Impossible. NVIDIA explicitly confirmed `sm_70` and `sm_75` are permanently out of scope for CUDA Tile IR.

---

## 4. Functional Requirements (RF-N)

| ID | Description | Verifiable Acceptance |
| :--- | :--- | :--- |
| **RF-1** | **`cuda-core` Context Migration** | `crates/ramshared-cuda` initializes GPU contexts and allocates device buffers via `cuda-core`, removing raw unsafe FFI pointers. |
| **RF-2** | **Asynchronous Cancellation** | All GPU I/O operations return cancellable `DeviceOperation` futures. If a watchdog timeout occurs, the operation is aborted without blocking the caller thread. |
| **RF-3** | **In-GPU Pure Rust Page Compression** | Provide an optional GPU compression pass in `ramshared-cuda` that compresses 4KB pages on the GPU, achieving a compression ratio $\ge 1.8\times$ on standard memory workloads. |
| **RF-4** | **Architecture Detection & Fallback** | Runtime automatically detects GPU compute capability: selects `cutile-rs` on `sm_80+`, `cuda-oxide` on `sm_75`, or pure DMA if compute kernels are unavailable. |

---

## 5. Non-Functional Requirements (NFR-N)

| ID | Category | Target Metric |
| :--- | :--- | :--- |
| **NFR-1** | **Memory Amplification** | Effective VRAM capacity increased by $\ge 2.0\times$ under compressed swap mode. |
| **NFR-2** | **Kernel Execution Latency** | 4KB page compression latency on GPU $\le 5\,\mu\text{s}$ per page. |
| **NFR-3** | **Host Safety & Zero Freeze** | `PASS_ZERO_FREEZE`: Cancellable streams ensure no thread hangs in `dxgkrnl.sys` ioctls. |

---

## 6. Execution Flows

### 6.1 Compressed Swap Write Flow
1. Linux kernel sends 4KB dirty swap page to `ramsharedd` via NBD or in-tree driver.
2. Daemon stages page into pinned host transfer buffer.
3. Asynchronous DMA transfers page to GPU global memory.
4. Pure Rust compression kernel launches on GPU, compressing 4KB into $\le 2\text{ KB}$ chunk in VRAM.
5. Inode/block map records compressed offset and size.
6. Operation completes with sub-microsecond latency; host receives `NBD_OK`.

---

## 7. Data and State Model

```text
┌─────────────────────────┐
│ Host Swap Page (4 KB)   │
└────────────┬────────────┘
             │ Pinned DMA Transfer
             ▼
┌─────────────────────────┐
│ GPU Staging Buffer      │
└────────────┬────────────┘
             │ Launch In-GPU Rust Kernel (cuda-oxide / cutile-rs)
             ▼
┌─────────────────────────┐
│ Compressed Chunk in VRAM│ (e.g. 1.5 KB to 2.0 KB)
└─────────────────────────┘
```

---

## 8. Dependencies and Risks

- **Dependencies**: NVIDIA CUDA 13.x driver; `cuda-core` and `cuda-async` crates; `cargo-oxide` compiler for `sm_75` kernels.
- **Risks**: Nightly compiler requirement for `cuda-oxide` device kernels.
- **Mitigation**: Device kernels are pre-compiled into static PTX / cubin artifacts during release packaging; the host daemon runs on stable Rust.

---

## 9. Documents to Update

- `docs/architecture/CUDA-RUST-ACCELERATION-BLUEPRINT.md`
- `docs/specs/no-milestone/cuda-rust-native-tiering/PRD.md` (This document)
- `docs/specs/no-milestone/cuda-rust-native-tiering/SPEC.md`
- `ARCHITECTURE.md`

---

## 10. Acceptance Criteria

1. `crates/ramshared-cuda` compiles cleanly using `cuda-core` and `cuda-async`.
2. Asynchronous DMA operations support clean cancellation within 50ms upon simulated GPU stalls.
3. GPU compression test passes with verified round-trip page integrity (`original_page == decompress(compress(original_page))`).
