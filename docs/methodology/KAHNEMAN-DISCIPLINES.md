# Disciplinas Kahneman para Desenvolvimento no Kernel (RamShared)

Nossa implementação em Ring-0 (Kernel) e Ring-3 de baixo nível é perigosa. Um simples erro de acesso à memória derruba a máquina inteira (Kernel Panic) ou corrompe silenciosamente o SSD através de I/O mal direcionado.

Para forçar a ativação do "Sistema 2" (pensamento analítico, lento e calculista) de Daniel Kahneman, exigimos a aplicação rigorosa das seguintes disciplinas em cada etapa crítica descrita no `SPEC.md`.

---

## 1. Disciplina de Segurança de Barramento e DMA

**Risco combatido:** Interrupção (hang) do barramento PCIe ou corrupção de memória durante transferências DMA, causando congelamento completo do hardware.

- **Pergunta Obrigatória:** "A transferência DMA foi devidamente validada com `dma_mapping_error` e o cache da CPU foi invalidado (flushed) para esse endereço antes e depois da operação?"
- **Evidência Mínima Exigida:** Código deve conter os pares `dma_map_single()` / `dma_unmap_single()` com tratamento de erro explícito. Um teste unitário isolado provando que `panic()` não ocorre em falha simulada do hardware.
- **Gatilho de Aborto (Abort Trigger):** Se a placa de vídeo não garantir suporte coerente à área de memória DMA mapeada (snoop/non-snoop incoerente), ABORTAR implementação.

---

## 2. Disciplina de Sobrevivência ao Hardware (Device Removal / D3 State)

**Risco combatido:** O usuário entra em modo de suspensão, a GPU desliga (D3cold), ou o driver reseta a placa. Se o kernel não for notificado, ele tentará ler VRAM desligada e explodirá.

- **Pergunta Obrigatória:** "O que acontece matematicamente com os processos do usuário se a VRAM perder energia em 2 milissegundos?"
- **Evidência Mínima Exigida:** O módulo deve interceptar os callbacks de Power Management do barramento PCI (`pm_ops->suspend`, `pm_ops->resume`) e forçar uma evicção de memória para o Swap local antes de liberar a energia da GPU.
- **Gatilho de Aborto (Abort Trigger):** Se o driver fechado da NVIDIA (blob) mascarar o callback de estado de energia ou não conceder tempo para evicção, o driver deve recusar a montagem (Forward-only error).

---

## 3. Disciplina de Prevenção de Deadlock em Memória (OOM Loop)

**Risco combatido:** O módulo precisa alocar memória RAM primária para gerenciar as rotinas de transferir páginas para a VRAM. Se o sistema já estiver sem memória RAM primária, o processo de liberar memória falha ao tentar alocar memória para si mesmo, travando tudo num "OOM (Out Of Memory) Deadlock".

- **Pergunta Obrigatória:** "As estruturas internas do daemon/módulo estão garantidas e travadas na RAM física usando `mlockall` ou pools pré-alocados (mempools) no boot?"
- **Evidência Mínima Exigida:** Uso de `mempool_alloc` no espaço de kernel ou verificação explícita do retorno de `mlockall` no userspace antes da alocação de qualquer I/O path.
- **Gatilho de Aborto (Abort Trigger):** Se houver chamadas `kmalloc(..., GFP_KERNEL)` ocorrendo no caminho quente de paginação (page fault handler), ABORTAR (deve-se usar `GFP_NOWAIT` ou pools).

---

## 4. Disciplina de Isolamento entre Processos

**Risco combatido:** Como a VRAM é muitas vezes gerida num espaço contíguo aberto pelo driver da GPU, há o risco de que os dados do Processo A escritos na VRAM sejam lidos diretamente pelos shaders do Processo B, quebrando a segurança de memória do SO.

- **Pergunta Obrigatória:** "A zona de memória roubada pela VRAM está 100% zerada (`memset` / DMA fill) no instante que é alocada e no instante em que é desalocada e devolvida para a placa gráfica?"
- **Evidência Mínima Exigida:** Inspeção visual de um log de debug provando que blocos ejetados sofrem um "secure wipe" se o destinatário não os sobreescreveu no ciclo imediato.
- **Gatilho de Aborto (Abort Trigger):** Vazamento de bytes residuais entre processos em VRAM. Em caso de falha no wipe, travar (panic) do que permitir vazamento de chaves ou segredos em userspace.
