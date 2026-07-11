# PRD — WSL2-native VRAM autotier

## 1. Summary

**Confirmed in codebase:** RamShared exposes sparse CUDA memory through NBD and orders zram (200), VRAM (100), then disk. **Confirmed in official sources:** WSL2 delegates GPU memory ownership to Windows GPU-PV/WDDM, while `/dev/dxg` forwards the per-process video-memory budget. The product must use that budget as policy authority without claiming guest-owned GPU PFNs.

## 2. Technical context

- **Confirmed in codebase:** `SparseVramBackend` commits 128 MiB chunks and currently gates them with CUDA `mem_info()` plus a static cap.
- **Confirmed in codebase:** teardown is swapoff-first; ublk remains blocked after measured WSL teardown hangs.
- **Confirmed in official sources:** `LX_DXENUMADAPTERS2`, `LX_DXQUERYVIDEOMEMORYINFO`, and `LX_DXCLOSEADAPTER` exist in the WSL kernel uAPI.
- **Inference:** polling is required until GPU-PV offers budget-pressure notifications.

## 3. Recommended option

Phase 1 adds a vendor-neutral `GpuBudgetProvider`, uses WDDM local-segment budget as the primary commit gate, retains CUDA as allocation/copy and fail-closed sanity signal, and keeps NBD. Phase 2 evaluates zram writeback. Phase 3 is an external Microsoft/Windows design proposal.

## 4. Functional requirements

- **RF-1:** enumerate `/dev/dxg` adapters and select the only adapter automatically.
- **RF-2:** reject multiple adapters unless an explicit stable selection is supplied; never guess CUDA↔LUID identity.
- **RF-3:** expose budget, usage, reservation, available reservation, adapter LUID, source, and timestamp.
- **RF-4:** compute `usable_budget = budget - external_usage - safety_margin`, saturating at zero.
- **RF-5:** stop new chunk commits immediately when the next commit exceeds usable budget.
- **RF-6:** CUDA free memory remains a secondary fail-closed check.
- **RF-7:** when dxg is unavailable, use the existing CUDA free-floor and report `cuda-fallback` explicitly.
- **RF-8:** implement `available`, `constrained`, `demoting`, `parked`, and `recovering` with hysteresis.
- **RF-9:** demote only after absorption preflight and successful bounded swapoff; release chunks only after `used_kb == 0`.
- **RF-10:** recover by enabling an empty VRAM tier; do not eagerly copy disk pages.
- **RF-11:** preserve priorities 200/100/disk and NBD transport.

## 5. Non-functional requirements

- **NFR-1:** malformed, stale, removed-adapter, and ioctl-error samples fail closed.
- **NFR-2:** no backend teardown while live swap references remain.
- **NFR-3:** ioctl layouts are `repr(C)` and checked against official sizes/constants.
- **NFR-4:** minimum 80% line coverage for new pure policy code.
- **NFR-5:** no real swap pressure on the live WSL host.

## 6. Flows

Startup enumerates adapters, rejects ambiguity, captures logical capacity once, and starts empty. Each prospective chunk commit obtains a fresh budget and CUDA-free sample. Sustained constraint requests demote; recovery waits for sustained headroom and re-enables only an empty device.

## 7. Data model

`BudgetSnapshot` contains adapter LUID, WDDM counters, CUDA committed bytes, source, and monotonic sample time. `AutotierStatus` adds logical capacity, usable budget, state, reason, transition count, and demote/recovery durations.

## 8. API / interfaces

Internal trait `GpuBudgetProvider::snapshot()`. Phase 1 adds no stable kernel ABI. A future prototype may expose read-only `state`, `budget_bytes`, `committed_bytes`, and `reclaim_count` sysfs files only after a host contract exists.

## 9. Dependencies and risks

Dxg handles are per-open/process. Multi-adapter identity is unresolved. Polling may observe stale data. Swapoff may be slow or fail when lower-tier absorption is insufficient. Phase 3 requires Microsoft/Windows changes.

## 10. Implementation strategy

Isolate dxg `unsafe` in one crate, test layouts first, implement pure policy next, wire the sparse allocation gate, then add daemon lifecycle transitions. Later phases revise the single `SPEC.md` in place.

## 11. Documents to update

`ARCHITECTURE.md`, `docs/INDEX.md`, reliability degradation matrix, this SSDV3 pack, and append-only validation when hardware evidence exists.

## 12. Out of scope

Fake NUMA, HMM `DEVICE_PRIVATE`, guest-owned GPU PFNs, ublk production enablement, eager promotion, vendor-specific policy knobs, and claims that Phase 3 is locally complete.

## 13. Acceptance criteria

RF-1..RF-11 have automated tests or an explicit environment-bound gate; ambiguity and stale/error samples fail closed; zero corruption, ghost swap, Oops, or forced daemon termination.

## 14. Validation

Unit tests cover ioctl layout, arithmetic boundaries, state hysteresis, stale/error samples, idempotency, and `used_kb > 0`. Hardware validation requires NVIDIA/AMD/Intel where available and at least 3 runs reporting median and p99 for demote, bytes returned, first-write, and recovery.
