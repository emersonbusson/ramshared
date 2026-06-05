# SPEC — Fase B — zram writeback do frio direto na VRAM

Fonte: [`PRD.md`](PRD.md). **DESIGN-ONLY / kernel-gated** (sem `CONFIG_ZRAM_WRITEBACK` no WSL2
atual — verificado). Fecha as decisões de design; o Passo 3 (IMPL) fica para quando houver
kernel custom. Itens não-testáveis agora estão marcados **(kernel-gated)**.

## Escopo fechado

**Entra (quando houver kernel):** `backing_dev` do zram = device de VRAM; política de
writeback por `idle`/`huge` com `writeback_limit`; VHDX como overflow final; DEMOTE adaptado.
Flag `up --writeback` (default off → mantém o Day-0 de 2 tiers).
**Fica fora:** IMPL/validação agora; writeback a arquivo; trocar o worker único; ublk (item 5).

## Matriz de rastreabilidade PRD → SPEC

| PRD | SPEC |
| --- | --- |
| RF-1 (backing na VRAM) | DT-1, ITEM-1 |
| RF-2 (política writeback) | DT-2, ITEM-2 |
| RF-3 (VHDX overflow) | DT-3, ITEM-1 |
| RF-4 (DEMOTE coerente) | DT-4, ITEM-3 |

## Decisões técnicas

| # | Decisão | Justificativa |
| --- | --- | --- |
| DT-1 | `backing_dev` = o device de VRAM já existente (`/dev/nbdX` do daemon H1, ou ublk na Fase B). Setado **antes** do `disksize` (exigência do zram). A VRAM **deixa** de ser `swapon` separado; vira store de writeback do zram. | reusa o device validado (H1); elimina o 2º swap-out do caminho frio. |
| DT-2 | Writeback por **`idle`** (cadência, ex.: a cada T) + **`huge`** (incompressíveis na hora); `writeback_limit` = função do tamanho da VRAM (não saturar). | nativo do kernel; controla volume; idle = frio de verdade. |
| DT-3 | VHDX permanece `swapon` com prio menor que o zram, como overflow quando a VRAM (backing) enche. | rede final; sem OOM. |
| DT-4 | **DEMOTE adaptado:** com a VRAM como backing (não swap), o gatilho do canário passa a **desabilitar o writeback** (`writeback_limit 0`) + drenar o backing, em vez de `swapoff`. As páginas já no backing precisam ser lidas de volta para o zram/VHDX antes de soltar a VRAM. **(kernel-gated — decisão a confirmar na semântica real do zram.)** | mantém a proteção §9 sob a nova arquitetura. |

## Fronteira de atomicidade e rollback

- Atomicidade: cada writeback é I/O de bloco do kernel ao backing (atômico por página). DEMOTE
  drena o backing antes de soltar.
- Rollback: **app-only** + flag `--writeback` default off (o Day-0 de 2 tiers é o fallback).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (backing=VRAM via daemon) | #5 Worst-case | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "o writeback do zram ao backing (NBD→daemon→CUDA) reentra na RAM que o zram está liberando → deadlock?" | (kernel) `bd_stat` evolui sob carga sem stall; sem deadlock em lockdep | stall de I/O / deadlock no writeback → reverter p/ 2 tiers |
| ITEM-3 (DEMOTE adaptado) | #2 Counterfactual | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "soltar a VRAM-backing com páginas vivas nela perde dado?" | (kernel) DEMOTE drena backing→VHDX 0 corrupção | corrupção ao soltar o backing → reverter |

## Arquivos a MODIFICAR (quando houver kernel)

### `crates/ramshared-cli/src/cascade.rs`
- **O que muda:** `up --writeback [--wb-limit N]`: após `modprobe zram`, `echo <vram_dev> >
  /sys/block/zramN/backing_dev` **antes** do `disksize`; **não** fazer `swapon` do nbd0 (a VRAM
  vira backing, não swap); setar `writeback_limit`; iniciar política `idle` em cadência. `down`:
  `writeback_limit 0` + drenar + `swapoff zram` + soltar backing + daemon zera a VRAM.
- **Requisitos:** RF-1, RF-2, RF-3, DT-1/2/3. **(kernel-gated)**
- **Disciplina Kahneman:** ITEM-1 (#5) — ver Mapa.

### `crates/ramshared-cli/src/main.rs` (check/status)
- **O que muda:** a linha "Tiers" passa a reportar `backing_dev` (via `/sys/block/zramN/bd_stat`)
  quando `--writeback`, em vez do 2º swap. **(kernel-gated)**

### `crates/ramshared-wsl2d` (daemon)
- **O que muda:** possivelmente nada (o backing emite BIOs ao mesmo device NBD; o worker único
  serve igual). **A confirmar (kernel-gated):** se o writeback exige semântica de FLUSH/discard
  diferente da do swap.

## Plano de testes (quando houver kernel)

- Aceitação §14 adaptada: pressão → `bd_stat.bd_writes` cresce (frio na VRAM); RAM do zram cai;
  overflow → VHDX; integridade (hash) pós-writeback; DEMOTE drena backing 0 corrupção.
- **Hoje (sem kernel):** N/A — só o desenho é auditável.

## Checklist de validação

- [ ] (kernel) `CONFIG_ZRAM_WRITEBACK=y`; `backing_dev` aceito; `bd_stat` evolui.
- [ ] (kernel) §14 adaptado verde; DEMOTE 0 corrupção.
- [x] Desenho fechado + riscos (deadlock de reentrância, DEMOTE-vs-backing) registrados.

## Documentos a atualizar (no IMPL futuro)

`docs/zram-writeback-vram/IMPL.md`; `ROADMAP.md` (Fase B → em progresso); `LIBRARIES.md`
(zram-writeback: de "NÃO usado" → "ativo, kernel custom"); `ARCHITECTURE.md` (cascata com backing).
