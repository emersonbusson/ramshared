# Roadmap

Current release posture: **Linux / WSL2 beta, incident gate open**. Ordered
teardown is retained; guaranteed capacity, control-plane headroom, and external
heartbeat must pass requalification before the cascade is called shippable.
Windows StorPort is a **lab** track. Bare metal / CXL is later. No fake dates.

Evidence lives in [validation.md](validation.md) and feature IMPL files. We don’t invent “host-real PASS.”

---

## Done

### Why VRAM isn’t “hot swap”

Phase 0 on real GPU-PV: eviction keeps data intact but can make a tiny read take **~1.18 s**. So VRAM sits **behind** compressed RAM (zram), not in front of it.

### Linux / WSL2 product

- Cascade: zram → VRAM (NBD + CUDA) → disk/VHDX  
- DEMOTE without killing processes (measured: hundreds of MiB, 0 corruption)  
- Anti-hang `down`: swapoff before stopping the daemon; refuse ghost/orphan mess  
- Live demote drill (~648 MiB, ~15 s swapoff)  
- Health sampler scripts  

### Boot opt-in (2026-07)

- `ramshared-cascade.service` via `scripts/safety/install-cascade-boot.sh`  
- Fail-closed preflight; stop = ordered `down`  
- SPEC: [docs/specs/no-milestone/wsl2-cascade-boot/](docs/specs/no-milestone/wsl2-cascade-boot/)

### Control app (2026-07-10)

- `scripts/safety/cascade-app.sh` (zenity GUI + CLI) + desktop launcher  
- SPEC: [docs/specs/no-milestone/cascade-desktop-app/](docs/specs/no-milestone/cascade-desktop-app/)

### Kernel-true track inventory (2026-07-10)

- This lab is **WSL2 GPU-PV only** (PCI vendor `0x1414`, no `/dev/dri`) → Gate A1 **FAIL**  
- Record: [docs/specs/no-milestone/kernel-vram-as-memory/PASSO0-INVENTORY.md](docs/specs/no-milestone/kernel-vram-as-memory/PASSO0-INVENTORY.md)  
- LKM/HMM/NUMA **blocked here**; product path remains cascade + app

### WSL2 Upstream Kernel Contribution (2026-08)

- Formal evidence and candidate branch prepared for Microsoft WSL kernel ([microsoft/WSL#41054](https://github.com/microsoft/WSL/issues/41054))
- Dual-architecture validation: x86_64 and ARM64 compiled cleanly with zero W=1 diagnostics
- Sparse C=2 static analysis and QEMU capability test with 1,024 pages written
- Public candidate branch published at [emersonbusson/WSL2-Linux-Kernel](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/config/ublk-zram-writeback-6.18)

### Windows Host Driver (MVP)

Format, pagefile residency, kernel-page drill, ordered teardown (DT-9), and SCM service are validated on the guest VM. The driver is now in open beta / MVP for physical host integration (requires Secure Boot disabled and testsigning).

---

## Next

| Priority | Work |
| --- | --- |
| WSL2 incident | Requalify guaranteed 1/2/4 GiB profiles, managed workload containment, external heartbeat, and the VMBus control-plane reserve |
| Windows | Product CUDA path + MSVC service on physical host, telemetry collection under budget pressure |
| Upstream WSL2 | Track Microsoft triage on #41054 and promote ublk transport to standard product upon kernel release |

---

## Later (gated) — kernel-true VRAM as memory

**Question:** should process pages map VRAM as real memory (HMM / NUMA / DEVICE_PRIVATE) instead of swap-over-NBD?

**Answer (by environment):** see decision PRD  
[`docs/specs/no-milestone/kernel-vram-as-memory/PRD.md`](docs/specs/no-milestone/kernel-vram-as-memory/PRD.md)

| Environment | Verdict |
| --- | --- |
| WSL2 GPU-PV | **No** for Day-0 — cascade stays (ADR-0001, ~1.18 s reclaim) |
| Bare-metal Linux + BAR/HMM | **Research GO / implement NO-GO** until measurement gates pass |
| Next SSD step | Lab inventory + Passo 0 numbers → only then `SPEC.md` |

Cascade remains the product candidate while that track and the current
stability incident are gated.

---

## How we decide

- Structural mm/lock/driver work: **SSDV3** (PRD → SPEC → IMPL).  
- Measure before bragging.  
- Prefer refuse-to-start over hang.  
- Day-0: no permanent shims pretending to be the product.
