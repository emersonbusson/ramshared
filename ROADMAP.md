# Roadmap

Shippable product today: **Linux / WSL2 cascade** (borrow idle GPU memory, give it back under pressure).  
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

### Windows lab (Hyper-V only)

Format, pagefile residency, kernel-page drill, ordered teardown (DT-9), lab SCM — on **`win11-drill`**.  
Pagefile-hot kill → **0x7A** (expected); product refuses that.  
**Physical host driver: still no.**

---

## Next

| Priority | Work |
| --- | --- |
| Product polish | Keep cascade boot + demote healthy on real daily WSL |
| Windows | Product CUDA path + MSVC service; measured K; soak; signing — then revisit host-real |
| Phase B (optional) | Custom WSL kernel: zram writeback, ublk — only if it earns its keep |

---

## Later (gated)

NUMA / HMM / CXL style ideas need hardware and paths WSL GPU-PV doesn’t give you. No active bare-metal SPEC folder yet; when there is one, it goes under `docs/specs/…`.

---

## How we decide

- Structural mm/lock/driver work: **SSDV3** (PRD → SPEC → IMPL).  
- Measure before bragging.  
- Prefer refuse-to-start over hang.  
- Day-0: no permanent shims pretending to be the product.
