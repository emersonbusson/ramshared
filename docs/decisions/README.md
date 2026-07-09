# Architecture Decision Records (ADRs)

Registros de decisão numerados e **append-only**. Cada decisão não-trivial de
arquitetura/lock/DMA/memória vira um ADR.

## Formato

`ADR-NNNN-slug.md`, com seções:

- **Status** — Proposed | Accepted | Superseded by ADR-XXXX (com data).
- **Context** — o problema e as forças; fatos com número.
- **Decision** — o que foi decidido (imperativo).
- **Consequences** — trade-offs (+ e −), incl. o que fica pior.
- **Alternatives considered** — o que foi descartado e por quê.
- **Kahneman** — disciplina(s) aplicada(s) ([`../methodology/kahneman-disciplines.md`](../methodology/kahneman-disciplines.md)).
- **Rollback trigger** — condição **numérica/observável** que reverte a decisão
  (governance.md exige isto em mudança estrutural).

## Índice

| ADR | Título | Status |
| --- | --- | --- |
| [0001](ADR-0001-vram-cascade-tiering.md) | Cascata de swap zram→VRAM→VHDX (VRAM como tier frio) | Accepted |
| [0002](ADR-0002-rust-userspace-port.md) | Daemon/CLI userspace em Rust (port do design nbd-vram) | Accepted |
| [0003](ADR-0003-page-state-swap-safety.md) | Estados de página: herdar swap do Linux; daemon garante durabilidade/DEMOTE/atomicidade | Accepted |
