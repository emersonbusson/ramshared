---
slug: kernel-vram-as-memory
title: Kernel-true VRAM as process memory (HMM / NUMA / DEVICE_PRIVATE) — decision PRD
milestone: —
issues: []
---

# PRD — VRAM “for real” in the kernel (process pages map VRAM)

> **Type:** **decision / discovery** PRD (SSDV3 Step 1).
> Does **not** authorize LKM implementation. A SPEC follows only after the
> **environment gates** (§14).
> **Day-1 product in production:** the WSL2 cascade continues
> ([`wsl2-cascade-swap`](../wsl2-cascade-swap/PRD.md), ADR-0001).

## 1. Summary

The RamShared manifesto prefers the **lowest stable architectural level**:
HMM, NUMA, CXL — not eternal userspace wrappers. The honest question is:

> **Is the best “elegant” way to use idle VRAM for the kernel to treat VRAM as process memory (fault/migrate), rather than swap in a block device?**

**This PRD's short answer:**

| Environment | Is it the best approach? |
| --- | --- |
| **WSL2 + GPU-PV (consumer GeForce)** | **No** for Day-0. No useful guest DRM/BAR/`nvidia_p2p`; VRAM under WDDM is **latency-unsafe** (~1.18 s measured). The zram→VRAM→disk cascade remains the shippable path. **Confirmed in docs** (ADR-0001, FASE0-FINAL). |
| **Bare-metal Linux with GPU + BAR/ReBAR + cooperative DRM/HMM stack** | **Yes, a long-term candidate** — aligns with the manifesto and removes the NBD/daemon hop. Requires hardware, driver, and empirical gates **before** SPEC/IMPL. **Inference** until measured in a bare-metal lab. |
| **Native Windows host** | A different contract (pagefile plus miniport lab). Not Linux HMM. See `windows-swap-driver`. |

This PRD does **not cancel** the cascade. It **opens track K (kernel-true)**
with go/no-go criteria so that manifesto aspirations are not confused with what
runs on today's WSL notebook.

## 2. Technical context

### 2.1 What “truly in the kernel” means

Today (cascade):

```text
process page → anon → swap → block I/O (NBD) → userspace CUDA → VRAM
```

Kernel-true (target of this track):

```text
process page → page table / migration → device memory (VRAM) as memory tier
              (HMM migrate, DEVICE_PRIVATE, or NUMA hotplug memory)
```

The process does **not** “read a GPU disk”; the MMU / migration path moves
pages between DRAM and device memory.

### 2.2 Why the cascade exists (it is not laziness)

| Fact | Source | Classification |
| --- | --- | --- |
| WDDM eviction: data intact, 4K read up to **~1.18 s** | `docs/reliability/wsl2-fase0-final.md` | **Confirmed in docs** plus empirical |
| Proven zram→VRAM→VHDX cascade (spill ~983 MiB VRAM) | FASE0 Part C, ADR-0001 | **Confirmed** |
| NUMA/HMM/`nvidia_p2p` rejected in the WSL GeForce guest | ADR-0001 Alternatives | **Confirmed in docs** |
| Manifesto: bare-metal first, HMM/NUMA/CXL | `MANIFESTO.md` | **Confirmed in docs** |
| Shippable Day-1 cascade plus opt-in boot | `wsl2-cascade-*`, README | **Confirmed in codebase** |

**Context conclusion:** under GPU-PV, “mapping VRAM as RAM” does **not remove**
the true memory owner (WDDM on the host). An LKM in the guest that pretends
VRAM is DRAM **inherits the same reclaim latency** — but on the **page-fault /
migration** path, which can be **worse** (a stall in process context without a
clean swapoff demotion).

### 2.3 Kernel-true options (tree)

| Option | Mechanism | Requires | WSL GPU-PV? |
| --- | --- | --- | --- |
| **K1 — HMM + `DEVICE_PRIVATE`** | migrate to/from device pages; fault-in | GPU driver + HMM + kernel config | **No** (no cooperative stack in the consumer guest) |
| **K2 — NUMA hotplug through ReBAR/BAR** | `add_memory` / PCIe-region memory block | ReBAR, IOMMU, coherence or explicit non-coherence rules | **No** in a typical PV guest |
| **K3 — DRM/TTM “stolen” / carve-out** | DRM-managed memory as a pool | control of the DRM driver in the OS | Bare-metal Linux possible; not WSL |
| **K4 — CXL / coherent device memory** | coherent device memory | CXL hardware | Future; outside the current lab |
| **C — Cascade (current)** | swap priorities + NBD/CUDA + DEMOTE | CUDA userspace + nbd | **Yes — product** |

## 3. Recommended option

### 3.1 Product decision (now)

1. **Keep the cascade** as the only **shippable** path on WSL2/Linux GPU-PV.
2. **Do not** start an LKM implementation of “VRAM = RAM” in WSL.
3. **Open track K** with this PRD: gated, **bare-metal-only research**.
4. Create a track-K SPEC **only** if §14 gates pass (hardware plus measurements).

### 3.2 Why not “truly in the kernel” in WSL now

| Kernel argument | Counterfactual |
| --- | --- |
| “More elegant / manifesto-aligned” | Elegance without an I/O and reclaim path = panic and freeze |
| “No userspace hop” | In PV, the real hop is **host WDDM**, not NBD |
| “The application trusts the MMU” | A 1 s page-in fault is worse UX than cold-swap demotion |
| “Clean Day-0” | Clean Day-0 in WSL **is already** the cascade (ADR-0001) |

### 3.3 When kernel-true **is** the best approach

When **all** are true:

1. Bare-metal Linux (or a VM with real GPU passthrough), not only GPU-PV.
2. Access to a device-memory region **visible** to the kernel (BAR/ReBAR or
   vendor HMM API).
3. Measurement of migration/fault latency under GPU pressure **before** exposing
   it to generic applications.
4. A **demote/offline** plan for the node/device memory without UAF (A1 plus
   canary equivalent).
5. Complete SSDV3 (SPEC + AUDIT-2.5 go) — locks, DMA, IRQ, lifetime.

Until then, “truly in the kernel” is an **architecture goal**, not a sprint plan.

## 4. Functional requirements (track K — if gates pass)

Scope is **bare-metal research only**. IDs support future traceability.

| ID | Requirement | Class |
| --- | --- | --- |
| RF-K1 | Expose a device-memory pool usable by mm (migration or hotplug) with configurable size | Inference until lab |
| RF-K2 | Anonymous processes can hold pages in device memory without a swap block device | Inference |
| RF-K3 | Under GPU pressure / reset / unplug: **offline or migrate back** to DRAM without panic; applications survive or receive documented SIGBUS | Confirmed pattern (cascade-demote analogue) |
| RF-K4 | Minimal uAPI surface (privileged sysfs/debugfs or ioctl) with `capable` plus bounds | Confirmed practice (security rules) |
| RF-K5 | No dual “cascade + LKM” path on the same host without a Day-0 exception ADR | Day-0 policy |
| RF-K6 | Cascade remains installable and documented while track K lacks numerical P0 evidence | Confirmed product need |

### Outside track K (do not conflate)

- Replacing the WSL cascade with an LKM.
- Windows StorPort as HMM.
- Promising “the app maps VRAM” without root/driver.

## 5. Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-K1 | Fault/migration latency under idle and GPU load: a **number** (p50/p99), ≥3 runs — benchmark rule |
| NFR-K2 | No panic path during GPU reset / D3; degradation matrix updated |
| NFR-K3 | checkpatch/sparse/lockdep for any LKM; no unstructured `printk` |
| NFR-K4 | Host safety: memory pressure only in an isolated VM if the lab is shared |
| NFR-K5 | Numerical rollback trigger in the SPEC (for example, p99 fault > p99 VHDX swap under load → demote feature) |

## 6. Flows

### 6.1 Discovery (now — this PRD)

```text
Manifesto question → lab hardware inventory
  → if WSL-only: STOP track K (cascade only)
  → if bare-metal GPU+BAR: Step 0 measurement (BAR/HMM-probe latency)
  → if PASS: track-K SPEC.md
  → if FAIL: append validation + keep cascade
```

### 6.2 Happy path (future, post-SPEC)

1. Administrator enables the device-memory pool (size ≤ GPU free floor).
2. mm places cold pages in device memory (policy / cgroup / `madvise` — to be
   decided in the SPEC).
3. GPU application increases → curator demotes/offlines device pages → DRAM.
4. The process continues.

### 6.3 Failure path

GPU reset mid-page → I/O/migration failure → DRAM pages or stable signal;
**never** silent corruption.

## 7. Data model

| Concept | Notes |
| --- | --- |
| Device memory pool | size, NUMA node or HMM device, free floor |
| Page state | DRAM / DEVICE / migrating (SPEC details) |
| Lease / holder | who “owns” the carve-out versus CUDA apps (co-residency) |

## 8. API / Interfaces (draft — SPEC freezes it)

- Sysfs or debugfs under `/sys/kernel/ramshared/` or a device class —
  **privileged**.
- Possible minimal ioctl only if sysfs is insufficient.
- **Forbidden** in the PRD: copy Windows uAPI; copy NBD ABI.

No final structs here (avoids premature ABI).

## 9. Dependencies and risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Confusing track K with the WSL product | User installs an LKM and hangs | Docs plus gates; README points to cascade |
| WDDM-like latency-unsafe behavior in flawed passthrough | Freeze on fault | Measure before SPEC; canary required |
| Incorrect CPU↔GPU cache coherence | corruption | Only vendor-documented paths; do not invent snooping |
| Monster HMM+DRM+IOMMU scope | never-ship | One mechanism per SPEC (K1 **or** K2, not both) |
| Manifesto halo (#11) | “kernel = better” without evidence | This PRD plus ADR if pivoting |

**Dependencies:** bare-metal lab, kernel headers, and possibly an
out-of-tree versus mainline policy (decide in the SPEC).

## 10. Implementation strategy

| Phase | Action | Artifact |
| --- | --- | --- |
| **0** | This PRD plus ADR pointer (optional) | `PRD.md` |
| **0.5** | Lab inventory: bare-metal? ReBAR? open driver? | `validation.md` entry |
| **1** | Step 0 measurement (no production LKM): region-access latency / HMM probe | runbook plus numbers |
| **2** | Exactly one SPEC from {K1, K2, K3} — what the lab supports | `SPEC.md` + AUDIT-2.5 |
| **3** | Minimal IMPL plus kselftest | `IMPL.md` |
| **—** | In parallel: cascade polish (app, boot, demote) | already in progress |

**Preference order if the lab allows:** K1 (HMM) if a vendor stack exists;
otherwise K2 (NUMA/BAR) if ReBAR is stable; K3 only with a DRM-aware maintainer;
K4 when the hardware exists.

## 11. Documents to update (when gates pass)

- `ROADMAP.md` — gated track K (this PRD already references it).
- New ADR if the cascade is abandoned **on bare metal** (not WSL).
- `DEGRADATION-MATRIX.md` — fault/migration/GPU-reset rows.
- `MANIFESTO.md` — optional: “cascade = bridge; kernel-true = bare-metal destination”.
- `docs/INDEX.md` through the generation script.

## 12. Out of scope

- LKM implementation in this cycle.
- Replacing cascade as the sole README path.
- Windows HMM.
- CXL without hardware.
- “App store” packaging for track K.

## 13. Acceptance criteria (this PRD — decision)

- [x] The question “is a true kernel better?” is answered **by environment**.
- [x] Cascade is preserved as Day-1.
- [x] Explicit gates before SPEC (§14).
- [x] K1–K4 plus C options documented.
- [x] Kernel abuse cases listed (§ below).
- [ ] SPEC: **only** after Step 0 bare-metal PASS (does not block merging this PRD).

### Abuse cases (mandatory in discovery)

| Abuse | Risk | Treatment in track K |
| --- | --- | --- |
| ioctl size/TOCTOU | kernel memory corruption | copy_from_user once; max bounds |
| map device memory without ownership | UAF / DMA to freed memory | get/put lifetime; unplug path |
| GFP in IRQ during migration | deadlock | context matrix in the SPEC |
| capability bypass | unprivileged DoS/hang | CAP_SYS_ADMIN |
| address info leak | KASLR break | no `%px` in default logs |
| CUDA co-residency plus pool | thrash / 1.18 s class | free floor plus demote/offline |

## 14. Validation / gates (go → SPEC)

### Gate A — Environment (must)

| Check | Pass condition |
| --- | --- |
| A1 | Lab is **not** only a WSL GPU-PV guest; bare-metal Linux or documented passthrough |
| A2 | Inventory tool records: GPU, driver, ReBAR y/n, relevant `/proc/iomem` (without leaking secrets) |
| A3 | Operator confirms test pressure is **not** on the daily work host when there is hang risk |

### Gate B — Measurement (must before SPEC freeze)

| Check | Pass condition |
| --- | --- |
| B1 | ≥3 runs of access latency or migration probe; median + p99 + idle/loaded condition tag |
| B2 | Compare p99 with disk swap **in the same snapshot** (benchmark rule) |
| B3 | If device-path p99 under GPU load exceeds the “freezes UI” threshold (define in Step 0; default: >50 ms generic page fault or > p99 disk×N) → **no-go** as hot memory; only cold-tier policy in the SPEC |

### Gate C — Process

| Check | Pass |
| --- | --- |
| C1 | AUDIT-2.5 go in the SPEC (locks/DMA/mm) |
| C2 | Numerical rollback trigger in the SPEC |
| C3 | Cascade docs do not promise “we already have NUMA” |

### This PRD's verdict

| Track | Verdict |
| --- | --- |
| **Cascade (WSL/product)** | **GO continue** |
| **Kernel-true (track K)** | **GO research / NO-GO implement** until A+B |
| **Kernel-true in WSL GPU-PV** | **NO-GO** (reaffirmed ADR-0001) |

## 15. Kahneman map (summary)

| # | Application |
| --- | --- |
| #2 | Counterfactual: LKM in WSL without BAR → 1 s fault → worse than cascade |
| #3 | 1.18 s is an anchor; not the adjective “fast in the kernel” |
| #5 | GPU reset / reclaim in the worst case |
| #11 | Anti-halo: bare-metal manifesto ≠ best on WSL |
| #13 | HMM existing in mainline kernel ≠ our hardware |
| #16 | Fail-safe: offline/demote independent from CUDA application path |
| #18 | Cascade is not an eternal shim if track K proves itself; sunset only with evidence |

## 16. Answer to the product question (plain language)

**“Can we not make this elegantly in the real kernel?”**
We can **imagine and plan** it elegantly — and **should** (this PRD).
In **WSL with virtualized GPU**, we cannot **deliver** it as a clean, reliable
Day-0 path: the Linux kernel does not own VRAM.
The elegant form **today** is the cascade (kernel in swap, userspace only in
the backend).
The elegant form **later**, on metal, is track K — **if** the measurement gates
pass.

**“Should we generate an SSD-following PRD?”**
**Yes.** This file **is** that PRD. The next realistic SSD step is **not** an
automatic SPEC — it is **Step 0 bare-metal inventory/measurement** or explicitly
archive track K as “blocked on hardware” in `validation.md`.
