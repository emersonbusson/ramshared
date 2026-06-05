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
| `ramshared-cuda` — wrapper seguro CUDA (FFI dlopen, RAII, Cuda/Context/DeviceMem) | `crates/ramshared-cuda/{Cargo.toml, src/lib.rs, src/ffi.rs, src/driver.rs}` | §4, §8 | RF-1 | fmt ok · `clippy -D warnings` limpo · 2 unit + doctest verdes · **roundtrip GPU real (RTX 2060) verde** (`--ignored`, 256 MiB write/read/OOB) |
| `ramshared-block` — protocolo NBD fixed-newstyle + I/O (BlockBackend, inflight) | `crates/ramshared-block/{Cargo.toml, src/lib.rs, src/protocol.rs, src/request.rs, src/inflight.rs}` | §8, §10.1 | RF-2 (revisado: nbd) | fmt ok · `clippy -D warnings` limpo · **13 testes** (parse/encode, serve/validação, inflight), sem root |

### Decisões pequenas (não pediram ADR nova)

- **Zero dependências externas** (apenas `std` + FFI), espelhando a CLI existente
  e a regra de Day-0 (sem cadeia de deps desnecessária).
- Erros via `enum` + `Display` (sem `thiserror`) para manter zero-dep.
- `lints.clippy.unwrap_used/expect_used = "deny"` no crate (regra `coding.md`: sem
  `.unwrap()/.expect()` em produção).
- `#![forbid(unsafe_code)]` no `ramshared-tier` (lógica pura; `unsafe` só viverá
  vida no `ramshared-cuda`, isolado, conforme §4/§8).
- `ramshared-cuda` carrega `libcuda` via **`dlopen` em runtime** (não link-time)
  → **sem `build.rs`** (o §5 listava `build.rs`; desvio justificado: o WSL2 usa a
  stub `libcuda` do host, sem toolkit). FFI cru isolado em `ffi.rs`; wrappers RAII
  em `driver.rs`. Roundtrip validado em GPU real, não em mock (disciplina #13).
- `ramshared-block` separa **protocolo/I/O** (lib pura, `#![forbid(unsafe_code)]`, 13 testes sem root) da **fiação do device** (`/dev/nbdX` via ioctl, `unsafe`+root) — esta fica para o módulo de integração, testada via `--ignored`/kselftest.

### Pendente (próximos incrementos, na ordem do SPECv3)

1. `check`: adicionar tiers **zram + cgroup** (§6.1).
2. Comandos `up` / `status` / `down` (§6.2–6.4) usando `ramshared-tier`.
3. `ramshared-wsl2d`: máquina de estados (§7, com `Demoted`) + canário de
   residência com **DEMOTE** por latência (§9).
4. `ramshared-integrity` (§8.1) + **device-wiring NBD** (ioctl `NBD_SET_SOCK`/`NBD_DO_IT`, precisa root) + daemon `ramshared-wsl2d` (§7).
   [`ramshared-cuda` ✅, `ramshared-block` ✅ — protocolo+I/O testados sem root]
5. Testes de aceitação §14: cascata sob pressão **confinada em cgroup** (§14.3) e
   **DEMOTE sob latência** (§14.4).

### Fase 0 (evidência que fundou o SPECv3)

`FASE0-FINAL.md`: `go` com pivô. Cascata `zram→VRAM→VHDX` provada; VRAM é
data-safe mas latency-unsafe (canário 4K = 1,18 s sob pressão) → tier frio.
