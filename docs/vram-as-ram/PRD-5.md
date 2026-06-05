---
slug: vram-userfaultfd-userspace-migration
title: Migração de Páginas em Userspace via userfaultfd + Idle Page Tracking
milestone: M01
issues: []
---

# PRD-5 — Migração de Páginas para VRAM via userfaultfd (Pure Userspace)

## Resumo

Este PRD propõe uma solução **inteiramente em userspace** que elimina a necessidade de módulos de kernel ou modificações no kernel. O daemon (`ramshared-uffd`) utiliza três mecanismos nativos do Linux para migrar páginas de processos entre RAM e VRAM:

1. **Idle Page Tracking** (`/sys/kernel/mm/page_idle/bitmap`): identifica páginas frias por PFN.
2. **userfaultfd** (`UFFDIO_REGISTER_MODE_MISSING` + `UFFDIO_REGISTER_MODE_WP`): intercepta page faults quando processos acessam páginas que foram evictas para a VRAM.
3. **OpenCL/Vulkan**: transfere dados entre RAM e VRAM via API gráfica, sem acesso direto ao barramento PCI.

**Diferença arquitetural crítica:** O PRD-2 (ublk) também é userspace, mas introduz o overhead do Block Layer inteiro (montagem de BIOs, filas de I/O, serialização). O PRD-5 opera **no nível de páginas individuais** — a granularidade é 4KB (ou 2MB com THP), não blocos de disco. Isso elimina a camada de abstração de bloco e permite migração seletiva página-a-página.

**Prior art validada:** O CRIU (Checkpoint/Restore In Userspace) usa exatamente este padrão (`userfaultfd` + lazy page restore) em produção para migração ao vivo de containers. O projeto `networked-linux-memsync` (TU Berlin, 2023) implementou memória remota via userfaultfd sobre rede TCP.

## Contexto arquitetural e de Hardware

- **Subsistemas do Kernel Utilizados (sem modificação):** userfaultfd (4.3+), Idle Page Tracking, `process_madvise()` (5.10+), `/proc/<pid>/pagemap`.
- **Hardware Alvo:** Qualquer GPU com driver OpenCL 1.2+ ou Vulkan 1.0+. **Funciona com driver proprietário da NVIDIA** (RTX 2060) e AMD (RX 6700).
- **Versão Mínima do Kernel:** Linux 5.10+ (requer `process_madvise`). Recomendado 5.13+ (minor fault handling).
- **Zero Módulo de Kernel:** O daemon roda inteiramente em Ring 3 (userspace). Requer `CAP_SYS_PTRACE` + `CAP_SYS_ADMIN` para monitorar processos e ler PFNs do pagemap.
- **Proposta:** Um daemon Rust (`ramshared-uffd`) que:
  1. Monitora processos-alvo (opt-in via cgroups, CLI, ou LD_PRELOAD)
  2. Periodicamente escaneia páginas frias via idle page tracking
  3. Copia conteúdo de páginas frias para buffers na VRAM (OpenCL)
  4. Libera as páginas via `madvise(MADV_DONTNEED)` (ou equivalente)
  5. Intercepta page faults via userfaultfd e restaura dados da VRAM

## Opção recomendada

**Daemon userspace com userfaultfd para interceptação de faults e OpenCL para acesso à VRAM.**

- **Motivo da escolha:**
  - **Compatibilidade máxima:** Funciona com NVIDIA proprietário, AMD, Intel — qualquer GPU com OpenCL.
  - **Sem risco de kernel panic:** O daemon é um processo userspace. Se crashar, os processos monitorados recebem SIGBUS nos page faults pendentes e são terminados pelo SO, mas o kernel continua estável.
  - **Granularidade de página:** Diferente do PRD-2 (granularidade de bloco 4KB-128KB com BIO overhead), o PRD-5 migra páginas individuais com `UFFDIO_COPY` atômico.
  - **Benchmarks de referência:** `vramfs` (FUSE + OpenCL) demonstrou 2.4 GB/s leitura e 2.0 GB/s escrita em PCIe 3.0. O PRD-5 deve atingir throughput similar para a cópia de dados, com latência de fault de ~5-50μs por página.

- **Alternativas descartadas:**
  - **NBD (nbdkit-vram-plugin):** Solução existente com TCP loopback — overhead de protocolo de rede desnecessário.
  - **PRD-2 (ublk):** Melhor throughput bulk, mas ~10x mais latência por acesso aleatório de página individual devido ao overhead do block layer.
  - **ptrace-based migration:** Invasivo demais, overhead de sinalização por página proibitivo.

- **Trade-offs:**
  - **Transparência limitada:** Processos devem ser registrados no userfaultfd. Três modos:
    1. **Cooperativo:** Processo usa biblioteca `libramshared` que registra automaticamente.
    2. **LD_PRELOAD:** Shim injeta registro de userfaultfd em processos não-modificados.
    3. **ptrace:** Daemon injeta syscalls de registro (mais lento, fallback).
  - **Latência de fault:** ~5-50μs por página (vs ~60ns para RAM DDR). Aceitável para páginas frias, perceptível para páginas quentes erroneamente evictas.
  - **Overhead de monitoramento:** Idle page tracking requer leitura periódica do bitmap. Para 16GB de RAM → 4M páginas → 512KB de bitmap por scan.

## Pesquisa obrigatória realizada

### 1. Codebase e APIs do Kernel
- `userfaultfd(2)`: Suporte a `UFFDIO_REGISTER_MODE_MISSING` (4.3+), `UFFDIO_REGISTER_MODE_WP` (5.7+), `UFFD_FEATURE_EVENT_REMOVE` (4.11+).
- `/proc/<pid>/pagemap`: 8 bytes por página, contém PFN quando lido por root (`CAP_SYS_ADMIN`).
- `/sys/kernel/mm/page_idle/bitmap`: 8 bytes por 64 PFNs, set bit = idle. Requer `CONFIG_IDLE_PAGE_TRACKING=y`.
- `process_madvise(2)` (5.10+): Suporta `MADV_COLD` e `MADV_PAGEOUT` cross-process. **NÃO suporta** `MADV_DONTNEED` cross-process — o processo-alvo deve executar `madvise` ele mesmo (via cooperação ou ptrace).

### 2. Validação de Viabilidade
- **CRIU lazy-pages:** Usa userfaultfd + UFFDIO_COPY para restauração lazy de páginas de processos migrados. Produção em Kubernetes/Podman. Validação direta do padrão.
- **networked-linux-memsync (TU Berlin):** Implementou memória paginada remotamente via userfaultfd sobre TCP. Demonstrou que o padrão funciona para latências de milissegundos — PCIe (~0.3μs para 4KB) é 1000x mais rápido.
- **Race condition MADV_DONTNEED:** Se o processo escreve na página entre a cópia para VRAM e o MADV_DONTNEED, os dados são perdidos. **Mitigação validada:** usar `UFFDIO_WRITEPROTECT` para proteger a página contra escrita antes de copiar. Se o processo escrever, gera fault WP que o daemon detecta e aborta a evicção.

### 3. Padrões de Hardware
- OpenCL `clEnqueueWriteBuffer`/`clEnqueueReadBuffer`: Transferência síncrona ou assíncrona RAM↔VRAM.
- RTX 2060 (PCIe 3.0 x16): ~15.75 GB/s teórico, ~12 GB/s prático para transferências OpenCL.
- RX 6700 XT (PCIe 4.0 x16): ~31.5 GB/s teórico, ~25 GB/s prático.

## Requisitos funcionais

- **RF-1:** O daemon deve aceitar processos-alvo via:
  - (a) cgroup membership (monitora todos os processos de um cgroup)
  - (b) PID explícito via CLI ou socket UNIX
  - (c) biblioteca `libramshared.so` (LD_PRELOAD ou link direto)
- **RF-2:** O daemon deve executar ciclos periódicos de idle page tracking:
  1. Ler `/proc/<pid>/pagemap` para obter PFNs das páginas anônimas do processo.
  2. Setar bits no `/sys/kernel/mm/page_idle/bitmap`.
  3. Aguardar período de resfriamento configurável (default: 30s).
  4. Reler bitmap; páginas com bit ainda setado = frias.
- **RF-3:** Para cada página fria identificada:
  1. Ativar write-protect via `UFFDIO_WRITEPROTECT` na região.
  2. Copiar conteúdo (4KB) para buffer VRAM via `clEnqueueWriteBuffer`.
  3. Registrar mapeamento (endereço virtual → offset VRAM) em tabela interna.
  4. Executar `madvise(MADV_DONTNEED)` para liberar a página física (via cooperação ou ptrace).
- **RF-4:** Quando ocorrer page fault em região registrada:
  1. Receber evento `UFFD_PAGEFAULT` via leitura do fd.
  2. Buscar dados correspondentes no buffer VRAM via `clEnqueueReadBuffer`.
  3. Injetar página via `UFFDIO_COPY`.
  4. O processo retoma execução transparentemente.
- **RF-5:** O daemon deve usar `mlockall(MCL_CURRENT | MCL_FUTURE)` para impedir que suas próprias páginas sejam evictas.
- **RF-6:** O daemon deve lidar com eventos `UFFD_EVENT_FORK` (registrar processo filho), `UFFD_EVENT_REMAP` (atualizar mapeamentos) e `UFFD_EVENT_REMOVE` (limpar mapeamentos).

## Requisitos não-funcionais

- **Performance:**
  - Latência de page fault (VRAM→RAM): ≤ 50μs (inclui leitura OpenCL + UFFDIO_COPY + wake do faulter).
  - Throughput de evicção (RAM→VRAM): ≥ 2 GB/s em PCIe 3.0, ≥ 8 GB/s em PCIe 4.0 (transferências em batch).
  - Overhead de idle page tracking: ≤ 5ms por scan de 4M páginas.
- **Estabilidade:**
  - Crash do daemon: processos monitorados recebem SIGBUS em page faults pendentes. Não afeta o kernel.
  - GPU driver crash: `clEnqueueReadBuffer` retorna erro. Daemon sinaliza SIGBUS para processos afetados e faz cleanup.
  - O daemon DEVE ter watchdog interno que detecta se a GPU ficou irresponsiva (timeout de OpenCL) e executa migração de emergência de todos os mapeamentos de volta para RAM (usando dados em staging buffer ou sinalizando perda de dados).
- **Segurança:**
  - O daemon requer `CAP_SYS_PTRACE` + `CAP_SYS_ADMIN` (mínimo necessário).
  - Buffers VRAM devem ser alocados com `CL_MEM_HOST_NO_ACCESS` quando não em uso ativo (previne leitura via outros contextos OpenCL).
  - Tabela de mapeamentos (endereço→offset) deve ser protegida contra acesso por outros processos.

## Fluxos de Memória

### Happy path (Evicção de Página Fria)
1. Processo `firefox` está usando 4GB de RAM, dos quais 1.5GB não foram acessados em 30s.
2. Daemon detecta 1.5GB de páginas frias via idle page tracking.
3. Daemon ativa write-protect nas 384K páginas frias.
4. Daemon copia páginas para VRAM em batches de 256 (1MB por batch) via OpenCL.
5. Daemon executa `madvise(MADV_DONTNEED)` em cada batch concluído.
6. RAM é liberada. Sistema agora tem 1.5GB livres adicionais.

### Happy path (Restauração sob Demanda)
1. Firefox acessa uma aba que estava inativa (página na VRAM).
2. Kernel gera page fault (página missing, PTE nulo).
3. userfaultfd entrega evento ao daemon.
4. Daemon lê 4KB da VRAM via `clEnqueueReadBuffer` (~0.3μs transferência + ~5-20μs overhead total).
5. Daemon injeta página via `UFFDIO_COPY`.
6. Firefox retoma. Latência total percebida: ~10-50μs (imperceptível para UI).

### Fluxo de Erro: Race Condition de Escrita
1. Daemon marca página como candidata e ativa write-protect.
2. Processo escreve na página ANTES da cópia completar.
3. Kernel gera `UFFD_PAGEFAULT_FLAG_WP` event.
4. Daemon detecta, aborta a evicção desta página, remove write-protect.
5. Nenhum dado é perdido.

### Fluxo de Erro: GPU Driver Crash
1. Driver NVIDIA/AMD reseta a GPU.
2. `clEnqueueReadBuffer` retorna `CL_DEVICE_NOT_AVAILABLE`.
3. Daemon marca todos os mapeamentos VRAM como "perdidos".
4. Para processos com páginas pendentes: daemon tenta realocar contexto OpenCL; se falhar, sinaliza SIGBUS.
5. Daemon loga o evento e reinicializa o contexto OpenCL quando a GPU estiver disponível.

## Estruturas de Dados

**Tabela de Mapeamento (in-memory, daemon):**
```rust
struct VramMapping {
    pid: u32,
    vaddr: u64,          // endereço virtual no processo
    vram_offset: u64,    // offset no buffer OpenCL
    size: u32,           // 4096 (PAGE_SIZE) ou 2MB (THP)
    timestamp: u64,      // quando foi evicta (para métricas)
}

struct DaemonState {
    cl_context: cl_context,
    cl_queue: cl_command_queue,
    vram_buffer: cl_mem,           // buffer contíguo na VRAM
    vram_allocator: BumpAllocator, // alocador simples para offsets
    mappings: HashMap<(u32, u64), VramMapping>,  // (pid, vaddr) → mapping
    uffd_fds: HashMap<u32, RawFd>, // pid → userfaultfd
    config: DaemonConfig,
}
```

**Interface de Controle (socket UNIX `/run/ramshared-uffd.sock`):**
- `ADD_PID <pid>` — começar a monitorar processo.
- `REMOVE_PID <pid>` — parar de monitorar, restaurar todas as páginas.
- `STATUS` — retornar JSON com métricas (páginas evictas, faults servidos, latência média).
- `FLUSH` — forçar restauração de todas as páginas de volta para RAM.

## Dependências e riscos

- **Pré-requisitos:**
  - `CONFIG_USERFAULTFD=y` (habilitado por padrão na maioria das distros).
  - `CONFIG_IDLE_PAGE_TRACKING=y` (pode precisar de recompilação em algumas distros).
  - OpenCL 1.2+ runtime para a GPU alvo.
  - `vm.unprivileged_userfaultfd=0` (padrão) — daemon roda como root ou com capabilities.
- **Riscos:**
  - **Transparência parcial:** Processos não-cooperativos precisam de ptrace ou LD_PRELOAD. Não é 100% transparente como PRD-1/4.
  - **Overhead de monitoramento:** Ler `/proc/<pid>/pagemap` para processos com muita memória (>32GB) pode ser lento (~50ms). Mitigação: sampling adaptativo.
  - **TLB shootdown storms:** `madvise(MADV_DONTNEED)` em muitas páginas gera IPIs de invalidação de TLB entre todas as CPUs. Mitigação: batch de 256 páginas com delay configurável entre batches.
  - **GPU reclaim:** Se o runtime OpenCL fizer garbage collection do buffer VRAM (ex: NVIDIA gerenciamento automático de memória), dados são perdidos. Mitigação: usar `CL_MEM_ALLOC_HOST_PTR` com pinning, ou Vulkan com explicit memory management.

## Portão Cognitivo de Kahneman (Obrigatório antes do SPEC)

- **Disciplina Aplicável:** Disciplina 3 (Prevenção de Deadlock em Memória) + Disciplina 4 (Isolamento entre Processos).

- **Justificativa (Disciplina 3):** O daemon é um processo userspace que precisa de RAM para funcionar. Se o sistema estiver sob pressão de memória extrema e o OOM killer matar o daemon, todos os processos monitorados perdem acesso às páginas na VRAM (SIGBUS). O daemon DEVE usar `mlockall(MCL_CURRENT | MCL_FUTURE)` e ajustar `oom_score_adj` para -1000 (imune ao OOM killer). Além disso, o daemon DEVE pré-alocar todos os buffers de trabalho no startup, nunca alocando memória no hot path de fault handling.

- **Justificativa (Disciplina 4):** O daemon tem acesso ao conteúdo de memória de múltiplos processos (via pagemap + UFFDIO_COPY). Se o daemon for comprometido, ele pode vazar dados entre processos. O SPEC DEVE definir sandboxing do daemon (seccomp-bpf, namespaces) e criptografia dos mapeamentos VRAM (AES-256-CTR sobre o offset) para mitigar este vetor.

- **Evidência exigida para o SPEC:**
  1. Demonstrar `mlockall(MCL_CURRENT | MCL_FUTURE)` + `oom_score_adj = -1000` antes de qualquer IO.
  2. Demonstrar o protocolo de write-protect (`UFFDIO_WRITEPROTECT`) antes de cópia, com abort path quando `WP fault` é detectado.
  3. Demonstrar que nenhuma alocação de heap ocorre no handler de `UFFD_PAGEFAULT` (usar arena pré-alocada).
  4. Demonstrar perfil de seccomp-bpf que restringe syscalls do daemon ao mínimo necessário.

- **Gatilho de Aborto:**
  - Se `CONFIG_IDLE_PAGE_TRACKING` não estiver habilitado no kernel-alvo e não puder ser habilitado, ABORTAR (sem idle tracking, não há como identificar páginas frias sem polling invasivo).
  - Se o runtime OpenCL não garantir pinning de buffers VRAM (ex: driver proprietário faz evicção silenciosa), ABORTAR e usar Vulkan com `VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT` + explicit allocation.
