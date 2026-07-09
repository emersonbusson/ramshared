# Degradation Matrix — RamShared

> Modos de falha **raros mas caros** (Kahneman #5 — projetar pro pior caso).
> Âncora da disciplina #5 em [`kahneman-disciplines.md`](../methodology/kahneman-disciplines.md).
>
> Prioridade = **probabilidade × impacto**. Cenário cosmético fica
> "monitorado, não desenhado" (contra a paranoia — contra-exemplo #5 do doc).
> Cada cenário tem **detecção** (sinal observável) e **unwind** (recuperação em
> ordem inversa de alocação — idioma `goto out_err`).

## Matriz

| Cenário | Prob×Impacto | Comportamento projetado (degradação) | Detecção | Unwind / recuperação | Status |
| --- | --- | --- | --- | --- | --- |
| `dma_map_single`/`dma_alloc` falha (`-ENOMEM`) | médio × alto | abortar a operação, propagar errno; nunca expor device meio-mapeado | retorno != 0 checado | `goto out_err`: desfazer mapeamentos na ordem inversa, liberar páginas, sem leak | **desenhado** |
| OOM killer dispara com lock segurado | baixo × crítico | daemon de swap não pode depender de alloc no hot path; `mlockall` + `oom_score_adj=-1000` (SPECv3 §6.2/§11) | `dmesg` OOM; processo morto | pré-alocar tudo antes de expor device; nada de `GFP_KERNEL` em seção crítica | **desenhado** |
| **GPU-PV WDDM evicta a VRAM viva** (nosso caso) | **alto × alto** | latência 4K explode (~1,18 s medido, FASE0-FINAL); **DEMOTE**: `swapoff` só do tier VRAM, páginas caem pro VHDX, sem matar processo | canário de latência (SPECv3 §9.1, gatilho (a) p99 > K×baseline) | `swapoff` VRAM (timeout `T_demote`=30s) → estado `Demoted`; se estourar → `recover` | **desenhado** (SPECv3 §9.2) |
| `swapoff` do VRAM trava sob eviction | médio × alto | não desconectar NBD com swap ativo (= panic); escalonar | processos em `D` > `T_stuck`; `nbd: ... timed out` no `dmesg` | `recover` escalonado (SPECv3 §13); `wsl --terminate` só em último recurso | **desenhado** |
| PCIe bus reset / device removal mid-DMA | baixo × crítico | I/O em voo → erro; marcar `Failed`; não completar I/O como sucesso parcial | erro CUDA / `dmesg` reset | parar filas, drenar/rejeitar pendentes, `recover` | **desenhado** (SPECv3 §8.2) |
| IOMMU fault | baixo × crítico | falhar a operação que causou; não mascarar | `dmesg` IOMMU fault | unwind do mapeamento ofensor; logar contexto | **monitorado** (fora do MVP WSL2) |
| CXL link-down / coerência perdida | baixo × crítico | tratar como device lost | `dmesg`/EDAC | offline do nó/tier afetado | **monitorado** (hardware futuro) |
| Race suspend/resume (S3/S4) | baixo × alto | reârmar contexto CUDA no resume ou recusar; sem assumir estado preservado | falha de `cuCtx*` pós-resume | reinicializar tier ou `Demoted` | **monitorado** (não no MVP manual) |

## Como usar

- Toda feature crítica nova **adiciona ou revisa** linhas aqui antes do merge
  (sinal mensurável da disciplina #5: "a matrix foi atualizada na última feature").
- Um postmortem cujo cenário **não estava** aqui → adicionar a linha como ação
  corretiva (ver [`postmortems/TEMPLATE.md`](../postmortems/TEMPLATE.md)).

## Referências

- [`kahneman-disciplines.md`](../methodology/kahneman-disciplines.md) §5
- [`wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §8 (erros), §9 (eviction/DEMOTE), §13 (recovery)
- [`wsl2-fase0-final.md`](wsl2-fase0-final.md) — a medida de 1,18 s que fundou a linha de eviction
