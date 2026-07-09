# Postmortem — <título curto do incidente>

> Postmortem blameless: avalia **processo, não outcome** (Kahneman #7). Copie
> para `docs/postmortems/AAAA-MM-DD-<slug>.md`.

## Metadados

- **Data:** AAAA-MM-DD
- **Severidade:** P0 (panic/perda de dado) · P1 (OOPS/stall grave) · P2 (regressão) · P3 (menor)
- **Componente:** `mm` | `drm` | `cxl` | `pci` | `dma` | `core`
- **Autor:** @usuario
- **ADR/SPEC relacionada:** `docs/decisions/ADR-NNN.md` / `docs/<feature>/SPEC*.md`

## Resumo

Um parágrafo, suficiente para alguém fora do incidente.

## Timeline (UTC, baseada em `dmesg`/`ftrace`)

| Hora | Evento (com a linha de `dmesg`/contador relevante) |
| --- | --- |
| 00:00 | ... |

## Blast radius (NÚMERO, não adjetivo — #3)

- Tasks/CPUs/nós NUMA afetados: `<n>`
- Orçamento estourado: ex. stall de interrupção `<x> ms` (limite `<y> ms`); DMA `<z> ns`
- Duração até detecção / até mitigação: `<min>`
- Dados perdidos? (páginas de swap, BOs, etc.): `<sim/não + n>`

## Detecção

Como apareceu: `lockdep` splat | `kmemleak` report | `KASAN` | OOPS/panic em `dmesg` |
`kselftest`/KUnit falhou | contador `/proc/vmstat` | report de usuário. **Existe ≠
funciona (#13):** se um teste verde não pegou, dizer por quê.

## Causa raiz

Dimensão técnica precisa (lock invertido, `dma_map` não desfeito, GFP errado em
contexto atômico, race suspend/resume, eviction não tratada...). **Não fechar como
"sorte/azar"** sem a causa técnica.

## Análise de processo (não de outcome) — #7

- O processo estava **certo na hora**, mesmo sabendo hoje que deu errado? (válido)
- Ou **funcionou/falhou por acidente** (decisão Sistema 1 que deu sorte/azar)? (alarme)
- O `Rollback trigger:` do patch/ADR existia e tinha número? Disparou? Foi acionado?

## Counterfactual retrospectivo (rollback trigger) — #2

O critério numérico que, em retrospecto, deveria ter revertido:
`se <métrica> <op> <valor> por <janela>, reverter para <release/commit>`.
(Ex.: `se stall de interrupção > 1 ms em 3 amostras, reverter o patch`.)

## Ações corretivas

| Ação | Tipo | Dono | Prazo | Critério de done (numérico) | PR/commit |
| --- | --- | --- | --- | --- | --- |
| ... | prevent\|detect\|respond | @u | AAAA-MM-DD | ex. "kselftest cobre o modo de falha; 0 kmemleak em soak 24h" | #NNN |

> **Obrigatório:** ao menos uma ação `detect` = um **kselftest/KUnit de regressão**
> que reproduz o modo de falha real (#13), não um mock.

## Anti-padrões (não fazer)

- ❌ Blame em pessoa. ❌ Hindsight ("era óbvio"). ❌ "Vamos ter mais cuidado" sem
  ação mensurável. ❌ Fechar sem causa técnica. ❌ Ação corretiva sem dono/prazo/número.

## Referências

- [`kahneman-disciplines.md`](../methodology/kahneman-disciplines.md) §7 (hindsight), §2 (counterfactual), §13 (ilusão de validade)
- [`reliability/DEGRADATION-MATRIX.md`](../reliability/DEGRADATION-MATRIX.md) — atualizar se o cenário não estava previsto
