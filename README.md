# RamShared

**When your PC runs out of RAM, use idle GPU memory as a safety cushion — automatically, and pull back if the GPU gets busy.**

> **PT:** Quando a RAM acaba, usa a memória ociosa da placa de vídeo como colchão — e devolve se a GPU precisar.

<p align="center">
  <img alt="RamShared swap cascade" src="docs/marketing/cascade-diagram.png" width="900">
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Platform" src="https://img.shields.io/badge/Platform-Linux%20%7C%20WSL2-blue?style=flat-square&logo=linux&logoColor=white">
  <img alt="CUDA" src="https://img.shields.io/badge/CUDA-Enabled-green?style=flat-square&logo=nvidia&logoColor=white">
</p>

## Why it exists

1. **More headroom under pressure** — compile, containers, browsers stop thrashing the SSD as hard.  
2. **GPU stays usable** — VRAM is a *cold* overflow tier; under host GPU pressure we **DEMOTE** (give VRAM back).  
3. **Open source & measured** — numbers in the repo, not vibes.

This is **not** “free RAM for games at 100% GPU load.” It is a **swap cushion** for developer machines where the GPU often sits idle.

## Get started (3 steps)

**Needs:** Linux or WSL2, NVIDIA GPU + CUDA driver, Rust toolchain, sudo.

```bash
# 1) Build (once)
./scripts/quickstart.sh

# 2) Check machine + start 1 GiB zram + 1 GiB VRAM cascade
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024

# 3) Success = three tiers (or zram + VRAM + existing disk swap)
swapon --show
```

**Success looks like:** `swapon --show` lists **zram** (high priority), a **VRAM/NBD** device (medium), and your **disk/VHDX** swap (lowest).  
**Stop cleanly:** `sudo ./target/release/ramshared down`

Manual build (same result):

```bash
cargo build -p ramshared-cli -p ramshared-wsl2d --release
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
```

If `check` fails, run `sudo ./target/release/ramshared doctor` and fix the blockers it prints.

---

## Will this break my PC? (FAQ)

**Short answer: designed not to.** Swap is a normal Linux feature; we add a tier and a safety valve.

| Fear | What we do |
| --- | --- |
| “GPU will freeze Windows” | Under WDDM pressure VRAM can get **slow** (~1.18 s on a 4K read, measured). We treat VRAM as **cold** and can **DEMOTE** ( `swapoff` only the VRAM tier ) so pages move to disk **without killing your apps**. |
| “Data corruption” | Cascade drill logged **~511 MiB** on VRAM and **~481 MiB** demoted with **0 corruption** (see reliability docs). |
| “It will thrash my WSL2” | We **forbid** thrash tests on the live WSL2 host. Real pressure drills run in **isolated VMs**. |
| “I can’t undo it” | `sudo ./target/release/ramshared down` tears the cascade down. |

Full FAQ: [`docs/FAQ.md`](docs/FAQ.md).

---

## How it works (one picture, three lines)

```text
Memory pressure → zram (HOT, prio 200) → VRAM (COLD, prio 100) → disk (LAST, prio −2)
```

- **zram** — compressed RAM for the hot working set.  
- **VRAM** — idle GPU memory as cold overflow (CUDA + NBD/ublk daemon).  
- **DEMOTE** — latency canary / host pressure → drop VRAM tier → pages fall to disk, processes keep running.

**Measured (registered):** ublk p50 **~241 µs** vs NBD-Unix **~326 µs**; WDDM stall **~1.18 s** on small reads under pressure. Sources: [`docs/reliability/`](docs/reliability/).

---

## What this is *not* (on purpose)

| Out of day-1 pitch | Where it lives |
| --- | --- |
| Multi-tenant memory broker | specs / later phases |
| Windows StorPort miniport | specs — not the first install path |
| Bare-metal CXL / HMM NUMA | roadmap, not WSL2 |

Day-1 product = **cascade on Linux/WSL2**. Everything else is optional depth.

---

<details>
<summary><strong>PT-BR</strong> — resumo em português</summary>

### Em uma frase
Quando a RAM acaba, usa a memória ociosa da placa de vídeo como colchão — e devolve se a GPU precisar.

### Três pontos
1. Mais fôlego sob pressão (compile, containers, browser).  
2. GPU continua utilizável (tier frio + DEMOTE).  
3. Open source e medido, com limites claros.

### Começar
```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show   # sucesso = zram + VRAM + disco
sudo ./target/release/ramshared down
```

### Vai quebrar o PC?
Não por design: DEMOTE devolve a VRAM sob pressão do host; `down` desliga a cascata; não fazemos thrash no WSL2 live. Detalhes em [`docs/FAQ.md`](docs/FAQ.md).

</details>

---

## Docs map

| Audience | Doc |
| --- | --- |
| You (user) | This README + [`docs/FAQ.md`](docs/FAQ.md) |
| Social posts | [`docs/marketing/LAUNCH-KIT.md`](docs/marketing/LAUNCH-KIT.md) |
| Demo recording | [`docs/marketing/DEMO.md`](docs/marketing/DEMO.md) |
| Architecture deep dive | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Specs / engineering process | [`docs/INDEX.md`](docs/INDEX.md), [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md) |
| Contributing | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## Launch kit (social)

* [`docs/marketing/LAUNCH-KIT.md`](docs/marketing/LAUNCH-KIT.md) — EN/PT copy, step IDs for r/rust  
* [`docs/marketing/cascade-diagram.png`](docs/marketing/cascade-diagram.png)

## Crate layout (for contributors)

| Crate | Role |
|---|---|
| `ramshared-tier` | Cascade priorities + DEMOTE invariants |
| `ramshared-cuda` | CUDA Driver API via `dlopen` (**only unsafe boundary**) |
| `ramshared-block` | NBD protocol / I/O |
| `ramshared-integrity` | Block checksums |
| `ramshared-uring` | `io-uring` wrapper |
| `ramshared-wsl2d` / `ramsharedd` | Daemon: VRAM backend, canary, residency |
| `ramshared-cli` / `ramshared` | `check` / `doctor` / `up` / `down` / `status` |

## Contribution (engineers)

Structural work (locks, DMA, uAPI, mm) uses SSDV3 under `docs/specs/…`. See [`CONTRIBUTING.md`](CONTRIBUTING.md). Non-trivial commits need a numerical `Rollback trigger:` when they touch memory/sync paths.
