# IMPL — Issue #8 — Canário de residência dedicado (§9.4)

SPEC ativo: [`SPECv3.md`](SPECv3.md) (Passo 2.5 = `go` sobre o SPECv2; histerese).
Implementa **estritamente** o SPECv3. Zero criatividade fora do escopo (SSDV3 #3).

## Escopo entregue

Canário dedicado §9.4: região-canário separada (não endereçável por NBD) +
`ResidencySampler` com **histerese** (streak). Corrupção de conteúdo demove
imediato; free-floor e erros transientes exigem `consecutive` amostras. A
detecção por **latência por-request** (§9, gatilho primário) fica **intacta**.

## Arquivos

| Ação | Arquivo | Conteúdo | SPEC |
|---|---|---|---|
| CRIAR | `crates/ramshared-wsl2d/src/canary_probe.rs` | `Cadence` (pura) + `CanaryProbe::check_content` (round-trip + sentinela) + `zero()` (teardown) + consts | DT-1, DT-2, DT-4, DT-12 |
| MOD | `crates/ramshared-wsl2d/Cargo.toml` | + dep `ramshared-integrity` | DT-4 |
| MOD | `crates/ramshared-wsl2d/src/lib.rs` | `pub mod canary_probe;` + re-exports (`Cadence`, `CanaryProbe`, consts, `ResidencySampler`) | — |
| MOD | `crates/ramshared-wsl2d/src/residency.rs` | `free_floor_bytes` 0→64 MiB; `ResidencySampler` (streak) + 4 testes | DT-3, DT-9, DT-11 |
| MOD | `crates/ramshared-wsl2d/src/main.rs` | aloca `canary_region`; `probe`/`cadence`/`sampler`; `spawn_swapoff` (unificado); bloco de cadência; `probe.zero()` no teardown; corrige 2 comentários "§9.4 = futuro" | DT-8, DT-9, DT-10, DT-11, DT-12, M4 |

## Rastreabilidade RF → entrega

- **RF-1** (região-canário): `ctx.alloc(CANARY_BYTES)` em `main.rs`; `CanaryProbe`. DT-1.
- **RF-2** (corrupção): `ResidencySampler::sample(content=Some(false), _)` ⇒ `Demote(Corruption)` **imediato**. DT-9.
- **RF-3** (latência por-request): **inalterado** — o bloco de latência segue por-request (não regride §14).
- **RF-4** (free-floor): `free < floor` ou amostra degradada ⇒ `bad_streak`; `>= consecutive` ⇒ `Demote(FreeFloor)`. DT-9/DT-10.
- **RF-5** (sentinela/cadência): `Cadence(64)`; `fill_block`/`verify_block`, `idx = seq`. DT-2/DT-4.
- **RF-6** (DEMOTE unificado): `spawn_swapoff` único para latência **e** sonda. DT-8.

## Disciplina Kahneman (itens críticos)

- **ITEM-5 — corrupção imediata (#13 ilusão de validade).** Pergunta: "e se a VRAM
  devolver dado corrompido apesar de data-safe?" Evidência: `corruption_is_immediate`
  (`Some(false)` → `Demote(Corruption)`, `bad_streak` não usado). Round-trip real em
  GPU = teste `--ignored` (rig). É guarda; não dispara em operação sã.
- **ITEM-6 — free-floor/erro transiente (#5 worst-case + #2 counterfactual).**
  Pergunta: "1 leitura de free baixa / 1 erro CUDA é evicção iminente ou ruído?"
  Evidência: `free_floor_needs_consecutive`, `transient_error_needs_consecutive`,
  `good_sample_resets_streak` (1 amostra ruim → `Ok`; `consecutive` → `Demote`; boa
  zera). **Rollback trigger:** DEMOTE por free-floor em operação normal (sem `vramhog`)
  → subir `free_floor_bytes`/`consecutive` e reverter os commits (app-only).

## Decisões pequenas (não pediram nova ADR / SPEC)

- `probe.check_content().ok()` e `ctx.mem_info().ok().map(|(f,_)| f as u64)` no lugar do
  `match { Ok => Some, Err => None }` do SPEC: idêntico em semântica (DT-11 preservada),
  mas `clippy::manual_ok_err` falha sob `-D warnings`. A forma `.ok()` é a idiomática.
- `CanaryProbe::zero()` (delega a `region.zero()`): a região fica **encapsulada** no
  probe, então o teardown DT-12 usa `probe.zero()` — opção explicitamente prevista no SPEC.
- Testes do `ResidencySampler` em `mod sampler_tests` separado: evita colisão de nome com
  `Canary::good_sample_resets_streak` (mesmo nome, módulos distintos).
- Corrigidos 2 comentários em `main.rs` que descreviam o §9.4 como "trabalho futuro"
  (agora falsos): Day-0 não deixa texto enganoso adjacente à mudança.

## Validação

- `cargo fmt --all -- --check` — limpo.
- `cargo clippy --workspace --all-targets -- -D warnings` — limpo.
- `cargo test --workspace` — verde. `ramshared-wsl2d` lib: **15 passou** (2 cadência + 4
  `ResidencySampler` novos + 5 `Canary` intactos + estado), **1 ignorado** (round-trip GPU);
  workspace ~54 testes, 2 ignorados (GPU). Sem regressão nos 5 testes de `Canary`.
- **Pendente no rig (GPU + root):** `cascade-validate.sh` / `cascade-demote.sh`
  (`/home/emdev/fase0/`) — confirmar **sem regressão §14** (spill + DEMOTE ao vivo).

## Atomicidade & rollback

Cada ciclo de sonda é independente; o estado novo é `bad_streak` (in-memory, no daemon).
DEMOTE (`swapoff`) atômico no kernel (caminho existente). **Rollback: app-only** (revert
dos commits); sem migration/dados.

## uAPI/ABI

Sem mudança: `serve()`/protocolo NBD inalterados; o device anunciado segue = região de swap;
a região-canário é separada e não endereçável.
