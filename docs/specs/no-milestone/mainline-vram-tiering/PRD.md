---
slug: mainline-vram-tiering
title: Path to native mainline Linux — VRAM as a memory tier (long-term)
milestone: —
issues: []
---

# PRD — One day in the Linux mainline kernel (VRAM as a memory tier)

> **Type:** **strategy / destination** PRD (SSDV3 Step 1).
> Does **not** authorize dumping an out-of-tree LKM as “already mainline.”
> Related tracks: `kernel-vram-as-memory` (lab gates),
> `wsl2-cascade-swap` (shippable bridge).

## 1. Summary

Product question:

> **What is the best approach for this to become part of the Linux kernel natively one day?**

**This PRD's answer:**

The mainline destination is **not** “NBD + CUDA daemon” or a “zenity app.” It
is a **device-memory tier** integrated with mm (HMM / memory tiers / demotion)
with:

1. a **memory model** accepted by the community (tiering, not “pretend it is
   cheap DRAM”),
2. a cooperative **device driver** (DRM/HMM or CXL),
3. a **demotion policy** under pressure (the GPU needs VRAM),
4. small, reviewable **patch series** with benchmarks and selftests.

The WSL cascade and Hyper-V/dual-boot lab are **evidence steps**, not the final
patch.

## 2. Technical context

### 2.1 What “native in mainline” means

| Layer | Mainline? | Notes |
| --- | --- | --- |
| Block-device swap + userspace CUDA | Not “native mm” | Useful as a **bridge** / product |
| `memory tiering` + demotion (cold pages → slow memory) | Infrastructure already exists | CXL/DRAM tiers; extend to **device memory** |
| HMM `DEVICE_PRIVATE` / migrate to device | Already in kernel | Requires a **driver** that registers device memory |
| BAR NUMA node (hotplug) | Possible, controversial | Coherence, poisoning, offline |
| Out-of-tree `ramshared.ko` module | **Not** mainline | Prototype only until upstream |

### 2.2 Why not “upstream the entire RamShared monolith at once”

- Mainline requires **one problem per series**, owners (mm, drm, open
  nvidia/amd), and does **not** depend on the Windows WDDM stack.
- Latency evidence (1.18 s under reclaim) proves that VRAM **without a
  demotion policy** is unacceptable as hot memory — that **must** be in the
  upstream design.
- Vendor lock (closed CUDA only) **blocks** merge; paths prefer **open DRM/HMM**
  or CXL.

### 2.3 Lab reality (this project)

| Lab | Useful for mainline? |
| --- | --- |
| WSL GPU-PV | Product plus demotion policy; does **not** validate real BAR/HMM |
| Hyper-V without GPU | Kernel build, kselftest, QEMU; **without** device memory |
| Hyper-V + DDA (experimental) | Possible `lspci 10de` in guest; fragile on GeForce |
| Bare-metal dual boot | **Best** for driver plus mm experiments |
| Upstream CI | QEMU plus virtio plus selftests required even with a GPU lab |

## 3. Recommended option (best approach for mainline)

### Four-layer strategy (ordered)

```text
L0  Product bridge     cascade zram→VRAM→disk (userspace)     [already exists]
L1  Policy & metrics   demote, free-floor, latency canary     [already exists / polish]
L2  Out-of-tree proto  minimal LKM or driver hook on bare metal [only with Gate A PASS]
L3  Upstream series    mm tiering + driver hooks + selftests  [destination]
```

**Best path to L3 (mainline):**

1. **Do not** propose a “RamShared swap filesystem” as core.
2. Propose **VRAM (or device memory) as a cold memory tier** with automatic
   demotion (reuse demotion/CXL-tier ideas).
3. Implement **first** on hardware where the kernel already has an owner
   (AMDGPU HMM, CXL, or NVIDIA open-gpu-kernel-modules where applicable).
4. Keep an optional **userspace policy agent** (sysfs) — mainline accepts knobs;
   it does not accept an NBD daemon as the mm ABI.
5. Each RFC: problem, API, rollback, numbers.

### Alternatives rejected as a “mainline path”

| Alternative | Why not |
| --- | --- |
| NBD/CUDA only forever | Never becomes native mm |
| Monster LKM in WSL | Wrong environment plus unreviewable |
| Kernel fork | Outside the goal of “part of Linux” |
| Windows StorPort as “upstream Linux” | Different OS |

## 4. Functional requirements (L3 destination)

| ID | Requirement |
| --- | --- |
| RF-M1 | Device memory registerable as a tier with capacity and “cold” latency class |
| RF-M2 | Migrate/demote cold anonymous pages to device memory under DRAM pressure |
| RF-M3 | Reverse promotion/demotion when the device free floor or driver signals “GPU needs it” |
| RF-M4 | Safe offline of the tier (GPU reset, unbind) without silent corruption |
| RF-M5 | Minimal stable documented uAPI (sysfs/debugfs); no eternal experimental ioctl |
| RF-M6 | kselftest or selftest for migration plus failure injection |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-M1 | Patches checkpatch-clean; series ≤ reviewable (~10–20 thematic commits) |
| NFR-M2 | Numbers: p50/p99 fault and bandwidth versus disk swap (`benchmarks.md`) |
| NFR-M3 | Zero CUDA-userspace dependency in the kernel hot path |
| NFR-M4 | Documentation in `Documentation/admin-guide` or `mm/` |

## 6. Flows

### 6.1 Contribution (human plus lab)

```text
Evidence (cascade + bare-metal numbers)
  → design RFC (lore.kernel.org / dri-devel / linux-mm)
  → prototype out-of-tree or behind CONFIG_EXPERIMENTAL
  → selftests green on QEMU + one real GPU
  → v1..vN patch series
  → maintainer ack → mainline
```

### 6.2 Runtime (system with merged feature)

```text
DRAM pressure → cold pages → device tier
GPU workload → driver free-floor signal → demote/offline device pages → DRAM/disk
```

## 7. Data model

- Memory tier descriptor (cost, bandwidth class, nodes)
- Device memory regions (PFN ranges, owner driver)
- Stats: migrated bytes, demotion latency, failure counters

## 8. API (draft — future SPEC freezes it)

Preference: **sysfs** under memory tier / device class; avoid a new ioctl if
sysfs suffices.
Align with existing demotion/tiering APIs (reuse before creating).

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| Closed vendor stack | Prioritize open drivers; dual path forbidden in the mainline design |
| Latency-unsafe hot use | Default **cold tier only**; canary inherited from RamShared evidence |
| Scope creep “all of RamShared in mm” | RF-M* only tiering+migrate; broker/app excluded |
| WSL-only lab | Blocks L2/L3 until bare metal (`kernel-vram-as-memory` PRD) |

## 10. Implementation strategy (years, not sprints)

| Phase | What | Exit criterion |
| --- | --- | --- |
| **P0** | Product bridge (cascade) plus honest docs | already |
| **P1** | Bare-metal lab (dual boot / DDA) plus Step 0 B numbers | Gate A+B PASS |
| **P2** | Minimal prototype aligned to HMM or tiering (out of tree) | migration plus demotion demo |
| **P3** | RFC plus QEMU selftests | mm/drm feedback |
| **P4** | Mainline series | merged or documented NACK |

**Hyper-V on R:** accelerates P1 (generic kernel build/boot).
**DDA:** accelerates GPU P1 if it works.
**Dual boot:** best for P1–P2.
**None of these alone is P4.**

## 11. Documents

- This PRD
- `kernel-vram-as-memory/PRD.md` plus PASSO0
- MANIFESTO (bridge versus destination)
- Future: `SPEC.md` only after P1 PASS and K1 versus K2 selection

## 12. Out of scope

- Guarantee of a mainline merge
- Windows support in the Linux kernel
- A zenity app as an upstream requirement

## 13. Acceptance (this PRD)

- [x] Mainline destination described without conflating it with cascade
- [x] L0–L3 layers
- [x] RF-M* and vendor risks
- [x] Explicit Hyper-V / dual-boot lab connection as P1, not “already native”
- [ ] L3 SPEC: **blocked** until P1 is measured

## 14. Validation

- Human review of this PRD
- Labs: scripts `New-LinuxKernelLabVm.ps1`, `Prepare-DdaGpu.ps1`,
  `Prepare-DualBootRussia.ps1`
- Any “mainline-ready” claim requires a citation to a real upstream commit

## 15. Kahneman

| # | Use |
| --- | --- |
| #11 | Anti-halo: having a local LKM ≠ mainline |
| #13 | HMM existing ≠ our driver registered |
| #3 | Measured latency before RFC |
| #18 | Sunset cascade only with proof of the same problem class in the native path |

## 16. Plain answer

**Best approach to become native in Linux one day:**
treat VRAM as a **cold memory tier with demotion**, through **existing mm
infrastructure plus a cooperative driver**, with **RFCs and selftests** — not
through permanent NBD.

**PRD for it:** this file.
**Next realistic SSD:** P1 lab (VM plus dual boot) → numbers → only then a
kernel-prototype SPEC.
