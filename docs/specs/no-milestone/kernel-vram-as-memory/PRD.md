---
slug: kernel-vram-as-memory
title: Kernel-true VRAM as process memory (HMM / NUMA / DEVICE_PRIVATE) — decision PRD
milestone: —
issues: []
---

# PRD — VRAM “de verdade” no kernel (página de processo mapeia VRAM)

> **Tipo:** PRD de **decisão / descoberta** (Passo 1 SSDV3).  
> **Não** autoriza IMPL de LKM. SPEC só depois de **gates de ambiente** (§14).  
> **Produto Day-1 em produção:** continua a cascata WSL2 ([`wsl2-cascade-swap`](../wsl2-cascade-swap/PRD.md), ADR-0001).

## 1. Summary

O manifesto do RamShared prefere o **menor nível arquitetural estável**: HMM, NUMA, CXL — não wrappers eternos de userspace. A pergunta honesta é:

> **A melhor forma “linda” de usar VRAM ociosa é o kernel tratar VRAM como memória de processo (fault/migrate), em vez de swap em block device?**

**Resposta curta deste PRD:**

| Ambiente | É a melhor abordagem? |
| --- | --- |
| **WSL2 + GPU-PV (GeForce consumer)** | **Não** para Day-0. Sem DRM/BAR/`nvidia_p2p` útil no guest; VRAM sob WDDM é **latency-unsafe** (~1,18 s medido). Cascata zram→VRAM→disco permanece o path shippable. **Confirmed in docs** (ADR-0001, FASE0-FINAL). |
| **Linux bare-metal com GPU + BAR/ReBAR + stack DRM/HMM cooperativo** | **Sim, candidato de longo prazo** — alinha manifesto e remove o hop NBD/daemon. Exige hardware, driver e gates empíricos **antes** de SPEC/IMPL. **Inference** até medir no lab bare-metal. |
| **Windows host nativo** | Outro contrato (pagefile + miniport lab). Não é HMM Linux. Ver `windows-swap-driver`. |

Este PRD **não cancela** a cascata. Ele **abre a trilha K (kernel-true)** com critérios de go/no-go, para não misturar sonho de manifesto com o que roda no notebook WSL de hoje.

## 2. Technical context

### 2.1 O que “kernel de verdade” significa

Hoje (cascade):

```text
process page → anon → swap → block I/O (NBD) → userspace CUDA → VRAM
```

Kernel-true (alvo desta trilha):

```text
process page → page table / migration → device memory (VRAM) as memory tier
              (HMM migrate, DEVICE_PRIVATE, ou memória hotplug NUMA)
```

O processo **não** “lê um disco GPU”; a MMU / migrate path move páginas entre DRAM e device memory.

### 2.2 Por que a cascata existe (não é preguiça)

| Fato | Fonte | Classificação |
| --- | --- | --- |
| Eviction WDDM: dado íntegro, leitura 4K até **~1,18 s** | `docs/reliability/wsl2-fase0-final.md` | **Confirmed in docs** + empirical |
| Cascata zram→VRAM→VHDX comprovada (spill ~983 MiB VRAM) | FASE0 Part C, ADR-0001 | **Confirmed** |
| NUMA/HMM/`nvidia_p2p` rejeitados no WSL GeForce guest | ADR-0001 Alternatives | **Confirmed in docs** |
| Manifesto: bare-metal first, HMM/NUMA/CXL | `MANIFESTO.md` | **Confirmed in docs** |
| Cascade Day-1 + boot opt-in shippable | `wsl2-cascade-*`, README | **Confirmed in codebase** |

**Conclusão de contexto:** no GPU-PV, “mapear VRAM como RAM” **não elimina** o dono real da memória (WDDM no host). Um LKM no guest que finja que VRAM é DRAM **herda a mesma latência de reclaim** — só que no path de **page fault / migrate**, o que pode ser **pior** (stall no context do processo, sem demote de swapoff limpo).

### 2.3 Opções de kernel-true (árvore)

| Opção | Mecanismo | Precisa | WSL GPU-PV? |
| --- | --- | --- | --- |
| **K1 — HMM + `DEVICE_PRIVATE`** | migrate to/from device pages; fault-in | driver GPU + HMM + kernel config | **No** (sem stack cooperativo no guest consumer) |
| **K2 — NUMA hotplug via ReBAR/BAR** | `add_memory` / memory block de região PCIe | ReBAR, IOMMU, coerência ou regras explícitas de não-coerência | **No** no guest PV típico |
| **K3 — DRM/TTM “stolen” / carve-out** | memória gerida pelo DRM como pool | controle do driver DRM no SO | Bare-metal Linux possível; WSL não |
| **K4 — CXL / device coherent memory** | memória de dispositivo coerente | hardware CXL | Futuro; fora do lab atual |
| **C — Cascade (atual)** | swap priorities + NBD/CUDA + DEMOTE | CUDA userspace + nbd | **Yes — product** |

## 3. Recommended option

### 3.1 Decisão de produto (agora)

1. **Manter cascade** como único path **shippable** em WSL2/Linux GPU-PV.  
2. **Não** iniciar IMPL de LKM “VRAM = RAM” no WSL.  
3. **Abrir trilha K** com este PRD: pesquisa **bare-metal only**, gateada.  
4. SPEC da trilha K **só** se §14 gates passarem (hardware + medições).

### 3.2 Por que não “kernel de verdade” no WSL agora

| Argumento a favor do kernel | Contra-fato |
| --- | --- |
| “Mais lindo / manifesto” | Beleza sem caminho de I/O e reclaim = panic e freeze |
| “Sem hop userspace” | No PV, o hop real é **host WDDM**, não o NBD |
| “App confia na MMU” | Fault de 1 s em page-in é pior UX que demote de swap frio |
| “Day-0 limpo” | Day-0 limpo no WSL **já é** cascade (ADR-0001) |

### 3.3 Quando kernel-true **é** a melhor abordagem

Quando **todas** forem verdade:

1. Linux bare-metal (ou VM com GPU passthrough real), não só GPU-PV.  
2. Acesso a região de memória de device **visível** ao kernel (BAR/ReBAR ou API HMM do vendor).  
3. Medição de latência de migrate/fault sob pressão da GPU **antes** de expor a apps genéricas.  
4. Plano de **demote/offline** do nó/device memory sem UAF (equivalente A1 + canário).  
5. SSDV3 completo (SPEC + AUDIT-2.5 go) — locks, DMA, IRQ, lifetime.

Até lá, “kernel de verdade” é **objetivo de arquitetura**, não plano de sprint.

## 4. Functional requirements (trilha K — se gates passarem)

Escopo **somente bare-metal research**. IDs para traceability futura.

| ID | Requirement | Class |
| --- | --- | --- |
| RF-K1 | Expor um pool de device memory usável pelo mm (migrate ou hotplug) com tamanho configurável | Inference até lab |
| RF-K2 | Processos anônimos podem ter páginas em device memory sem block device de swap | Inference |
| RF-K3 | Sob pressão da GPU / reset / unplug: **offline ou migrate-back** para DRAM sem panic; apps sobrevivem ou recebem SIGBUS documentado | Confirmed pattern (cascade demote analog) |
| RF-K4 | Superfície uAPI mínima (sysfs/debugfs ou ioctl privileged) com `capable` + bounds | Confirmed practice (security rules) |
| RF-K5 | Zero dual-path “cascade + LKM” no mesmo host sem ADR de exceção Day-0 | Day-0 policy |
| RF-K6 | Cascade permanece instalável e documentada enquanto trilha K não tiver P0 numérico | Confirmed product need |

### Fora da trilha K (não confundir)

- Substituir cascade no WSL por LKM.  
- Windows StorPort como HMM.  
- Prometer “app mappeia VRAM” sem root/driver.

## 5. Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-K1 | Latência de fault/migrate sob idle e sob load GPU: **número** (p50/p99), ≥3 runs — regra benchmarks |
| NFR-K2 | Nenhum path de panic em GPU reset / D3; matriz de degradação atualizada |
| NFR-K3 | checkpatch/sparse/lockdep em qualquer LKM; sem `printk` solto |
| NFR-K4 | Host safety: pressão de memória só em VM isolada se o lab for compartilhado |
| NFR-K5 | Rollback trigger numérico no SPEC (ex.: p99 fault > p99 swap VHDX sob load → demote feature) |

## 6. Flows

### 6.1 Discovery (agora — este PRD)

```text
Pergunta manifesto → inventário de hardware lab
  → se WSL-only: STOP trilha K (cascade only)
  → se bare-metal GPU+BAR: Passo 0 medição (latência BAR/HMM probe)
  → se PASS: SPEC.md trilha K
  → se FAIL: append validation + manter cascade
```

### 6.2 Happy path (futuro, pós-SPEC)

1. Admin habilita pool device memory (tamanho ≤ free-floor GPU).  
2. mm coloca páginas frias em device memory (policy / cgroup / madvise — a decidir no SPEC).  
3. GPU app sobe → curator demove/offline device pages → DRAM.  
4. Processo continua.

### 6.3 Failure path

GPU reset mid-page → I/O/migrate fail → páginas DRAM ou sinal estável; **nunca** silent corruption.

## 7. Data model

| Conceito | Notas |
| --- | --- |
| Device memory pool | tamanho, nó NUMA ou HMM device, free-floor |
| Page state | DRAM / DEVICE / migrating (SPEC detalha) |
| Lease / holder | quem “possui” o carve-out vs CUDA apps (co-residência) |

## 8. API / Interfaces (rascunho — SPEC congela)

- Sysfs ou debugfs sob `/sys/kernel/ramshared/` ou device class — **privileged**.  
- Possível ioctl mínimo só se sysfs não bastar.  
- **Proibido** no PRD: copiar uAPI Windows; copiar NBD ABI.

Sem structs finais aqui (evita ABI prematura).

## 9. Dependencies and risks

| Risco | Impacto | Mitigação |
| --- | --- | --- |
| Confundir trilha K com produto WSL | Usuário instala LKM e trava | Docs + gates; README aponta cascade |
| Latency-unsafe igual WDDM em passthrough mal feito | Freeze em fault | Medir antes de SPEC; canário obrigatório |
| Coerência de cache CPU↔GPU errada | corrupção | Só paths documentados vendor; sem inventar snoop |
| Escopo monstro HMM+DRM+IOMMU | never-ship | Um mecanismo por SPEC (K1 **ou** K2, não os dois) |
| Halo do manifesto (#11) | “kernel = melhor” sem evidência | Este PRD + ADR se pivotar |

**Dependências:** lab bare-metal, kernel headers, possivelmente out-of-tree vs mainline policy (decidir no SPEC).

## 10. Implementation strategy

| Fase | Ação | Artefato |
| --- | --- | --- |
| **0** | Este PRD + ADR pointer (opcional) | `PRD.md` |
| **0.5** | Inventário lab: bare-metal? ReBAR? driver open? | `validation.md` entry |
| **1** | Passo 0 medição (sem LKM de produção): latência acesso região / probe HMM | runbook + números |
| **2** | SPEC **um** de {K1, K2, K3} — o que o lab suportar | `SPEC.md` + AUDIT-2.5 |
| **3** | IMPL mínimo + kselftest | `IMPL.md` |
| **—** | Em paralelo: cascade polish (app, boot, demote) | já em andamento |

**Ordem de preferência se lab permitir:** K1 (HMM) se vendor stack existir; senão K2 (NUMA/BAR) se ReBAR estável; K3 só com maintainer DRM consciente; K4 quando houver hardware.

## 11. Documents to update (quando gates passarem)

- `ROADMAP.md` — trilha K “gated” (este PRD já referencia).  
- Novo ADR se abandonarmos cascade **em bare-metal** (não no WSL).  
- `DEGRADATION-MATRIX.md` — linhas fault/migrate/GPU reset.  
- `MANIFESTO.md` — opcional: “cascade = bridge; kernel-true = destino bare-metal”.  
- `docs/INDEX.md` via generate script.

## 12. Out of scope

- IMPL LKM neste ciclo.  
- Substituir cascade no README como path único.  
- Windows HMM.  
- CXL sem hardware.  
- “App store” packaging da trilha K.

## 13. Acceptance criteria (deste PRD — decisão)

- [x] Pergunta “kernel de verdade é melhor?” respondida **por ambiente**.  
- [x] Cascade preservada como Day-1.  
- [x] Gates explícitos antes de SPEC (§14).  
- [x] Opções K1–K4 + C documentadas.  
- [x] Abuse cases kernel listados (§ abaixo).  
- [ ] SPEC: **só** após Passo 0 bare-metal PASS (não bloqueia merge deste PRD).

### Abuse cases (obrigatório em discovery)

| Abuse | Risco | Tratamento na trilha K |
| --- | --- | --- |
| ioctl size/TOCTOU | kernel memory corruption | copy_from_user once; max bounds |
| map device memory sem ownership | UAF / DMA to freed | get/put lifetime; unplug path |
| GFP em IRQ no migrate | deadlock | context matrix no SPEC |
| capability bypass | unprivileged DoS/hang | CAP_SYS_ADMIN |
| info-leak de endereços | KASLR break | sem %px em logs default |
| co-residência CUDA + pool | thrash / 1,18 s class | free-floor + demote/offline |

## 14. Validation / gates (go → SPEC)

### Gate A — Environment (must)

| Check | Pass condition |
| --- | --- |
| A1 | Lab **não** é só WSL GPU-PV guest; bare-metal Linux ou passthrough documentado |
| A2 | Ferramenta de inventário registra: GPU, driver, ReBAR y/n, `/proc/iomem` relevantes (sem vazar segredo) |
| A3 | Operador confirma: pressão de teste **não** no host de trabalho diário se risco de hang |

### Gate B — Measurement (must before SPEC freeze)

| Check | Pass condition |
| --- | --- |
| B1 | ≥3 runs latência acesso ou migrate probe; median + p99 + condition tag idle/loaded |
| B2 | Comparar p99 com swap em disco **no mesmo snapshot** (regra benchmarks) |
| B3 | Se p99 device path sob load GPU > limiar de “congela UI” (definir no Passo 0, default: >50 ms page fault genérico ou > p99 disk×N) → **no-go** como hot memory; só cold tier policy no SPEC |

### Gate C — Process

| Check | Pass |
| --- | --- |
| C1 | AUDIT-2.5 go no SPEC (locks/DMA/mm) |
| C2 | Rollback trigger numérico no SPEC |
| C3 | Cascade docs não prometem “já temos NUMA” |

### Verdict deste PRD

| Track | Verdict |
| --- | --- |
| **Cascade (WSL/product)** | **GO continue** |
| **Kernel-true (trilha K)** | **GO research / NO-GO implement** até A+B |
| **Kernel-true no WSL GPU-PV** | **NO-GO** (reaffirmed ADR-0001) |

## 15. Kahneman map (resumo)

| # | Aplicação |
| --- | --- |
| #2 | Counterfactual: LKM no WSL sem BAR → fault 1 s → pior que cascade |
| #3 | 1,18 s é âncora; não adjetivo “rápido no kernel” |
| #5 | GPU reset / reclaim no pior caso |
| #11 | Anti-halo: manifesto bare-metal ≠ melhor no WSL |
| #13 | Existência de HMM no kernel mainline ≠ nosso hardware |
| #16 | Fail-safe: offline/demote independente do path CUDA app |
| #18 | Cascade não é shim eterno se trilha K provar; sunset só com evidência |

## 16. Answer to the product question (plain language)

**“Não conseguimos fazer de forma linda no kernel de verdade?”**  
Conseguimos **imaginar e planejar** de forma linda — e **devemos** (este PRD).  
No **WSL com GPU virtualizada**, não conseguimos **entregar** isso de forma Day-0 limpa e confiável: o dono da VRAM não é o kernel do Linux.  
A forma linda **hoje** é a cascata (kernel no swap, userspace só no backend).  
A forma linda **depois**, no metal, é a trilha K — **se** os gates de medição passarem.

**“Deveríamos gerar um PRD seguindo o SSD?”**  
**Sim.** Este arquivo **é** esse PRD. Próximo passo SSD: **não** SPEC automático — **Passo 0 de inventário/medição bare-metal** ou explicitamente arquivar trilha K como “blocked on hardware” em `validation.md`.
