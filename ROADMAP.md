# Roadmap — RamShared

The active implementation target is **WSL2**; the ultimate destination is **Ring 0 bare-metal** (refer to [MANIFESTO.md](MANIFESTO.md)). Dates are omitted intentionally — this is an R&D project.

## Completed

*   **Evaluation of the 6 PRDs** + real environment (WSL2/GPU-PV): Only PRD-2 (block device + CUDA) is viable in the guest; the others require hardware features (DRM/BAR/DAMON) that are missing.
*   **Phase 0 (Real GPU Validation):** WDDM eviction is *data-safe, latency-unsafe* (4K reads can take up to 1.18 s under load); cascade order proved successful (zram saturation + VRAM absorbing 983 MiB of overflow).
*   **SPECv3-WSL2:** Convergence reached via Passo 2.5 (SPEC → SPECv2 → SPECv3): VRAM configured as a cold tier + DEMOTE logic.
*   **Rust Port:** 6 crates developed, and **validation of acceptance §14** successfully run on live system (spilling 511 MiB intact; DEMOTE migrating 481 MiB back, 0 corruption).
*   **Adversarial Hardening (Issue #3):** C3 (duplicate CUDA FFI removed, CLI `forbid(unsafe_code)`), M1/M2/M3/M4/M5 + name-buffer.

## Active — Issue #3 (Phase A, WSL2)

*   **C1 — Dedicated Canary (§9.4):** ✅ **Completed** — `ResidencySampler` with hysteresis (immediate on content-check; free-floor/transient errors require a streak). Reference: `docs/008-vram-residency-canary/`.
*   **H1 — Multi-Connection Daemon / Dedicated Reader:** ✅ **Completed** — Single CUDA worker thread + independent read/write handlers per connection (`nbd-client -C N`, `CAN_MULTI_CONN`); avoids head-of-line blocking. Reference: `docs/daemon-multiconn/`.
*   **LOW — Fixed:** Typed errors via `CascadeError` (zero-dependency, matching the `CudaError` pattern in `ramshared-cli`/`ramshared-tier`). **`clap` rejected** (violates the zero-dependency policy for Ring-0 adjacent binaries — decision recorded in [`docs/LIBRARIES.md`](docs/LIBRARIES.md)); the daemon (`ramshared-wsl2d`) keeps `Box<dyn Error>` at the binary boundary.

## Phase B — Custom Kernel (WSL2 + Custom WSL2 Kernel)

*   `CONFIG_ZRAM_WRITEBACK`: Writeback cold zram pages directly to VRAM (eliminates the userspace hop in the cold path).
*   `ublk` replacing NBD (reduces memory copies and context-switch overhead).

## Long-term Vision — Bare-metal (Gated on leaving WSL2)

Exploratory paths; require DRM/BAR/DAMON/CXL layers unavailable in the guest GPU-PV. Each has a dedicated PRD:

*   **NUMA node** mapping for VRAM ([`PRD`](docs/vram-as-ram/PRD.md), [`PRD-4`](docs/vram-as-ram/PRD-4.md) with DAMON/proactive tiering).
*   **zswap/zpool backend** inside VRAM via BAR access ([`PRD-3`](docs/vram-as-ram/PRD-3.md)).
*   **HMM `DEVICE_PRIVATE` + SDMA + eBPF** ([`PRD-6`](docs/vram-as-ram/PRD-6.md)).
*   **CXL / PCIe Gen5** — Coherent device memory as a native storage tier.

## Principles of Progress

*   Every structural feature goes through the **SSDV3** pipeline (PRD → SPEC → IMPL) and respects **Kahneman disciplines** (counterfactuals + numerical rollback triggers).
*   No memory block enters VRAM without latency evidence; **measure before coding** (Phase 0).
*   **Day-0 Policy:** Zero tolerance for shims; leaving WSL2 requires rewriting paths rather than stacking wrappers.
