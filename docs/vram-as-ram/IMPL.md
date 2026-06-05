---
slug: vram-wsl2-cuda-swap
title: IMPL — VRAM como tier frio (cascata) no WSL2
spec: SPECv3-WSL2.md
step: SSDV3 PASSO 3
status: scaffold-em-andamento
---

# IMPL — vram-as-ram (WSL2)

Implementa **estritamente** o `SPECv3-WSL2.md`. Zero criatividade fora do escopo
(regra dura SSDV3 #3): decisão nova → atualiza o SPEC, depois implementa.

## Estado: scaffold iniciado (2026-06-05)

### Feito

| Componente | Arquivos | SPEC | RF | Validação |
|---|---|---|---|---|
| `ramshared-tier` — cascata: prioridades + invariante A1 | `crates/ramshared-tier/{Cargo.toml, src/lib.rs, src/priority.rs, src/cascade.rs}` | §1, §6.2, §9.2 | revisa **RF-3** | `cargo fmt --check` ok · `clippy -D warnings` limpo · **8 testes** verdes · `cargo check --workspace` ok |
| (pré-existente) `ramshared check`/`doctor` | `crates/ramshared-cli/src/main.rs` | §6.1 | RF-1 (parcial) | testes verdes |

### Decisões pequenas (não pediram ADR nova)

- **Zero dependências externas** (apenas `std` + FFI), espelhando a CLI existente
  e a regra de Day-0 (sem cadeia de deps desnecessária).
- Erros via `enum` + `Display` (sem `thiserror`) para manter zero-dep.
- `lints.clippy.unwrap_used/expect_used = "deny"` no crate (regra `coding.md`: sem
  `.unwrap()/.expect()` em produção).
- `#![forbid(unsafe_code)]` no `ramshared-tier` (lógica pura; `unsafe` só viverá
  vida no `ramshared-cuda`, isolado, conforme §4/§8).

### Pendente (próximos incrementos, na ordem do SPECv3)

1. `check`: adicionar tiers **zram + cgroup** (§6.1).
2. Comandos `up` / `status` / `down` (§6.2–6.4) usando `ramshared-tier`.
3. `ramshared-wsl2d`: máquina de estados (§7, com `Demoted`) + canário de
   residência com **DEMOTE** por latência (§9).
4. `ramshared-cuda` / `ramshared-block` / `ramshared-integrity` (§5, §8).
5. Testes de aceitação §14: cascata sob pressão **confinada em cgroup** (§14.3) e
   **DEMOTE sob latência** (§14.4).

### Fase 0 (evidência que fundou o SPECv3)

`FASE0-FINAL.md`: `go` com pivô. Cascata `zram→VRAM→VHDX` provada; VRAM é
data-safe mas latency-unsafe (canário 4K = 1,18 s sob pressão) → tier frio.
