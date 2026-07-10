---
slug: wsl2-native-vram-tier
title: Native VRAM memory tier on WSL2 kernel and/or Ubuntu — decision PRD
milestone: —
issues: []
---

# PRD — VRAM “nativa” no kernel do WSL2 (e Ubuntu): o que é possível e onde testar

> **Tipo:** PRD de **decisão / descoberta** (SSDV3 Passo 1).  
> **Não** autoriza IMPL de LKM no WSL.  
> **Produto Day-1:** continua a cascata userspace (`wsl2-cascade-swap` + boot/app).  
> Relacionados: `kernel-vram-as-memory`, `mainline-vram-tiering`, lab `linux-kernel-lab`.

## 1. Summary

### O que este PRD responde

1. Como seria o kernel (WSL2 **e/ou** Ubuntu) fazer **nativamente** o papel de “usar VRAM ociosa como memória”, em vez de só o daemon NBD/CUDA.  
2. O que é **viável sob GPU-PV (WSL2)** vs o que só existe em **bare-metal / mainline**.  
3. **Onde testar** cada camada (WSL host, VM Hyper-V, dual-boot) — **sem** misturar os três.  
4. **Qual linguagem** usar em cada camada.

### Resposta curta

| Camada | “Nativo”? | Viável no WSL2 GPU-PV hoje? | Onde provar |
| --- | --- | --- | --- |
| **P0 — Cascade** (zram → VRAM NBD/CUDA → disco + DEMOTE) | Kernel faz **swap**; backend VRAM é **userspace** | **Sim** — produto | **WSL2** |
| **P1 — Kernel mais perto** (ublk, zram writeback, canário no kernel, prioridade/policy sysfs) | Mais kernel, ainda não “VRAM = página anônima” | **Parcial** (custom WSL kernel / config) | **WSL2** (+ build na VM) |
| **P2 — Device memory mm** (HMM / `DEVICE_PRIVATE` / tier de device) | Sim, mm nativo | **Não** de forma Day-0 limpa sob GPU-PV (VRAM é do WDDM host) | Bare-metal / dual-boot / DDA — **não** a VM sem GPU |
| **P3 — Mainline** | Upstream Linux | Fora do WSL como target primário | Mainline + hardware open |

**VM `linux-kernel-lab`:** serve para **compilar, kselftest, quebrar kernel sem matar o WSL**.  
**Não** prova sozinha “VRAM nativa no mm”, porque em geral **não enxerga a GPU** como device memory bare-metal.

**Dual-boot:** opcional; **não** é requisito do produto WSL. Espaço em E: já preparado se um dia for preciso.

---

## 2. Technical context

### 2.1 Confirmed (codebase / lab)

| Fato | Fonte | Class |
| --- | --- | --- |
| Cascade zram→VRAM→VHDX + DEMOTE medido | FASE0, validation, ADR-0001 | Confirmed |
| WDDM eviction: data-safe, latency-unsafe (~1,18 s) | FASE0-FINAL | Confirmed |
| WSL guest GPU-PV: vendor Microsoft `0x1414`, sem `/dev/dri` típico | PASSO0 inventory | Confirmed |
| Produto Day-1: Rust userspace + `ramshared up/down` | crates, README | Confirmed |
| Hyper-V `linux-kernel-lab`: Ubuntu cloudimg, lab auth | validation 2026-07-10 | Confirmed |
| Dual-boot space ~32 GB unallocated on E: | DUALBOOT-KERNEL-TRUE | Confirmed |

### 2.2 O que “nativo” **não** significa neste PRD

- Não significa “abandonar o WSL e só dual-boot”.  
- Não significa “a VM Linux é o ambiente de prova de VRAM”.  
- Não significa “NBD/CUDA deixam de existir amanhã no WSL”.

### 2.3 Modelo mental (três kernels)

```text
A) Kernel WSL2 (Microsoft / custom bzImage)
     - real Linux kernel, virtualized MM + GPU-PV
     - best product surface for RamShared today

B) Kernel Ubuntu genérico (VM Hyper-V ou dual-boot)
     - A-like if dual-boot/bare-metal GPU
     - VM without GPU: kernel engineering only

C) Kernel mainline (upstream)
     - long-term home for mm tier / HMM cooperation
```

---

## 3. Recommended option

### 3.1 Estratégia em fases (Day-0 honest)

| Phase | Nome | O que construir | Kernel “nativo”? |
| --- | --- | --- | --- |
| **P0** | Product bridge | Manter/melhorar cascade + demote + app/boot | Swap nativo; VRAM via userspace |
| **P1** | WSL kernel-closer | Custom WSL kernel options: ublk, `CONFIG_ZRAM_WRITEBACK` se útil; sysfs/policy; menos hop | Mais nativo no **I/O e política** |
| **P2** | Research device-memory | HMM/tier só com evidência de device memory real | Só fora de GPU-PV “limpo” |
| **P3** | Mainline | RFC + selftests (ver `mainline-vram-tiering`) | Upstream |

**Recomendação de produto:** **P0 obrigatório**; **P1** quando custom kernel WSL valer a pena; **P2/P3** sem bloquear o dia a dia.

### 3.2 Onde testar (matriz obrigatória)

| Hipótese | Ambiente de prova | Anti-padrão |
| --- | --- | --- |
| Cascade / demote / latência WDDM | **WSL2 no host com GPU** | Só VM sem GPU |
| Build kernel, checkpatch, kselftest sem GPU | **`linux-kernel-lab` (Hyper-V)** | Thrash no WSL diário |
| Crash de módulo / lockdep pesado | VM ou dual-boot | Host WSL de trabalho |
| “Página anônima em device memory” | Dual-boot/DDA + GPU real | VM sem GPU; WSL GPU-PV sozinho |
| Claim de mainline | QEMU selftests + um lab GPU | Chat-only |

### 3.3 Dual-boot neste PRD

**Opcional.** Não é o caminho “ligar WSL e usar”.  
Espaço em **E: ESPANHA (~32 GB unallocated)** existe se P2 precisar de bare-metal.  
R: RUSSIA continua mau para shrink NTFS (~2,7 GB shrinkable).

---

## 4. Functional requirements

| ID | Requirement | Phase |
| --- | --- | --- |
| RF-W1 | Documentar contrato P0: kernel swap + userspace VRAM backend + DEMOTE | P0 |
| RF-W2 | CLI/app continua fail-closed (ghost swap, swapoff-first) | P0 |
| RF-W3 | Se P1: lista explícita de `CONFIG_*` / ublk / writeback e rollback | P1 |
| RF-W4 | Qualquer uAPI kernel (sysfs/debugfs/ioctl) com `capable`, bounds, sem info-leak | P1+ |
| RF-W5 | Matriz de teste WSL vs VM vs bare-metal preenchida em IMPL | all |
| RF-W6 | Não afirmar “nativo VRAM mm” sem evidência de device memory / bare-metal ou DDA | all |
| RF-W7 | VM lab permanece isolada (sem senha lab ok); host UAC intocado | ops |

---

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-W1 | Latência: número + unidade + n≥3 se for gate (benchmarks.md) |
| NFR-W2 | Host safety: sem thrash no WSL diário |
| NFR-W3 | Day-0: sem dual-path “ImDisk forever” / shim eterno |
| NFR-W4 | Linguagem por camada (ver §8) — sem misturar no hot path |

---

## 6. Flows

### 6.1 Uso produto (o que o usuário “liga”)

```text
WSL starts → (optional systemd cascade) → ramshared up
  → kernel swapon order zram > vram > disk
  → pressure → pages to VRAM tier
  → GPU pressure → DEMOTE → disk
```

### 6.2 Engenharia P1 (custom WSL kernel)

```text
Build kernel (VM or WSL) → boot-kernel-safe → measure ublk/writeback
  → go/no-go vs NBD cascade
```

### 6.3 Pesquisa P2 (só se hardware permitir)

```text
Bare-metal or DDA → driver/device memory → migrate/demote pages
  → Gate B numbers → SPEC separado
```

---

## 7. Data model / interfaces (rascunho)

### P0 (hoje)

- Userspace: `ramshared`, `ramsharedd`, sockets NBD, `/proc/swaps`  
- Config: `/etc/ramshared/cascade.conf`

### P1 (futuro)

- Possível: sysfs `.../ramshared/` ou parâmetros de módulo  
- ublk device nodes se kernel custom  

### P2 (futuro)

- Memory tier / HMM registration — **SPEC próprio** após Gate A/B

Sem congelar ABI neste PRD.

---

## 8. Implementation languages (resposta à pergunta “qual linguagem?”)

| Camada | Linguagem **adequada** | Por quê |
| --- | --- | --- |
| **Kernel Linux** (LKM, mm, ublk glue, sysfs) | **C11 estilo mainline** (TAB 8, checkpatch) | ABI, reviewers, lockdep, ecossistema |
| **Kernel novo código “verde”** (opcional) | **Rust for Linux** só em subsistemas que o projeto já aceite e sem hot path opaco | Segurança de tipos; ainda menos familiar em mm crítico |
| **Daemon / CLI / broker / cascade (P0)** | **Rust** (já é o stack) | `forbid(unsafe)` fora de `ramshared-cuda`; performance e segurança |
| **FFI CUDA** | **Rust + `unsafe` isolado** em `ramshared-cuda` | Única fronteira unsafe userspace |
| **Windows StorPort lab** | **C (WDK)** + userspace Rust/C# lab | Kernel Windows |
| **Scripts lab / Hyper-V** | **PowerShell** (host) + **bash** (WSL) | Automação, não produto hot path |
| **Inadequado** para “nativo no kernel” | Python/Node/Go como LKM | Não é o modelo do kernel Linux |

### Recomendação prática do projeto

1. **Continuar P0 em Rust** (userspace).  
2. Qualquer **P1/P2 no kernel Linux → C primeiro** (patches/módulo estilo mainline).  
3. Avaliar **Rust for Linux** só se o SPEC P2 for código *novo* isolado e a toolchain WSL/custom kernel suportar — **não** reescrever mm inteiro em Rust.  
4. **Não** misturar “app zenity” com lógica de swap no mesmo processo privilegiado sem bounds.

---

## 9. Dependencies and risks

| Risco | Mitigação |
| --- | --- |
| Halo: “kernel nativo” = joga fora cascade | P0 permanece shippable |
| Halo: VM Linux prova VRAM | Matriz §3.2 |
| Custom WSL kernel brick boot | `boot-kernel-safe.ps1`, dual entry MS kernel |
| GPU-PV latency 1,18 s em path “quente” | VRAM sempre cold; demote |
| Scope P2 no WSL | PRD diz NO-GO até evidência |

---

## 10. Implementation strategy

| Step | Artifact | Env |
| --- | --- | --- |
| Now | Este PRD | — |
| P0 polish | IMPL cascade/app já em andamento | WSL |
| P1 SPEC (se go) | `SPEC.md` ublk/writeback/sysfs | WSL custom kernel; **build** na VM ok |
| P2 | Só após bare-metal/DDA evidence | Fora do escopo “só WSL” |
| P3 | `mainline-vram-tiering` | upstream |

---

## 11. Documents to update

- Este PRD + pointer em `docs/labs/DUALBOOT-KERNEL-TRUE.md` (opcional)  
- README: uma linha “native kernel = research; product = cascade on WSL”  
- `PASSO0` kernel-vram: link cruzado  

---

## 12. Out of scope

- Obrigar dual-boot para usar RamShared  
- Reescrever cascade em C  
- StorPort Windows como “kernel Linux nativo”  
- PROMETER P2 no GPU-PV  

---

## 13. Acceptance (deste PRD)

- [x] Distingue P0/P1/P2/P3  
- [x] Matriz de teste WSL vs VM vs bare-metal  
- [x] Dual-boot opcional, não central  
- [x] Linguagens por camada  
- [x] RF/NFR e riscos  
- [ ] SPEC P1: só se houver decisão de custom WSL kernel  

---

## 14. Validation

- Leitura cruzada ADR-0001, FASE0, cascade IMPL  
- Lab: WSL for P0; `linux-kernel-lab` for kernel **builds**; E: unallocated for optional dual-boot  

---

## 15. Kahneman

| # | Uso |
| --- | --- |
| #11 | Anti-halo “nativo = melhor no WSL agora” |
| #13 | Existir HMM ≠ GPU-PV expõe device memory |
| #2 | Counterfactual: VRAM hot no WSL → stall 1 s |
| #18 | Cascade sunset só com prova da mesma classe de problema no path nativo |

---

## 16. Plain language (for humans)

**What you turn on in WSL today** is not “the kernel maps VRAM like RAM.”  
It is “the Linux kernel’s **swap** uses a **GPU-backed disk** as a cold tier, and we pull that tier out if the GPU gets busy.”

**A “native kernel” future** would push more of that into mm/device-memory APIs — that is a **research** track (P2/P3), not the day-1 install.

**The Linux Hyper-V VM** is a **safe sandbox to build and crash kernels**, not the place that proves GPU VRAM nativeness without a real GPU path.

**Languages:** Rust for the product daemon/CLI; **C** for real Linux kernel work; scripts for lab only.
