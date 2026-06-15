# SPEC — Fase B — ublk no lugar do NBD para o tier VRAM

> **Superseded/no-go histórico.** Este arquivo preserva a primeira proposta auditada. O design
> ativo é [`SPECv2.md`](SPECv2.md); a decisão de dependência ativa é ADR-0004 Accepted
> (`io-uring 0.7.12`, não `ramshared-uring` hand-rolled).

Fonte: [`PRD.md`](PRD.md). **DESIGN-ONLY / kernel-gated em 2026-06-05** (naquele kernel WSL2,
sem `CONFIG_BLK_DEV_UBLK`). Fecha o desenho inicial + a **decisão de política** proposta
(io_uring vs zero-dep/`forbid(unsafe)`), depois revisada pelo SPECv2.
Passo 3 fica para kernel custom. Itens não-testáveis = **(kernel-gated)**.

## Escopo fechado

**Entra (quando houver kernel):** crate `ramshared-uring` (FFI io_uring com `unsafe` isolado);
daemon modo ublk reusando o worker CUDA único (H1); `up --transport ublk` (default `nbd`).
**Fica fora:** IMPL/validação agora; crate io_uring externa; dual NBD+ublk permanente; trocar o
worker único; zram-writeback (item 4).

## Matriz de rastreabilidade PRD → SPEC

| PRD | SPEC |
| --- | --- |
| RF-1 (device ublk) | DT-2, ITEM-2 |
| RF-2 (worker único) | DT-3, ITEM-2 |
| RF-3 (canário/DEMOTE) | DT-3, ITEM-2 |
| RF-4 (unsafe isolado) | DT-1, ITEM-1 |

## Decisões técnicas

| # | Decisão | Justificativa |
| --- | --- | --- |
| DT-1 | **Crate novo `ramshared-uring`** encapsula a FFI io_uring/liburing com `unsafe` contido + `// SAFETY:` por bloco (espelha `ramshared-cuda`). O daemon e a lib `ramshared-wsl2d` seguem `#![forbid(unsafe_code)]`; **zero dep externa** (FFI hand-rolled, não a crate `io-uring`). Registrar em `LIBRARIES.md` + ADR. | resolve a tensão io_uring×política sem quebrar zero-dep nem espalhar `unsafe`. |
| DT-2 | O daemon ganha **modo ublk**: loop io_uring (`UBLK_IO_FETCH_REQ`/`COMMIT_AND_FETCH`) traduz cada request da uAPI ublk num `Job` do **mesmo** canal `WMsg` do H1. NBD permanece como `--transport nbd` (default) até paridade; ublk = `--transport ublk`. | reusa toda a máquina H1 (worker, canário, DEMOTE); só troca o transporte. |
| DT-3 | **Afinidade CUDA preservada:** a thread do io_uring (`io_uring_enter`/completion) **não** roda CUDA — ela só enfileira `Job`s e completa I/O; o **worker CUDA único** (thread do `ctx`) serve a VRAM, como no H1. Completar via io_uring a partir do worker exige passar o handle do request pelo `Reply` (análogo ao `Sender<Reply>` por conexão). **(kernel-gated — confirmar a thread-safety do io_uring CQE cross-thread no SPEC do IMPL.)** | mantém a garantia `!Send` do `Context`; não regride H1. |
| DT-4 | **DEMOTE/canário:** latência serve-only (DT-16 do H1) medida no worker; gatilho e `spawn_swapoff` iguais; teardown zera a VRAM. ublk device removal = EOF equivalente. | reuso direto do H1. |

## Fronteira de atomicidade e rollback

- Atomicidade: worker único serializa a VRAM (igual H1). io_uring SQ/CQ por request.
- Rollback: **app-only** + `--transport` default `nbd` (fallback Day-0 validado). ublk substitui
  NBD só na paridade (sem dual-path permanente).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (`ramshared-uring`, unsafe) | #11 halo + rust/security | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "io_uring justifica `unsafe` novo vs manter NBD?" | bench (kernel) latência ublk < NBD por ≥X%; `unsafe` só no crate; `// SAFETY:` por bloco | sem ganho de latência medível → manter NBD |
| ITEM-2 (io_uring × afinidade CUDA) | #5 Worst-case | [`KAHNEMAN`](../methodology/KAHNEMAN-DISCIPLINES.md) | "completar CQE de outra thread roda CUDA fora do ctx?" | (kernel) worker é o único a tocar CUDA; lockdep/asan limpos | CUDA chamado fora da thread do ctx → reverter |

## Arquivos a CRIAR / MODIFICAR (quando houver kernel)

### `crates/ramshared-uring/` (CRIAR — kernel-gated)
- FFI io_uring (RAII, `unsafe` isolado, `// SAFETY:` por bloco), espelhando `ramshared-cuda`.
  ADR + `LIBRARIES.md` registram a decisão (#11).

### `crates/ramshared-wsl2d/src/` (MODIFICAR — kernel-gated)
- Modo ublk: loop io_uring → `WMsg`/worker H1; `Reply` carrega o handle do request ublk p/
  completar. `--transport {nbd,ublk}`. Mantém `forbid(unsafe_code)` (o `unsafe` vive no crate novo).

### `crates/ramshared-cli/src/cascade.rs` (MODIFICAR — kernel-gated)
- `up --transport ublk` → daemon em modo ublk; `swapon /dev/ublkbN` no lugar do nbd.

## Plano de testes (quando houver kernel)

- §14 adaptado sobre ublk (spill/DEMOTE/integridade); **bench de latência ublk vs NBD** (a
  justificativa da feature — sem ganho, não adota). `grep unsafe` confina ao crate novo + cuda.
- **Hoje (sem kernel):** N/A — só o desenho/política é auditável.

## Checklist de validação

- [ ] (kernel) `CONFIG_BLK_DEV_UBLK=y`; `/dev/ublkbN` servido; round-trip íntegro.
- [ ] (kernel) latência ublk < NBD (bench, número) — senão **não adota** (mantém NBD).
- [ ] (kernel) `unsafe` só em `ramshared-uring` + `ramshared-cuda`; daemon `forbid(unsafe_code)`.
- [x] Desenho + decisão de política (crate `ramshared-uring`) + riscos (afinidade CUDA) fechados.

## Documentos a atualizar (no IMPL futuro)

`docs/ublk-backend/IMPL.md`; ADR-novo (crate `ramshared-uring`/io_uring); `LIBRARIES.md`
(ublk: NÃO usado → ativo; io_uring FFI); `ARCHITECTURE.md`; `ROADMAP.md` (Fase B).
