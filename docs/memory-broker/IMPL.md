# IMPL — Memory Broker P1 (broker core Linux↔Linux)

Passo 3 do SSDV3 para [`SPECv2.md`](SPECv2.md) (estado `go`). Documenta o que foi implementado,
a rastreabilidade ITEM→commit, as decisões pequenas e a validação. Branch: `feat/fase-b-prep`.

## Escopo entregue

Broker (árbitro) + agente (tenant) + lease revogável de VRAM, com o daemon servindo a VRAM fatiada
em N exports NBD (Unix + TCP) e o árbitro decidindo quem usa cada slice por pressão (PSI). Caminho
**Linux↔Linux mesma-máquina validado end-to-end ao vivo** (drill qemu = PASS). Caminho cross-host
(civm) com código pronto e runbook; execução ao vivo é gate operacional.

## Rastreabilidade ITEM → commit

| ITEM | Commit(s) | Requisitos |
| --- | --- | --- |
| 1 — P0 (gate) | `09fb1ea`, `54fc596` | P0/§10, R1, R4 |
| 2 — ADR-0005 (JSON-lines) | `09fb1ea` | RF-B1 |
| 3 — `protocol.rs` | `49d37fc` | RF-B1 |
| 4 — `slices.rs` + árbitro | `49d37fc` | RF-B3, RF-L1 |
| 5/6/7 — handshake/`SliceView`/streams+TCP | `e3518b4` | RF-L1, RF-L2 |
| 8 — `broker_srv` core | `e3518b4` | RF-B1..B4, RF-P2 |
| 8 — fiação `run_broker` no daemon | `b0bae97` | RF-L1/L2, DT-28 |
| 8 — `--backend ram` (broker) | `4b14070` | habilita ITEM-11 |
| 9 — agente | `cd15ba4` | RF-L3, RNF-1, DT-27 |
| 10 — e2e in-process | `e3518b4` | RNF-4 |
| 11 — drill qemu | `18f5cbf` | RNF-1 (gate) |
| 12 — `--advertise-nbd` + runbook | `02faf6b`, `129c177` | RF-L4, RNF-2, DT-25 |

## Arquivos

**Criados:** `crates/ramshared-broker/` (model/protocol/slices/arbiter); `crates/ramshared-agent/`
(psi/swap/watchdog/main); `crates/ramshared-wsl2d/src/broker_srv.rs`; `crates/ramshared-wsl2d/tests/broker_e2e.rs`;
`scripts/p0/*`; `scripts/kernel/qemu-broker-drill.sh`; `docs/decisions/ADR-0005-broker-protocol-jsonl.md`;
`docs/memory-broker/{P0-RESULTS,CIVM-TENANT}.md`.

**Modificados:** `crates/ramshared-block/src/handshake.rs` (export por nome); `crates/ramshared-wsl2d/src/{backend,conn,lib,main}.rs`
(SliceView, TCP acceptor, ZeroExport, `run_broker`/`run_broker_ram`); `Cargo.toml` (membros).

## Decisões pequenas (sem nova ADR)

- **`residency_check` extraído** (`main.rs`): a lógica de canário §9/§9.4 virou helper compartilhado
  pelo worker single (ação = swapoff local) e pelo broker (ação = `DemoteAll`). Single-mode inalterado.
- **`broker_setup` + `serve_broker_jobs<B>`**: control-plane backend-agnóstico + worker genérico, p/
  o backend RAM (qemu) reusar tudo menos a residência (injetada por closure; RAM = `|_| None`).
- **`--advertise-nbd`** resolve o gap R3-adjacente do endpoint TCP anunciado: o agente civm precisa do
  endereço forwarded do host, não do bind do daemon (DT-25). Padrão = addr de bind.
- **DT-28 no worker do broker**: `recv_timeout` + checagem de `SHUTDOWN`; NÃO encerra por `LiveCount`
  (o broker persiste quando as conexões NBD caem). Ponte do `SHUTDOWN` estático → `Arc` do broker.

## Validação

- **Workspace verde:** `cargo test --workspace` = 0 falhas (~210 testes; broker 41, broker_srv 30,
  handshake 23, agente 25, e2e in-process 3, ublk_control 15, …; ~19 ignored = gated GPU/ublk/root).
- **Clippy:** `cargo clippy -p ramshared-{broker,agent,wsl2d} -D warnings` limpo (lib+bin).
- **fmt:** `cargo fmt` aplicado nos crates tocados.
- **Drill qemu (ITEM-11) = PASS** (rodado aqui, em qemu isolado, backend RAM):
  `KTEST-NBD=ok`, `KTEST-SWAP-ACTIVE=ok` (broker assina slice → agente `nbd-client`+`mkswap`+`swapon`
  → swap ativo via NBD servido pelo broker), `KTEST-SWAPOFF=ok`, `KTEST-DAEMON-TERMINATED=ok`.
  Disciplina 13: o drill pegou 2 bugs reais antes do PASS (loopback DOWN no initramfs → ENETUNREACH;
  contagem `grep -c || echo` duplicada).

## Pendências

- **ITEM-12 ao vivo (civm):** seguir [`CIVM-TENANT.md`](CIVM-TENANT.md) (precisa civm + netsh no host).
  Anexar números (RTT, p50 de page-out) ao P0-RESULTS quando rodado.
- **P0 §4 (render):** input do tester (Alex); vira input do P2.
- **DT-5 rename `ramsharedd`:** ✅ feito (commit `chore(core)`): bin name + prefixos de log + 2
  scripts qemu + doc vivo F12. Pacote/lib/dir seguem `ramshared-wsl2d`. Drill re-rodado = PASS.
- **DT-29 (fronteira servidor-only):** ✅ registrado na SPECv2 + `CIVM-TENANT.md` — o e2e civm tem o
  WSL2 só como servidor; o vetor de D-state cai no consumidor (civm), não no host.
