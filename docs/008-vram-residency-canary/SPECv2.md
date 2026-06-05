# SPECv2 — Issue #8 — Canário de residência dedicado (§9.4)

> Versão melhorada após auditoria do Passo 2.5.
> Baseline preservado: [`SPEC.md`](SPEC.md).
> Motivo: o `SPEC.md` substituía a latência por-request (validada no §14) por uma sonda
> a cada `CANARY_EVERY`, regredindo a granularidade de detecção e atrasando o baseline
> 64×. O SPECv2 adota um desenho **híbrido**: latência por-request **inalterada** +
> sonda em cadência só para **conteúdo + free-floor** (imediatos).

## 0. Proveniência da auditoria (regra de saída do Passo 2.5)

- **Auditado:** `docs/008-vram-residency-canary/SPEC.md`.
- **Resultado:** `no-go`.
- **Findings bloqueantes endereçados:**
  - B1 — DT-6 substituía a latência por-request → **revertido**: latência por-request
    permanece como está hoje (validada §14); a sonda **não** alimenta o streak.
  - B2 — baseline 64× mais longo deixava conteúdo/free sem checagem no startup → os
    gatilhos de conteúdo/free agora são **imediatos** e ativos desde o 1º ciclo de
    cadência (não dependem do baseline de latência).
  - H1(audit) — erro de sonda mapeado a `Corruption` → resolvido por **mapeamento
    conservador via sentinelas** (erro de `mem_info` ⇒ `free=0`; erro de `run` ⇒
    `content_ok=false`), reusando os motivos existentes sem inventar variante.
- **Este `SPECv2.md` é o candidato ativo** para nova auditoria / Passo 3.

## Escopo fechado desta implementação

**Entra agora:**
- Latência por-request: **inalterada** (canário de latência atual permanece).
- Região-canário dedicada + sonda em cadência para **conteúdo** (sentinela write/read) e
  **free-floor** (`cuMemGetInfo`), com DEMOTE imediato em falha.
- `ResidencyConfig.free_floor_bytes` default > 0; checagem conteúdo/free extraída para
  reuso.

**Fica fora:** detecção em idle (thread amostradora / `cuCtxSetCurrent` — H1);
multi-conexão; writeback; alimentar a sonda no streak de latência.

**Dependências prontas:** `ramshared-cuda`, `residency.rs`, `ramshared-integrity`.

## Matriz de rastreabilidade PRD → SPECv2

| PRD  | Implementação no SPECv2 |
| ---- | ----------------------- |
| RF-1 | ITEM-1, DT-1 |
| RF-2 | ITEM-2, ITEM-5, DT-4, DT-7 |
| RF-3 | **inalterado** (latência por-request já existente; DT-6 revertido) |
| RF-4 | ITEM-4, ITEM-5, ITEM-6, DT-3, DT-7 |
| RF-5 | ITEM-3, DT-2 |
| RF-6 | ITEM-4, ITEM-6 (reusa o caminho de DEMOTE; helper compartilhado DT-8) |

## Decisões técnicas

| #    | Decisão | Justificativa |
| ---- | ------- | ------------- |
| DT-1 | `CANARY_BYTES = 4096` | round-trip mínimo de 1 página (alinhado ao `BLOCK_SIZE`). |
| DT-2 | `CANARY_EVERY = 64` requests | cadência da sonda **de conteúdo/free** (não da latência). |
| DT-3 | `free_floor_bytes` default = `64 * 1024 * 1024` | "GPU criticamente cheia"; conservador e tunável. |
| DT-4 | Sentinela = `ramshared_integrity::fill_block`/`verify_block`, `Pattern::Random`, `idx = seq` | reuso + pega leitura stale (idx por ciclo). |
| DT-5 | **Erro CUDA conservador via sentinela:** `mem_info` `Err` ⇒ `free = 0`; `probe.run` `Err` ⇒ `content_ok = false` | dispara `Demote(FreeFloor)`/`Demote(Corruption)` pelos motivos existentes; disciplina #5 (worst-case) sem nova variante de enum. |
| DT-6 | **REVERTIDA** (era: latência da sonda substitui a do `serve()`). A latência por-request permanece intacta | corrige o blocker B1 (não regredir a detecção validada §14). |
| DT-7 | Extrair `ResidencyConfig::check_residency(&self, content_ok, free_bytes) -> Verdict` (só conteúdo+free, imediato); `Canary::sample` delega a checagem conteúdo/free a ele | a sonda checa conteúdo/free **sem** tocar o streak de latência; ativa os ramos hoje mortos sem duplicar lógica. |
| DT-8 | Extrair `fn spawn_swapoff(dev: &str) -> mpsc::Receiver<bool>` | um único caminho de DEMOTE (swapoff confirmado por canal) usado pela latência e pela sonda. |

## Fronteira de atomicidade e política de rollback

- **Atomicidade:** cada ciclo de sonda é independente; o DEMOTE (`swapoff`) é atômico no
  kernel (caminho existente, confirmação por canal + re-arm). Sem multi-write novo.
- **Rollback:** **app-only** (revert dos commits). Sem migration/dados; sem
  `forward-only`.

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-6 (DEMOTE por free-floor / erro) | #5 Worst-case + #2 Counterfactual/rollback | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "free baixo / erro de sonda é eviction real ou ruído do GPU-PV?" | unit: `check_residency(true, free<floor)` → `Demote(FreeFloor)`; ao vivo: `vramhog` consome a VRAM livre → `Demote(FreeFloor)` no log | DEMOTE por free-floor em operação normal (sem `vramhog`) → subir `free_floor`/tunar e reverter |
| ITEM-5 (gatilho de conteúdo) | #13 Ilusão de validade | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "e se a VRAM devolver dado stale/corrompido apesar de data-safe?" | unit: `check_residency(false, _)` → `Demote(Corruption)`; round-trip real da sonda em teste `--ignored` (GPU) | N/A (guarda; não deve disparar em operação sã) |

## Checklist de segurança (pré-implementação)

- [x] Isolamento: região-canário é `DeviceMem` separado, **não endereçável** por NBD.
- [x] OOB: `write_at`/`read_at` já fazem bounds-check; `wbuf`/`rbuf` têm `CANARY_BYTES`.
- [x] Permissões: daemon já roda como root (subido pelo `up`).
- [x] Input validation: sonda sem input externo (sentinela sintética).
- [x] Sem `unsafe` novo; `forbid(unsafe_code)` mantido na lib.

## Arquivos a CRIAR

### `crates/ramshared-wsl2d/src/canary_probe.rs`

- **Propósito:** cadência pura + sonda de conteúdo (round-trip) da região-canário.
- **Requisitos:** RF-2, RF-5, DT-1, DT-2, DT-4.
- **Types/funcs:**
  ```rust
  use ramshared_cuda::{CudaError, DeviceMem};
  use ramshared_integrity::{Pattern, fill_block, verify_block};

  pub const CANARY_BYTES: usize = 4096; // DT-1
  pub const CANARY_EVERY: u32 = 64;     // DT-2

  pub struct Cadence { every: u32, counter: u32 }
  impl Cadence {
      pub fn new(every: u32) -> Self;       // { every, counter: 0 }
      pub fn tick(&mut self) -> bool;       // counter+=1; if counter>=every {counter=0; true} else {false}
  }

  pub struct CanaryProbe<'c, 'a> { region: DeviceMem<'c, 'a>, wbuf: Vec<u8>, rbuf: Vec<u8>, seq: u64 }
  impl<'c, 'a> CanaryProbe<'c, 'a> {
      pub fn new(region: DeviceMem<'c, 'a>) -> Self;     // bufs = vec![0u8; CANARY_BYTES], seq: 0
      /// fill(seq) -> write_at(0) -> read_at(0) -> verify(seq). Retorna content_ok.
      pub fn check_content(&mut self) -> Result<bool, CudaError>;
  }
  ```
  `check_content`: `self.seq += 1`; `fill_block(&mut self.wbuf, self.seq, Pattern::Random)`;
  `self.region.write_at(0, &self.wbuf)?`; `self.region.read_at(0, &mut self.rbuf)?`;
  `Ok(verify_block(&self.rbuf, self.seq, Pattern::Random))`. (Latência da sonda **não** é
  exportada — DT-6 revertida.)
- **Testes (sem GPU):** `cadence_fires_every_n`, `cadence_resets`.

## Arquivos a MODIFICAR

### `crates/ramshared-wsl2d/Cargo.toml`
- **Muda:** + `ramshared-integrity = { path = "../ramshared-integrity" }` (DT-4). Sem dep externa.

### `crates/ramshared-wsl2d/src/lib.rs`
- **Muda:** `pub mod canary_probe;` + `pub use canary_probe::{Cadence, CanaryProbe, CANARY_BYTES, CANARY_EVERY};`.

### `crates/ramshared-wsl2d/src/residency.rs`
- **Muda (DT-3):** `free_floor_bytes` default `0` → `64 * 1024 * 1024` (atualizar comentário).
- **Muda (DT-7):** extrair `check_residency` e delegar de `sample`:
  - **Antes:** `sample` faz inline `if !content_ok {..} if free_bytes < cfg.free_floor_bytes {..}`.
  - **Depois:**
    ```rust
    impl ResidencyConfig {
        /// Gatilhos imediatos (sem streak de latência). §9.3 (b)(c).
        pub fn check_residency(&self, content_ok: bool, free_bytes: u64) -> Verdict {
            if !content_ok { return Verdict::Demote(DemoteReason::Corruption); }
            if free_bytes < self.free_floor_bytes { return Verdict::Demote(DemoteReason::FreeFloor); }
            Verdict::Ok
        }
    }
    // em Canary::sample, no topo:
    if let Verdict::Demote(r) = self.cfg.check_residency(content_ok, free_bytes) {
        return Verdict::Demote(r);
    }
    ```
- **Impacto:** os 5 testes existentes continuam verdes (comportamento de `sample`
  idêntico). **Testes novos:** `check_residency_corruption`, `check_residency_free_floor`.

### `crates/ramshared-wsl2d/src/main.rs`
- **Muda:** (a) alocar `canary_region` + `CanaryProbe` + `Cadence` + `let res_cfg = ResidencyConfig::default();`; (b) extrair `spawn_swapoff` (DT-8); (c) manter o canário de **latência por-request inalterado**, trocando só o spawn inline pelo helper; (d) adicionar, após o bloco de latência, o bloco de cadência conteúdo/free.
- **Requisitos:** RF-1, RF-4, RF-6, DT-5, DT-8.
- **Depois (bloco de cadência, novo):**
  ```rust
  if touches_vram && !demoted && demote_rx.is_none() && cadence.tick() {
      let free = match ctx.mem_info() { Ok((f, _)) => f as u64, Err(_) => 0 }; // DT-5
      let content_ok = match probe.check_content() { Ok(ok) => ok, Err(_) => false }; // DT-5
      if let Verdict::Demote(reason) = res_cfg.check_residency(content_ok, free) {
          eprintln!("[wsl2d] DEMOTE ({reason:?}) free={free} content_ok={content_ok} -> swapoff {nbd_dev}");
          demote_rx = Some(spawn_swapoff(&nbd_dev));
      }
  }
  ```
  O bloco de latência por-request permanece como hoje, apenas usando `spawn_swapoff(&nbd_dev)`
  no lugar do `thread::spawn` inline.
- **`spawn_swapoff` (DT-8):**
  ```rust
  fn spawn_swapoff(dev: &str) -> std::sync::mpsc::Receiver<bool> {
      let (tx, rx) = std::sync::mpsc::channel();
      let dev = dev.to_string();
      std::thread::spawn(move || {
          let ok = std::process::Command::new("swapoff").arg(&dev)
              .status().map(|s| s.success()).unwrap_or(false);
          let _ = tx.send(ok);
      });
      rx
  }
  ```
- **Impacto:** sem uAPI/ABI; `serve()`/NBD inalterados; latência por-request validada
  **preservada**; conteúdo/free agora ativos a cada `CANARY_EVERY`.
- **Testes:** `cargo test -p ramshared-wsl2d` (cadência + `check_residency` + os 5
  existentes); `clippy --workspace -D warnings`; re-rodar `cascade-validate.sh`/
  `cascade-demote.sh` (sem regressão §14).
  - **Disciplina Kahneman:** ITEM-6 (#5/#2) — ver Mapa.

## Documentos a atualizar no commit do IMPL

- `docs/008-vram-residency-canary/IMPL.md` (Passo 3).
- `docs/vram-as-ram/SPECv3-WSL2.md` §9.4 → implementado; `ARCHITECTURE.md` (C1 resolvido);
  `docs/vram-as-ram/IMPL.md`; `MEMORY.md`.
