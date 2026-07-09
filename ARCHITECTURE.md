# Architecture — RamShared

RamShared turns **idle GPU memory** into a **cold emergency-memory tier** with an explicit **give-back** path when the GPU or host reclaims that memory.

Two implementation tracks share the same idea and much of the Rust core (`ramshared-cuda`, `ramshared-block`, integrity, tier rules). They differ in **who owns the block device** and **where evidence is allowed**.

| Track | Status | Evidence env |
| --- | --- | --- |
| **Linux / WSL2 cascade** | Day-1 product | Live WSL2 + qemu drills |
| **Native Windows StorPort** | Lab-complete (VM) | Hyper-V `win11-drill` only — **host-real blocked** |

Bare-metal NUMA/HMM/CXL is roadmap-only ([ROADMAP.md](ROADMAP.md)).

---

## Track 1 — Linux / WSL2 cascade

RamShared **orchestrates** a priority-ordered swap cascade and **manages the VRAM tier**. `zram` and `VHDX`/disk swap are kernel mechanisms RamShared configures, not reimplements.

```text
Memory pressure ─► zram   (compressed RAM)   prio 200  HOT
                ─► VRAM   (CUDA + NBD daemon) prio 100  COLD
                ─► VHDX   (WSL2 default swap) prio  -2  LAST
```

### Safety model (the pivot)

Validation on real GPU hardware showed WDDM eviction is:

* **data-safe** — page checksums remain intact after eviction;
* **latency-unsafe** — a 4 KB read with VRAM under host reclaim measured **~1.18 s**.

If VRAM were hot swap, eviction would stall the system. As a **cold** tier behind `zram`, it only receives cooler pages. When latency or free-floor canaries fire (host reclaiming VRAM), the **DEMOTE** path runs `swapoff` on the VRAM tier so the kernel migrates pages to VHDX **without killing processes**.

**Invariant A1:** Demotion is only safe if a lower-priority tier is active and ready to absorb pages (checked at startup and in `ramshared-tier`).

### Components (Linux/WSL2)

| Crate | Responsibility | `unsafe` |
| --- | --- | --- |
| `ramshared-tier` | Priorities (`zram 200 > vram 100 > vhdx -2`), `validate_order`, A1 | `forbid` |
| `ramshared-cuda` | Runtime load of `libcuda` / `nvcuda.dll` (no toolkit link); RAII | **Isolated here** |
| `ramshared-block` | NBD protocol + shared `VramBackend` | `forbid` (backend uses CUDA) |
| `ramshared-integrity` | Checksums (FNV-1a) + validation patterns | `forbid` |
| `ramshared-wsl2d` | Daemon: state machine, NBD serve, canary/DEMOTE, `mlockall` | `mlockall` only (+ CUDA via crate) |
| `ramshared-cli` | `check` / `doctor` / `up` / `down` / `status` | `forbid` |

### Execution flow

1. **`up`:** Validate order + A1 → start zram → start daemon → attach NBD (`nbd-client`) → `mkswap` / `swapon -p`.
2. **Daemon:** Allocate and zero VRAM, `mlockall`, `oom_score_adj=-1000`, serve READ/WRITE via `cuMemcpy*`.
3. **Canary:** Latency / free-floor / corruption streaks → spawn `swapoff` on VRAM device (**DEMOTE**) while serving read-back.
4. **`down`:** `swapoff` NBD **before** disconnect (avoids panic) → tear zram → wipe VRAM → stop daemon.

### Key decisions

* **NBD over ublk (Phase A):** `CONFIG_BLK_DEV_NBD` is common on WSL2; ublk often needs a custom kernel.
* **Runtime CUDA loader:** no build-time CUDA Toolkit dependency.
* **Priority via `swapon -p`:** not zram writeback (not in baseline WSL2 kernel) — writeback is Phase B.
* **Canary vs cascade separation:** pure unit tests without root/GPU; runner owns CUDA and `swapoff`.

Evidence: [`docs/reliability/wsl2-cascade-validation.md`](docs/reliability/wsl2-cascade-validation.md) · [`docs/reliability/wsl2-fase0-final.md`](docs/reliability/wsl2-fase0-final.md) · live DEMOTE: [`validation.md`](validation.md).

---

## Track 2 — Native Windows (StorPort virtual miniport)

**Product idea:** a secondary **pagefile** on a **virtual disk** whose blocks are served from VRAM (or a lab file backend), with **fail-closed teardown** so the disk is never yanked while the pagefile is hot.

```text
Windows memory manager
        │
        ▼
 pagefile on volume D:  (secondary)
        │
        ▼
 StorPort virtual miniport (ramshared.sys)
        │  SQ/CQ rings + COMMIT_AND_FETCH
        ▼
 Userspace backend (lab: WinDriveBackend file; product: CUDA VramBackend)
```

### Safety model (Windows)

| Scenario | Empirical outcome | Product rule |
| --- | --- | --- |
| **B1** surprise remove with **no** secondary pagefile | Contained (PASS_B1_SAFE_ARM) | Lab OK |
| **B2** kill backend with pagefile **Usage > 0** | **BugCheck 0x7A** / `STATUS_IO_DEVICE_ERROR` | **Never** — DT-9 |
| **DT-9** ordered teardown | Refuse kill while PF hot; after reboot unload, kill clean | Mandatory |
| Lab SCM delayed-auto | Backend + disk after boot | Lab path only |

**Invariant (DT-9):** teardown never removes the disk with an active pagefile. Order: disable pagefile → (reboot if OS will not release hot) → drain I/O → destroy disk → wipe → release lease.

### Components (Windows)

| Piece | Role |
| --- | --- |
| `drivers/windows/ramshared` | StorPort miniport + `\\.\RamSharedCtl`; bounce-buffer I/O; `StorPortGetSystemAddress` |
| `crates/ramshared-winsvc` | Protocol, pagefile helpers, teardown policy; product bin needs MSVC |
| Lab `RamSharedWinSvc` (C#) | Delayed-auto SCM orchestrating Start/Stop lab scripts (stand-in until Rust service builds on Windows) |
| `scripts/windows/*` | Preflight, build, install, B1/B2/DT-9/ITEM-8 drills, disciplined campaign |

**Not day-1 public install.** Do not load on a physical host you care about. Full SPEC: [`docs/specs/no-milestone/windows-swap-driver/SPEC.md`](docs/specs/no-milestone/windows-swap-driver/SPEC.md) · gates: [`IMPL.md`](docs/specs/no-milestone/windows-swap-driver/IMPL.md) · ADR-0006.

### Lab vs product (Day-0 honesty)

| Layer | Lab (proven) | Product (blocked until evidence) |
| --- | --- | --- |
| Disk backend | File-backed `WinDriveBackend` | CUDA `VramBackend` + free-floor |
| SCM | C# `RamSharedWinSvc` delayed-auto | Rust `ramshared-winsvc` + installers |
| Signing | Test-sign in VM | Attestation / R9 |
| Host | `win11-drill` only | Host-real **forbidden** until gates |

---

## Cross-cutting methodology

* **SSDV3:** PRD → SPEC → IMPL under `docs/specs/…`; Passo 2.5 adversarial go/no-go; SPEC revised in-place. [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md) · [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md).
* **Kahneman (18):** counterfactual + numerical rollback (#2); calibrated retry (#15); fail-safe / independent curator (#16); replay idempotency (#17); right-layer root cause + proven sunset (#18); **#13 no theater** (green script ≠ green host-real).
* **Day-0:** no permanent shims or dual-path “ImDisk forever” as product.
* **Host safety:** real pressure and crash drills only in isolated VM/qemu — never thrash the daily WSL2/Windows desktop host ([`.claude/rules/benchmarks.md`](.claude/rules/benchmarks.md)).

Failure modes: [`docs/reliability/DEGRADATION-MATRIX.md`](docs/reliability/DEGRADATION-MATRIX.md).
