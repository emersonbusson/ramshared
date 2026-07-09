# Architecture — RamShared

Focus: The **WSL2** implementation (SPECv3), which is the only viable path on GPU-PV guest systems. Bare-metal targets (NUMA/HMM/CXL) are detailed in the [ROADMAP.md](ROADMAP.md).

## Overview

RamShared **orchestrates** a priority-ordered swap cascade and **manages the VRAM tier**; `zram` and `VHDX` are kernel-level mechanisms that RamShared configures rather than implements directly.

```text
Memory Pressure ─► zram  (Compressed RAM)  prio 200  HOT
                ─► VRAM  (CUDA + NBD daemon)  prio 100  COLD
                ─► VHDX  (WSL2 default swap)  prio  -2  LAST
```

## Safety Model (The Pivot)

Validation on real GPU hardware demonstrated that WDDM eviction is:

*   **data-safe** — the page checksum remains intact after eviction;
*   **latency-unsafe** — a 4KB read with VRAM fully occupied resulted in a **1.18 s** delay.

If configured as hot swap space, VRAM eviction would lock up the system. Operating as a **cold** tier behind `zram`, it only receives rarely accessed pages — hiding the latency. When a latency spike is detected (indicating the host OS is reclaiming VRAM), the **DEMOTE** thread runs `swapoff` on the VRAM tier, causing the kernel to migrate active pages back to `VHDX` **without interrupting running processes**.

**Invariant A1:** The demotion is only safe if there is a lower-priority tier active and ready to absorb the pages (verified at startup and checked in the `ramshared-tier` safety net).

## Components

| Crate | Responsibility | `unsafe` |
|---|---|---|
| `ramshared-tier` | Priority management (`zram 200 > vram 100 > vhdx -2`), `validate_order`, A1 safety net | `forbid` |
| `ramshared-cuda` | `libcuda` loading via `dlopen`/`LoadLibrary` (runtime, no toolkit/`build.rs`); RAII bindings (`Cuda`→`Context`→`DeviceMem`) | **Isolated here** |
| `ramshared-block` | NBD fixed-newstyle protocol: parsing/encoding, handshake state machine | `forbid` |
| `ramshared-integrity` | Checksum calculations (FNV-1a) + validation testing patterns | `forbid` |
| `ramshared-wsl2d` | Daemon: state machine, `VramBackend` (connects CUDA↔NBD), `mlockall`/`oom_score_adj`, canary/DEMOTE triggers | `mlockall` only |
| `ramshared-cli` | Command-line management: `check`/`doctor` (preflight) + `up`/`down`/`status` (orchestration) | `forbid` |

All `unsafe` blocks are isolated within `ramshared-cuda` (FFI) with `// SAFETY:` markers, except for the daemon's `mlockall` call which is documented and isolated. Minimal external dependencies (`std` + FFI/libc).

## Execution Flow

1.  **`up`:** Validates priority configuration and invariant A1, starts `zram`, spins up the daemon, and attaches `/dev/nbd0` using `nbd-client -unix`. The daemon does not call ioctl or use unsafe memory mapping for the device attachment — this is handled by `nbd-client`. Finally, `mkswap` and `swapon -p` mount the swap tiers.
2.  **Daemon:** Allocates and zeroes VRAM, locks userspace memory (`mlockall`), protects itself from the out-of-memory killer (`oom_score_adj=-1000`), and serves NBD requests: every READ/WRITE maps to a `cuMemcpyDtoH`/`HtoD` call on the VRAM memory range.
3.  **Inline Canary:** The daemon monitors I/O latency. Under a latency spike (latency > N× baseline across M samples), it spawns a thread to run `swapoff <nbd>` (**DEMOTE**), while continuing to serve read-backs. The demote trigger is only disarmed once `swapoff` completes successfully.
4.  **`down`:** Runs `swapoff` on the NBD device **before** disconnecting (avoiding kernel panics), tears down `zram`, and waits for the daemon to wipe VRAM clean before sending termination signals (`pkill`).

## Key Decisions

*   **NBD instead of ublk (Phase A):** `CONFIG_BLK_DEV_NBD=m` is standard on WSL2, whereas `ublk` would require compiling a custom kernel. This keeps the daemon free of custom kernel module requirements.
*   **Runtime Loader:** Loading `libcuda` (or `nvcuda.dll` on Windows) dynamically at runtime rather than link-time avoids build-time dependencies on the CUDA Toolkit.
*   **Priority Cascade via `swapon -p`:** Used instead of writebacks. Since `CONFIG_ZRAM_WRITEBACK` is not compiled in the baseline kernel, direct writebacks (zram evicting cold data directly to VRAM) are reserved for Phase B.
*   **Separation of Concerns:** Canary logic (`residency.rs`) and cascade logic (`tier`) are pure Rust and unit-tested without root or GPU access; only the daemon runner invokes CUDA and system `swapoff` commands.

## Methodology

*   **SSDV3:** Spec-Driven Development. PRD → SPEC → IMPL under `docs/specs/…`, with Passo 2.5 adversarial audits (go/no-go); SPEC revised in-place (git is history). Reference: [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md), [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md).
*   **Kahneman Disciplines (18):** Counterfactuals and numerical rollback (#2); calibrated retry (#15), fail-safe/independent curator (#16), replay idempotency (#17), right-layer root cause + proven sunset (#18). Reference: [`docs/methodology/kahneman-disciplines.md`](docs/methodology/kahneman-disciplines.md).
*   **Day-0 Policy:** Zero-tolerance for shims, temporary workarounds, or warning bypasses.

## Validation

System validation within cgroup-confined workloads: **511 MiB** spilled to VRAM (**332,800 intact pages**) and a **481 MiB** demotion path migrated VRAM -> VHDX under active pressure with **zero corruption**.

Log evidence: [`docs/reliability/wsl2-cascade-validation.md`](docs/reliability/wsl2-cascade-validation.md) · phase-0 summary: [`docs/reliability/wsl2-fase0-final.md`](docs/reliability/wsl2-fase0-final.md).
