---
slug: vram-as-ram-numa-node
title: Integração Nativa da VRAM como Nó NUMA via CXL/HMM
milestone: M01
issues: []
---

# PRD — Integração da VRAM como RAM Nativa (Nó NUMA)

## Resumo

Este PRD descreve a arquitetura para integrar transparentemente a VRAM (Video RAM) de GPUs discretas no pool de memória global do sistema operacional Linux. O projeto visa resolver a limitação atual onde a VRAM é isolada para cargas de trabalho gráficas e computacionais, permitindo que o sistema operacional utilize a VRAM ociosa como memória RAM de uso geral (system memory) de forma nativa. O foco é a integração via emulação de Nós NUMA, utilizando HMM (Heterogeneous Memory Management) e, de forma prospectiva, tirando vantagem de conexões CXL (Compute Express Link) para coerência de cache em hardware.

## Contexto arquitetural e de Hardware

- **Subsistemas do Kernel:** Gerenciamento de Memória (`mm/`), PCIe, DRM (Direct Rendering Manager).
- **Hardware Alvo:** GPUs com suporte a PCIe Resizable BAR (ReBAR) habilitado na BIOS/UEFI e futuras GPUs compatíveis com o padrão CXL 2.0/3.0.
- **Topologia:** A CPU e a RAM principal formam o Nó NUMA 0 (alta performance, baixa latência). A GPU e a VRAM serão mapeadas como o Nó NUMA 1 (alta largura de banda, latência PCIe).
- **Confirmado no Kernel Mainline:** O HMM já suporta a migração de páginas entre memória do sistema e memória de dispositivo para cargas de trabalho específicas de GPU (ex: CUDA Managed Memory).
- **Proposta:** Expandir o modelo HMM e NUMA para tornar a VRAM disponível ao alocador de páginas genérico do Linux (buddy allocator) de maneira transparente para aplicativos convencionais (userspace applications).

## Opção recomendada

**Criação de um módulo de kernel (ramshared.ko) que expõe a VRAM mapeada via ReBAR como memória "hotplug" no subsistema NUMA.**

- **Motivo da escolha:** Usar o subsistema NUMA existente permite que o alocador do kernel Linux priorize a RAM principal (Nó 0) e "vaze" (spillover) para a VRAM (Nó 1) apenas quando necessário, ou quando políticas explícitas (`numactl`) exigirem. Isso não requer a reescrita do gerenciador de memória virtual inteiramente do zero.
- **Alternativas descartadas:** Userspace block devices (como o `ublk-vram` formatado como Swap). Foram descartadas por introduzirem overhead de troca de contexto (context switch), cópia de blocos desnecessária e latência imprevisível para acesso à memória em tempo real.
- **Trade-offs:** Acessar VRAM via PCIe Gen4/Gen5 para cargas da CPU não-coerentes adiciona latência comparado a DDR4/DDR5. Até que o hardware CXL seja onipresente, a CPU precisará de *cache flushing* agressivo para manter a consistência, impactando a performance de processos executando no Nó 1.

## Requisitos funcionais

- **RF-1:** O módulo deve interceptar o mapeamento de memória PCIe (via ReBAR) da GPU primária no boot ou carregamento.
- **RF-2:** O módulo deve invocar a API de hotplug de memória do kernel (`add_memory()`) para registrar os blocos de VRAM físicos como uma zona de memória gerenciável pelo sistema.
- **RF-3:** A VRAM registrada deve ser classificada como um nó NUMA separado (distante) para que o alocador prefira a RAM primária.
- **RF-4:** O módulo deve implementar um mecanismo de evicção prioritária ("GPU First"), liberando páginas alocadas por processos de usuários de volta para a RAM primária (ou disco de swap) se o driver DRM nativo da GPU solicitar a VRAM para renderização.

## Requisitos não-funcionais

- **Performance:** O mapeamento deve ocorrer de forma direta no TLB. O atraso no *page fault* para VRAM não deve exceder significativamente o tempo de travessia do barramento PCIe.
- **Estabilidade:** Evitar corrupção. Caso a GPU entre em modo D3cold (suspensão), o kernel deve migrar as páginas de VRAM ativas para a RAM antes de cortar a energia.
- **Segurança:** As páginas de VRAM usadas pelo OS devem ser limpas com zeros antes de serem devolvidas ao pool do driver DRM para evitar o vazamento de dados da memória do sistema (chaves, senhas) para processos de shader da GPU.

## Fluxos de Memória

### Happy path (Alocação Transparente)
1. Memória RAM primária (Nó 0) se aproxima de 95% de uso.
2. Kernel tenta alocar páginas para um processo de usuário comum.
3. O alocador NUMA falha no Nó 0 e busca no Nó 1 (VRAM).
4. Página física é alocada dentro do espaço de endereço MMIO da GPU.
5. CPU envia dados para a VRAM; o acesso é transparente para o aplicativo.

### Evicção Forçada (Mass Eviction)
1. O usuário abre um jogo 3D (exigindo alta alocação de VRAM via driver `amdgpu` ou `nouveau`).
2. O driver nativo sinaliza o kernel sobre a falta de VRAM exclusiva.
3. O módulo `ramshared` aciona um *callback* de emergência.
4. O gerenciador de memória do Linux congela os processos mapeados no Nó 1 e executa DMA transfer para migrar as páginas rapidamente para a área de Swap no disco SSD.
5. A VRAM é limpa e entregue ao jogo.

## Estruturas de Dados e Sysfs

**Nós Sysfs planejados:**
- `/sys/kernel/ramshared/status`: Status do módulo (Ativo/Inativo, tamanho de VRAM sequestrada).
- `/sys/kernel/ramshared/vram_node_id`: O ID do nó NUMA designado para a GPU.
- `/sys/kernel/ramshared/evict_trigger`: Gatilho manual para forçar a migração de dados da VRAM para a RAM primária.

**Struct Kernel Base:**
```c
struct ramshared_device {
    struct pci_dev *pdev;
    phys_addr_t bar_start;
    resource_size_t bar_size;
    int numa_node;
    struct memory_block *mem_blk;
    // Callbacks de migração e estado de energia
};
```

## Dependências e riscos

- **Pré-requisitos de hardware:** Resizable BAR (ReBAR) deve estar habilitado para que a totalidade da VRAM possa ser alocada fisicamente em um bloco contíguo de espaço MMIO (Memory-Mapped I/O) mapeável pela CPU.
- **Riscos de corrupção:** A natureza volátil do driver gráfico proprietário da NVIDIA (quando aplicável) torna imprevisível o compartilhamento de recursos de baixo nível se o driver não estiver em conformidade com as APIs DRM abertas.
- **Mitigação:** O desenvolvimento inicial foca estritamente em drivers open-source que implementam os padrões DRM/KMS completos (ex: `amdgpu` e `nouveau`).

#### Portão Cognitivo de Kahneman (Obrigatório antes do SPEC)
- **Disciplina Aplicável:** Disciplina 2 (Sobrevivência ao Hardware) e Disciplina 4 (Isolamento entre Processos).
- **Justificativa:** Como estamos interceptando o gerenciamento do subsistema PCIe via ReBAR, uma falha de hardware aqui ou evicção massiva gera vazamento de estado gráfico entre processos ou travamento da máquina.
- **Evidência exigida para o SPEC:** O SPEC deve obrigatoriamente apresentar o hook do kernel usado para interceptar o sleep da GPU (`pm_ops`) e o mecanismo de `memset` que limpa as páginas de VRAM antes de devolvê-las.
