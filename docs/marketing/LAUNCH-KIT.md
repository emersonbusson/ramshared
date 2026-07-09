# RamShared — Launch / Launch kit (EN + PT-BR)

> **Purpose:** One place to promote RamShared without inventing metrics or re-litigating architecture in every post.
> **Rule (Kahneman #3):** every number below must already live in `docs/reliability/*` or `docs/BENCHMARKS.md`. If you change a number, update the source first, then this kit.
> **Repo:** https://github.com/emersonbusson/ramshared

### Public one-liners (always lead with these)

| ID | Lang | Text |
| --- | --- | --- |
| **L-EN-1** | EN | When your PC runs out of RAM, use idle GPU memory as a safety cushion — automatically, and pull back if the GPU gets busy. |
| **L-PT-1** | PT | Quando a RAM acaba, usa a memória ociosa da placa de vídeo como colchão — e devolve se a GPU precisar. |

**Three bullets (any channel — human first):**

1. More breathing room when RAM is under pressure (compile, containers, browser).  
2. The GPU can still work — we **give the cushion back** if the system needs graphics memory.  
3. Open source and measured, with honest limits (not “free RAM for maxed-out games”).

Prefer “give the cushion back” over “DEMOTE” in social posts; define DEMOTE only if someone asks.

User-facing: [`docs/FAQ.md`](../FAQ.md) · Demo: [`DEMO.md`](DEMO.md) · Install: `./scripts/quickstart.sh` · Public README leads with plain language.

---

## START HERE — Today’s post only (r/rust)

Do **only** this checklist. Do not open X/LinkedIn today.

| Step ID | Action | Copy ID (what to paste) |
| --- | --- | --- |
| **S0** | Open this file on GitHub or locally | — |
| **S1** | Go to https://www.reddit.com/r/rust → **Create Post** → type **Text** | — |
| **S2** | Subreddit must be **r/rust** | — |
| **S3** | Paste **title** → use **`T-EN-1`** below | **T-EN-1** |
| **S4** | Paste **body** → use **`B-EN-1`** below | **B-EN-1** |
| **S5** | Attach image → file **`IMG-1`** = `docs/marketing/cascade-diagram.png` | **IMG-1** |
| **S6** | Flair (if list shows it) → **Show & Tell** | **FLAIR-1** |
| **S7** | Click **Post** | — |
| **S8** | **Stop.** Close the tab. Nothing else today. | — |

### Quick paste map (r/rust today)

| Copy ID | What it is | Where in this file |
| --- | --- | --- |
| **T-EN-1** | Title (English) | § English → Reddit — r/rust → **Title** |
| **B-EN-1** | Body (English) | § English → Reddit — r/rust → **Body** |
| **IMG-1** | Cascade diagram PNG | `docs/marketing/cascade-diagram.png` |
| **FLAIR-1** | Reddit flair name | `Show & Tell` (skip if r/rust has no flair) |

Download **IMG-1**:  
https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png  
(→ raw/download, then upload on Reddit)

---

## Assets

| File | Use |
| --- | --- |
| [`cascade-diagram.svg`](cascade-diagram.svg) | Source diagram (edit labels here) |
| [`cascade-diagram.png`](cascade-diagram.png) | Attach to Reddit / X / LinkedIn (1200×675) |

Regenerate PNG after editing the SVG (example):

```bash
convert -background none docs/marketing/cascade-diagram.svg docs/marketing/cascade-diagram.png
# or: rsvg-convert -w 1200 docs/marketing/cascade-diagram.svg -o docs/marketing/cascade-diagram.png
```

---

## Registered numbers (do not improvise)

| Claim | Value | Source |
| --- | --- | --- |
| WDDM / host eviction 4K stall | up to **~1.18 s** | `docs/reliability/wsl2-fase0-final.md`, `ARCHITECTURE.md` |
| ublk block path p50 | **~241 µs** | `docs/reliability/memory-broker-p0-results.md` (Phase B baseline) |
| NBD-Unix block path p50 | **~326 µs** | same |
| Spill to VRAM tier | **~511 MiB** (332,800 pages intact) | `docs/reliability/wsl2-cascade-validation.md`, `ARCHITECTURE.md` |
| Demote VRAM → disk | **~481 MiB**, **0 corruption** | same |
| Idle VRAM framing | **~90% idle** (non-graphical workloads) | `README.md` (product framing) |

Measurement policy: [`.claude/rules/benchmarks.md`](../../.claude/rules/benchmarks.md).

---

## Pattern (channel-agnostic)

| Block | Job | Reddit | X/Twitter | LinkedIn | HN |
| --- | --- | --- | --- | --- | --- |
| Hook | concrete problem | title + first line | tweet 1 | headline | title |
| One-liner | what it is | paragraph | tweet 1–2 | lead | first line |
| Numbers | credibility | bullets / table | one numbers tweet | bullets | bullets |
| How | architecture | cascade + DEMOTE | 1–2 tweets | short diagram text | short paragraph |
| Honest limits | anti-hype | section | one tweet | bullets | near top |
| Stack | community | Rust list | light tags | tech stack | “Written in Rust” |
| Ask | engagement | technical question | question | “who faces this?” | “looking for…” |
| Link | CTA | end | last tweet | end | end |

**Same story, different shape.** Do not paste the full Reddit body into X.

### Suggested publish order

1. **Day 0:** r/rust (EN) + diagram PNG  
2. **+2–6 h:** X EN thread (link repo; link Reddit only if it has traction)  
3. **+1 d:** r/brdev or X PT  
4. **+2 d:** LinkedIn EN or PT (summary + repo)

Avoid same-day spam across many subreddits (r/rust: no low-effort content).

---

## Channel notes

### r/rust

- Prefer **English**.
- Flair: Show & Tell if available.
- Rules from the sub: on-topic, constructive, **no low-effort**, keep perspective.
- Tone: peer review, not pitch deck. Numbers and limits sell better than adjectives.

### X / Twitter

- Thread 4–5 tweets max.
- One visual: `cascade-diagram.png`.
- First line must stand alone if truncated.

### LinkedIn

- Professional systems/efficiency framing.
- Shorter than Reddit; still include limits.

### Hacker News

- Title without `[Show & Tell]`.
- Limits early; expect technical roast.

---

# English copy

## Reddit — r/rust

### T-EN-1 — Title (paste into Reddit “Title” field)

```text
[Show & Tell] RamShared — idle GPU VRAM as a cold swap tier on Linux/WSL2 (zram → VRAM → disk), with measured DEMOTE under WDDM pressure
```

### B-EN-1 — Body (paste into Reddit “Text” / markdown field)

Attach **IMG-1** (`cascade-diagram.png`) when Reddit asks for image / media.

```markdown
**When your PC runs out of RAM, use idle GPU memory as a safety cushion — and give it back if the GPU gets busy.**

I built **RamShared** (Rust, Linux/WSL2, NVIDIA): a practical way to borrow **idle graphics memory** when system RAM is tight, without pretending GPU memory is as safe/fast as main RAM.

## Problem (human)
You’re compiling / running containers / drowning in tabs. RAM is gone. The machine starts thrashing the **SSD**. Meanwhile the **GPU memory** is often almost empty. You already paid for that silicon.

## Why not “just put all swap on the GPU”?
When Windows reclaims graphics memory under pressure, that memory can get **very slow**. We measured about **1.2 seconds** for a tiny read in the bad case. If that were your *first* emergency store, the whole machine freezes. So GPU memory is only a **second** cushion — and we can **give it back**.

## Design (still short)
```text
Need memory?  →  1) compressed RAM (zram)     — first, fast
              →  2) idle GPU memory           — second, colder
              →  3) disk (SSD / VHDX)         — last resort
```

If latency spikes / host pressure: **stop using the GPU cushion**, data slides to disk, **apps keep running** (we call that path DEMOTE in the code).

## Numbers (measured)
- Bad case under host GPU reclaim: up to **~1.2 s** for a small read (why GPU is second, not first).
- Faster plumbing path **~241 µs** median vs older path **~326 µs** (same window, multi-run).
- Stress drill: **~500 MB** on GPU tier, **~480 MB** moved back, **0 corruption**.

## Try it
```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show   # success ≈ three lines: zram + GPU + disk
```

## Honest limits
- Day-1 path is **Linux/WSL2 + NVIDIA**, not “every GPU / every OS.”
- Not free RAM for maxed-out games.
- We don’t thrash live WSL2 on purpose; heavy tests use isolated VMs.
- Not bare-metal CXL magic — practical workstation tool.

## Looking for feedback
Especially from people who’ve fought **swap, block devices, CUDA, or WSL2**:
1. Second-cushion + give-back vs other APIs under Windows GPU reclaim.
2. What you’d want in a “it just works” install.
3. Where the safety story still feels thin.

Repo + plain FAQ: https://github.com/emersonbusson/ramshared  
```

## X / Twitter — EN thread

> Do **not** post today. Only after S1–S8 (r/rust) are done.  
> Thread IDs: **X-EN-1** … **X-EN-5** (one tweet each). Attach **IMG-1** on **X-EN-1** or **X-EN-3**.

### X-EN-1 — hook

```text
Your GPU sits ~90% idle while your laptop swaps compile jobs to SSD.

I open-sourced RamShared: idle VRAM as a *cold* Linux/WSL2 swap tier (zram → VRAM → disk), with measured DEMOTE when WDDM gets nasty.

https://github.com/emersonbusson/ramshared
```

### X-EN-2 — constraint

```text
Why not “just swapon VRAM”?

Under host GPU pressure (WDDM/GPU-PV) we measured ~1.18s stalls on 4K reads. Data-safe ≠ latency-safe. Hot swap on that freezes the machine.
```

### X-EN-3 — design

```text
Cascade:

zram  prio 200  HOT   (compressed RAM)
VRAM  prio 100  COLD  (CUDA + NBD/ublk)
disk  prio  -2  LAST

Canary latency spike → DEMOTE (swapoff VRAM only) → pages drain to disk, processes keep running.
```

### X-EN-4 — numbers

```text
Measured:
• ublk p50 ~241µs vs NBD-Unix ~326µs (same window, ≥3 runs)
• ~511 MiB on VRAM tier; ~481 MiB demoted; 0 corruption in cascade drill

Rust 2024 workspace. Feedback welcome from swap/block/CUDA folks.
```

### X-EN-5 — limits + ask

```text
Honest limits: WSL2 ≠ bare-metal CXL. No thrash on live WSL2 host (freeze risk). Not free RAM under full GPU load.

What would you challenge first—cold-tier design or the demote path?
```

## LinkedIn — EN (short)

```text
Open source: RamShared — use idle GPU VRAM as a cold swap tier on Linux/WSL2.

Problem: workstations burn money on DDR while GDDR sits idle; when RAM is gone, the machine swaps to SSD.

Constraint: under WDDM/GPU-PV pressure, VRAM can stall ~1.18s on small reads — fine for data integrity, fatal as hot swap.

Approach: zram (hot) → VRAM (cold) → disk (last). Latency canary can DEMOTE the VRAM tier without killing processes.

Measured: ublk p50 ~241µs vs NBD-Unix ~326µs; cascade drills with hundreds of MiB demoted and 0 corruption logged.

https://github.com/emersonbusson/ramshared

Curious how others handle GPU memory pressure on WSL2 / hybrid hosts.
```

## Hacker News — title + comment body

**Title**

```text
RamShared – idle GPU VRAM as cold swap on Linux/WSL2 (zram → VRAM → disk)
```

**Text** (optional self-post style comment if link post)

Use the Reddit EN body, but move **Honest limits** above Numbers and drop flair language.

---

# Portuguese copy (PT-BR)

## Reddit — r/brdev (ou similar)

> Só **depois** de S1–S8. IDs: **T-PT-1** (título), **B-PT-1** (corpo), **IMG-1** (mesma imagem).

### T-PT-1 — Título

```text
[Show] RamShared — VRAM ociosa da GPU como tier frio de swap no Linux/WSL2 (zram → VRAM → disco) com DEMOTE medido sob pressão WDDM
```

### B-PT-1 — Corpo

Anexar **IMG-1**.

```markdown
**Quando a RAM acaba, usa a memória ociosa da placa de vídeo como colchão — e devolve se a GPU precisar.**

Montei o **RamShared** (Rust, Linux/WSL2, NVIDIA): emprestar **memória ociosa da GPU** quando a RAM do sistema aperta, sem fingir que a memória da placa é tão segura/rápida quanto a RAM principal.

## Problema (humano)
Compile, containers, mil abas. A RAM acaba. O PC engasga no **SSD**. Enquanto isso a **memória da placa de vídeo** está quase vazia — e você já pagou por ela.

## Por que não “jogar todo o swap na GPU”?
Quando o Windows recupera memória da GPU sob pressão, essa memória pode ficar **muito lenta** (medimos cerca de **1,2 s** numa leitura pequena no pior caso). Se isso for o *primeiro* recurso de emergência, a máquina trava. Por isso a GPU é só o **segundo** colchão — e dá para **devolver**.

## Design (curto)
```text
Precisa de memória?  →  1) RAM comprimida     — primeiro, rápido
                     →  2) GPU ociosa         — segundo, mais “frio”
                     →  3) disco (SSD/VHDX)   — último
```

Se a latência disparar: **paramos de usar o colchão da GPU**, os dados vão pro disco, **os apps continuam**.

## Números (medidos)
- Pior caso sob pressão da GPU no host: até **~1,2 s** numa leitura pequena.
- Caminho mais rápido ~**241 µs** vs caminho antigo ~**326 µs** (várias rodadas).
- Stress: ~**500 MB** no colchão da GPU, ~**480 MB** de volta, **0 corrupção**.

## Experimentar
```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show   # sucesso ≈ três linhas
```

## Limites honestos
- Dia 1: **Linux/WSL2 + NVIDIA**, não “qualquer GPU / qualquer SO”.
- Não é RAM grátis para jogo no talo.
- Não thrashamos WSL2 do dia a dia de propósito.
- Não é mágica bare-metal / CXL — ferramenta de workstation.

## Feedback
1. Colchão secundário + devolver vs outras abordagens.
2. O que falta para “só funciona”.
3. Onde a história de segurança ainda parece frágil.

Repo + FAQ simples: https://github.com/emersonbusson/ramshared
```

**Nota:** no **r/rust**, preferir a versão **EN**. PT para r/brdev, LinkedIn BR, X PT.

## X / Twitter — thread PT

**1**

```text
Sua GPU fica ~90% ociosa enquanto o notebook joga compile no swap de SSD.

Open source: RamShared — VRAM ociosa como tier *frio* de swap no Linux/WSL2 (zram → VRAM → disco), com DEMOTE medido quando o WDDM aperta.

https://github.com/emersonbusson/ramshared
```

**2**

```text
Por que não “só swapon na VRAM”?

Sob pressão da GPU no host (WDDM/GPU-PV) medimos ~1,18s de stall em leitura 4K. Data-safe ≠ latency-safe. Swap quente nisso congela a máquina.
```

**3**

```text
Cascata:

zram  prio 200  QUENTE
VRAM  prio 100  FRIO
disco prio  -2  ÚLTIMO

Spike de latência → DEMOTE (swapoff só da VRAM) → páginas caem pro disco, processos seguem.
```

**4**

```text
Números:
• ublk p50 ~241µs vs NBD-Unix ~326µs (≥3 runs, mesma janela)
• ~511 MiB no tier VRAM; ~481 MiB demote; 0 corrupção no drill

Rust 2024. Feedback de quem mexe com swap/bloco/CUDA.
```

**5**

```text
Limites: WSL2 ≠ CXL bare-metal. Sem thrash no WSL2 live. Não é RAM grátis com GPU a 100%.

O que você atacaria primeiro: o tier frio ou o caminho de demote?
```

## LinkedIn — PT (curto)

```text
Open source: RamShared — VRAM ociosa da GPU como tier frio de swap no Linux/WSL2.

Problema: workstations pagam caro por DDR enquanto GDDR fica ociosa; sem RAM, o sistema troca em SSD.

Constraint: sob pressão WDDM/GPU-PV, VRAM pode stallar ~1,18s em leituras 4K — íntegra, mas inaceitável como swap quente.

Abordagem: zram (quente) → VRAM (fria) → disco (último). Canário de latência pode fazer DEMOTE do tier VRAM sem matar processos.

Medido: ublk p50 ~241µs vs NBD-Unix ~326µs; drills de cascata com centenas de MiB demovidos e 0 corrupção logada.

https://github.com/emersonbusson/ramshared

Curioso como outras equipes lidam com pressão de memória + GPU em WSL2 / hosts híbridos.
```

---

## Reply kit (EN / PT)

| Attack | EN | PT |
| --- | --- | --- |
| Why not zswap only? | zswap still needs a *backing* store under pressure; we add idle VRAM as an extra cold tier, not a replacement for zram. | zswap ainda precisa de backing sob pressão; a VRAM ociosa entra como tier frio extra, não como substituto do zram. |
| AMD / non-NVIDIA? | CUDA path is first; backend is trait-shaped for more providers later — no false claim of multi-vendor today. | Caminho CUDA primeiro; backend via trait para outros providers depois — sem fingir multi-vendor hoje. |
| Bare metal / CXL? | Long-term roadmap; WSL2/GPU-PV is the shippable path *now* with measured constraints. | Roadmap de longo prazo; WSL2/GPU-PV é o caminho shipável *agora*, com constraints medidas. |
| Freezes my WSL2? | We ban thrash on live WSL2; pressure drills run in isolated VMs. Demote exists because host eviction is real. | Proibimos thrash no WSL2 live; pressão em VM isolada. DEMOTE existe porque eviction no host é real. |

---

## Checklist before posting

- [ ] Numbers still match the sources table above  
- [ ] Diagram attached where the channel allows images  
- [ ] Limits section present (especially r/rust / HN)  
- [ ] One technical ask (not “please star”)  
- [ ] Link: https://github.com/emersonbusson/ramshared  
- [ ] EN on r/rust; PT on BR channels  

---

## Maintenance

When a number changes:

1. Update `docs/reliability/*` or `docs/BENCHMARKS.md` first.  
2. Update the **Registered numbers** table in this file.  
3. Grep this file for the old number and fix copy blocks.  
4. Regenerate `cascade-diagram.png` if labels change.
