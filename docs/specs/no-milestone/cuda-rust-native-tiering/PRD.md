---
slug: cuda-rust-native-tiering
title: Native CUDA-Rust acceleration, in-GPU page compression, and async cancellation
milestone: —
issues: []
---

# PRD - Native CUDA-Rust Acceleration, In-GPU Page Compression, and Async Cancellation

## 1. Summary

RamShared's working GPU backend (`crates/ramshared-cuda`) loads the CUDA Driver API dynamically (`libcuda.so.1` / `nvcuda.dll`) and transfers uncompressed bytes. `cuda-core` and `cuda-async` are declared as optional dependencies behind the `cuda-rust` feature, but production code does not use them. Neither `cutile` nor `cuda-oxide` is a RamShared dependency. This document is a proposal, not a description of an installed acceleration path.

The proposed investigation has three independently gated parts:
1. **CUDA binding evaluation**: Compare the existing lifetime-checked Driver API wrapper with `cuda-core` and `cuda-async` before replacing a proven path. A Rust Future cancellation signal does not itself interrupt an in-flight CUDA or `/dev/dxg` call.
2. **In-GPU page compression feasibility**: Measure end-to-end transfer, launch, compression, metadata, decompression, and fallback cost. No capacity multiplier or per-page latency is currently qualified.
3. **Dual-Track Hardware Architecture**:
   - **Track A (`cuda-oxide` SIMT)**: Candidate for Turing (`sm_75`), subject to toolchain, kernel, and host validation.
   - **Track B (`cutile-rs` Tile IR)**: Candidate for `sm_80+` only, subject to CUDA toolkit and GPU validation. Upstream Tile benchmarks are not RamShared swap benchmarks.

---

## 2. Technical Context & Topology

### 2.1 Hardware Topology & Compute Capabilities
- **Local Host Workstation**: NVIDIA GeForce RTX 2060 with 6,144 MB VRAM, Compute Capability **`sm_75` (Turing)**.
- **Modern Datacenter Targets**: NVIDIA A100 (`sm_80`), H100 (`sm_90`), and B200 (`sm_100+`).
- **Hardware Constraint (Audit Finding)**: `cutile-rs` requires `sm_80+`; the local RTX 2060 is `sm_75` and cannot execute Tile kernels. `cuda-oxide` is a candidate for `sm_75`, not an implemented RamShared acceleration path. The local host also lacks `nvcc`; CUDA toolkit and compatible hardware are required for Tile validation elsewhere.

### 2.2 Codebase Anchors
- **Confirmed in codebase (`crates/ramshared-cuda/src/lib.rs`)**: Uses manual `loader_unix` and `loader_win` to resolve `cuMemAlloc_v2`, `cuMemcpyHtoD_v2`, and `cuMemcpyDtoH_v2`.
- **Confirmed in codebase (`crates/ramshared-vram/src/lib.rs`)**: Defines `VramProvider` and `VramMemory` traits that abstract memory allocation but lack asynchronous cancellation or compute dispatch.
- **Unverified hypothesis**: A GPU compressor may improve effective capacity for compressible workloads, but a 4KB transfer and kernel launch may dominate useful work. Random or already compressed pages must be measured separately and stored raw when compression is not beneficial.

---

## 3. Recommended Option

Adopt an **Adaptive Dual-Track CUDA-Rust Architecture**:

1. **Adopt `cuda-core` and `cuda-async`**:
   - Prototype the optional dependencies in an isolated path and compare ownership, context affinity, binary size, failures, and performance with the existing RAII wrapper.
   - Define a bounded admission/queueing policy and test what can actually be cancelled; keep an in-flight buffer and context alive until the driver reports completion.

2. **Develop In-VRAM GPU Page Compression**:
   - Author a pure Rust page-compression kernel.
   - For `sm_75` workstations: compile via `cuda-oxide` and embed the PTX artifact.
   - For `sm_80+` systems: author tile-based kernels using `#[cutile::module]` on stable Rust.

### Discarded Alternatives
- **Continue with the existing Driver API wrapper**: Retained as the working baseline and fallback. Its Rust ownership checks do not eliminate all FFI risk, but replacing it is not a prerequisite for GPU compute.
- **Force `cutile-rs` on `sm_75`**: Rejected because current Tile support starts at `sm_80`.

---

## 4. Functional Requirements (RF-N)

| ID | Description | Verifiable Acceptance |
| :--- | :--- | :--- |
| **RF-1** | **`cuda-core` Evaluation** | An isolated backend passes the same allocation, transfer, lifetime, and failure tests as the existing wrapper before migration is considered. |
| **RF-2** | **Bounded Asynchronous Work** | Queue admission, timeout reporting, in-flight ownership, and driver completion are measured separately; a cancelled Future must not free DMA memory prematurely. |
| **RF-3** | **Optional GPU Compression** | Round-trip integrity, incompressible fallback, capacity, throughput, and tail latency are measured on named workloads and hardware before enabling it for swap. |
| **RF-4** | **Architecture Detection & Fallback** | An unsupported GPU/toolkit or failed kernel initialization leaves the current uncompressed CUDA path available without data loss. |

---

## 5. Non-Functional Requirements (NFR-N)

| ID | Category | Target Metric |
| :--- | :--- | :--- |
| **NFR-1** | **Capacity** | Report physical bytes, logical bytes, metadata, and ratio by workload; do not assert a universal ratio. |
| **NFR-2** | **Latency** | Report end-to-end p50/p95/p99 and tail stalls against the current uncompressed CUDA path. |
| **NFR-3** | **Host Safety** | Exercise pressure, timeouts, failed allocation, driver reset, and swapoff-first recovery; never equate Future cancellation with driver-level abort. |

---

## 6. Execution Flows

### 6.1 Compressed Swap Write Flow
1. Linux kernel sends 4KB dirty swap page to `ramsharedd` via NBD or in-tree driver.
2. Daemon stages page into pinned host transfer buffer.
3. Asynchronous DMA transfers page to GPU global memory.
4. If a supported, qualified kernel is enabled, it attempts compression; incompressible or failed pages use the raw representation.
5. A crash-consistent block map records representation, offset, length, and integrity metadata.
6. The host acknowledges the write only after the selected storage path has completed according to its durability contract.

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
│ Compressed or Raw Chunk │ (size depends on page content)
└─────────────────────────┘
```

---

## 8. Dependencies and Risks

- **Dependencies for an eventual Tile path**: compatible `sm_80+` GPU and the toolkit version supported by the chosen `cutile-rs` revision; currently absent on the local `sm_75` host. Optional `cuda-core`/`cuda-async` manifest entries alone do not provide a runtime backend.
- **Risks**: nightly compiler/build reproducibility for `cuda-oxide`, GPU memory lifetime across cancellation, crash consistency of variable-sized swap data, and shared-GPU pressure.
- **Mitigation**: retain the current uncompressed path, keep new kernels opt-in until live and recovery gates pass, and preserve exact artifact provenance for any precompiled kernels.

---

## 9. Documents to Update

- `docs/architecture/CUDA-RUST-ACCELERATION-BLUEPRINT.md`
- `docs/specs/no-milestone/cuda-rust-native-tiering/PRD.md` (This document)
- `docs/specs/no-milestone/cuda-rust-native-tiering/SPEC.md`
- `ARCHITECTURE.md`

---

## 10. Acceptance Criteria

1. The optional backend passes equivalent CUDA lifetime and transfer tests; the current backend remains available on failure.
2. Simulated cancellation tests prove that buffers remain alive until actual completion; live tests characterize driver behavior and bounded pressure without promising an ioctl deadline.
3. GPU compression passes round-trip, incompressible-page, crash/restart, and swapoff-first tests with named hardware and workload evidence. Tile tests run on `sm_80+`, not on the local `sm_75` host.
