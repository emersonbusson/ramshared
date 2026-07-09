# RamShared

**When your PC runs out of RAM, use idle GPU memory as a safety cushion — automatically, and give it back if the GPU gets busy.**

> **PT:** Quando a RAM aperta, usa memória ociosa da placa de vídeo — e devolve se a GPU precisar.

<p align="center">
  <img alt="RamShared: memory pressure goes to zram, then idle GPU memory, then disk" src="docs/marketing/cascade-diagram.png" width="900">
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Linux/WSL2" src="https://img.shields.io/badge/Day--1-Linux%20%7C%20WSL2-blue?style=flat-square&logo=linux&logoColor=white">
  <img alt="Windows lab" src="https://img.shields.io/badge/Windows-lab%20VM%20only-orange?style=flat-square&logo=windows&logoColor=white">
  <img alt="CUDA" src="https://img.shields.io/badge/CUDA-runtime%20load-green?style=flat-square&logo=nvidia&logoColor=white">
  <img alt="Host-real" src="https://img.shields.io/badge/host--real%20Windows-blocked-red?style=flat-square">
</p>

---

## Project status (honest)

| Track | What it is | Maturity | Public install? |
| --- | --- | --- | --- |
| **Linux / WSL2 cascade** | zram → idle VRAM (NBD) → disk/VHDX + DEMOTE | **Day-1 product** — measured on live WSL2 | **Yes** — three commands below |
| **Native Windows (StorPort)** | Virtual disk + secondary pagefile on VRAM path | **Lab-complete on Hyper-V `win11-drill` only** | **No** — host-real load **forbidden** until product CUDA path + signing |

**What “lab-complete” means (Windows):** format/NTFS, pagefile residency (DT-21), kernel-page drill 3/3, ordered teardown (DT-9), B1 safe arm, delayed-auto lab SCM — all on a **disposable VM**. It does **not** mean install on your daily Windows host.

**What remains blocked:** physical-host driver load, Partner Center attestation, 72h soak, invented “K” latency without measurement, product `ramshared-winsvc` + `nvcuda.dll` on a real GPU box.

Authoritative detail: [`ROADMAP.md`](ROADMAP.md) · Windows IMPL gates · [`docs/specs/no-milestone/windows-swap-driver/IMPL.md`](docs/specs/no-milestone/windows-swap-driver/IMPL.md) · live log · [`validation.md`](validation.md).

---

## In plain words

| | |
| --- | --- |
| **Problem** | Your machine runs out of system RAM. It starts using the **disk** as emergency memory. Everything feels sticky. Meanwhile the **GPU memory** is often almost empty. |
| **Idea** | Use that idle GPU memory as an **extra cushion** when RAM is tight. |
| **Safety** | If Windows/Linux needs the GPU back, RamShared **stops using GPU memory for that cushion** and moves data to disk. Your apps keep running. |

**Who it’s for:** people on **Linux or WSL2** with an **NVIDIA GPU**, who compile code, run containers, or leave many browser tabs open — and hate waiting on disk swap.

**Who it’s not for (day 1):** “I want free RAM for a game at max settings.” Under full GPU load there is little idle memory to borrow. **Also not for:** installing the Windows kernel driver on a machine you care about — that path is VM-lab only.

### Three takeaways

1. **More breathing room** when RAM is under pressure.  
2. **The GPU can still work** — the cushion is given back if needed.  
3. **Open source**, with **measured** results in the repo (not marketing adjectives).

---

## Get started — Linux / WSL2 (about 5 minutes)

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

**We designed the Linux/WSL2 path so it shouldn’t.** Linux already uses disk as emergency memory; we only **add a middle cushion** and a **way out**.

| You might worry… | What actually happens |
| --- | --- |
| “Windows will freeze” (WSL2 GPU-PV) | When the host is hungry for GPU memory, that memory can get **very slow** (we measured about **1.2 seconds** for a tiny read under pressure). So we **don’t** treat GPU memory as the *first* place to put hot data. If things get bad, we **hand the cushion back** and push data to disk. Apps stay alive. |
| “My files will corrupt” | In a logged stress run we put **~500 MB** on the GPU cushion and moved **~480 MB** back to disk with **no corruption**. A later DEMOTE drill moved **~648 MiB** with **0 corruption**. |
| “I can’t undo it” | One command: `sudo ./target/release/ramshared down`. |
| “It will freeze WSL2” | We **don’t** run aggressive thrash tests on your daily WSL2. Heavy tests go in a **separate VM**. |
| “Windows kernel driver on my laptop” | **Don’t.** Lab only (`win11-drill`). Pagefile-hot surprise kill is **BugCheck 0x7A** by design; product path refuses that teardown (DT-9). |

More questions: **[docs/FAQ.md](docs/FAQ.md)**.

---

## How it works (still plain)

When memory is tight, Linux walks a **priority list** of emergency stores:

```text
Need memory?  →  1) compressed RAM (zram)     prio 200  HOT
              →  2) idle GPU memory (VRAM)    prio 100  COLD
              →  3) disk (SSD / VHDX)         prio  -2  LAST
```

| Layer | Everyday name | Role |
| --- | --- | --- |
| **zram** | Compressed RAM | First cushion — stays in system memory, just packed tighter |
| **VRAM** | Idle GPU memory | Second cushion — only for cooler data when zram isn’t enough |
| **disk** | Normal swap file | Last resort — slow but always there |
| **Give-back** | We call it DEMOTE | If the GPU is under pressure, stop using GPU memory; data slides to disk; apps keep running |

### Numbers we actually measured

| What | Result | Where |
| --- | --- | --- |
| Tiny read when the host is reclaiming GPU memory | up to **~1.18 s** (why GPU is only a *second* cushion) | Phase 0 / FASE0-FINAL |
| Stress spill to VRAM | **~511 MiB** intact · DEMOTE **~481 MiB** · **0 corruption** | acceptance §14 |
| Live DEMOTE action path | **~648 MiB** on nbd · swapoff **~14.8 s** · **0 corruption** · VHDX absorbed | `validation.md` 2026-07-09 |
| Windows lab (VM only) | LUN 64 MiB · pagefile Usage **25%** · KPD **3/3** · DT-9 refuse/reboot · SCM delayed-auto | Windows IMPL / `validation.md` |
| Windows pagefile-hot kill | **BugCheck 0x7A** / `c0000185` → mitigated by **DT-9** (never tear down hot) | DEGRADATION-MATRIX |

Methods: [`docs/reliability/`](docs/reliability/) · live log: [`validation.md`](validation.md).

---

## Two tracks (same product idea)

```text
                    ┌─────────────────────────────────────┐
                    │         Idle GPU memory             │
                    └───────────────┬─────────────────────┘
                                    │
           ┌────────────────────────┼────────────────────────┐
           ▼                                                 ▼
   Linux / WSL2 (Day-1)                          Windows native (lab)
   zram → NBD/CUDA daemon → disk                 StorPort miniport + pagefile
   DEMOTE = swapoff VRAM tier                    DT-9 ordered teardown
   public: ramshared up/down                     host-real: FORBIDDEN
```

| | Linux / WSL2 | Windows (P4) |
| --- | --- | --- |
| **Role** | Shippable cushion today | Secondary pagefile on virtual disk |
| **Backing** | CUDA userspace + NBD | StorPort virtual miniport + userspace backend |
| **Safety pivot** | Latency-unsafe WDDM → cold tier + DEMOTE | Pagefile-hot kill → 0x7A → DT-9 refuse |
| **Evidence env** | Live WSL2 + qemu drills | Hyper-V `win11-drill` only (RNF-6) |
| **Docs** | [`ARCHITECTURE.md`](ARCHITECTURE.md) · [`wsl2-cascade-swap`](docs/specs/no-milestone/wsl2-cascade-swap/) | [`windows-swap-driver`](docs/specs/no-milestone/windows-swap-driver/) · [`drivers/windows/`](drivers/windows/) |

---

## What we don’t push on day one

| Later / advanced | Why it’s not in “get started” |
| --- | --- |
| Multi-machine “broker” | Extra product surface — not required to try the cushion |
| **Windows kernel driver on a real host** | Lab-complete in VM; host-real still **blocked** (no theater) |
| Product Windows SCM + CUDA | Lab uses C# `RamSharedWinSvc` + file backend; product `ramshared-winsvc` + `nvcuda.dll` still env-bound |
| Future bare-metal / CXL / NUMA | Research roadmap, not the install path |

Day one = **one machine, Linux or WSL2, three commands, `swapon --show`**.

---

<details>
<summary><strong>PT-BR</strong> — o essencial em português</summary>

### Em uma frase
Quando a RAM aperta, usa memória ociosa da placa de vídeo — e devolve se a GPU precisar.

### Status do projeto (honesto)
| Trilha | Maturidade |
| --- | --- |
| **Linux / WSL2** | Produto day-1 — cascade zram → VRAM → disco + DEMOTE medido |
| **Windows nativo (StorPort)** | **Lab completo** só na VM Hyper-V `win11-drill` — **proibido** carregar no host físico |

### Em três pontos
1. Mais fôlego quando a memória aperta (compile, containers, abas).  
2. A placa de vídeo continua utilizável — essa memória **volta** se o sistema precisar da GPU.  
3. Código aberto e **medido**, com limites claros (não é “RAM grátis para jogo no talo”; não é driver Windows no laptop do dia a dia).

### Começar (Linux/WSL2)
```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show    # sucesso ≈ 3 linhas: zram + GPU + disco
sudo ./target/release/ramshared down
```

### Vai quebrar o PC?
No caminho Linux/WSL2, por desenho, não: a GPU é o **segundo** recurso, e um comando (`down`) desliga tudo. No Windows, kill com pagefile quente = **0x7A** — por isso o lab **recusa** teardown quente (DT-9) e host-real continua bloqueado. FAQ: [`docs/FAQ.md`](docs/FAQ.md).

</details>

---

## More docs (when you want depth)

| If you want… | Open |
| --- | --- |
| Simple Q&A | [`docs/FAQ.md`](docs/FAQ.md) |
| Architecture (both tracks) | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| What’s done / next | [`ROADMAP.md`](ROADMAP.md) |
| Empirical “does it work now?” | [`validation.md`](validation.md) |
| Failure modes | [`docs/reliability/DEGRADATION-MATRIX.md`](docs/reliability/DEGRADATION-MATRIX.md) |
| Windows lab / SPEC / IMPL | [`docs/specs/no-milestone/windows-swap-driver/`](docs/specs/no-milestone/windows-swap-driver/) |
| Post today (r/rust only) | [`docs/marketing/posts/01-reddit-rust-en.md`](docs/marketing/posts/01-reddit-rust-en.md) |
| All social posts | [`docs/marketing/posts/`](docs/marketing/posts/) |
| Record a 40s demo | [`docs/marketing/DEMO.md`](docs/marketing/DEMO.md) |
| Specs index | [`docs/INDEX.md`](docs/INDEX.md) |
| Contribute code | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## For contributors (crates)

| Crate / tree | Role |
| --- | --- |
| `ramshared-cli` / `ramshared` | User commands: check, doctor, up, down, status |
| `ramshared-wsl2d` / `ramsharedd` | Background service that serves GPU memory (Linux/WSL2) |
| `ramshared-tier` | Priority rules + give-back safety (A1 sink) |
| `ramshared-cuda` | Talks to NVIDIA driver (only `unsafe` FFI boundary; Unix + Windows loaders) |
| `ramshared-block` | Block I/O protocol + shared `VramBackend` |
| `ramshared-integrity` | Checksums |
| `ramshared-uring` | Async I/O helper |
| `ramshared-winsvc` | Windows userspace service (pure tests on Linux; product bin needs MSVC) |
| `drivers/windows/ramshared` | StorPort virtual miniport (C/WDK) — **VM lab only** until host-real gate |

Structural kernel-ish changes use SSDV3 under `docs/specs/…`. See [`CONTRIBUTING.md`](CONTRIBUTING.md) · methodology: [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md).
