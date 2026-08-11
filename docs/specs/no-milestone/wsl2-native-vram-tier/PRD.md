---
slug: wsl2-native-vram-tier
title: Native VRAM memory tier on WSL2 kernel and/or Ubuntu — decision PRD
milestone: —
issues: []
---

# PRD — “Native” VRAM in the WSL2 kernel (and Ubuntu): what is possible and where to test it

> **Type:** **decision / discovery** PRD (SSDV3 Step 1).
> Does **not** authorize LKM implementation in WSL.
> **Day-1 product:** the userspace cascade continues
> (`wsl2-cascade-swap` plus boot/app).
> Related: `kernel-vram-as-memory`, `mainline-vram-tiering`, and the
> `linux-kernel-lab` lab.

## 1. Summary

### What this PRD answers

1. How the kernel (WSL2 **and/or** Ubuntu) could **natively** fulfill the role
   of “using idle VRAM as memory,” rather than only the NBD/CUDA daemon.
2. What is **viable under GPU-PV (WSL2)** versus what exists only on
   **bare metal / mainline**.
3. **Where to test** each layer (WSL host, Hyper-V VM, dual boot) — **without**
   conflating the three.
4. **Which language** to use in each layer.

### Short answer

| Layer | “Native”? | Viable in WSL2 GPU-PV today? | Where to prove it |
| --- | --- | --- | --- |
| **P0 — Cascade** (zram → VRAM NBD/CUDA → disk plus DEMOTE) | Kernel performs **swap**; VRAM backend is **userspace** | **Yes** — product | **WSL2** |
| **P1 — Closer kernel** (ublk, zram writeback, kernel canary, sysfs priority/policy) | More kernel, but not yet “VRAM = anonymous page” | **Partial** (custom WSL kernel / config) | **WSL2** (plus VM build) |
| **P2 — Device-memory mm** (HMM / `DEVICE_PRIVATE` / device tier) | Yes, native mm | **No** as a clean Day-0 path under GPU-PV (VRAM belongs to host WDDM) | Bare metal / dual boot / DDA — **not** a VM without GPU |
| **P3 — Mainline** | Upstream Linux | Outside WSL as the primary target | Mainline plus open hardware |

**`linux-kernel-lab` VM:** used to **build, run kselftest, and break kernels
without killing WSL**.
It does **not** independently prove “native VRAM in mm,” because it normally
**does not see the GPU** as bare-metal device memory.

**Dual boot:** optional; **not** a requirement for the WSL product. Space on E:
is already prepared if it is needed one day.

---

## 2. Technical context

### 2.1 Confirmed (codebase / lab)

| Fact | Source | Class |
| --- | --- | --- |
| Measured zram→VRAM→VHDX cascade plus DEMOTE | FASE0, validation, ADR-0001 | Confirmed |
| WDDM eviction: data-safe, latency-unsafe (~1.18 s) | FASE0-FINAL | Confirmed |
| WSL guest GPU-PV: Microsoft vendor `0x1414`, without typical `/dev/dri` | PASSO0 inventory | Confirmed |
| Day-1 product: Rust userspace plus `ramshared up/down` | crates, README | Confirmed |
| Hyper-V `linux-kernel-lab`: Ubuntu cloud image, lab auth | validation 2026-07-10 | Confirmed |
| Dual-boot space ~32 GB unallocated on E: | DUALBOOT-KERNEL-TRUE | Confirmed |

### 2.2 What “native” **does not** mean in this PRD

- It does not mean “abandon WSL and use only dual boot.”
- It does not mean “the Linux VM is the VRAM proof environment.”
- It does not mean “NBD/CUDA cease to exist tomorrow in WSL.”

### 2.3 Mental model (three kernels)

```text
A) WSL2 kernel (Microsoft / custom bzImage)
     - real Linux kernel, virtualized MM + GPU-PV
     - best product surface for RamShared today

B) Generic Ubuntu kernel (Hyper-V VM or dual boot)
     - A-like if dual-boot/bare-metal GPU
     - VM without GPU: kernel engineering only

C) Mainline kernel (upstream)
     - long-term home for mm tier / HMM cooperation
```

---

## 3. Recommended option

### 3.1 Phase strategy (honest Day-0)

| Phase | Name | What to build | “Native” kernel? |
| --- | --- | --- | --- |
| **P0** | Product bridge | Keep/improve cascade plus demote plus app/boot | Native swap; VRAM through userspace |
| **P1** | WSL kernel-closer | Custom WSL kernel options: ublk, useful `CONFIG_ZRAM_WRITEBACK`; sysfs/policy; fewer hops | More native in **I/O and policy** |
| **P2** | Device-memory research | HMM/tier only with evidence of real device memory | Only outside clean GPU-PV |
| **P3** | Mainline | RFC plus selftests (see `mainline-vram-tiering`) | Upstream |

**Product recommendation:** **P0 is mandatory**; **P1** when a custom WSL
kernel is worthwhile; **P2/P3** must not block day-to-day use.

### 3.2 Where to test (mandatory matrix)

| Hypothesis | Proof environment | Anti-pattern |
| --- | --- | --- |
| Cascade / demote / WDDM latency | **WSL2 on the host with GPU** | VM without GPU only |
| Kernel build, checkpatch, kselftest without GPU | **`linux-kernel-lab` (Hyper-V)** | Thrash on daily WSL |
| Module crash / heavy lockdep | VM or dual boot | Work WSL host |
| “Anonymous page in device memory” | Dual boot/DDA plus real GPU | VM without GPU; WSL GPU-PV alone |
| Mainline claim | QEMU selftests plus one GPU lab | Chat-only |

### 3.3 Dual boot in this PRD

**Optional.** It is not the “turn on WSL and use it” path.
Space on **E: ESPANHA (~32 GB unallocated)** exists if P2 requires bare metal.
R: RUSSIA remains bad for NTFS shrinking (~2.7 GB shrinkable).

---

## 4. Functional requirements

| ID | Requirement | Phase |
| --- | --- | --- |
| RF-W1 | Document the P0 contract: kernel swap plus userspace VRAM backend plus DEMOTE | P0 |
| RF-W2 | CLI/app remain fail-closed (ghost swap, swapoff-first) | P0 |
| RF-W3 | If P1: explicit `CONFIG_*` / ublk / writeback list and rollback | P1 |
| RF-W4 | Any kernel uAPI (sysfs/debugfs/ioctl) has `capable`, bounds, and no information leak | P1+ |
| RF-W5 | WSL versus VM versus bare-metal test matrix is filled in IMPL | all |
| RF-W6 | Do not claim “native VRAM mm” without device-memory / bare-metal or DDA evidence | all |
| RF-W7 | VM lab remains isolated (passwordless lab is acceptable); host UAC unchanged | ops |

---

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-W1 | Latency: number plus unit plus n≥3 when it is a gate (`benchmarks.md`) |
| NFR-W2 | Host safety: no thrash on daily WSL |
| NFR-W3 | Day-0: no dual “ImDisk forever” / eternal shim path |
| NFR-W4 | Language by layer (see §8) — do not mix in the hot path |

---

## 6. Flows

### 6.1 Product use (what the user “turns on”)

```text
WSL starts → (optional systemd cascade) → ramshared up
  → kernel swapon order zram > vram > disk
  → pressure → pages to VRAM tier
  → GPU pressure → DEMOTE → disk
```

### 6.2 P1 engineering (custom WSL kernel)

```text
Build kernel (VM or WSL) → boot-kernel-safe → measure ublk/writeback
  → go/no-go vs NBD cascade
```

### 6.3 P2 research (only if hardware permits)

```text
Bare-metal or DDA → driver/device memory → migrate/demote pages
  → Gate B numbers → separate SPEC
```

---

## 7. Data model / interfaces (draft)

### P0 (today)

- Userspace: `ramshared`, `ramsharedd`, NBD sockets, `/proc/swaps`
- Config: `/etc/ramshared/cascade.conf`

### P1 (future)

- Possible: sysfs `.../ramshared/` or module parameters
- ublk device nodes if the kernel is custom

### P2 (future)

- Memory-tier / HMM registration — a **dedicated SPEC** after Gate A/B

Do not freeze ABI in this PRD.

---

## 8. Implementation languages (answer to “which language?”)

> **Canonical decision (audited):** [ADR-0007](../../../decisions/ADR-0007-kernel-native-language-c.md) ·
> [kernel-native-language PRD + AUDIT-2.5](../kernel-native-language/) → **go**.

| Layer | Appropriate **language** | Why |
| --- | --- | --- |
| **Linux kernel** (LKM, mm, ublk glue, sysfs) | **C11 mainline style** (TAB 8, checkpatch) | ABI, reviewers, lockdep, ecosystem |
| **New “greenfield” kernel code** (optional) | **Rust for Linux** only with SPEC plus exception audit | Not the default (ADR-0007) |
| **Daemon / CLI / broker / cascade (P0)** | **Rust** (already the stack) | ADR-0002 |
| **CUDA FFI** | **Rust plus isolated `unsafe`** in `ramshared-cuda` | Only unsafe userspace boundary |
| **Windows StorPort lab** | **C (WDK)** plus Rust/C# userspace lab | Windows kernel |
| **Lab / Hyper-V scripts** | **PowerShell** (host) plus **bash** (WSL) | Automation, not product hot path |
| **Inappropriate** for “native in the kernel” | Python/Node/Go as LKM; **Rust application pretending to be an LKM** | Does not run in kernel context |

### Practical project recommendation

1. **Continue P0 in Rust** (userspace).
2. **“Truly native in the kernel” → C** (mainline/custom WSL kernel) —
   **ADR-0007**.
3. **Rust for Linux** only as an audited exception — do not rewrite mm in Rust.
4. Choosing C does **not** prove device memory under GPU-PV (#13).

---

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| Halo: “native kernel” = throw away cascade | P0 remains shippable |
| Halo: Linux VM proves VRAM | §3.2 matrix |
| Custom WSL kernel bricks boot | `boot-kernel-safe.ps1`, dual Microsoft-kernel entry |
| GPU-PV 1.18 s latency in a “hot” path | VRAM always cold; demote |
| P2 scope in WSL | PRD says NO-GO until evidence |

---

## 10. Implementation strategy

| Step | Artifact | Environment |
| --- | --- | --- |
| Now | This PRD | — |
| P0 polish | Cascade/app IMPL already in progress | WSL |
| P1 SPEC (if go) | **Delivery PRD (authoritative):** [`../wsl2-custom-kernel-p1/PRD.md`](../wsl2-custom-kernel-p1/PRD.md) → then `SPEC.md` in that folder | Custom WSL kernel; build in `RamShared-Kernel` lab |
| P2 | Only after bare-metal/DDA evidence | Outside “WSL only” scope |
| P3 | `mainline-vram-tiering` | upstream |

---

## 11. Documents to update

- This PRD plus optional pointer in `docs/labs/DUALBOOT-KERNEL-TRUE.md`
- README: one line, “native kernel = research; product = cascade on WSL”
- `PASSO0` kernel-vram: cross-link

---

## 12. Out of scope

- Require dual boot to use RamShared
- Rewrite cascade in C
- Windows StorPort as “native Linux kernel”
- PROMISE P2 under GPU-PV

---

## 13. Acceptance (this PRD)

- [x] Distinguishes P0/P1/P2/P3
- [x] WSL versus VM versus bare-metal test matrix
- [x] Dual boot optional, not central
- [x] Languages by layer
- [x] RF/NFR and risks
- [ ] P1 SPEC: only when there is a custom WSL-kernel decision

---

## 14. Validation

- Cross-reading ADR-0001, FASE0, cascade IMPL
- Lab: WSL for P0; `linux-kernel-lab` for kernel **builds**; E: unallocated
  for optional dual boot

---

## 15. Kahneman

| # | Use |
| --- | --- |
| #11 | Anti-halo: “native = better in WSL now” |
| #13 | HMM existing ≠ GPU-PV exposes device memory |
| #2 | Counterfactual: hot VRAM in WSL → 1 s stall |
| #18 | Sunset cascade only with proof of the same problem class in the native path |

---

## 16. Plain language (for humans)

**What you turn on in WSL today** is not “the kernel maps VRAM like RAM.”
It is “the Linux kernel’s **swap** uses a **GPU-backed disk** as a cold tier, and we pull that tier out if the GPU gets busy.”

**A “native kernel” future** would push more of that into mm/device-memory APIs — that is a **research** track (P2/P3), not the day-1 install.

**The Linux Hyper-V VM** is a **safe sandbox to build and crash kernels**, not the place that proves GPU VRAM nativeness without a real GPU path.

**Languages:** Rust for the product daemon/CLI; **C** for real Linux kernel work; scripts for lab only.
