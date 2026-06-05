---
slug: vram-hmm-device-private-sdma
title: HMM DEVICE_PRIVATE Pages + GPU SDMA Engine + eBPF Policy
milestone: M01
issues: []
---

# PRD-6 — Páginas DEVICE_PRIVATE via HMM + GPU SDMA + Políticas eBPF

## Resumo

Este PRD propõe usar o mecanismo **correto** do kernel Linux para memória de dispositivo: `MEMORY_DEVICE_PRIVATE` (ZONE_DEVICE) via HMM (Heterogeneous Memory Management). Diferente do PRD-1 que tenta enganar o buddy allocator fazendo VRAM parecer RAM normal, o PRD-6 usa a infraestrutura que o kernel **já provê especificamente para memória de dispositivo** — a mesma usada em produção pelos drivers `amdgpu` (SVM/KFD) e `nouveau`.

A inovação arquitetural é tripla:
1. **DEVICE_PRIVATE pages** para VRAM: o kernel sabe que é memória de dispositivo, não RAM. CPU access gera page fault automático com migração transparente.
2. **GPU SDMA/Copy Engine** para transferências: o hardware do GPU realiza as cópias de dados, liberando a CPU completamente. Zero overhead de CPU para migração.
3. **eBPF struct_ops** para políticas de evicção/prefetch: inspirado no projeto `gpu_ext` (LPC 2025), permite políticas de migração programáveis em runtime sem recompilação do módulo.

**Diferença arquitetural do PRD-1:** PRD-1 faz `add_memory()` e coloca VRAM no buddy allocator como `ZONE_NORMAL`. O kernel trata como RAM equivalente. PRD-6 usa `devm_memremap_pages()` com `MEMORY_DEVICE_PRIVATE` — o kernel trata como memória de dispositivo com semânticas específicas de migração. Páginas na VRAM **não são diretamente acessíveis pela CPU** — todo acesso gera fault e migração automática para RAM. Isso é intrinsecamente mais seguro: não há risco de corrupção por incoerência de cache PCIe.

**Prior art validada:**
- `amdgpu` SVM (`drivers/gpu/drm/amd/amdkfd/kfd_migrate.c`): usa exatamente este padrão para VRAM de GPUs AMD.
- `nouveau` (`drivers/gpu/drm/nouveau/nouveau_dmem.c`): usa DEVICE_PRIVATE para VRAM de GPUs NVIDIA (Pascal+).
- `lib/test_hmm.c`: módulo de teste do kernel que implementa o padrão completo sem GPU real — template direto para implementação.
- `drm_gpusvm` (6.14+): framework unificado de SVM do kernel que abstrai HMM/migrate_vma para drivers GPU. AMD está prototipando suporte.

## Contexto arquitetural e de Hardware

- **Subsistemas do Kernel:** HMM (`mm/hmm.c`), ZONE_DEVICE (`mm/memremap.c`), migrate_vma (`mm/migrate_device.c`), DRM/TTM, PCIe, eBPF (`kernel/bpf/`).
- **Hardware Alvo:**
  - **AMD:** GPUs com driver `amdgpu` (RX 580+). SDMA engines disponíveis.
  - **NVIDIA:** GPUs com driver `nouveau` (Pascal/NV130+). Copy Engine (CE) class NVA0B5.
  - ReBAR não é obrigatório (DEVICE_PRIVATE mapeia VRAM via struct page, não exige BAR completo).
- **Versão Mínima do Kernel:** Linux 5.15+ (amdgpu SVM com HMM maduro). Recomendado 6.14+ (drm_gpusvm).
- **Proposta:** Um módulo de kernel (`ramshared_hmm.ko`) que:
  1. Registra região de VRAM como `MEMORY_DEVICE_PRIVATE` via `devm_memremap_pages()`.
  2. Implementa `dev_pagemap_ops->migrate_to_ram()` para migração VRAM→RAM sob page fault.
  3. Usa a API `migrate_vma_setup()`/`migrate_vma_pages()`/`migrate_vma_finalize()` para migração RAM→VRAM.
  4. Delega transferências de dados ao GPU SDMA/CE engine (zero CPU copy).
  5. Expõe hooks eBPF `struct_ops` para políticas de evicção/prefetch customizáveis.

## Opção recomendada

**Módulo de kernel usando DEVICE_PRIVATE + GPU SDMA + eBPF struct_ops para migração transparente e programável.**

- **Motivo da escolha:**
  - **Mecanismo correto:** `MEMORY_DEVICE_PRIVATE` é a abstração do kernel para exatamente este caso — memória de dispositivo que pode hospedar páginas do sistema. Não é hack, é a API oficial.
  - **Segurança de coerência:** Como a CPU **nunca acessa VRAM diretamente** (todo acesso gera fault + migração), não há problema de coerência de cache PCIe. O dado sempre está na RAM quando a CPU lê.
  - **GPU SDMA:** Transferências via hardware do GPU atingem bandwidth plena (~25-30 GB/s em Gen4 x16) sem usar um único ciclo de CPU. CPU memcpy via MMIO atinge ~6-12 GB/s.
  - **eBPF programável:** Em vez de hard-coded heuristics para decidir quais páginas migrar, políticas são programas eBPF que podem ser atualizados em runtime. Inspirado em `gpu_ext` (4.8x throughput improvement demonstrado em LPC 2025).
  - **Template pronto:** `lib/test_hmm.c` implementa 100% do padrão DEVICE_PRIVATE sem GPU real. Substituir o "fake device" por acesso real à VRAM é o caminho mais curto para um protótipo funcional.

- **Alternativas descartadas:**
  - **MEMORY_DEVICE_COHERENT:** Requer hardware coerente (XGMI/CXL). GPUs consumidor via PCIe não suportam. Reservado para MI250X e similares.
  - **PRD-1 (ZONE_NORMAL hotplug):** Kernel não sabe que é VRAM, trata como RAM pura. Risco de corrupção por incoerência de cache se GPU ou outro dispositivo acessar a região.
  - **PRD-4 (Memory Tiering via DAMON):** Melhor para cenário onde VRAM pode ser acessada diretamente pela CPU (ex: futuro CXL). Para PCIe sem coerência, DEVICE_PRIVATE é mais seguro.

- **Trade-offs:**
  - **Cooperação com driver DRM obrigatória:** O módulo DEVE integrar-se com o driver DRM para acessar os SDMA engines e alocar VRAM via TTM. Não funciona como módulo totalmente independente.
  - **Overhead de page fault:** Cada acesso da CPU a uma página na VRAM gera fault + migração completa (~50-200μs). Para workloads com muitos acessos aleatórios a páginas frias, isso pode ser perceptível.
  - **Complexidade de eBPF:** Programas eBPF no kernel space requerem verificador BPF (limites de loop, stack, etc). Políticas complexas podem atingir limites do verificador.

## Pesquisa obrigatória realizada

### 1. Codebase e APIs do Kernel
- `mm/migrate_device.c`: API `migrate_vma_setup()`/`migrate_vma_pages()`/`migrate_vma_finalize()` — estável e exportada.
- `include/linux/memremap.h`: `MEMORY_DEVICE_PRIVATE` — páginas ZONE_DEVICE que NÃO são CPU-acessíveis. `dev_pagemap_ops->migrate_to_ram()` chamado em page fault.
- `lib/test_hmm.c`: Módulo de teste completo. Aloca DEVICE_PRIVATE pages via `request_free_mem_region()` + `memremap_pages()`. Implementa `migrate_to_ram` e ioctl para migrar páginas em ambas as direções. **Template direto para o módulo ramshared_hmm.**
- `drivers/gpu/drm/amd/amdkfd/kfd_migrate.c`: Implementação de produção. `svm_migrate_vma_to_vram()` usa `migrate_vma_setup()` + SDMA copy + `migrate_vma_finalize()`.
- `drivers/gpu/drm/nouveau/nouveau_dmem.c`: Implementação nouveau. Copy Engine class `NVA0B5` para transferências.
- `drivers/gpu/drm/xe/xe_svm.c`: Intel Xe usa `drm_gpusvm` — framework unificado de SVM que abstrai HMM.

### 2. GPU SDMA/Copy Engine
- **AMD SDMA:** Engines dedicados em todas as GPUs discretas. `amdgpu_copy_buffer()` submete jobs SDMA via ring buffer. Múltiplas instâncias disponíveis (Linux 7.1: patch para usar todas para TTM moves).
- **NVIDIA CE (nouveau):** `nvc0b5_migrate_copy()` programa Copy Engine via pushbuffer. Aperture LOCAL_FB (VRAM) → COHERENT_SYSMEM (RAM). Não-pipelined (sequencial).
- **Bandwidth:** SDMA/CE atingem full PCIe bandwidth (~32 GB/s Gen4 x16) sem CPU involvement. CPU memcpy via MMIO bar: ~6-12 GB/s (limitado por single-core throughput e write-combining).

### 3. eBPF struct_ops para GPU
- `gpu_ext` (github.com/eunomia-bpf/gpu_ext, LPC 2025): Demonstrou eBPF attach points em drivers GPU para eviction e prefetch. 4.8x throughput improvement em workloads de oversubscription.
- `kernel/bpf/bpf_struct_ops.c`: Framework genérico para substituir structs de callbacks por programas eBPF. Usado por TCP congestion control (`bpf_tcp_ca`), schedulers (`sched_ext`).
- Viabilidade: Definir `struct ramshared_policy_ops` com callbacks `should_migrate_to_vram()`, `should_migrate_to_ram()`, `on_eviction()`. Registrar como BPF struct_ops. Programas eBPF podem consultar contadores de acesso, pressão de memória, cgroups.

### 4. Edge Cases de Hardware
- **GPU Reset:** Se o driver fizer GPU reset enquanto há DEVICE_PRIVATE pages, as páginas ficam em limbo. Kernel precisa migrar de volta para RAM antes do reset. `amdgpu` faz isso via `svm_range_list_lock_and_flush_work()`.
- **Multi-GPU:** DEVICE_PRIVATE pages são associadas a um device via `pagemap.owner`. Multi-GPU requer múltiplas instâncias de pagemap.
- **Folio/THP:** Kernel 6.x suporta folios grandes em ZONE_DEVICE. `nouveau_dmem_folio_split()` foi adicionado para split de THPs migrados.

## Requisitos funcionais

- **RF-1:** O módulo deve registrar região de VRAM como `MEMORY_DEVICE_PRIVATE` via:
  1. `request_free_mem_region(&iomem_resource, size, "ramshared")` para obter resource range.
  2. `devm_memremap_pages()` com `pagemap.type = MEMORY_DEVICE_PRIVATE` e `pagemap.ops` implementando `migrate_to_ram` e `folio_free`.
- **RF-2:** O módulo deve implementar `migrate_to_ram()` callback:
  1. Alocar página de sistema (RAM) via `alloc_page(GFP_HIGHUSER_MOVABLE)` (com fallback `mempool_alloc`).
  2. Submeter job SDMA/CE para copiar dados VRAM→RAM.
  3. Aguardar fence de conclusão.
  4. Retornar a nova página. Kernel atualiza PTE automaticamente.
- **RF-3:** O módulo deve expor ioctl ou sysfs para política de migração RAM→VRAM:
  1. Usar `migrate_vma_setup()` com `MIGRATE_VMA_SELECT_SYSTEM` para selecionar páginas.
  2. Alocar DEVICE_PRIVATE pages como destino.
  3. Submeter job SDMA/CE para copiar dados RAM→VRAM.
  4. Chamar `migrate_vma_pages()` + `migrate_vma_finalize()`.
- **RF-4:** O módulo deve integrar com o driver DRM para:
  1. Reservar região de VRAM via TTM (ou coordenação direta).
  2. Acessar SDMA/CE rings para submeter jobs de cópia.
  3. Receber notificações de GPU reset para migrar páginas de emergência.
- **RF-5:** O módulo deve expor hooks eBPF `struct_ops` com interface:
  ```c
  struct ramshared_policy_ops {
      bool (*should_migrate_to_vram)(struct page *page, int nid);
      bool (*should_migrate_to_ram)(struct page *page, u64 access_count);
      int (*on_eviction_priority)(struct page *page, gfp_t gfp);
  };
  ```
  Quando nenhum programa eBPF é carregado, usar implementação default (LRU-based, mesma heurística que DAMON cold threshold).
- **RF-6:** O módulo deve implementar um scanner periódico que identifica páginas frias (via access bits ou DAMON integration) e invoca migração RAM→VRAM para páginas que `should_migrate_to_vram()` retorna true.

## Requisitos não-funcionais

- **Performance:**
  - Migração VRAM→RAM (page fault): ≤ 200μs (inclui alocação + SDMA copy + PTE update).
  - Migração RAM→VRAM (proativa): throughput ≥ 20 GB/s em PCIe Gen4 x16 (batch SDMA).
  - CPU overhead para migração: ~0% (SDMA faz tudo; CPU apenas submete job e espera fence).
- **Estabilidade:**
  - GPU reset: módulo deve interceptar notificação de reset e migrar 100% das DEVICE_PRIVATE pages para RAM antes de permitir reset.
  - Power state D3: migração completa para RAM antes de qualquer transição D3hot/D3cold.
  - Fallback: se SDMA engine falhar, fallback para CPU memcpy via BAR (degradação graceful, não crash).
  - Uso de `mempool_create()` com pool pré-alocado para todas as alocações no caminho de page fault.
- **Segurança:**
  - DEVICE_PRIVATE pages não são CPU-acessíveis — não há risco de leitura direta de dados na VRAM por processos não-autorizados.
  - Quando página DEVICE_PRIVATE é liberada (`folio_free`), a região correspondente na VRAM deve ser zerada via SDMA fill.
  - Programas eBPF passam pelo verificador BPF — não podem acessar memória arbitrária ou causar loops infinitos.

## Fluxos de Memória

### Happy path (Migração Proativa RAM→VRAM)
1. Scanner periódico identifica 2GB de páginas frias no sistema (via access bits).
2. Programa eBPF `should_migrate_to_vram()` confirma candidatas baseado em cgroup, idade, e pressão de memória.
3. Módulo executa `migrate_vma_setup()` em batches de 512 páginas.
4. Para cada batch: aloca DEVICE_PRIVATE pages, submete SDMA copy job (RAM→VRAM).
5. Aguarda fence SDMA.
6. Executa `migrate_vma_pages()` + `migrate_vma_finalize()`.
7. Kernel substitui PTEs por migration entries. Páginas RAM são liberadas ao buddy allocator.
8. Sistema ganhou 2GB de RAM livre. Dados residem na VRAM como DEVICE_PRIVATE pages.

### Happy path (Fault-Driven VRAM→RAM)
1. Processo acessa endereço virtual cuja página está na VRAM (migration entry no PTE).
2. Kernel detecta page fault, chama `migrate_to_ram()` do módulo.
3. Módulo aloca página RAM, submete SDMA copy (VRAM→RAM), aguarda fence.
4. Retorna nova página ao kernel.
5. Kernel atualiza PTE, processo retoma. Latência total: ~50-200μs.

### Evicção para GPU (GPU First)
1. Driver DRM sinaliza necessidade de VRAM (novo contexto gráfico alocado).
2. Módulo recebe callback do TTM ou sinalização via sysfs.
3. eBPF `on_eviction_priority()` determina ordem de evicção (páginas menos prioritárias primeiro).
4. Módulo migra páginas selecionadas VRAM→RAM via SDMA batch.
5. Libera região para o driver DRM.

### Fluxo de Erro: GPU Reset
1. Driver amdgpu/nouveau detecta GPU hang e prepara reset.
2. Driver sinaliza módulo ramshared_hmm via callback.
3. Módulo entra em modo de emergência: todas as DEVICE_PRIVATE pages são migradas para RAM via CPU memcpy (SDMA pode estar indisponível durante reset).
4. Módulo confirma zero páginas restantes na VRAM.
5. Driver procede com reset.
6. Após reset, módulo re-registra região VRAM e retoma scanner.

### Fluxo de Erro: Falha de Alocação no Page Fault
1. `migrate_to_ram()` tenta alocar página RAM, mas sistema está sob OOM.
2. Módulo tenta `mempool_alloc()` do pool pré-alocado.
3. Se pool vazio: retorna `VM_FAULT_OOM`. Kernel aciona OOM killer.
4. Após OOM killer liberar memória, o page fault é retentado.

## Estruturas de Dados e Sysfs

**Nós Sysfs planejados:**
- `/sys/kernel/ramshared/hmm/status`: Ativo/Inativo, VRAM total/usada pelo módulo.
- `/sys/kernel/ramshared/hmm/gpu_driver`: Nome do driver DRM cooperante (amdgpu/nouveau).
- `/sys/kernel/ramshared/hmm/sdma_available`: Se SDMA engine está acessível (true/false).
- `/sys/kernel/ramshared/hmm/pages_in_vram`: Contagem de DEVICE_PRIVATE pages ativas.
- `/sys/kernel/ramshared/hmm/migration_stats`: JSON com latências P50/P95/P99, throughput, fault count.
- `/sys/kernel/ramshared/hmm/scanner_interval_ms`: Intervalo do scanner de páginas frias (default: 10000).
- `/sys/kernel/ramshared/hmm/evict_all`: Trigger manual para migrar tudo de volta para RAM.
- `/sys/kernel/ramshared/hmm/ebpf_policy`: Nome da política eBPF carregada (ou "default").

**Structs Kernel:**
```c
struct ramshared_hmm_device {
	struct pci_dev *pdev;
	struct drm_device *drm;

	/* VRAM region */
	struct resource *vram_res;
	struct dev_pagemap pagemap;
	unsigned long *vram_bitmap;       /* bitmap de páginas livres na VRAM */
	spinlock_t bitmap_lock;

	/* SDMA/CE engine */
	struct ramshared_dma_engine {
		void *ring;                   /* ring buffer do SDMA */
		struct dma_fence *last_fence;
		bool available;
	} dma_engine;

	/* Memory pools (deadlock prevention) */
	struct mempool_s *page_pool;      /* pré-alocado para migrate_to_ram */
	struct mempool_s *fence_pool;     /* fences para SDMA jobs */

	/* eBPF policy */
	struct ramshared_policy_ops *policy;  /* NULL = default */

	/* Scanner */
	struct delayed_work scanner_work;
	unsigned int scanner_interval_ms;

	/* State */
	atomic_t pages_in_vram;
	bool gpu_resetting;
};

/* dev_pagemap_ops implementation */
static const struct dev_pagemap_ops ramshared_pagemap_ops = {
	.folio_free = ramshared_folio_free,
	.migrate_to_ram = ramshared_migrate_to_ram,
};

/* eBPF struct_ops interface */
struct ramshared_policy_ops {
	bool (*should_migrate_to_vram)(u64 pfn, int nid, u64 idle_ms);
	bool (*should_migrate_to_ram)(u64 pfn, u64 access_count);
	int (*on_eviction_priority)(u64 pfn, u32 gfp_flags);
};
```

## Dependências e riscos

- **Pré-requisitos:**
  - Kernel 5.15+ (HMM maduro com amdgpu SVM). Recomendado 6.14+ (drm_gpusvm).
  - Driver DRM open-source (`amdgpu` ou `nouveau`). **NVIDIA proprietário não coopera** — este PRD não funciona com driver fechado.
  - `CONFIG_ZONE_DEVICE=y`, `CONFIG_HMM_MIRROR=y`, `CONFIG_DEVICE_PRIVATE=y`.
  - eBPF: `CONFIG_BPF=y`, `CONFIG_BPF_SYSCALL=y`.
- **Riscos:**
  - **Acesso ao SDMA engine:** Os SDMA/CE engines são gerenciados internamente pelo driver DRM. Acessá-los requer:
    - Opção A: Ser uma extensão in-tree do driver (longo prazo, upstream-friendly).
    - Opção B: Usar API exportada (ex: `amdgpu_copy_buffer()` se exportada para módulos).
    - Opção C: Fallback para CPU memcpy via BAR (6-12 GB/s vs 25-30 GB/s — degradação aceitável para prototipagem).
  - **VRAM address space:** `request_free_mem_region()` cria região iomem "fake" para struct pages — NÃO aloca VRAM real. A alocação de VRAM real é feita via TTM/DRM. A coordenação entre as duas é não-trivial.
  - **drm_gpusvm em evolução:** A API está mudando rapidamente (CVE-2025-40336 fix recente). Construir sobre ela agora é arriscado; pode precisar de ajustes frequentes.
  - **dmem cgroup (Linux 6.14):** O novo cgroup controller para device memory pode interagir com as DEVICE_PRIVATE pages do módulo. Precisa validar compatibilidade.

## Portão Cognitivo de Kahneman (Obrigatório antes do SPEC)

- **Disciplina Aplicável:** Disciplina 1 (Segurança de Barramento e DMA) + Disciplina 2 (Sobrevivência ao Hardware) + Disciplina 4 (Isolamento entre Processos).

- **Justificativa (Disciplina 1):** O módulo usa extensivamente DMA via SDMA engine para copiar dados entre RAM e VRAM. Transferências DMA incoerentes corromperão páginas. O SPEC DEVE demonstrar:
  1. Par `dma_map_single()`/`dma_unmap_single()` com `DMA_BIDIRECTIONAL` em toda transferência.
  2. Tratamento explícito de `dma_mapping_error()` em todo mapeamento.
  3. Fence waiting (`dma_fence_wait()`) antes de qualquer acesso à página destino.
  4. Fallback para CPU memcpy quando SDMA retorna erro.

- **Justificativa (Disciplina 2):** Se a GPU fizer reset ou entrar em D3cold enquanto há DEVICE_PRIVATE pages, os migration entries no PTE apontam para memória inexistente. Todo page fault subsequente causará kernel panic. O SPEC DEVE demonstrar:
  1. Callback de GPU reset que migra 100% das páginas via CPU memcpy (SDMA indisponível) antes de permitir reset.
  2. Callback `pm_ops->suspend` que faz o mesmo para transições D3.
  3. Timeout: se migração de emergência não completar em 5 segundos, forçar `VM_FAULT_SIGBUS` para processos afetados (matar processo é melhor que kernel panic).

- **Justificativa (Disciplina 4):** DEVICE_PRIVATE pages herdam o isolamento de memória do kernel (cada página pertence a um `mm_struct`). Porém, a VRAM física é um espaço contíguo — se o módulo não zerar páginas liberadas, shaders do Processo B podem ler resíduos do Processo A via acesso direto à VRAM. O SPEC DEVE demonstrar:
  1. `folio_free()` executa SDMA fill de zeros na região VRAM antes de devolver ao bitmap livre.
  2. Se SDMA indisponível, fallback para `memset_io()` via BAR mapping.

- **Evidência exigida para o SPEC:**
  1. Código de `migrate_to_ram()` com `mempool_alloc()`, SDMA copy com fence, e tratamento de `dma_mapping_error()`.
  2. Código de scanner com integração eBPF `should_migrate_to_vram()` e uso de `migrate_vma_setup()`.
  3. Teste unitário (baseado em `lib/test_hmm.c`) provando migração bidirecional sem corrupção de dados.
  4. Código de emergência para GPU reset com CPU memcpy fallback e timeout de 5 segundos.
  5. `folio_free()` com SDMA zero-fill e fallback `memset_io()`.

- **Gatilho de Aborto:**
  - Se o driver DRM (`amdgpu`/`nouveau`) não exportar nenhuma API para submeter DMA copies de módulos externos E o driver não aceitar patches para isso, ABORTAR e usar Opção C (CPU memcpy via BAR).
  - Se o verificador eBPF rejeitar o tipo de callback `struct_ops` proposto (ex: por restrições de acesso a struct page), simplificar para configuração via sysfs sem eBPF.
  - Se testes em `lib/test_hmm.c` demonstrarem corrupção em migração bidirecional com o hardware alvo (RTX 2060 / RX 6700), ABORTAR módulo de kernel e recomendar PRD-5 (userspace).
