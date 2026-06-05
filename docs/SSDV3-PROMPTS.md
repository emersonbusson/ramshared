# SSDV3 — Spec-Driven Development: Prompts Base (RamShared)

Metodologia em 3 passos: **PRD → SPEC → IMPL**

Versão revisada para o stack RamShared: Kernel Linux (C/Rust) · Módulos de Kernel (LKM) · Subsistemas de Memória (HMM, NUMA, DRM) · CXL (Compute Express Link) · PCIe Resizable BAR.

Objetivo desta versão:
- preservar a fase de descoberta focada em arquitetura de hardware e kernel antes de decidir
- reduzir ambiguidade entre limitações físicas de hardware e propostas de software
- produzir PRD e SPEC executáveis para desenvolvimento de baixo nível
- incorporar guardrails cognitivos contra kernel panics e corrupção de memória (Sistema 2)

## Como usar

1. Use o **Passo 1** para gerar `docs/{feature-slug}/PRD.md`
2. Use o **Passo 2** para transformar o PRD em `docs/{feature-slug}/SPEC.md`
3. Use o **Passo 3** para implementar (patch no kernel ou LKM) estritamente a partir do SPEC

---

## PASSO 1 — Geração do PRD.md

### Prompt

Preciso gerar o PRD técnico (design de sistema baixo nível) para a seguinte mudança:

**[DESCREVA A FEATURE NO KERNEL / HARDWARE EM 1-2 FRASES]**

Camada(s) envolvida(s):
- [ ] Kernel Core (Memory Management - mm)
- [ ] Drivers (DRM, Nouveau, AMDGPU)
- [ ] Barramento (PCIe, CXL, ReBAR)
- [ ] Userspace (sysfs, udev, cgroups)
- [ ] Módulo independente (LKM)

### Pesquisa obrigatória antes de gerar o PRD

#### 1. Codebase e Topologia do Kernel
- Identifique os subsistemas do kernel afetados (`mm/`, `drivers/gpu/drm/`, `drivers/pci/`)
- Mapeie o uso de memória (HMM - Heterogeneous Memory Management, NUMA nodes)
- Avalie o impacto em cache coherency (Coerência de Cache entre CPU/GPU)

#### 2. Padrões de Hardware e Edge Cases
- Latência do PCIe vs CXL
- Volatilidade da VRAM (power states D3hot/D3cold)
- Interrupções e Evicção de páginas de memória

### Saída esperada do PRD.md

Gere o arquivo `docs/{feature-slug}/PRD.md` com a seguinte estrutura:

#### Resumo
O que é, por que existe, qual limitação física resolve.

#### Contexto arquitetural e de Hardware
- Subsistemas do Kernel envolvidos
- Hardware alvo (GPUs suportadas, CXL vs PCIe Gen4/5)
- O que está confirmado no Kernel Mainline
- O que está sendo proposto no patch/módulo

#### Opção recomendada
Solução escolhida, trade-offs (latência, throughput) e alternativas descartadas.

#### Requisitos Funcionais
- RF-N: descrição objetiva (ex: "Expor a VRAM como um nó NUMA secundário")

#### Requisitos Não-Funcionais
- Performance: overhead de page fault, latência de migração via DMA
- Estabilidade: comportamento em driver crash, prevenção de kernel panic
- Segurança: isolamento de processos (evitar que processo A leia VRAM do processo B)

#### Fluxos de Memória
**Happy path**: Alocação de página, page fault, migração para VRAM, TLB shootdown.
**Fluxos de erro**: Esgotamento de VRAM, GPU reset.

#### Estruturas de Dados / Sysfs
Definição de novas structs em C, hooks do kernel, ou nós exportados no sysfs.

#### Dependências e Riscos
Pré-requisitos de hardware (ReBAR ativo), riscos de corrupção de memória.

---

## PASSO 2 — Geração do SPEC.md

### Prompt
Gere o `SPEC.md` detalhando as alterações no código fonte C/Rust, definindo exatamente quais hooks do kernel interceptar, quais funções de alocação substituir e como lidar com locks (spinlocks, mutexes) para evitar deadlocks.

---

## PASSO 3 — Implementação
Execute o código do SPEC, gerando o código C/Rust para o patch ou módulo e o Makefile associado.
