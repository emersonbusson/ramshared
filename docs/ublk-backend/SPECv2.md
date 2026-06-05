# SPECv2 — Fase B — ublk no lugar do NBD

> Versão após auditoria do Passo 2.5. Baseline: [`SPEC.md`](SPEC.md). **DESIGN-ONLY / kernel-gated.**
> Motivo (no-go do SPEC): (H5-1) DT-3 mandava **completar o CQE a partir do worker** = submissão
> cross-thread no mesmo io_uring (modelo de threading inválido); (M5-1) custo do `unsafe` de
> io_uring hand-rolled subestimado (barreiras de memória, ring compartilhado ≠ FFI dlopen do CUDA);
> (M5-2) gate só no Kconfig do ublk, faltando `io_uring`/`ublk-control`; (M5-3) `nbd_dev` hardcoded
> no DEMOTE.

## 0. Proveniência da auditoria

- **Auditado:** `SPEC.md`. **Resultado:** `no-go` (1 HIGH de design + 3 MEDIUM). Day-0, política e
  rastreabilidade do SPEC foram aprovados; o furo era localizado (threading do ring).
- **Este SPECv2 é o design ativo** (kernel-gated; IMPL no Passo 3 futuro).

## Decisões técnicas (delta sobre o SPEC)

| # | Decisão | Corrige |
| --- | --- | --- |
| DT-3 | **A thread io_uring é a ÚNICA dona do ring** (submit + complete). Fluxo: thread io_uring colhe `UBLK_IO_FETCH_REQ` → enfileira `Job` no canal `WMsg` (worker H1) → worker CUDA processa → devolve `Reply` (com o handle do request) por um canal **de volta à thread io_uring** → a thread io_uring submete o `UBLK_IO_COMMIT_AND_FETCH_REQ`. O worker **nunca** toca o ring (espelha exatamente o writer do H1: o worker manda `Reply`, outra thread faz o I/O de saída). Resolve a submissão cross-thread. | H5-1 |
| DT-1 | **Calibração honesta do `unsafe` (M5-1):** io_uring hand-rolled exige `mmap` das filas SQ/CQ + **barreiras de memória** (`acquire`/`release` nos índices) + protocolo de ring — `unsafe` **qualitativamente mais perigoso** que o `dlopen`+chamadas síncronas do `ramshared-cuda`. Por isso o ADR deve pesar **duas** opções de verdade: (a) crate `ramshared-uring` hand-rolled (zero-dep, mas barreiras em `unsafe` = risco de correção alto); (b) crate **`io-uring` externa auditada** (quebra zero-dep, mas a unsafe de baixo nível fica numa lib madura). **Recomendação revisada:** dado o risco de correção de barreiras de memória, a crate auditada pode ser a escolha **mais segura** apesar do zero-dep — decisão para o ADR com critério (LoC unsafe, histórico de CVEs da crate, bench). Não pré-decidir hand-rolled. | M5-1 |
| DT-5 | **Gate completo (M5-2):** exigir `CONFIG_BLK_DEV_UBLK` **e** `io_uring` funcional + `/dev/ublk-control` (o `check` do projeto já sabe checar io_uring). Nota factual: `CONFIG_IO_URING=y` **já existe** no WSL2 atual — falta só o `ublk_drv`. | M5-2 |
| DT-6 | **Generalizar o device de swap (M5-3):** o DEMOTE/`spawn_swapoff` e o arg `--nbd` viram **`--swap-dev`/`swap_dev`** genérico (`/dev/nbd0` ou `/dev/ublkbN`). Sob ublk a VRAM **continua em swap** (só muda o transporte), então o `swapoff <swap_dev>` do DEMOTE permanece seguro (kernel drena) — diferente do item 4. | M5-3 |
| DT-2 | **Mantida:** daemon modo ublk reusa o worker H1; `--transport {nbd,ublk}` default `nbd`; ublk substitui NBD na paridade (sem dual-path permanente). | — |

## Fronteira de atomicidade e rollback

- Atomicidade: worker único serializa a VRAM (H1); ring possuído por 1 thread (DT-3). DEMOTE =
  `swapoff <swap_dev>` (seguro, kernel drena — a VRAM segue swap sob ublk).
- Rollback: app-only + `--transport` default `nbd` (fallback Day-0 validado §14).

## Mapa Kahneman (corrigido)

| Etapa / ITEM | Disciplina | Link | Pergunta | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (`ramshared-uring` vs crate) | #11 halo + rust/security | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "hand-roll barreiras de io_uring (risco de correção) vs crate auditada (quebra zero-dep)?" | ADR com LoC unsafe + CVEs da crate + bench; latência ublk < NBD por ≥X% | sem ganho de latência → manter NBD |
| ITEM-2 (ring threading, DT-3) | #5 Worst-case | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "quem toca o ring? CUDA fora do ctx?" | thread io_uring é única dona do ring; worker é única a tocar CUDA; asan/lockdep limpos | submissão cross-thread no ring / CUDA fora do ctx → reverter |

## Recomendação final (design ativo)

Desenho **sólido e pronto como spec de design** (kernel-gated). Diferente do item 4, o ublk
**mantém o DEMOTE seguro** (VRAM segue swap) e o furo de threading foi fechado (DT-3). A única
decisão aberta — e deliberadamente roteada ao **ADR** — é hand-rolled vs crate `io-uring` (DT-1),
a ser fechada com números quando o kernel custom existir. Passo 3 abre com `CONFIG_BLK_DEV_UBLK`.

## Validação

- **Hoje:** N/A (design-only). Entregue: desenho de threading correto + decisão de política roteada.
- **Futuro (kernel):** §14 sobre ublk + bench latência ublk vs NBD (justificativa) + `grep unsafe`
  confinado a `ramshared-uring`/`ramshared-cuda`.
