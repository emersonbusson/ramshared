# RamShared

**When your PC runs out of RAM, use idle GPU memory as a safety cushion — automatically, and give it back if the GPU gets busy.**

> **PT:** Quando a RAM aperta, usa memória ociosa da placa de vídeo — e devolve se a GPU precisar.

<p align="center">
  <img alt="RamShared: memory pressure goes to zram, then idle GPU memory, then disk" src="docs/marketing/cascade-diagram.png" width="900">
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Platform" src="https://img.shields.io/badge/Platform-Linux%20%7C%20WSL2-blue?style=flat-square&logo=linux&logoColor=white">
  <img alt="CUDA" src="https://img.shields.io/badge/CUDA-Enabled-green?style=flat-square&logo=nvidia&logoColor=white">
</p>

## In plain words

| | |
| --- | --- |
| **Problem** | Your machine runs out of system RAM. It starts using the **disk** as emergency memory. Everything feels sticky. Meanwhile the **GPU memory** is often almost empty. |
| **Idea** | Use that idle GPU memory as an **extra cushion** when RAM is tight. |
| **Safety** | If Windows/Linux needs the GPU back, RamShared **stops using GPU memory for that cushion** and moves data to disk. Your apps keep running. |

**Who it’s for:** people on **Linux or WSL2** with an **NVIDIA GPU**, who compile code, run containers, or leave many browser tabs open — and hate waiting on disk swap.

**Who it’s not for (day 1):** “I want free RAM for a game at max settings.” Under full GPU load there is little idle memory to borrow.

### Three takeaways

1. **More breathing room** when RAM is under pressure.  
2. **The GPU can still work** — the cushion is given back if needed.  
3. **Open source**, with **measured** results in the repo (not marketing adjectives).

---

## Get started (about 5 minutes)

**You need:** Linux or WSL2 · NVIDIA GPU · driver that makes `nvidia-smi` work · [Rust](https://rustup.rs/) · sudo.

```bash
# 1) Build once
./scripts/quickstart.sh

# 2) Check the machine, then turn the cushion on (1 GB compressed RAM + 1 GB GPU)
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024

# 3) Did it work?
swapon --show
```

### You succeeded if…

`swapon --show` shows roughly **three** emergency-memory lines:

1. **zram** — first, “fast” cushion (compressed system RAM)  
2. A **GPU-backed** device — second cushion (idle graphics memory)  
3. **Disk / VHDX** — last resort (what Linux already uses)

Exact names vary. What matters is **order**: fast stuff first, disk last.

### Turn it off

```bash
sudo ./target/release/ramshared down
```

If step 2 says **blocked**, run:

```bash
sudo ./target/release/ramshared doctor
```

…and follow the checklist it prints.

Same thing by hand (if you prefer not to use the script):

```bash
cargo build -p ramshared-cli -p ramshared-wsl2d --release
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
```

---

## Will this break my PC?

**We designed it so it shouldn’t.** Linux already uses disk as emergency memory; we only **add a middle cushion** and a **way out**.

| You might worry… | What actually happens |
| --- | --- |
| “Windows will freeze” | When the host is hungry for GPU memory, that memory can get **very slow** (we measured about **1.2 seconds** for a tiny read under pressure). So we **don’t** treat GPU memory as the *first* place to put hot data. If things get bad, we **hand the cushion back** and push data to disk. Apps stay alive. |
| “My files will corrupt” | In our logged stress run we put **~500 MB** on the GPU cushion and moved **~480 MB** back to disk with **no corruption**. |
| “I can’t undo it” | One command: `sudo ./target/release/ramshared down`. |
| “It will freeze WSL2” | We **don’t** run aggressive thrash tests on your daily WSL2. Heavy tests go in a **separate VM**. |

More questions: **[docs/FAQ.md](docs/FAQ.md)** (short answers, less jargon).

---

## How it works (still plain)

When memory is tight, Linux walks a **priority list** of emergency stores:

```text
Need memory?  →  1) compressed RAM (zram)
              →  2) idle GPU memory (VRAM)
              →  3) disk (SSD / VHDX)
```

| Layer | Everyday name | Role |
| --- | --- | --- |
| **zram** | Compressed RAM | First cushion — stays in system memory, just packed tighter |
| **VRAM** | Idle GPU memory | Second cushion — only for cooler data when zram isn’t enough |
| **disk** | Normal swap file | Last resort — slow but always there |
| **Give-back** | We call it DEMOTE | If the GPU is under pressure, stop using GPU memory; data slides to disk; apps keep running |

### Numbers we actually measured

| What | Result |
| --- | --- |
| Tiny read when the host is reclaiming GPU memory | up to **~1.2 s** (why GPU memory is only a *second* cushion) |
| Fast path for GPU-backed swap (ublk) | about **241 µs** median |
| Older path (NBD) | about **326 µs** median |
| Stress drill | **~500 MB** on GPU tier · **~480 MB** moved back · **0 corruption** |

Details and methods: [`docs/reliability/`](docs/reliability/).

---

## What we don’t push on day one

| Later / advanced | Why it’s not in “get started” |
| --- | --- |
| Multi-machine “broker” | Extra product surface — not required to try the cushion |
| Windows kernel driver | Separate track — Linux/WSL2 cascade first |
| Future bare-metal / CXL ideas | Research roadmap, not the install path |

Day one = **one machine, Linux or WSL2, three commands, `swapon --show`**.

---

<details>
<summary><strong>PT-BR</strong> — o essencial em português</summary>

### Em uma frase
Quando a RAM aperta, usa memória ociosa da placa de vídeo — e devolve se a GPU precisar.

### Em três pontos
1. Mais fôlego quando a memória aperta (compile, containers, abas).  
2. A placa de vídeo continua utilizável — essa memória **volta** se o sistema precisar da GPU.  
3. Código aberto e **medido**, com limites claros (não é “RAM grátis para jogo no talo”).

### Começar
```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show    # sucesso ≈ 3 linhas: zram + GPU + disco
sudo ./target/release/ramshared down
```

### Vai quebrar o PC?
Por desenho, não: a GPU é o **segundo** recurso (não o primeiro), e um comando (`down`) desliga tudo. Sob pressão no Windows a memória da placa pode ficar lenta — por isso devolvemos essa memória em vez de travar o sistema. FAQ: [`docs/FAQ.md`](docs/FAQ.md).

</details>

---

## More docs (when you want depth)

| If you want… | Open |
| --- | --- |
| Simple Q&A | [`docs/FAQ.md`](docs/FAQ.md) |
| Post today (r/rust only) | [`docs/marketing/posts/01-reddit-rust-en.md`](docs/marketing/posts/01-reddit-rust-en.md) |
| All social posts (1 file each) | [`docs/marketing/posts/`](docs/marketing/posts/) |
| Record a 40s demo | [`docs/marketing/DEMO.md`](docs/marketing/DEMO.md) |
| Full architecture | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Specs / process | [`docs/INDEX.md`](docs/INDEX.md) |
| Contribute code | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## For contributors (crates)

| Crate | Role |
|---|---|
| `ramshared-cli` / `ramshared` | User commands: check, doctor, up, down, status |
| `ramshared-wsl2d` / `ramsharedd` | Background service that serves GPU memory |
| `ramshared-tier` | Priority rules + give-back safety |
| `ramshared-cuda` | Talks to NVIDIA driver (only `unsafe` boundary) |
| `ramshared-block` | Block I/O protocol |
| `ramshared-integrity` | Checksums |
| `ramshared-uring` | Async I/O helper |

Structural kernel-ish changes use SSDV3 under `docs/specs/…`. See [`CONTRIBUTING.md`](CONTRIBUTING.md).
