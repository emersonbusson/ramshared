# SPEC - Native CUDA-Rust Acceleration, In-GPU Page Compression, and Async Cancellation

## 1. Closed Scope

### In Scope for Investigation (Not Implemented)
- Evaluate the declared-but-unused optional `cuda-core` and `cuda-async` dependencies against the working Driver API wrapper.
- Design bounded asynchronous work with explicit GPU completion and buffer lifetime, without assuming that dropping a Future cancels driver work.
- Prototype `cuda-oxide` on `sm_75` and `cutile-rs` Tile kernels only on `sm_80+` hardware with a compatible toolkit.
- Qualify optional page compression with end-to-end latency, integrity, incompressible fallback, and recovery evidence.

### Out Now
- Kernel-space LKM changes (userspace daemon and GPU runtime scope).
- Modification of Windows display driver internals.

### Available Baseline and Missing Gates
- `crates/ramshared-cuda` and `crates/ramshared-vram` provide the existing uncompressed CUDA path.
- `cuda-core` and `cuda-async` are optional manifest entries, not wired into runtime code; `cutile` and `cuda-oxide` are not RamShared dependencies.
- The local RTX 2060 is `sm_75`, so it cannot execute `cutile` Tile kernels; no `nvcc` is installed locally. Tile validation requires a separate `sm_80+` host and supported CUDA toolkit.

---

## 2. Traceability

| PRD Requirement | Implementation / Decision Item | Covered By Test / Evidence |
| :--- | :--- | :--- |
| **RF-1** (`cuda-core` Context) | `ITEM-1`, `DT-1` | `test_cuda_core_context_lifecycle` |
| **RF-2** (Async Cancellation) | `ITEM-2`, `DT-2` | `test_async_dma_cancellation_token` |
| **RF-3** (In-GPU Compression) | `ITEM-3`, `DT-3` | `test_in_gpu_page_compression_roundtrip` |
| **RF-4** (Architecture Detection) | `ITEM-4`, `DT-4` | `test_gpu_compute_capability_dispatch` |
| **NFR-1** (Capacity) | `ITEM-3` | Physical/logical byte accounting by named workload |
| **NFR-2** (Latency) | `ITEM-3` | End-to-end p50/p95/p99 against uncompressed baseline |
| **NFR-3** (Host Safety) | `ITEM-2` | Pressure, timeout, completion, and swapoff-first recovery evidence |

---

## 3. Technical Decisions

| # | Decision | Why |
| :--- | :--- | :--- |
| **DT-1** | **Compare backends before migration**: Prototype `cuda-core`/`cuda-async` behind an opt-in feature while preserving the existing Driver API path. | Manifest presence is not implementation; safe wrappers still require validated context, stream, and buffer lifetimes. |
| **DT-2** | **Separate cancellation from completion**: A token may stop new work or report timeout; in-flight DMA retains its buffers until CUDA completion is observed. | Dropping a Future cannot guarantee abort of a foreign driver call or prevent a host stall. |
| **DT-3** | **Hardware-gated kernel experiments**: Test `cuda-oxide` artifacts on `sm_75`; test `cutile-rs` Tile IR on `sm_80+` with a supported toolkit. | The local RTX 2060 cannot validate Tile execution. Neither compiler is integrated into RamShared today. |
| **DT-4** | **Crash-consistent representation**: Model raw/compressed slot metadata, checksums, allocation bounds, and recovery before changing block mappings. | Variable-sized chunks add fragmentation and durability risks; no compression ratio is assumed. |

---

## 4. Atomicity and Rollback

- **Required atomicity proof**: Define the exact point at which a block-map entry changes from raw to compressed, ensure the old representation remains readable until the new one is complete, and test interrupted writes/restart. CRC32 alone does not prove correct ordering or durability.
- **Required lifetime proof**: On timeout, retain context, pinned host memory, and device buffers until the driver reports completion or a qualified teardown path succeeds.
- **Rollback**: Preserve an opt-in feature and the existing uncompressed CUDA backend; no kernel-space change is proposed here. Host rollback still requires swapoff-first, artifact identity, and recovery checks.

---

## 5. Kahneman Map (Critical Steps)

| ITEM / Stage | # | Question | Min Evidence | Abort |
| :--- | :--- | :--- | :--- | :--- |
| **ITEM-1** (Core) | **#13** (Refusal + Legitimate) | Does the optional backend refuse unsupported systems and match existing transfer/lifetime behavior? | Named unit tests plus a live CUDA probe after implementation | Panic, resource leak, or fallback regression |
| **ITEM-2** (Cancellation) | **#15** (Transient Retry / Failover) | Are queued and in-flight operations distinguished under timeout and driver stalls? | Deterministic lifetime tests plus live pressure trace | Premature free, lost completion, or unbounded queue growth |
| **ITEM-3** (Compression) | **#17** (Idempotency & Integrity) | Do raw/compressed pages survive random writes, restart, and swapoff-first? | Named GPU tests on supported hardware and recovery evidence | Any byte mismatch or unreadable block |

---

## 6. Security Checklist (Pre-Impl; Open Until Verified)

- [ ] **Privilege and platform**: Validate Linux/WSL2/Windows device access independently.
- [ ] **Copy bounds**: Fuzz offsets, lengths, alignment, and allocation failure for both backends.
- [ ] **Driver errors**: Propagate exact CUDA errors and refuse unsupported device/toolkit combinations.
- [ ] **Information flow**: Prove that raw/compressed buffers and metadata do not expose stale bytes.
- [ ] **Lifetime**: Prove context affinity and in-flight DMA ownership across timeout/drop.
- [ ] **Shared-hardware reserve**: Respect each production policy rather than one universal 2 GiB floor: broker/NBD `max(1536 MiB, 20%)` plus a separate 768 MiB runtime-free buffer; origin cache `max(2 GiB, 20%)`; StorPort `max(configuration, 512 MiB, 10%)`.
- [ ] **Recovery**: Test interrupted writes, GPU reset, swapoff-first, and rollback before host installation.

---

## 7. Files to CREATE / MODIFY / DELETE

### CREATE
**`crates/ramshared-cuda/src/async_backend.rs`** (proposed)
- **Purpose**: Bounded GPU I/O operations with explicit completion and ownership.
- **Required Tests**: Queue refusal, cancellation-before-submit, timeout-while-in-flight, and delayed completion.

### MODIFY
**`crates/ramshared-cuda/Cargo.toml`**
- **Purpose**: Keep optional dependencies isolated until a backend is implemented and validated; entries already exist.

---

## 8. Observability

| Signal | Where | Level / Type |
| :--- | :--- | :--- |
| `gpu_compression_ratio` | `telemetry.jsonl` | INFO / Float metric |
| `gpu_async_cancellation` | `stderr` + `telemetry.jsonl` | WARN / Structured JSON |

---

## 9. Implementation Order

- **ITEM-1**: Test an isolated `cuda-core`/`cuda-async` backend against the existing Driver API behavior, including negative paths; do not switch production by manifest change alone.
- **ITEM-2**: Specify queue bounds, timeout semantics, completion observation, context affinity, and in-flight buffer ownership; then implement and test them.
- **ITEM-3**: Prototype optional compression with raw fallback, crash-consistent mapping, and integrity tests. Validate `cuda-oxide` on `sm_75` and `cutile` only on `sm_80+`.
- **ITEM-4**: Wire the qualified backend into the broker with telemetry and a reversible feature gate; perform live pressure, swapoff-first, and binary-match checks before any host replacement.

---

## 10. Required Tests Matrix

| Production Path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| :--- | :--- | :--- | :--- | :--- |
| Proposed context backend | `test_cuda_core_context_lifecycle` (to create) | unit + live | #13 | ≥80% after implementation |
| Proposed async backend | `test_async_dma_cancellation_token` (to create) | unit + live | #15 | ≥80% after implementation |
| Proposed GPU compression | `test_in_gpu_page_compression_roundtrip` (to create) | GPU + recovery | #17 | ≥80% after implementation |
| Proposed architecture dispatch | `test_gpu_compute_capability_dispatch` (to create) | unit + GPU | #13 | ≥80% after implementation |

---

## 11. Validation Checklist

- [ ] `cargo fmt` / `cargo clippy -p ramshared-cuda -- -D warnings` / `cargo test -p ramshared-cuda`
- [ ] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cuda --files crates/ramshared-cuda/src/async_backend.rs --min 80`
- [ ] Live path for each supported backend (`sm_75` existing CUDA; `sm_80+` Tile on a separate host)
- [ ] Every matrix row has an implemented test, not only a proposed name
- [ ] Kahneman critical rows have executable evidence
