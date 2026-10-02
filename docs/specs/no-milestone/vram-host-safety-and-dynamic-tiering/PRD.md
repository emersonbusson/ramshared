---
slug: vram-host-safety-and-dynamic-tiering
title: Adapter-bound VRAM cache safety and fallback contract
milestone: —
issues: []
---

# PRD — Adapter-Bound VRAM Cache Safety and Fallback Contract

## 1. Summary

RamShared may use GPU memory as a revocable cache only when the cache is backed
by an authoritative origin. The origin path must remain correct when the GPU
worker is unavailable, stale, mismatched, or unresponsive.

The production NBD path starts a separate cache worker and uses bounded IPC;
cache failures fall back to the authoritative origin. The legacy direct GPU
`--slices` broker makes synchronous driver calls in the NBD process, so the
argument planner now refuses it before any backend or device initialization.
GPU use through the separate `ublk` path is outside this contract and remains
subject to its own platform qualification.

This contract establishes source-level budget and fallback behavior. It does
not claim a hard deadline for a GPU driver call, a freeze-free result, or
universal vendor support. Hardware-backed qualification remains required.

## 2. Technical context

### 2.1 Codebase facts

- `crates/ramshared-wsl2d/src/main.rs` selects the single-NBD origin path and
  starts the isolated GPU worker.
- `crates/ramshared-block/src/isolated_origin.rs` keeps origin reads/writes
  authoritative and makes cache reads best effort.
- `crates/ramshared-block/src/ipc_cache_client.rs` bounds worker IPC waits and
  revokes a cache client after a transport failure.
- `crates/ramshared-block/src/gpu_cache_worker.rs` owns sparse chunk allocation
  and rechecks the active provider's budget before allocation.
- `crates/ramshared-wsl2d/src/gpu_budget.rs` validates budget freshness,
  identity, WDDM correlation, adapter selection, and target sizing.
- `crates/ramshared-vram/src/lib.rs` defines the shared adapter-bound budget
  contract. CUDA, Vulkan, and DXG provide backend-specific reports.
- `select_daemon_action` rejects GPU-backed `--slices` before the production
  runner can load CUDA/Vulkan or create broker devices. The RAM-only broker
  remains available for test/control-plane use.

### 2.2 Failure boundary

A synchronous call into a GPU driver cannot be interrupted by measuring its
elapsed time after it returns. The origin path isolates those calls in a child
process; a timed-out cache request falls through to the origin, so the NBD data
path does not wait for that GPU result. The operating system may still retain a
stuck worker if the driver call is in uninterruptible kernel sleep. Source tests
do not prove physical driver behavior or zero host freezes.

## 3. Recommended option

Use one supported product path for GPU caching: NBD plus an authoritative
origin and a process-isolated, revocable cache worker. Require a fresh,
driver-reported budget bound to an adapter identity. Where a fresh WDDM sample
has the exact same LUID, constrain admission by the lower available budget.
Reject invalid, future, stale, estimated, or mismatched data. If the cache
fails, serve data from the origin.

Refuse direct GPU `--slices` broker actions at argument selection. Keep any
standalone `ublk` GPU use out of the product claim until its independent
lifecycle and hardware gates pass.

## 4. Functional requirements

| ID | Requirement | Verifiable acceptance |
| --- | --- | --- |
| RF-1 | GPU admission uses a fresh, identified, driver-reported budget and preserves both the capacity reserve and current-use reserve. | Named unit tests verify valid admission and refusal for stale, future, malformed, mismatched, and estimated samples. |
| RF-2 | The origin remains authoritative; an unavailable, timed-out, or failed cache request cannot replace the origin result. | `cache_timeout_falls_back_to_origin` and worker-kill integration test pass. |
| RF-3 | Adapter selection is deterministic and the opened CUDA/Vulkan adapter matches the candidate whose budget was measured. | Candidate ranking, exact-ordinal reopen, and identity revalidation tests pass. |
| RF-4 | GPU-backed direct `--slices` broker requests are refused before provider or device initialization. | `daemon_gpu_legacy_broker_refuses_before_backend_initialization` passes for `auto`, `vram`, and `vulkan`; RAM broker remains accepted. |
| RF-5 | Status surfaces identify the active adapter and publish only internally consistent, fresh budget samples. | Budget telemetry and dashboard unit tests pass. |

The reserve policy is surface-specific. For the isolated origin cache, the
safe target is bounded by the request, `capacity - reserve`, and
`live_available - reserve - 640 MiB`; each chunk admission rechecks live
headroom and the runtime buffer. The legacy direct broker policy is not a
supported product path; its planner refusal is the safety boundary.

## 5. Non-functional requirements

| ID | Category | Target |
| --- | --- | --- |
| NFR-1 | Correctness | GPU cache is disposable; acknowledged origin data remains authoritative. |
| NFR-2 | Host safety | No claim of bounded driver-call latency or zero-freeze behavior without physical evidence. GPU allocations remain opt-in through the origin-backed path. |
| NFR-3 | Observability | Publish adapter identity, budget, usage, available bytes, backend, and freshness only when validation succeeds. |
| NFR-4 | Reversibility | Cache disable/revoke preserves origin operation; teardown evidence must prove cache release before declaring it clean. |
| NFR-5 | Portability | Unknown or absent budget/identity information selects origin-only behavior; vendor presence alone does not qualify a backend. |

## 6. Flows

### 6.1 Origin-backed cache admission

1. Parse and validate the origin manifest before driver or swap effects.
2. Enumerate CUDA and Vulkan candidates. Read each candidate's budget and stable
   identity; correlate WDDM only through an exact LUID.
3. Compute safe targets using current capacity and live headroom. Open the exact
   winning adapter and revalidate its identity and budget.
4. Start the worker with a bounded IPC contract. Cache chunks are allocated
   lazily and rechecked against current headroom.
5. Read misses and cache failures are served from the origin. Origin writes are
   completed according to the authoritative write-through contract before
   acknowledgement.

### 6.2 Refusal and fallback

- Missing, stale, future, malformed, estimated, or mismatched budget: do not
  admit GPU allocations; keep or enter origin-only mode.
- GPU-backed `--slices`: return a planner error before opening a GPU provider,
  binding a broker socket, or touching a block device.
- Cache IPC timeout, disconnect, invalid response, or worker failure: revoke
  that cache client and serve through the origin.
- Driver call stuck inside the worker: parent data requests can use the origin;
  process termination and physical driver recovery remain hardware-bound
  qualification items.

## 7. Data and state model

`GpuBudgetSnapshot` is a monotonic-time sample with adapter identity, optional
physical total, budget, usage, source, and sample time. `GpuBudgetTelemetry` is
its wall-clock IPC/status form and is trusted only when its calculated
availability matches the published value and its age is within policy.

The cache worker moves through `Unavailable → Active → Off` or `Stuck` during
revocation. The authoritative origin has an independent state and remains the
source of truth through all cache states.

## 8. Interfaces

- Product NBD: `--origin-manifest`; GPU cache is optional and revocable.
- Legacy broker: `--slices` with `--backend ram` remains available. GPU-backed
  `auto`, `vram`, and `vulkan` variants are refused by the planner.
- GPU status: adapter key/LUID, backend, budget, use, available bytes, and sample
  time. Missing trusted data is omitted; it is never converted into a free-memory
  estimate.
- `ublk` is a separate transport and is not qualified by this PRD.

## 9. Dependencies and risks

- CUDA, Vulkan, and DXG drivers may expose different budget scopes; cross-API
  correlation requires the same physical adapter LUID.
- A userspace timeout cannot cancel a driver call already blocked in kernel
  context. Process isolation protects the origin data path but does not prove
  that the child exits or that the host driver recovers.
- WSL2 host memory, swap, and GPU pressure campaigns require a fresh admission
  sample and supervised isolated validation. No source test authorizes a live
  pressure run.

**Rollback trigger:** If a valid product-path cache request delays an origin
reply beyond its configured IPC timeout, or a rejected direct GPU broker reaches
provider initialization, revert the responsible daemon change. Any origin data
mismatch is an immediate stop condition.

## 10. Implementation strategy

1. Centralize budget validation and reserve arithmetic in the shared VRAM
   contract.
2. Revalidate the selected adapter and WDDM intersection before worker startup
   and before every allocation.
3. Keep origin correctness independent from the isolated cache and refuse the
   synchronous direct GPU broker before side effects.
4. Add unit and integration tests for valid admission, boundary refusal,
   timeout fallback, worker loss, and exact adapter identity.
5. Keep live NVIDIA/AMD/Intel and WSL2 host qualification open until evidence
   comes from physical hardware.

## 11. Documents to update

- `docs/specs/no-milestone/vram-host-safety-and-dynamic-tiering/SPEC.md`
- `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md`
- `docs/reliability/GAP-REGISTER.md`
- `ARCHITECTURE.md`

## 12. Out of scope

- Kernel-space `mm/swapfile.c`, DRM, or DMA driver changes.
- Windows StorPort driver behavior and its physical qualification.
- `ublk` GPU lifecycle qualification.
- A claim of zero freeze, universal GPU support, or a fixed throughput/latency
  across hardware.

## 13. Acceptance criteria

1. Named Rust tests cover budget arithmetic, adapter identity, freshness, and
   origin fallback.
2. The daemon planner refuses GPU-backed `--slices` before any backend effect.
3. Formatting, package tests, strict Clippy, and the slice coverage gate pass.
4. Live worker allocation, physical multi-adapter selection, 24-hour rollout,
   host-pressure behavior, and vendor comparison remain explicitly PARTIAL
   until independently evidenced.

## 14. Validation plan

- Unit/integration: `cargo test -p ramshared-vram -p ramshared-block -p ramshared-wsl2d`.
- Static: `cargo fmt --all -- --check`, strict Clippy for affected crates, and
  `node tools/ci/check-rust-slice-coverage.mjs` for changed business logic.
- Live qualification: exact installed binary, worker allocation, origin
  fallback, teardown, host telemetry, and pressure evidence on NVIDIA, AMD, and
  Intel. A software Vulkan device is not physical GPU evidence.
