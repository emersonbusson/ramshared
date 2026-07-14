# Architecture

RamShared turns **idle GPU memory** into a **cold** emergency tier and has a clear **give-back** path when the GPU or host needs that memory.

Two tracks share CUDA/block pieces. Only one is the daily product.

| Track | Status | Where we test |
| --- | --- | --- |
| Linux / WSL2 cascade | Product | Your machine + qemu drills |
| Windows StorPort | Lab only | Hyper-V disposable VM |

---

## Track 1 — Linux / WSL2 (what you run)

```text
Pressure  →  zram    (compressed RAM)   priority 200   hot
          →  VRAM    (WDDM budget + CUDA + NBD) priority 100 cold
          →  disk    (VHDX / swap)      priority  -2   last resort
```

**Why cold?** Under WDDM reclaim, a 4 KB read measured ~**1.18 s**. Putting VRAM first would stall the machine. Behind zram, only cooler pages land there.

**Give-back (DEMOTE):** canaries on latency, free GPU memory, and corruption → `swapoff` on the VRAM device → pages fall to disk → processes stay alive.

**Lifecycle observability:** pure phase derivation in `crates/ramshared-cli/src/cascade/lifecycle.rs` (Armed / Using* / Demoting / Degraded). See [`docs/specs/no-milestone/cascade-lifecycle-observability/SPEC.md`](specs/no-milestone/cascade-lifecycle-observability/SPEC.md).

**Invariant A1:** demote only if something lower can absorb pages (disk swap or enough free RAM). Checked at `up`.

On WSL2, Windows WDDM/VidMm remains the memory authority. `ramsharedd` queries the
local-segment process budget through `/dev/dxg` before each sparse CUDA chunk commit.
CUDA free memory is a second fail-closed check, not the policy authority. If dxg is
unavailable at startup, the daemon reports `budget_source=cuda-fallback`; a later dxg
failure blocks new commits. More than one dxg adapter is rejected until CUDA↔LUID
identity is proven.

### Main pieces

| Piece | Job |
| --- | --- |
| `ramshared` CLI | check / doctor / up / down / status (`status --json` lifecycle phase) |
| `ramsharedd` | holds VRAM, serves NBD |
| `ramshared-tier` | priority order + safety net |
| `ramshared-cuda` | load NVIDIA driver at runtime |
| `ramshared-dxg` | query the host-authoritative WDDM budget |
| `ramshared-cascade.service` | optional boot: preflight → up; stop → down |

### Anti-hang rules (learned the hard way)

1. Never kill the daemon while nbd/ublk is still in `/proc/swaps`.  
2. Always `swapoff` first.  
3. Refuse `up` on ghost `(deleted)` swap.  
4. Boot path is **opt-in** and **fail-closed** (preflight can refuse).

Boot install: `scripts/safety/install-cascade-boot.sh`  
SPEC: [docs/specs/no-milestone/wsl2-cascade-boot/](docs/specs/no-milestone/wsl2-cascade-boot/)

---

## Track 2 — Windows lab (not day-1 install)

Secondary pagefile on a StorPort virtual disk. Userspace completes I/O.  

**Hard rule:** never tear the disk down under a **hot** pagefile (BugCheck **0x7A** proven). Ordered teardown (DT-9) refuses that.

Do not load this on a physical daily driver. See the windows-swap-driver SPEC/IMPL.

---

## Process

Structural work uses **SSDV3** (PRD → SPEC → IMPL).  
We write failure modes in [docs/reliability/DEGRADATION-MATRIX.md](docs/reliability/DEGRADATION-MATRIX.md).  
We don’t thrash the live WSL you work in for “fun” benchmarks.
