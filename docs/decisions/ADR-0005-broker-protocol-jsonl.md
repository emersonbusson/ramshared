# ADR-0005 — Protocolo do broker (P1): JSON-lines via `serde`/`serde_json`, não length-prefixed

**Status:** Accepted (2026-06-13). Exceção userspace à política zero-dep **só para
`serde`/`serde_json`** e **só no control-plane do broker** (crate `ramshared-broker`, Fase P1);
ver Consequences. Roteado pelo [`docs/specs/no-milestone/memory-broker/SPEC.md`](../specs/no-milestone/memory-broker/SPEC.md) DT-1.

## Context

O Memory Broker (P1) precisa de um protocolo agente↔broker (RF-B1): `Register`, `Psi`,
`SwapOn/Off`, `LeaseRequest/Release`, `DemoteAll`, `Status` e respostas. É um **control-plane**:
baixa taxa (~**1 msg/s/tenant** — o `Psi` a 1 Hz), poucos tenants. O **data-plane** (a cópia de
páginas de swap) é o **NBD**, não este protocolo.

Forças (fatos):
- **Política atual:** zero dependências externas no caminho NBD (Cargo.lock confirma); a única
  exceção é `io-uring` (Fase B/ublk, gated — [ADR-0004](ADR-0004-ublk-io-uring-crate.md)). O único
  `unsafe` do projeto vive em `ramshared-cuda`/`ramshared-uring`.
- **Precedente `io-uring` (ADR-0004):** exceção userspace aceita quando o custo de evitar é alto e
  o risco de hand-roll é catastrófico. **Precedente `clap` (issue #3):** rejeitado porque era
  **trivialmente evitável** (`std::env::args` basta p/ ~4-9 flags).
- **Onde este caso cai:** serialização/validação de ~15 variantes de mensagem com evolução por
  campo é **não-trivial de hand-rollar com segurança** (parsing manual frágil, sem checagem de
  shape → bug de protocolo num caminho que comanda `swapon`/`swapoff`). Mas **não** exige barreiras
  de memória como o io_uring — o risco é de *correção de parsing*, não de concorrência.
- **JSON-lines é operacionalmente superior aqui:** 1 objeto JSON por linha (`\n`, UTF-8) é
  **debugável com `nc`/`jq`** num control-plane raro; evolução por campo opcional (forward-compat).
  Length-prefixed binário só ganharia em throughput de data-plane — que aqui é o NBD.

## Decision

Protocolo do broker = **JSON-lines** (um objeto JSON por linha, `\n`, UTF-8), serializado por
**`serde` + `serde_json`**. Encapsulado **só no crate `ramshared-broker`** (o `ramshared-agent`
herda transitivamente); o daemon e a lib seguem `#![forbid(unsafe_code)]` (o `derive` do serde é
código seguro). Sem `tokio` — threads `std`, padrão do workspace.

Versões (registry, 2026-06-13): **`serde 1.0.228`** (MIT OR Apache-2.0, `rust-version` 1.56,
repo `serde-rs/serde`) com feature `derive`; **`serde_json 1.0.150`** (MIT OR Apache-2.0,
`rust-version` 1.71, repo `serde-rs/json`). O pin exato + transitivas (`serde_derive`, `proc-macro2`,
`quote`, `syn`, `itoa`, `ryu`, `memchr`) entram no `Cargo.lock` no ITEM-3 (revisar o diff do lock).

**Anti-DoS:** o codec impõe teto de linha `MAX_LINE_BYTES = 64 KiB` **antes** de alocar (espelha
`MAX_OPT_LEN` do handshake NBD); shape inválido → `Err` (serde rejeita), nunca estado corrompido.

**Rollback trigger (#2):** se o protocolo precisar transportar **payload de dados (>64 KiB/msg)**
ou exceder **>100 msg/s/tenant**, migrar para **length-prefixed (ex.: `bincode`)** via ADR
superseding — JSON deixa de compensar quando vira data-plane. A exceção fica em `LIBRARIES.md`.

## Consequences

**+** Mensagens debugáveis (`nc`/`jq`), evolução por campo opcional, validação de shape **de graça**
(serde rejeita JSON malformado/desconhecido — RF-B1 input validation) em vez de parser manual.
**+** `unsafe`-free: o `derive` é seguro; o `#![forbid(unsafe_code)]` dos crates novos é preservado.
**+** Broker é userspace → o LKM Ring-0 futuro **não herda** a dep (zero-dep do destino preservado).
**−** Quebra o zero-dep **userspace** no caminho broker (serde + serde_json + transitivas de
proc-macro) — **exceção explícita**, restrita ao control-plane do broker.
**−** Supply chain: mitigado por pin no lockfile, revisão do diff de `Cargo.lock`, `cargo audit`/
`cargo deny` quando disponíveis. (serde é uma das libs mais usadas do ecossistema — risco baixo.)

## Alternatives considered

- **Length-prefixed + `bincode`** — rejeitado p/ P1: binário não-debugável num control-plane raro;
  o ganho de throughput só importa no data-plane, que aqui é o NBD. É o **alvo do rollback trigger**
  se o perfil de uso mudar.
- **Parser hand-rolled zero-dep** (formato simples próprio) — rejeitado: reimplementar
  serialização + validação de shape robusta p/ ~15 variantes evolutivas é frágil e mais arriscado
  que a lib madura; **diferente do `clap`** (que era trivialmente evitável). O risco aqui é
  correção de parsing num caminho que comanda swap.
- **`serde` com formato binário (`postcard`/`bincode`) em vez de JSON** — perde a debugabilidade
  `nc`/`jq` sem ganho relevante no control-plane; reconsiderar junto do rollback trigger.

## Kahneman

- **#11 (halo effect):** dep nova com critério mensurável (taxa/tamanho de msg), alternativas e
  quando-revisitar (rollback trigger numérico) — atendido; entrada em `LIBRARIES.md` no mesmo commit.
- **#2 (counterfactual):** rollback trigger explícito (>64 KiB/msg ou >100 msg/s/tenant → bincode).
- **#5 / #13 (worst-case / ilusão de validade):** shape inválido **falha** (serde `Err`), não
  corrompe estado; teto de linha de 64 KiB antes de alocar (anti-DoS) é testado no ITEM-3.
