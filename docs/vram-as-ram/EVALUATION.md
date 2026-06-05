# Avaliação de Arquiteturas: VRAM como RAM (RamShared)

Temos seis documentos de requisitos (PRDs) que descrevem caminhos tecnológicos distintos para alcançar o objetivo de utilizar VRAM como memória do sistema.

De acordo com o princípio da metodologia SSDV3 (**"Discovery antes de convergência"**), avaliamos os prós, contras e viabilidade de implementação antes de gerar o SPEC.

## Comparativo Geral das 6 Soluções

| Característica | PRD-1 (NUMA Hotplug) | PRD-2 (ublk Swap) | PRD-3 (zswap/zpool) | PRD-4 (DAMON Tiering) | PRD-5 (userfaultfd) | PRD-6 (HMM DEVICE_PRIVATE) |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Camada** | Kernel (mm) | Userspace (daemon) | Kernel (LKM) | Kernel (LKM) | Userspace (daemon) | Kernel (LKM + DRM) |
| **Complexidade** | Extrema | Baixa/Média | Alta | Alta | Média | Muito Alta |
| **Performance Pura** | Altíssima | Média | Alta | Alta | Média-Alta | Altíssima |
| **Risco de Kernel Panic** | Crítico | Muito Baixo | Médio | Médio | Zero | Médio-Alto |
| **Compat. NVIDIA Proprietário** | Improvável | Total | Baixa/Média | Baixa | **Total** | Impossível |
| **Compat. AMD/nouveau** | Possível | Total | Média | **Alta** | Total | **Nativa** |
| **Requisito ReBAR** | Obrigatório | Opcional | Opcional | Obrigatório | Opcional | Opcional |
| **Migração Proativa** | Não | Não | Não | **Sim (DAMON)** | **Sim (idle tracking)** | **Sim (scanner + eBPF)** |
| **GPU First (devolver VRAM)** | Possível (mass eviction) | Impraticável | Possível (shrink) | **Sim (offline_and_remove)** | Sim (FLUSH) | **Sim (TTM callback)** |
| **CPU overhead de migração** | DMA (baixo) | io_uring (baixo) | DMA (baixo) | migrate_pages (baixo) | OpenCL (médio) | **SDMA (~zero)** |
| **Kernel mín.** | 5.x | 6.0+ | 5.x | **6.11+** | 5.10+ | 5.15+ |

## Dimensões de Diferenciação

### Reativa vs. Proativa

| | PRD-1 | PRD-2 | PRD-3 | PRD-4 | PRD-5 | PRD-6 |
|---|---|---|---|---|---|---|
| **Quando atua** | RAM cheia | RAM cheia (swap) | RAM cheia (swap) | **Continuamente** | **Continuamente** | **Continuamente** |
| **Mecanismo** | NUMA fallback | Block I/O | Frontswap intercept | DAMON sampling | Idle page tracking | Scanner + eBPF policy |

PRDs 1-3 são **reativos**: esperam a RAM encher e então desviam para a VRAM. PRDs 4-6 são **proativos**: monitoram padrões de acesso e migram páginas frias antes que o sistema entre em pressão de memória.

### Como o Kernel Enxerga a VRAM

| | PRD-1 | PRD-2 | PRD-3 | PRD-4 | PRD-5 | PRD-6 |
|---|---|---|---|---|---|---|
| **Abstração** | RAM equivalente (ZONE_NORMAL) | Disco rápido | Pool comprimido | RAM lenta (tier baixa) | Invisível ao kernel | Memória de dispositivo (ZONE_DEVICE) |
| **Risco de coerência** | **Alto** | Nenhum | Nenhum | **Médio** | Nenhum | **Zero** (DEVICE_PRIVATE) |

### Transparência para Aplicativos

| | PRD-1 | PRD-2 | PRD-3 | PRD-4 | PRD-5 | PRD-6 |
|---|---|---|---|---|---|---|
| **Transparência** | Total | Total (swap) | Total (swap) | Total | **Parcial** (opt-in) | Total |

PRD-5 é o único que requer cooperação do processo-alvo (via LD_PRELOAD, biblioteca, ou ptrace).

## Análise de Viabilidade (Atualizada)

### PRD-1: A utopia técnica
O PRD-1 descreve como a funcionalidade seria se implementada pelos mantenedores do kernel. Para um desenvolvedor externo, modificar o sistema NUMA do Linux e fazer "hijack" do acesso PCI sem a cooperação do driver é praticamente impossível para uso diário estável. **Superseded pelo PRD-4** que usa a mesma infraestrutura NUMA mas com tiering inteligente.

### PRD-2: A solução "Pronta para Hoje"
O PRD-2 usa APIs consolidadas. O `ublk` + OpenCL funciona na RTX 2060 com drivers fechados. É a rota com maior chance de funcionar amanhã, embora trate VRAM como disco, não como RAM. **Complementado pelo PRD-5** que oferece granularidade de página em userspace.

### PRD-3: O "Meio Termo" Original
O PRD-3 é elegante: zswap intercepta e comprime, nosso módulo guarda na VRAM. O desafio é acesso ao barramento PCIe simultaneamente ao driver. **Continua válido** como alternativa de baixa complexidade quando apenas funcionalidade de swap é necessária.

### PRD-4: O Tiering Inteligente (NOVO)
A solução mais alinhada com a direção do kernel Linux. Usa infraestrutura de produção (Optane/CXL). DAMON com self-tuning (7.1+) ajusta automaticamente. **Principal candidato para driver open-source (AMD/nouveau).**

Pontos fortes:
- Infraestrutura testada em produção (Samsung CMM-D, Intel Optane)
- DAMON auto-tuning reduz necessidade de configuração manual
- Proativo: previne swap para disco inteiramente
- LPC 2025 validou a direção (DAMON para nós GPU)

Pontos fracos:
- Requer kernel 6.11+ (DAMOS migrate)
- Necessita coordenação com driver DRM para reservar VRAM
- PCIe sem coerência de cache exige região exclusiva

### PRD-5: Pure Userspace sem Block Layer (NOVO)
A alternativa mais segura e portável. Funciona com QUALQUER GPU e driver. Sem risco de kernel panic. Padrão validado pelo CRIU em produção.

Pontos fortes:
- **Única solução que funciona com NVIDIA proprietário E oferece migração proativa**
- Zero modificação no kernel
- Crash do daemon não derruba o sistema
- Granularidade de página (vs bloco no PRD-2)

Pontos fracos:
- Transparência parcial (processos precisam ser registrados)
- Latência de fault (~5-50μs) maior que acesso direto
- Requer capabilities elevados (CAP_SYS_PTRACE + CAP_SYS_ADMIN)

### PRD-6: O Mecanismo Correto do Kernel (NOVO)
Usa a API que o kernel provê especificamente para este caso. DEVICE_PRIVATE elimina risco de corrupção por incoerência de cache. GPU SDMA offloads transferências completamente. eBPF torna políticas programáveis.

Pontos fortes:
- **Mecanismo correto** — usa DEVICE_PRIVATE, não hacks de NUMA/buddy allocator
- **Zero CPU overhead** para transferências (GPU SDMA)
- **Políticas programáveis via eBPF** (atualizáveis em runtime)
- Alinhado com drm_gpusvm (direção do kernel upstream)
- Template pronto (`lib/test_hmm.c`)

Pontos fracos:
- **Exige cooperação profunda com driver DRM** (amdgpu/nouveau)
- Complexidade muito alta
- Não funciona com driver NVIDIA proprietário
- eBPF struct_ops experimental para este caso de uso

## Matriz de Decisão (Convergência)

### Ambiente WSL2/GPU-PV

Para WSL2 com NVIDIA via GPU-PV, a convergência muda: o Linux guest não controla
a GPU diretamente via DRM/TTM/ReBAR e o CUDA aparece pelo driver Windows exposto
ao guest. Nesse ambiente, o caminho executável é o `PRD-2` adaptado para CUDA e
backend de bloco seguro, documentado em `SPEC-WSL2.md`.

Resumo operacional:
- MVP: `ramshared-wsl2d` em userspace, CUDA, limite inicial de `512M` a `1G`,
  modo manual e backend `nbd` quando `ublk` não existir no kernel WSL2.
- Performance futura: `ublk`, exigindo kernel WSL2 customizado com
  `CONFIG_BLK_DEV_UBLK`.
- Fora do escopo WSL2 inicial: `PRD-4`/DAMON e `PRD-6`/HMM, que continuam
  relevantes para Linux bare-metal ou drivers DRM cooperativos.

### Por Objetivo do Usuário

| Objetivo | PRD Recomendado | Alternativa |
| :--- | :--- | :--- |
| **Usar amanhã na RTX 2060 (NVIDIA proprietário)** | **PRD-2** (ublk) | PRD-5 (userfaultfd) |
| **Melhor performance em GPU AMD open-source** | **PRD-6** (HMM DEVICE_PRIVATE) | PRD-4 (DAMON tiering) |
| **Máxima segurança (zero risco de kernel panic)** | **PRD-5** (userfaultfd) | PRD-2 (ublk) |
| **Proof of concept acadêmico / CXL futuro** | **PRD-4** (DAMON tiering) | PRD-1 (NUMA) |
| **Kernel hacking elegante (module simples)** | **PRD-3** (zswap backend) | PRD-4 (DAMON tiering) |
| **Upstream-friendly (direção do kernel)** | **PRD-6** (HMM DEVICE_PRIVATE) | PRD-4 (DAMON tiering) |

### Por Perfil Técnico

| Perfil | PRD Recomendado |
| :--- | :--- |
| **Iniciante em kernel** | PRD-2 (userspace, Rust/C, APIs estáveis) |
| **Intermediário em kernel** | PRD-3 (módulo LKM, interface zpool) ou PRD-5 (userspace avançado) |
| **Avançado em kernel** | PRD-4 (DAMON + tiering) ou PRD-6 (HMM + SDMA + eBPF) |

### Recomendação de Execução Sequencial

Para maximizar aprendizado e valor incrementalmente:

1. **Fase 1 — PRD-2** (ublk): Protótipo funcional em 1-2 semanas. Valida conceito, mede bandwidth real PCIe↔VRAM via OpenCL na RTX 2060.
2. **Fase 2 — PRD-5** (userfaultfd): Evolução userspace com migração proativa. Remove overhead do block layer. Valida idle page tracking + userfaultfd pattern.
3. **Fase 3 — PRD-3 ou PRD-4**: Primeiro módulo de kernel. PRD-3 se quiser menor complexidade, PRD-4 se quiser alinhar com direção do kernel.
4. **Fase 4 — PRD-6**: Integração profunda com driver DRM. Usa HMM DEVICE_PRIVATE + SDMA. Requer conhecimento sólido de kernel internals e driver DRM.

## Descobertas da Pesquisa em Issues/PRs do Kernel (torvalds/linux)

A pesquisa em issues, PRs e mailing lists do kernel Linux revelou os seguintes projetos e patches relevantes que informaram os novos PRDs:

| Descoberta | Impacto nos PRDs |
| :--- | :--- |
| **dmem cgroup** (Linux 6.14, Valve) | Controle de prioridade de VRAM por cgroup. Complementar a todos os PRDs. |
| **gpu_ext** (eBPF struct_ops, LPC 2025) | Políticas programáveis de evicção GPU. Inspirou RF-5 do PRD-6. |
| **drm_gpusvm** (Intel Xe, 6.14+) | Framework unificado SVM. Base futura para PRD-6. |
| **DAMON para nós GPU** (LPC 2025) | Validação direta de PRD-4 pela comunidade kernel. |
| **Batch DMA migration offload** (AMD RFC) | Otimização de throughput. Aplicável a PRD-4 e PRD-6. |
| **MEMORY_DEVICE_COHERENT** (AMD XGMI) | Prova que o kernel suporta VRAM como device memory. Inspirou PRD-6. |
| **nbdkit-vram-plugin** | Solução NBD existente. Valida PRD-2 mas com overhead TCP. |
| **vramfs** (Overv) | FUSE+OpenCL. Benchmark de 2.4GB/s leitura. Valida viabilidade OpenCL do PRD-5. |
| **MTD/phram** (Arch Wiki legacy) | Mapeamento direto MMIO. Descartado por performance (CPU memcpy). |

## Próximo Passo — RESOLVIDO (2026-06-05)

Convergência concluída: ambiente **WSL2/GPU-PV** → caminho executável é o **`PRD-2`
adaptado para CUDA** (ver §"Matriz de Decisão › Ambiente WSL2/GPU-PV"). Cadeia de
SPEC gerada e auditada (Passo 2.5): `SPEC-WSL2.md` (v1, superseded) →
`SPECv2-WSL2.md` (superseded) → **`SPECv3-WSL2.md` (candidato ativo)**. A Fase 0
(`FASE0-FINAL.md`) mediu e fixou a arquitetura final — VRAM como **tier frio** na
cascata `zram→VRAM→VHDX`, não swap quente. Implementação (Passo 3) em Rust,
registrada em `IMPL.md`. PRD-1/3/4/5/6 seguem como alternativas documentadas
(bare-metal / kernel custom).
