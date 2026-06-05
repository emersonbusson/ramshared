---
slug: vram-proactive-memory-tiering
title: Tiering Proativo de Memória via NUMA + DAMON/DAMOS
milestone: M01
issues: []
---

# PRD-4 — Tiering Proativo de VRAM via Nó NUMA + DAMON

## Resumo

Este PRD propõe uma abordagem fundamentalmente diferente das anteriores: em vez de reagir à pressão de memória (PRD-1/2/3 esperam a RAM encher), este sistema **proativamente** identifica páginas frias e as migra para a VRAM antes que o sistema entre em estado de swap. A solução utiliza a infraestrutura de Memory Tiering do kernel Linux (6.1+) combinada com o DAMON (Data Access MONitor, 5.15+) e suas DAMOS (Data Access Monitoring-based Operation Schemes) para migração automatizada e bidirecional de páginas entre RAM (tier alta) e VRAM (tier baixa).

**Diferença arquitetural crítica:** Os PRDs 1-3 são todos **reativos** — atuam quando a RAM já está cheia. O PRD-4 é **proativo** — monitora padrões de acesso e move páginas frias para a VRAM continuamente, mantendo a RAM livre para dados quentes. Isso previne o swap para disco inteiramente em cenários de workload com boa localidade temporal.

## Contexto arquitetural e de Hardware

- **Subsistemas do Kernel:** Memory Management (`mm/`), Memory Tiering (`mm/memory-tiers.c`), DAMON (`mm/damon/`), Memory Hotplug, NUMA, PCIe.
- **Hardware Alvo:** GPUs discretas com PCIe Resizable BAR (ReBAR) habilitado. Foco inicial em `amdgpu` e `nouveau` (drivers open-source).
- **Versão Mínima do Kernel:** Linux 6.11+ (requer `damos_migrate` com ações `migrate_cold` e `migrate_hot`).
- **Infraestrutura Existente no Mainline:**
  - `memory_tier` framework (6.1): organiza nós NUMA em tiers hierárquicas baseadas em "abstract distance" (adistance).
  - `dax_kmem` (5.1): converte dispositivos DAX em RAM do sistema via `add_memory_driver_managed()`.
  - DAMON DAMOS `migrate_cold`/`migrate_hot` (6.11): migração automatizada de páginas entre tiers.
  - DAMON self-tuning (7.1-rc1): auto-ajuste de quotas baseado em métricas de uso.
- **Confirmado no Kernel Mainline:** Este pipeline é usado em produção para CXL Memory Expanders (Samsung CMM-D) e Intel Optane DCPMM. A adaptação para VRAM é a proposta deste PRD.
- **Proposta:** Um módulo de kernel (`ramshared_tier.ko`) que:
  1. Reserva uma região da VRAM via coordenação com o driver DRM (TTM memory manager)
  2. Registra essa região como RAM do sistema via `add_memory_driver_managed()`
  3. Classifica o nó NUMA resultante como tier baixa via `memory_tier` com adistance calibrada
  4. Habilita DAMON com esquemas DAMOS para migração proativa bidirecional

## Opção recomendada

**Módulo de kernel que registra VRAM como nó NUMA de tier baixa, com DAMON gerenciando migração proativa.**

- **Motivo da escolha:** Reutiliza a infraestrutura de tiering já testada em produção (Optane/CXL). O DAMON com self-tuning (7.1+) ajusta automaticamente as taxas de promoção/demoção baseado em métricas reais de uso. Não tenta enganar o buddy allocator dizendo que VRAM é RAM pura (PRD-1) — o kernel **sabe** que a VRAM é mais lenta e gerencia de acordo.

- **Alternativas descartadas:**
  - **PRD-1 (NUMA cru):** Não usa tiering — o alocador trata VRAM como RAM equivalente, o que é falso (latência PCIe >> DDR). PRD-4 usa `adistance` para informar o kernel da performance real.
  - **PRD-2/3 (swap):** Atuam apenas quando RAM enche. PRD-4 age proativamente — páginas frias vão para VRAM antes da pressão, mantendo RAM livre.
  - **CXL Type-3 emulation:** Pesquisa em QEMU CXL e Samsung SMDK confirmou que é viável conceitualmente, mas a emulação de capacidades CXL-específicas (DVSEC, ACPI CEDT) sobre PCIe padrão é frágil e desnecessária — `add_memory_driver_managed()` fornece o mesmo resultado final sem a camada de emulação.

- **Trade-offs:**
  - DAMON tem overhead de monitoramento (~1% CPU), mas é configurável.
  - Requer coordenação com driver DRM para reservar região de VRAM exclusiva.
  - PCIe não fornece coerência de cache — seguro apenas se o GPU driver não acessa a região reservada simultaneamente.

## Pesquisa obrigatória realizada

### 1. Codebase e Topologia do Kernel
- `mm/memory-tiers.c`: Framework de tiers com `mt_calc_adistance()` e `init_node_memory_type()`.
- `mm/damon/`: DAMOS com ações `migrate_cold`/`migrate_hot` (6.11+), self-tuning via quota-goal metrics (7.1+).
- `drivers/dax/kmem.c`: `dax_kmem` usa `add_memory_driver_managed()` para hotplug de memória.
- LPC 2025: Sessão "DAMON-based Pages Migration for {C,G,X}PU [un]attached NUMA nodes" propõe generalização para nós GPU — validação direta da viabilidade.

### 2. Padrões de Hardware e Edge Cases
- PCIe MMIO não é cache-coerente, mas se apenas a CPU acessa a região reservada (GPU driver não toca), não há problema de coerência — análogo ao uso de Optane onde o dispositivo não modifica memória concorrentemente.
- DAMON TPP-DAMON multi-threaded (6.16): 94% de melhoria em llama.cpp — validação de performance em workloads reais.
- Batch DMA migration offload (RFC AMD): reduz overhead de `move_pages()` em 97% para folios de 2MB — otimização futura direta.

## Requisitos funcionais

- **RF-1:** O módulo deve coordenar com o driver DRM (`amdgpu`/`nouveau`) via TTM memory manager para reservar uma região contígua de VRAM que o driver gráfico não utilizará.
- **RF-2:** O módulo deve registrar a região reservada como memória do sistema via `add_memory_driver_managed()` com grupo de memória estático (`memory_group_register_static()`).
- **RF-3:** O módulo deve calcular o `adistance` correto baseado na latência real do barramento PCIe (medida no boot via leitura/escrita de teste) e registrar o nó NUMA na tier apropriada via `init_node_memory_type()`.
- **RF-4:** O módulo deve criar e configurar um contexto DAMON com esquemas DAMOS:
  - `migrate_cold`: Páginas com frequência de acesso abaixo do threshold por N milissegundos → demoção para o nó VRAM.
  - `migrate_hot`: Páginas no nó VRAM com frequência de acesso acima do threshold → promoção para DDR.
- **RF-5:** O módulo deve expor via sysfs controles para ajuste dinâmico dos thresholds de quente/frio e da quota de migração.
- **RF-6:** O módulo deve implementar callbacks de Power Management (`pm_ops->suspend`) para migrar todas as páginas residentes na VRAM de volta para DDR/Swap antes de qualquer transição de energia da GPU.

## Requisitos não-funcionais

- **Performance:**
  - Overhead do DAMON: < 1% de CPU (configurável via intervalo de amostragem e granularidade de regiões).
  - Latência de acesso a páginas no nó VRAM: ≤ 400ns (compatível com PCIe Gen4 x16).
  - Throughput de migração: deve saturar o barramento PCIe (≥ 25 GB/s em Gen4 x16 usando batch DMA quando disponível).
- **Estabilidade:**
  - Nenhuma página pode residir na VRAM durante transição D3hot/D3cold da GPU.
  - O módulo deve degradar graciosamente se o driver DRM solicitar a VRAM de volta (shrink callback).
  - Uso de `mempool_alloc` com pools pré-alocados para todas as estruturas no caminho crítico de migração (evitar OOM deadlock).
- **Segurança:**
  - Páginas migradas para VRAM devem ter a região DDR original zerada antes de ser liberada ao buddy allocator.
  - Páginas devolvidas da VRAM ao driver DRM devem ser zeradas via DMA fill.

## Fluxos de Memória

### Happy path (Demoção Proativa)
1. Sistema operando normalmente com 60% de uso de RAM.
2. DAMON monitora padrões de acesso via sampling (overhead < 1%).
3. DAMOS identifica 2GB de páginas que não foram acessadas nos últimos 30 segundos.
4. DAMOS aciona `migrate_cold`: páginas são migradas para o nó NUMA da VRAM via `migrate_pages()`.
5. RAM DDR libera 2GB, que ficam disponíveis para alocações futuras.
6. Sistema nunca atinge pressão de swap — SSD não é tocado.

### Promoção de Páginas Quentes
1. DAMON detecta que páginas no nó VRAM começaram a ser acessadas frequentemente.
2. DAMOS aciona `migrate_hot`: páginas são migradas de volta para DDR.
3. Processo percebe latência de uma operação `migrate_pages()` (~50-200μs por batch), depois acessa DDR normalmente.

### Evicção para DRM (GPU First)
1. Usuário inicia jogo 3D que exige toda a VRAM.
2. O driver DRM sinaliza necessidade via callback `shrinker` ou solicitação TTM.
3. O módulo `ramshared_tier` intercepta, pausa DAMON.
4. Executa `offline_and_remove_memory()` para remover o nó NUMA da VRAM do sistema.
5. Migra todas as páginas residentes para DDR (ou Swap se DDR estiver cheia).
6. Libera a região VRAM para o driver DRM.
7. Quando o jogo termina, o módulo re-registra a VRAM como nó NUMA e reinicia DAMON.

### Fluxo de Erro: GPU D3cold
1. Sistema entra em suspensão (pm_ops->suspend chamado).
2. O módulo recebe o callback ANTES da GPU desligar.
3. Pausa DAMON, executa migração completa de todas as páginas VRAM→DDR.
4. Confirma que zero páginas residem na VRAM.
5. Permite que a GPU entre em D3cold.
6. No resume, re-registra o nó e reinicia DAMON.

## Estruturas de Dados e Sysfs

**Nós Sysfs planejados:**
- `/sys/kernel/ramshared/tier/status`: Ativo/Inativo, tamanho de VRAM no tier pool.
- `/sys/kernel/ramshared/tier/numa_node`: ID do nó NUMA da VRAM.
- `/sys/kernel/ramshared/tier/adistance`: Abstract distance calculada (read-only).
- `/sys/kernel/ramshared/tier/damon_cold_threshold_ms`: Tempo em ms para considerar página fria (default: 30000).
- `/sys/kernel/ramshared/tier/damon_hot_threshold_ms`: Tempo em ms para considerar página quente (default: 1000).
- `/sys/kernel/ramshared/tier/migration_quota_mb_per_sec`: Quota de migração (default: 1024).
- `/sys/kernel/ramshared/tier/pages_in_vram`: Contador de páginas atualmente na VRAM.
- `/sys/kernel/ramshared/tier/evict_to_ddr`: Trigger manual para forçar promoção total.

**Struct Kernel Base:**
```c
struct ramshared_tier_device {
	struct pci_dev *pdev;
	phys_addr_t bar_start;
	resource_size_t bar_size;
	resource_size_t reserved_size;
	int numa_node;
	int memory_tier_id;
	unsigned int adistance;
	struct memory_group *mem_group;
	struct damon_ctx *damon_ctx;
	struct mempool_s *migrate_pool;
	bool gpu_active;       /* true quando DRM reclamou a VRAM */
	spinlock_t state_lock;
};
```

## Dependências e riscos

- **Pré-requisitos:**
  - Kernel 6.11+ (DAMOS migrate_cold/migrate_hot). Recomendado 7.1+ (self-tuning).
  - Resizable BAR habilitado para mapeamento completo da VRAM.
  - Driver DRM open-source (amdgpu/nouveau) para coordenação via TTM.
- **Riscos:**
  - **Coerência de cache:** PCIe MMIO não é cache-coerente. Mitigação: a região é reservada exclusivamente — GPU driver não acessa, logo não há conflito. Análogo a Optane DCPMM.
  - **Conflito com driver DRM:** Se o TTM não cooperar na reserva, o módulo não carrega. Mitigação: implementar como extensão do driver DRM (in-tree) em vez de módulo externo, longo prazo.
  - **Latência de promoção:** Quando uma página quente é acessada na VRAM, a migração leva ~50-200μs. Para workloads latency-sensitive, isso pode ser perceptível. Mitigação: DAMOS com quotas ajustáveis e auto-tuning.

## Portão Cognitivo de Kahneman (Obrigatório antes do SPEC)

- **Disciplina Aplicável:** Disciplina 2 (Sobrevivência ao Hardware) + Disciplina 3 (Prevenção de Deadlock em Memória).

- **Justificativa (Disciplina 2):** O módulo registra memória que pode desaparecer (GPU D3cold, driver reset, hotplug). Se o kernel tentar acessar uma página na VRAM após a GPU desligar, ocorre Machine Check Exception ou kernel panic. O fluxo de `pm_ops` DEVE completar a migração de 100% das páginas antes de qualquer transição de energia.

- **Justificativa (Disciplina 3):** A migração de páginas via `migrate_pages()` pode tentar alocar memória DDR para estruturas internas quando o sistema já está sob pressão de memória (o motivo pelo qual páginas foram para a VRAM). O módulo DEVE usar pools pré-alocados (`mempool_alloc`) e nunca chamar `kmalloc(..., GFP_KERNEL)` no caminho de migração.

- **Evidência exigida para o SPEC:**
  1. Demonstrar o callback `pm_ops->suspend` com migração completa e barreira de sincronização antes de retornar.
  2. Demonstrar que todas as alocações no hot path usam `GFP_NOWAIT` ou `mempool_alloc`, nunca `GFP_KERNEL`.
  3. Demonstrar o cálculo de `adistance` baseado em medição real de latência PCIe no boot.
  4. Demonstrar o mecanismo de `offline_and_remove_memory()` para devolver VRAM ao DRM.

- **Gatilho de Aborto:**
  - Se o driver DRM (TTM) não oferecer API para reservar regiões de VRAM de forma cooperativa, ABORTAR e cair no PRD-5 (userspace).
  - Se `migrate_pages()` mostrar uso de `GFP_KERNEL` no callpath interno do kernel, ABORTAR até que seja resolvido upstream.
