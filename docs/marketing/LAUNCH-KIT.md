# RamShared — Launch / Launch kit (EN + PT-BR)

> **Purpose:** One place to promote RamShared without inventing metrics or re-litigating architecture in every post.
> **Rule (Kahneman #3):** every number below must already live in `docs/reliability/*` or `docs/BENCHMARKS.md`. If you change a number, update the source first, then this kit.
> **Repo:** https://github.com/emersonbusson/ramshared

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

**Title**

```text
[Show & Tell] RamShared — idle GPU VRAM as a cold swap tier on Linux/WSL2 (zram → VRAM → disk), with measured DEMOTE under WDDM pressure
```

**Body** (attach `cascade-diagram.png`)

```markdown
I built **RamShared**: a Rust userspace stack that puts **idle GPU VRAM** into the Linux swap hierarchy on **WSL2/Linux**, without pretending VRAM is as safe as RAM.

## Problem
On a workstation with a GPU, VRAM is often **~90% idle** during compile/containers, while system RAM is exhausted and the machine swaps to **SSD** (orders of magnitude slower than GDDR). Buying more DDR is expensive; the silicon is already paid for.

## Constraint (why not “just swap to VRAM”)
Under host GPU pressure (WDDM / GPU-PV), VRAM is **data-safe but latency-unsafe**. We measured **~1.18 s** stalls on 4K reads during host eviction. If VRAM were the *hot* swap tier, that freezes the system.

## Design
Priority cascade (kernel swap priorities):

```text
pressure → zram   (compressed RAM)   HOT   prio 200
        → VRAM  (CUDA + NBD/ublk)    COLD  prio 100
        → VHDX/SSD                   LAST  prio  -2
```

- **zram** absorbs the hot working set.
- **VRAM** is a *cold* overflow tier.
- A residency/canary path watches latency; on spike it **DEMOTE**s VRAM via `swapoff` so pages fall to disk **without killing processes**.

## Numbers (measured, not vibes)
- Phase 0: WDDM eviction → **up to ~1.18 s** 4K read under pressure (reason for cold-tier + DEMOTE).
- Block path: **ublk p50 ~241 µs** vs **NBD-Unix p50 ~326 µs** (same load window, ≥3 runs).
- Validation: **~511 MiB** spilled to VRAM tier; **~481 MiB** demoted VRAM→disk; **0 corruption** in the logged cascade drill.

## Stack
- Rust 2024 workspace (CLI + daemon + CUDA via `dlopen` FFI boundary).
- Swap cascade + DEMOTE safety net in userspace; Phase B work toward ublk / custom WSL2 kernel.

## Honest limits
- WSL2/GPU-PV is not bare-metal coherent CXL/HMM. This is a **practical** path for developer machines today.
- Thrashing swap/ublk on a live WSL2 host is intentionally forbidden in our harnesses (host freeze risk); real pressure in **isolated** VMs.
- Not a magic free-RAM button for games under full GPU load — VRAM under WDDM pressure is the hard case we design around.

## Looking for
Feedback from people who have done **swap / block / CUDA / WSL2** work:
1. Cold-tier + demote vs trying UVM / other APIs on consumer NVIDIA under WDDM.
2. ublk vs NBD trade-offs you’ve hit in production-ish setups.
3. Anything underspecified in the safety story.

Repo: https://github.com/emersonbusson/ramshared  
Build: `cargo build -p ramshared-cli -p ramshared-wsl2d` (see README).
```

## X / Twitter — EN thread

**1 — hook**

```text
Your GPU sits ~90% idle while your laptop swaps compile jobs to SSD.

I open-sourced RamShared: idle VRAM as a *cold* Linux/WSL2 swap tier (zram → VRAM → disk), with measured DEMOTE when WDDM gets nasty.

https://github.com/emersonbusson/ramshared
```

**2 — constraint**

```text
Why not “just swapon VRAM”?

Under host GPU pressure (WDDM/GPU-PV) we measured ~1.18s stalls on 4K reads. Data-safe ≠ latency-safe. Hot swap on that freezes the machine.
```

**3 — design**

```text
Cascade:

zram  prio 200  HOT   (compressed RAM)
VRAM  prio 100  COLD  (CUDA + NBD/ublk)
disk  prio  -2  LAST

Canary latency spike → DEMOTE (swapoff VRAM only) → pages drain to disk, processes keep running.
```

**4 — numbers**

```text
Measured:
• ublk p50 ~241µs vs NBD-Unix ~326µs (same window, ≥3 runs)
• ~511 MiB on VRAM tier; ~481 MiB demoted; 0 corruption in cascade drill

Rust 2024 workspace. Feedback welcome from swap/block/CUDA folks.
```

**5 — limits + ask**

```text
Honest limits: WSL2 ≠ bare-metal CXL. No thrash on live WSL2 host (freeze risk). Not free RAM under full GPU load.

What would you challenge first—cold-tier design or the demote path?
```

Attach `cascade-diagram.png` on tweet 1 or 3.

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

**Título**

```text
[Show] RamShared — VRAM ociosa da GPU como tier frio de swap no Linux/WSL2 (zram → VRAM → disco) com DEMOTE medido sob pressão WDDM
```

**Corpo** (anexar `cascade-diagram.png`)

```markdown
Montei o **RamShared**: stack em **Rust** que coloca **VRAM ociosa da GPU** na hierarquia de swap do Linux (**WSL2/Linux**), sem fingir que VRAM é tão segura quanto RAM.

## Problema
Em workstation com GPU, a VRAM fica **~90% ociosa** em compile/containers, enquanto a RAM acaba e o sistema troca em **SSD** (ordens de magnitude mais lento que GDDR). Comprar mais DDR é caro; o silício já está pago.

## Constraint (por que não “só swap pra VRAM”)
Sob pressão da GPU no host (WDDM / GPU-PV), VRAM é **data-safe mas latency-unsafe**. Medimos **~1,18 s** de stall em leitura 4K durante eviction do host. Se VRAM fosse o tier *quente* de swap, o sistema trava.

## Design
Cascata de prioridade (prioridades de swap do kernel):

```text
pressão → zram   (RAM comprimida)     QUENTE  prio 200
       → VRAM  (CUDA + NBD/ublk)      FRIO    prio 100
       → VHDX/SSD                     ÚLTIMO  prio  -2
```

- **zram** segura o working set quente.
- **VRAM** é overflow *frio*.
- Canário de latência: em spike, **DEMOTE** via `swapoff` da VRAM — páginas caem pro disco **sem matar processos**.

## Números (medidos)
- Fase 0: eviction WDDM → até **~1,18 s** em 4K sob pressão (motivo do tier frio + DEMOTE).
- Path de bloco: **ublk p50 ~241 µs** vs **NBD-Unix p50 ~326 µs** (mesma janela, ≥3 runs).
- Validação: **~511 MiB** no tier VRAM; **~481 MiB** no caminho demote; **0 corrupção** no drill logado.

## Stack
- Workspace Rust 2024 (CLI + daemon + CUDA via `dlopen`).
- Cascata de swap + rede de segurança DEMOTE em userspace; Fase B em ublk / kernel WSL2 custom.

## Limites honestos
- WSL2/GPU-PV ≠ bare-metal CXL/HMM. É o caminho **prático** para máquina de dev hoje.
- Não thrashamos swap/ublk no WSL2 live (congela o host); pressão real só em **VM isolada**.
- Não é “RAM grátis” com GPU a 100% em jogo — WDDM sob carga é o caso difícil.

## Quero feedback
1. Tier frio + demote vs UVM/outras APIs em NVIDIA consumer + WDDM.
2. Trade-offs ublk vs NBD que vocês já apanharam.
3. Furos na história de safety.

Repo: https://github.com/emersonbusson/ramshared
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
