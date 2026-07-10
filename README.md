# RamShared

Your PC runs out of RAM. The disk kicks in and everything feels sticky.  
Meanwhile the **GPU often has free memory sitting idle**.

**RamShared borrows that idle GPU memory as a spare cushion — and gives it back when a game or 3D app needs the card.**

That’s it. No magic “free RAM for maxed-out games.” Just more room when you’re compiling, running containers, or drowning in browser tabs.

![Cascade: compressed RAM → idle GPU → disk](docs/marketing/cascade-diagram.png)

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Linux/WSL2" src="https://img.shields.io/badge/use%20this%20on-Linux%20%7C%20WSL2-blue?style=flat-square">
  <img alt="Windows lab" src="https://img.shields.io/badge/Windows%20driver-lab%20VM%20only-orange?style=flat-square">
</p>

---

## What’s ready today (honest)

| You want… | Ready? |
| --- | --- |
| Use it on **Linux or WSL2** with an NVIDIA GPU | **Yes** — this is the product |
| Turn it on every time WSL boots | **Yes, if you opt in** (script below) |
| Give VRAM back when you open a game on Windows | **Yes by design** (automatic DEMOTE) — you may feel a short slowdown, not a permanent freeze |
| Install a Windows kernel driver on your daily PC | **No** — lab VM only; do not load it on a machine you care about |

We measure things and write down what broke. Details: [validation.md](validation.md), [docs/FAQ.md](docs/FAQ.md).

---

## Try it in a few minutes

**Need:** Linux or WSL2 · NVIDIA GPU (`nvidia-smi` works) · [Rust](https://rustup.rs/) · sudo.

```bash
./scripts/quickstart.sh

sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
```

You want **about three** emergency-memory lines:

1. **zram** — compressed system RAM (first)  
2. **Something GPU-backed** (often `nbd0`) — idle graphics memory (second)  
3. **Disk / VHDX** — last resort  

```bash
sudo ./target/release/ramshared down
```

If `check` says blocked, run `sudo ./target/release/ramshared doctor` and fix what it prints.

Start small (`1024` MiB). Don’t grab your whole VRAM if you also game on the same card.

---

## Will this freeze WSL2?

**Normal use is built so it should not.** Freezes we hit in the past came from bad shutdowns (killing the daemon while swap was still on the GPU device). The code now **refuses** that path: it always turns swap off **before** stopping the daemon.

What *can* still happen:

| Situation | What you might feel |
| --- | --- |
| Open a heavy game while the cushion is on | WSL may **slow down for a bit** while pages leave the GPU | 
| Host Windows reclaims GPU memory hard | Tiny reads can take ~**1 second** until demote finishes |
| You force-kill processes / thrash swap on purpose | Don’t. That’s how people hang systems |

**DEMOTE** in plain words: we watch free GPU memory and latency. If the card is hungry, we stop using GPU as swap. Your apps keep running; data slides to disk.

---

## Auto-start when WSL boots (opt-in)

Needs **systemd** in the distro (`/etc/wsl.conf` → `[boot]` → `systemd=true`, then `wsl --shutdown` once).

```bash
# Install files only (safe):
sudo bash scripts/safety/install-cascade-boot.sh

# When you’re happy, turn it on for real:
sudo bash scripts/safety/install-cascade-boot.sh --enable
```

That unit:

1. **Checks** the machine first (refuses dirty/ghost swap, weak GPU free memory, missing tools).  
2. Runs `ramshared up` with sizes from `/etc/ramshared/cascade.conf`.  
3. On stop/shutdown, runs `ramshared down` (swap off first — the anti-hang path).

Undo:

```bash
sudo bash scripts/safety/uninstall-cascade-boot.sh
```

---

## How the cascade works

```text
Need memory?  →  compressed RAM (zram)     first
              →  idle GPU memory           second
              →  disk                      last
```

If the GPU needs memory back → **give-back (DEMOTE)** → disk holds the pages → apps stay alive.

Numbers we’ve actually seen (not slogans):

| Measurement | Result |
| --- | --- |
| Tiny read under host reclaiming GPU | up to ~**1.18 s** |
| Spill + demote (older run) | ~**511 MiB** out · ~**481 MiB** back · **0** corruption |
| Live demote drill | ~**648 MiB** moved · swapoff ~**15 s** · **0** corruption |

---

## Two tracks (don’t mix them up)

**Linux/WSL2** — what you install and use.  
**Windows StorPort driver** — research in a disposable Hyper-V VM. Lab is green for format/pagefile/teardown rules; **physical host load is still blocked.** Killing storage under a hot pagefile bluescreens (**0x7A**). We refuse that teardown on purpose.

---

<details>
<summary>Português (resumo direto)</summary>

### O que é
Quando a RAM aperta, usa VRAM ociosa como colchão e **devolve** se a placa precisar (jogo, render).

### Usar
```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
sudo ./target/release/ramshared down
```

### Boot automático (opcional)
```bash
sudo bash scripts/safety/install-cascade-boot.sh --enable
```

### Trava o WSL?
Uso normal: desenhado para **não**. Demote pode deixar lento por alguns segundos. Não mate o daemon na mão com swap ainda ativo.

### Windows driver no PC do dia a dia?
**Não.**

</details>

---

## Docs & code map

| If you need… | Open |
| --- | --- |
| Plain FAQ | [docs/FAQ.md](docs/FAQ.md) |
| How it’s built | [ARCHITECTURE.md](ARCHITECTURE.md) |
| What’s done / next | [ROADMAP.md](ROADMAP.md) |
| Live “did it work?” log | [validation.md](validation.md) |
| Boot feature (SSDV3) | [docs/specs/no-milestone/wsl2-cascade-boot/](docs/specs/no-milestone/wsl2-cascade-boot/) |
| Windows lab | [docs/specs/no-milestone/windows-swap-driver/](docs/specs/no-milestone/windows-swap-driver/) |
| Contributing | [CONTRIBUTING.md](CONTRIBUTING.md) |

| Crate / tree | Role |
| --- | --- |
| `ramshared` CLI | check, doctor, up, down, status |
| `ramsharedd` | serves GPU memory over NBD |
| `ramshared-tier` | priorities + demote safety net |
| `ramshared-cuda` | NVIDIA driver (only `unsafe` boundary) |
| `drivers/windows/` | StorPort lab only |

Patches that touch locks, DMA, or kernel contracts go through SSDV3 under `docs/specs/…`.
