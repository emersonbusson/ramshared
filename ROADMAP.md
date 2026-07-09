# Roadmap — RamShared

The **shippable** product path is **Linux / WSL2 cascade**. Native **Windows StorPort** is a second track, **lab-complete on disposable Hyper-V only**. The long-term destination is **Ring 0 bare-metal** (see [MANIFESTO.md](MANIFESTO.md)). Dates are omitted — this is an R&D project.

Honest status lives in [`validation.md`](validation.md) (append-only) and feature IMPL files under `docs/specs/…`. **Do not invent host-real PASS.**

---

## Completed

### Evaluation & Phase 0

* **Evaluation of the 6 PRDs** + real environment (WSL2/GPU-PV): only PRD-2 (block device + CUDA) is viable in the guest; the others need DRM/BAR/DAMON hardware missing under GPU-PV.
* **Phase 0 (real GPU):** WDDM eviction is *data-safe, latency-unsafe* (4K read up to **~1.18 s** under load); cascade order proved (zram saturation + VRAM absorbing overflow).

### Linux / WSL2 (Day-1 product)

* **SPECv3-WSL2:** VRAM as **cold** tier + **DEMOTE** (`swapoff` VRAM → pages to VHDX without killing processes).
* **Rust workspace:** cascade crates + acceptance validation on live system (spill **~511 MiB** intact; DEMOTE **~481 MiB**, 0 corruption).
* **Canary + multi-conn daemon:** residency sampler (hysteresis), single CUDA worker + multi-conn NBD readers.
* **Live DEMOTE action path (2026-07-09):** ~**648 MiB** on nbd · swapoff **~14.8 s** · VHDX absorbed · **0 corruption** · restore OK (`validation.md`).
* **Cascade hygiene:** refuse ghost ublk/nbd; `swapoff`-first on `down`; continuous health JSONL sampler; demote drill scripts.
* **Adversarial hardening (Issue #3):** C3 (duplicate CUDA FFI removed, CLI `forbid(unsafe_code)`), typed `CascadeError`, zero-dependency policy for Ring-0-adjacent binaries.

### Native Windows — lab track (P4 / Track 2)

> Environment: Hyper-V **`win11-drill` only** (RNF-6). **Host-real driver load: FORBIDDEN.**

| Gate | Result (2026-07-09) |
| --- | --- |
| StorPort + INF/devcon Root\RamShared | LUN **N=1 RAMSHARE VRAMDISK 64 MiB** |
| Format NTFS + smoke | **PASS** (`maxIo=1 MiB`) |
| Secondary pagefile + DT-21 residency | **PASS** (Usage **25%**, KPD **3/3**) |
| B1 safe arm (no secondary PF, kill backend) | **PASS_B1_SAFE_ARM** |
| B2 pagefile-hot kill | **FAIL 0x7A / c0000185** (by design) → **DT-9** mitigation |
| DT-9 refuse hot / reboot then kill | **PASS_DT9_REFUSE_KILL** · **PASS_DT9_REBOOT_KILL** |
| Lab SCM delayed-auto (`RamSharedWinSvc` C#) | **PASS_LAB_SCM** |
| Product CUDA `ramshared-winsvc` on host | **Not done** (env-bound: MSVC+cargo+GPU) |
| ITEM-9 K / ITEM-10 soak / ITEM-11 attestation | **Open** (no invented numbers) |

SPEC / IMPL: [`docs/specs/no-milestone/windows-swap-driver/`](docs/specs/no-milestone/windows-swap-driver/) · degradation: [`docs/reliability/DEGRADATION-MATRIX.md`](docs/reliability/DEGRADATION-MATRIX.md).

---

## Active

### Linux / WSL2 (product polish)

* Keep cascade health + demote drills green on lab machines.
* Optional Phase B prep (custom kernel features) — not required for day-1.
* Marketing / demo posts stay honest: numbers from `validation.md` / reliability docs only.

### Windows — next slices (still VM-first)

| Priority | Work | Gate before “done” |
| --- | --- | --- |
| 1 | Product `ramshared-winsvc` + `nvcuda.dll` on a **Windows GPU box or GPU-enabled VM** | `Cuda::load` + mem_info evidence |
| 2 | MSVC `link.exe` + cargo for Rust service (or keep C# lab SCM until then) | build green |
| 3 | ITEM-9 capacity / p99 **K** | measured, not invented (DT-13) |
| 4 | ITEM-10 soak 3×24 h Driver Verifier | script + logs |
| 5 | ITEM-11 / R9 attestation path | org + signed package |
| **Hard** | **Host-real driver load** | All of: product CUDA path, B1 policy, signing — **never** skip for convenience |

Day-1 public install remains **Linux/WSL2** in [`README.md`](README.md).

---

## Phase B — Custom kernel (WSL2 + custom WSL2 kernel)

* `CONFIG_ZRAM_WRITEBACK`: write cold zram pages directly to VRAM (removes userspace hop on cold path).
* `ublk` replacing NBD (fewer copies / context switches) when the guest kernel has it.

---

## Long-term — Bare-metal (gated on leaving WSL2)

Exploratory; needs DRM/BAR/DAMON/CXL unavailable under GPU-PV guest. **No active SSDV3 folder yet** — when work starts, create `docs/specs/no-milestone/{slug}/` via [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md) (see [`docs/INDEX.md`](docs/INDEX.md)).

Validated cascade (WSL2, not bare-metal): [`docs/specs/no-milestone/wsl2-cascade-swap/`](docs/specs/no-milestone/wsl2-cascade-swap/) · evidence: [`docs/reliability/wsl2-fase0-final.md`](docs/reliability/wsl2-fase0-final.md).

Planned themes (PRDs when gated off WSL2):

* **NUMA node** mapping for VRAM (DAMON / proactive tiering).
* **zswap/zpool** backend inside VRAM via BAR.
* **HMM `DEVICE_PRIVATE` + SDMA + eBPF**.
* **CXL / PCIe Gen5** — coherent device memory as a native storage tier.

---

## Principles of progress

* Structural features: **SSDV3** (PRD → SPEC → IMPL under `docs/specs/…`; SPEC revised in-place) + **Kahneman** (counterfactuals, numerical rollback, #13 no theater). See `docs/SSDV3-PROMPTS.md`.
* No VRAM as hot swap without latency evidence; **measure before coding**.
* **Day-0:** no shims; leaving WSL2 rewrites paths rather than stacking wrappers.
* **Host safety:** no thrash on live daily WSL2/Windows host; pressure and driver crash drills only in isolated VM/qemu.
* **Windows:** lab evidence ≠ host-real readiness. 0x7A under pagefile-hot kill is recorded truth, not a bug to “paper over” with a green badge.
