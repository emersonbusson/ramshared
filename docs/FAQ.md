# FAQ — RamShared (public)

Short answers first. Deep sources linked at the bottom.

## One sentence

**EN:** When your PC runs out of RAM, use idle GPU memory as a safety cushion — and give it back if the GPU gets busy.  
**PT:** Quando a RAM acaba, usa a memória ociosa da placa de vídeo como colchão — e devolve se a GPU precisar.

---

## Will this break my PC?

**Designed not to.** We use normal Linux swap, plus a cold VRAM tier and a **DEMOTE** valve.

| Fear | Reality |
| --- | --- |
| Freeze / BSOD | Host GPU eviction (WDDM) can make VRAM **slow** (~1.18 s on a 4K read, measured). That is why VRAM is **cold**, not hot, and why DEMOTE exists. |
| Corruption | Cascade validation logged **~511 MiB** on VRAM and **~481 MiB** demoted with **0 corruption**. |
| Can’t undo | `sudo ./target/release/ramshared down` removes the cascade. |
| Thrash WSL2 | Live thrash tests on the **host WSL2** are forbidden in project rules; hard pressure runs in **isolated VMs**. |

### DEMOTE in four lines

1. A canary watches swap / VRAM latency.  
2. If the host is reclaiming GPU memory (or latency spikes), we **stop using VRAM as swap**.  
3. Linux moves those pages to the **next tier** (disk / VHDX).  
4. Your processes keep running; we do **not** kill them to free VRAM.

---

## What do I need?

- Linux or **WSL2**
- NVIDIA GPU + working CUDA driver (`nvidia-smi` works)
- Rust (`rustup`) to build
- `sudo` for swap setup

Check: `sudo ./target/release/ramshared check`  
Fix list: `sudo ./target/release/ramshared doctor`

---

## How do I know it worked?

```bash
swapon --show
```

You should see roughly:

1. **zram** — high priority (hot)  
2. **VRAM path** (NBD/ublk device) — medium priority (cold)  
3. **Disk / VHDX** — lowest priority (last resort)

Exact device names vary; **order of priority** matters more than names.

---

## Is this free RAM for games?

**No.** Under full GPU load the host may reclaim VRAM aggressively. RamShared is for **developer / workstation pressure** (compile, containers, browsers) when the GPU is often idle—not a cheat code for 4K gaming.

---

## Why not only zram / zswap?

zram helps a lot but still lives in **system RAM**. RamShared adds **idle GDDR** as an extra cold tier when RAM+zram are not enough. zswap still needs backing under pressure; we do not claim to replace it.

---

## AMD / Intel GPU?

Day-1 path is **NVIDIA + CUDA**. Other backends are research / trait-shaped for later—no multi-vendor promise today.

---

## Bare metal / CXL / HMM?

Long-term roadmap. **Ship path today** is WSL2/Linux cascade with measured WDDM constraints. See `ROADMAP.md` and `docs/specs/`.

---

## Is the multi-tenant broker / Windows driver included?

Not in the day-1 “install and go” path. Those tracks live in specs for later phases. First product surface: **`ramshared check` / `up` / `down`**.

---

## Where are the numbers from?

| Claim | Source |
| --- | --- |
| ~1.18 s 4K stall under WDDM pressure | `docs/reliability/wsl2-fase0-final.md` |
| ublk ~241 µs / NBD-Unix ~326 µs p50 | `docs/reliability/memory-broker-p0-results.md` |
| ~511 MiB spill / ~481 MiB demote / 0 corruption | `docs/reliability/wsl2-cascade-validation.md`, `ARCHITECTURE.md` |

Do not invent new public metrics without updating those files first ([`.claude/rules/benchmarks.md`](../.claude/rules/benchmarks.md)).

---

## Something went wrong

1. `sudo ./target/release/ramshared down`  
2. `swapon --show` (confirm VRAM tier is gone)  
3. `sudo ./target/release/ramshared doctor`  
4. Open an issue with doctor output (no secrets / no kernel addresses that leak KASLR)
