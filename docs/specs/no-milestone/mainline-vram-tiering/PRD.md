---
slug: mainline-vram-tiering
title: Path to native mainline Linux — VRAM as a memory tier (long-term)
milestone: —
issues: []
---

# PRD — Um dia no kernel Linux mainline (VRAM como tier de memória)

> **Tipo:** PRD de **estratégia / destino** (SSDV3 Passo 1).  
> **Não** autoriza dump de LKM out-of-tree como “já é mainline”.  
> Tracks relacionados: `kernel-vram-as-memory` (lab gates), `wsl2-cascade-swap` (ponte shippable).

## 1. Summary

Pergunta do produto:

> **Qual é a melhor abordagem para, um dia, isso fazer parte do kernel Linux nativamente?**

**Resposta deste PRD:**

O destino mainline **não** é “NBD + CUDA daemon” nem “app zenity”. É um **tier de memória de device** integrado ao mm (HMM / memory tiers / demotion) com:

1. **modelo de memória** aceito pela comunidade (tiering, não “finja que é DRAM barata”),  
2. **driver de device** cooperativo (DRM/HMM ou CXL),  
3. **política de demote** sob pressão (GPU precisa da VRAM),  
4. **séries de patches** pequenas, revisáveis, com benchmarks e selftests.

A cascata WSL e o lab Hyper-V/dual-boot são **degraus de evidência**, não o patch final.

## 2. Technical context

### 2.1 O que “nativo no mainline” significa

| Camada | Mainline? | Notas |
| --- | --- | --- |
| Swap em block device + userspace CUDA | Não é “mm nativo” | Útil como **ponte** / product |
| `memory tiering` + demotion (cold pages → slow memory) | Já existe infraestrutura | CXL/DRAM tiers; estender a **device memory** |
| HMM `DEVICE_PRIVATE` / migrate to device | Já no kernel | Precisa **driver** que registre device memory |
| NUMA node de BAR (hotplug) | Possível, polêmico | Coerência, poisoning, offline |
| Módulo out-of-tree `ramshared.ko` | **Não** é mainline | Só protótipo até upstream |

### 2.2 Por que não “subir o monólito RamShared de uma vez”

- Mainline exige **um problema por série**, owners (mm, drm, nvidia/amd open), e **não** depende de stack Windows WDDM.  
- Evidência de latência (1,18 s sob reclaim) prova que VRAM **sem política de demote** é inaceitável como hot memory — isso **deve** estar no design upstream.  
- Vendor lock (só CUDA closed) **bloqueia** merge; paths preferem **DRM/HMM abertos** ou CXL.

### 2.3 Lab reality (este projeto)

| Lab | Serve para mainline? |
| --- | --- |
| WSL GPU-PV | Produto + demote policy; **não** valida BAR/HMM real |
| Hyper-V sem GPU | Build kernel, kselftest, QEMU; **sem** device memory |
| Hyper-V + DDA (experimental) | Possível `lspci 10de` no guest; frágil em GeForce |
| Dual-boot bare-metal | **Melhor** para driver + mm experiments |
| Upstream CI | QEMU + virtio + selftests obrigatórios mesmo com GPU lab |

## 3. Recommended option (melhor abordagem para mainline)

### Estratégia em 4 camadas (ordenadas)

```text
L0  Product bridge     cascade zram→VRAM→disk (userspace)     [já existe]
L1  Policy & metrics   demote, free-floor, latency canary     [já existe / polish]
L2  Out-of-tree proto  minimal LKM or driver hook on bare metal [só com Gate A PASS]
L3  Upstream series    mm tiering + driver hooks + selftests  [destino]
```

**Melhor caminho para L3 (mainline):**

1. **Não** propor “RamShared filesystem de swap” como core.  
2. Propor **VRAM (ou device memory) como memory tier frio** com demotion automática (reutilizar ideias de demotion/CXL tiers).  
3. Implementar **primeiro** em hardware onde o kernel já tem dono (AMDGPU HMM, ou CXL, ou NVIDIA open-gpu-kernel-modules onde aplicável).  
4. Manter **userspace policy agent** opcional (sysfs) — mainline aceita knobs; não aceita daemon NBD como ABI do mm.  
5. Cada RFC: problema, API, rollback, números.

### Alternativas rejeitadas como “caminho mainline”

| Alternativa | Por que não |
| --- | --- |
| Só NBD/CUDA forever | Nunca vira mm nativo |
| LKM monstro no WSL | Ambiente errado + unreviewable |
| Fork do kernel | Fora do objetivo “parte do Linux” |
| Windows StorPort como “upstream Linux” | Outro SO |

## 4. Functional requirements (destino L3)

| ID | Requirement |
| --- | --- |
| RF-M1 | Device memory registrável como tier com capacidade e latência de classe “cold” |
| RF-M2 | Migrate/demote de páginas anônimas frias para device memory sob pressão de DRAM |
| RF-M3 | Promote/demote reverso quando device free-floor ou driver sinaliza “GPU precisa” |
| RF-M4 | Offline seguro do tier (GPU reset, unbind) sem silent corruption |
| RF-M5 | uAPI estável mínima (sysfs/debugfs) documentada; sem ioctl experimental eterno |
| RF-M6 | kselftest ou selftest de migrate + failure injection |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-M1 | Patches checkpatch-clean; series ≤ reviewável (~10–20 commits temáticos) |
| NFR-M2 | Números: p50/p99 fault e bandwidth vs disk swap (benchmarks.md) |
| NFR-M3 | Zero dependência de CUDA userspace no path hot do kernel |
| NFR-M4 | Documentação Documentation/admin-guide ou mm/ |

## 6. Flows

### 6.1 Contribuição (humano + lab)

```text
Evidence (cascade + bare-metal numbers)
  → design RFC (lore.kernel.org / dri-devel / linux-mm)
  → prototype out-of-tree or behind CONFIG_EXPERIMENTAL
  → selftests green on QEMU + one real GPU
  → v1..vN patch series
  → maintainer ack → mainline
```

### 6.2 Runtime (sistema com feature merged)

```text
DRAM pressure → cold pages → device tier
GPU workload → driver free-floor signal → demote/offline device pages → DRAM/disk
```

## 7. Data model

- Memory tier descriptor (cost, bandwidth class, nodes)  
- Device memory regions (PFN ranges, owner driver)  
- Stats: migrated bytes, demote latency, fail counters  

## 8. API (rascunho — SPEC futuro congela)

Preferência: **sysfs** under memory tier / device class; evitar ioctl novo se sysfs bastar.  
Alinhamento com APIs existentes de demotion/tiering (reuso antes de criar).

## 9. Dependencies and risks

| Risco | Mitigação |
| --- | --- |
| Vendor closed stack | Priorizar drivers open; dual-path proibido no design mainline |
| Latency-unsafe hot use | Default **cold tier only**; canary inherited from RamShared evidence |
| Scope creep “todo RamShared no mm” | RF-M* só tiering+migrate; broker/app fora |
| Lab só WSL | Bloqueia L2/L3 até bare-metal (PRD kernel-vram-as-memory) |

## 10. Implementation strategy (anos, não sprints)

| Fase | O quê | Critério de saída |
| --- | --- | --- |
| **P0** | Ponte product (cascade) + docs honestas | já |
| **P1** | Lab bare-metal (dual-boot / DDA) + Passo 0 B numbers | Gate A+B PASS |
| **P2** | Protótipo mínimo alignado a HMM ou tiering (out-of-tree) | demo migrate + demote |
| **P3** | RFC + selftests QEMU | feedback mm/drm |
| **P4** | Series mainline | merged or NACK documentado |

**Hyper-V no R:** acelera P1 (build/boot kernel genérico).  
**DDA:** acelera P1 GPU se funcionar.  
**Dual-boot:** melhor P1–P2.  
**Nada disso é P4 sozinho.**

## 11. Documents

- Este PRD  
- `kernel-vram-as-memory/PRD.md` + PASSO0  
- MANIFESTO (bridge vs destination)  
- Futuro: `SPEC.md` só após P1 PASS e escolha K1 vs K2  

## 12. Out of scope

- Garantia de merge no mainline  
- Suporte Windows no kernel Linux  
- App zenity como requisito de upstream  

## 13. Acceptance (deste PRD)

- [x] Destino mainline descrito sem confundir com cascade  
- [x] Camadas L0–L3  
- [x] RF-M* e riscos vendor  
- [x] Ligação explícita labs Hyper-V / dual-boot como P1, não como “já é nativo”  
- [ ] SPEC L3: **bloqueado** até P1 medido  

## 14. Validation

- Review humano deste PRD  
- Labs: scripts `New-LinuxKernelLabVm.ps1`, `Prepare-DdaGpu.ps1`, `Prepare-DualBootRussia.ps1`  
- Qualquer claim “mainline-ready” exige citação de commit upstream real  

## 15. Kahneman

| # | Uso |
| --- | --- |
| #11 | Anti-halo: ter LKM local ≠ mainline |
| #13 | Existir HMM ≠ nosso driver registrado |
| #3 | Latência medida antes de RFC |
| #18 | Cascade sunset só com prova da classe de problema no path nativo |

## 16. Plain answer

**Melhor abordagem para um dia ser nativo no Linux:**  
tratar VRAM como **memory tier frio com demote**, via **infra mm existente + driver cooperativo**, com **RFC e selftests** — não via NBD permanente.  

**PRD disso:** este arquivo.  
**Próximo SSD realista:** P1 lab (VM + dual-boot) → números → só então SPEC de protótipo kernel.
