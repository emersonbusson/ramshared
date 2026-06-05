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
| `ramshared-wsl2d` (lib) — máquina de estados (§7) + `VramBackend` (CUDA→NBD) | `crates/ramshared-wsl2d/{Cargo.toml, src/lib.rs, src/state.rs, src/backend.rs, src/main.rs}` | §7, §8 | — | fmt ok · `clippy -D warnings` limpo · 4 unit + **composição GPU real verde** (`--ignored`: WRITE/READ NBD na VRAM) |
| `ramshared-integrity` — checksum por bloco (FNV-1a) + padrões + tabela pré-alocada | `crates/ramshared-integrity/{Cargo.toml, src/lib.rs, src/hash.rs, src/pattern.rs}` | §8.1, §14.2 | — | fmt ok · `clippy -D warnings` limpo · **7 testes** (hash/tabela/padrões), sem root |

> **Marco:** os 6 crates do §5 existem; CUDA↔NBD validado em GPU, o daemon serve
> `/dev/nbd0` real (write/readback 1 MiB OK) **e a cascata `up`/`down` sobe/desce
> como swap real** (`zram 200 > VRAM 100 > VHDX -2`). Falta: canário/DEMOTE (§9),
> `check`+zram, sequência `start` (mlockall/oom_score_adj).

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
- `VramBackend` (`ramshared-wsl2d`) é o ponto que liga CUDA↔NBD; composição validada em GPU real (WRITE/READ NBD round-trip na VRAM) — `cuda` + `block` formam o device de ponta a ponta (falta só a fiação do kernel).

### Concluído (Passo 3 — pipeline da cascata fim-a-fim)

1. ✅ **`check`+zram** (§6.1): probe de `CONFIG_ZRAM` + linha "Tiers (cascata)"
   (texto e json) reportando `zram / vram=nbd / vhdx`.
2. ✅ **`up`/`down`/`status`** (2026-06-05): monta `zram(200)>VRAM(100)>VHDX(-2)`
   (swapon confirmado) e desmonta limpo (anti-panic; `swapoff` antes do disconnect).
3. ✅ **Daemon `ramshared-wsl2d`**: `mlockall`+`oom_score_adj=-1000` (anti-deadlock,
   Disciplina 3) + **wiring do canário §9 inline** (mede a latência do serve, arma
   o `Canary` pós-baseline e dispara `swapoff <nbd>` numa thread no DEMOTE, mantendo
   o serve loop p/ o read-back). Estados §7 ✅, `VramBackend` ✅, decisão §9.3 ✅
   (5 testes; o spike 1,18 s da Fase 0 dispara `Demote(Latency)`).
4. ✅ **device-wiring NBD** (smoke `/home/emdev/fase0/wiring-smoke.sh`): daemon serve
   `/dev/nbd0` real via `nbd-client -unix` (handshake interop OK, **sem ioctl/`unsafe`
   no daemon**); write/readback 1 MiB → VRAM round-trip OK.
5. ✅ **Aceitação §14** — ver [`VALIDATION-CASCADE.md`](VALIDATION-CASCADE.md):
   - §14.3 spill confinado em cgroup: **511 MiB** spillaram p/ VRAM, **332.800
     páginas íntegras**, 0 falso-positivo do canário.
   - §14.4 DEMOTE: **481 MiB vivos** migraram da VRAM p/ VHDX via `swapoff` em 6 s,
     **384.000 páginas íntegras, 0 corrupção**, daemon serviu o read-back.

### Refinamentos futuros (não bloqueiam Day-0)

- Canário §9.4 **dedicado** (região-canário + checagem de conteúdo/free-floor): hoje
  o wiring inline cobre o gatilho de latência (o dominante na Fase 0); conteúdo e
  free-floor têm a lógica testada (`residency.rs`) mas exigem o sampler dedicado.
- Multi-conexão no daemon (hoje serve 1 conexão = exatamente a vida do swap).

### Fase 0 (evidência que fundou o SPECv3)

`FASE0-FINAL.md`: `go` com pivô. Cascata `zram→VRAM→VHDX` provada; VRAM é
data-safe mas latency-unsafe (canário 4K = 1,18 s sob pressão) → tier frio.
