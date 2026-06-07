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
| DT-1 | **Calibração honesta do `unsafe` (M5-1):** io_uring hand-rolled exige `mmap` das filas SQ/CQ + **barreiras de memória** (`acquire`/`release` nos índices) + protocolo de ring — `unsafe` **qualitativamente mais perigoso** que o `dlopen`+chamadas síncronas do `ramshared-cuda`. ADR-0004 foi aceita em 2026-06-07: usar crate **`io-uring` auditada** (`0.7.12`, MIT/Apache-2.0) via wrapper `ramshared-uring`, em vez de FFI hand-rolled. A exceção quebra zero-dep só no userspace/Fase B e continua gated em bench. | M5-1 |
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
| ITEM-1 (`io-uring` crate) | #11 halo + rust/security | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "crate auditada com barreiras corretas justifica quebrar zero-dep userspace?" | ADR-0004 Accepted + `LIBRARIES.md` + bench; latência ublk < NBD por ≥X% | sem ganho de latência → manter NBD e remover dep |
| ITEM-2 (ring threading, DT-3) | #5 Worst-case | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "quem toca o ring? CUDA fora do ctx?" | thread io_uring é única dona do ring; worker é única a tocar CUDA; asan/lockdep limpos | submissão cross-thread no ring / CUDA fora do ctx → reverter |

## Recomendação final (design ativo)

Desenho **sólido e pronto como spec de design** (kernel-gated). Diferente do item 4, o ublk
**mantém o DEMOTE seguro** (VRAM segue swap) e o furo de threading foi fechado (DT-3). A decisão
hand-rolled vs crate foi fechada pela **ADR-0004 Accepted**: usar `ramshared-uring` +
`io-uring 0.7.12` no primeiro smoke de ring. A adoção do ublk em produção continua dependente de
bench ublk vs NBD.

## Validação

- **Hoje:** N/A (design-only). Entregue: desenho de threading correto + decisão de política roteada.
- **Futuro (kernel):** §14 sobre ublk + bench latência ublk vs NBD (justificativa) + `grep unsafe`
  confinado a `ramshared-cuda`, `ramshared-uring` e à crate externa `io-uring` (sem `unsafe` novo
  no daemon).
