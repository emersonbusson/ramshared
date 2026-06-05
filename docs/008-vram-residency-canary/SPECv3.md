# SPECv3 — Issue #8 — Canário de residência dedicado (§9.4)

> Versão melhorada após auditoria do Passo 2.5.
> Baselines preservados: [`SPEC.md`](SPEC.md), [`SPECv2.md`](SPECv2.md).
> Motivo: o `SPECv2.md` demovia por **free-floor / erro transiente no single-sample**
> (sem histerese), arriscando falso-positivo caro; e não declarava a semântica do
> free-floor no GPU-PV. O SPECv3 adiciona **streak (`consecutive`)** para free-floor e
> erros transientes via um `ResidencySampler` puro; mantém **corrupção de conteúdo
> imediata** e a **latência por-request** intacta (herdada do SPECv2).

## 0. Proveniência da auditoria (regra de saída do Passo 2.5)

- **Auditado:** `docs/008-vram-residency-canary/SPECv2.md`.
- **Resultado:** `no-go`.
- **Findings bloqueantes endereçados:**
  - B1 — free-floor demovia no single-sample → **histerese**: free-floor e erros
    transientes exigem `consecutive` amostras consecutivas (DT-9).
  - B2 — erro transiente (`check_content`/`mem_info` `Err`) demovia imediato → passa a
    **contar para o streak**, não demove sozinho (DT-11).
  - H3 — semântica do free-floor no GPU-PV não declarada → documentada como **indicador
    antecedente de pressão GPU-wide** (DT-10).
- **Não-bloqueantes corrigidos:** M4 (log distingue gatilho real de erro), M5 (zerar a
  região-canário no teardown, DT-12).
- **Este `SPECv3.md` é o candidato ativo** para nova auditoria / Passo 3.

## Escopo fechado desta implementação

**Entra agora:**
- Latência por-request: **inalterada** (herdado do SPECv2 — não regredir o §14).
- Região-canário dedicada + sonda em cadência de **conteúdo** (imediato em corrupção
  confirmada) e **free-floor** (com streak).
- `ResidencySampler` puro (streak de free-floor + erros); `free_floor_bytes` default > 0.

**Fica fora:** detecção em idle (H1); multi-conexão; writeback; alimentar a sonda no
streak de latência.

**Dependências prontas:** `ramshared-cuda`, `residency.rs`, `ramshared-integrity`.

## Matriz de rastreabilidade PRD → SPECv3

| PRD  | Implementação no SPECv3 |
| ---- | ----------------------- |
| RF-1 | ITEM-1, DT-1 |
| RF-2 | ITEM-2, ITEM-5, DT-4, DT-9 (corrupção imediata) |
| RF-3 | **inalterado** (latência por-request; DT-6 segue revertida) |
| RF-4 | ITEM-4, ITEM-6, DT-3, DT-9, DT-10 (free-floor com streak) |
| RF-5 | ITEM-3, DT-2 |
| RF-6 | ITEM-6 (DEMOTE unificado via `spawn_swapoff`, DT-8) |

## Decisões técnicas (delta sobre o SPECv2)

| #    | Decisão | Justificativa |
| ---- | ------- | ------------- |
| DT-1..DT-2, DT-4, DT-6, DT-8 | **Herdadas do SPECv2** (CANARY_BYTES=4096; CANARY_EVERY=64; sentinela `ramshared-integrity`; latência por-request intacta; `spawn_swapoff` unificado) | — |
| DT-3 | `free_floor_bytes` default = `64 * 1024 * 1024` (mantido) | conservador; com o streak (DT-9) o risco de falso-positivo cai. |
| DT-5 | **REVISADA:** erro transiente não demove sozinho (ver DT-11). | corrige B2. |
| DT-7 | **SUPERSEDED:** `check_residency` (imediato) é substituído pelo `ResidencySampler` (com streak). | corrige B1. |
| DT-9 | **`ResidencySampler` puro:** corrupção confirmada (`content = Some(false)`) ⇒ `Demote(Corruption)` **imediato**; free abaixo do floor **OU** amostra degradada ⇒ incrementa `bad_streak`; `bad_streak >= consecutive` ⇒ `Demote(FreeFloor)`; amostra boa zera o streak. | histerese só onde o sinal é fraco/transiente; corrupção (raro, inequívoco) continua imediata. |
| DT-10 | **Semântica do free-floor:** no GPU-PV a evicção da nossa região tende a *aumentar* o free; o free-floor detecta **pressão GPU-wide** (indicador antecedente de evicção iminente), não a evicção direta. É um sinal secundário; a latência por-request é o primário. | corrige H3; evita falsa expectativa. |
| DT-11 | **Amostra degradada:** `content = None` (erro de sonda) ou `free = None` (erro de `mem_info`) conta como degradada no streak (DT-9), **não** demove sozinha. | corrige B2 (transiente não derruba o tier). |
| DT-12 | Zerar a região-canário no teardown (junto do `backend.zero()`). | consistência §11 (zerar toda a VRAM nossa). |
| M4 | O log de DEMOTE distingue gatilho real (`free=<n>`/`corruption`) de erro (`probe-err`/`meminfo-err`). | observabilidade. |

## Fronteira de atomicidade e política de rollback

- **Atomicidade:** cada ciclo de sonda é independente; o estado novo é o `bad_streak`
  (in-memory, no daemon). DEMOTE (`swapoff`) atômico no kernel (caminho existente). Sem
  multi-write novo.
- **Rollback:** **app-only** (revert dos commits). Sem migration/dados; sem `forward-only`.

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-6 (free-floor com streak) | #5 Worst-case + #2 Counterfactual/rollback | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "1 leitura de free baixa é evicção iminente ou ruído de outro app?" | unit: 1 amostra baixa → `Ok`; `consecutive` baixas → `Demote(FreeFloor)`; amostra boa zera | DEMOTE por free-floor em operação normal → subir `free_floor`/`consecutive` e reverter |
| ITEM-5 (corrupção imediata) | #13 Ilusão de validade | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "e se a VRAM devolver dado corrompido apesar de data-safe?" | unit: `content = Some(false)` → `Demote(Corruption)` imediato; round-trip real `--ignored` (GPU) | N/A (guarda) |
| ITEM-6 (erro transiente) | #5 Worst-case | [`KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) | "um erro CUDA isolado é perda de residência?" | unit: `content=None`/`free=None` 1× → `Ok`; `consecutive` → `Demote(FreeFloor)` | erros transientes frequentes sem evicção → investigar driver antes de baixar `consecutive` |

## Checklist de segurança (pré-implementação)

- [x] Isolamento: região-canário separada, **não endereçável** por NBD.
- [x] OOB: `write_at`/`read_at` bounds-check; `wbuf`/`rbuf` = `CANARY_BYTES`.
- [x] Permissões: daemon root (subido pelo `up`).
- [x] Input validation: sonda sem input externo.
- [x] Sem `unsafe` novo; `forbid(unsafe_code)` mantido.

## Arquivos a CRIAR

### `crates/ramshared-wsl2d/src/canary_probe.rs`
- **Igual ao SPECv2** (Cadence + CanaryProbe::check_content -> Result<bool, CudaError>;
  consts CANARY_BYTES/CANARY_EVERY; testes de cadência sem GPU). Sem mudança.

## Arquivos a MODIFICAR

### `crates/ramshared-wsl2d/Cargo.toml`, `src/lib.rs`
- **Igual ao SPECv2** (+ dep `ramshared-integrity`; `pub mod canary_probe;` + re-export).
- Adicional: re-exportar `ResidencySampler` de `residency` (lib.rs).

### `crates/ramshared-wsl2d/src/residency.rs`
- **DT-3:** `free_floor_bytes` default `0` → `64 * 1024 * 1024`.
- **DT-9 (substitui o `check_residency` do SPECv2):** adicionar `ResidencySampler`:
  ```rust
  /// Amostrador em cadência: corrupção é imediata; free-floor e amostras degradadas
  /// (erros transientes) precisam de `consecutive` amostras (histerese). §9.3 (b)(c).
  pub struct ResidencySampler { cfg: ResidencyConfig, bad_streak: u32 }

  impl ResidencySampler {
      pub fn new(cfg: ResidencyConfig) -> Self;            // { cfg, bad_streak: 0 }
      /// content: Some(true)=ok, Some(false)=corrupção (imediato), None=erro de sonda.
      /// free:    Some(bytes) ou None (erro de mem_info).
      pub fn sample(&mut self, content: Option<bool>, free: Option<u64>) -> Verdict {
          if content == Some(false) {
              return Verdict::Demote(DemoteReason::Corruption); // imediato
          }
          let degraded = content.is_none()
              || free.is_none()
              || free.is_some_and(|f| f < self.cfg.free_floor_bytes);
          if degraded {
              self.bad_streak += 1;
              if self.bad_streak >= self.cfg.consecutive {
                  return Verdict::Demote(DemoteReason::FreeFloor);
              }
          } else {
              self.bad_streak = 0;
          }
          Verdict::Ok
      }
      pub fn bad_streak(&self) -> u32; // observabilidade/teste
  }
  ```
- **Impacto:** os 5 testes de `Canary` continuam verdes (não tocados; `Canary::sample`
  segue como hoje, latência). **Testes novos** (`ResidencySampler`, sem GPU):
  `corruption_is_immediate`, `free_floor_needs_consecutive`, `transient_error_needs_consecutive`,
  `good_sample_resets_streak`.

### `crates/ramshared-wsl2d/src/main.rs`
- **DT-8/DT-9/DT-11/DT-12:** alocar `canary_region`, `CanaryProbe`, `Cadence`,
  `let mut sampler = ResidencySampler::new(ResidencyConfig::default());`; manter o
  canário de **latência por-request** intacto (só usando `spawn_swapoff`); adicionar o
  bloco de cadência; zerar a região-canário no teardown.
- **Bloco de cadência (DT-9/DT-11/M4):**
  ```rust
  if touches_vram && !demoted && demote_rx.is_none() && cadence.tick() {
      let content = match probe.check_content() { Ok(ok) => Some(ok), Err(_) => None }; // DT-11
      let free = match ctx.mem_info() { Ok((f, _)) => Some(f as u64), Err(_) => None }; // DT-11
      if let Verdict::Demote(reason) = sampler.sample(content, free) {
          eprintln!(
              "[wsl2d] DEMOTE ({reason:?}) content={content:?} free={free:?} streak={} -> swapoff {nbd_dev}",
              sampler.bad_streak()
          ); // M4: mostra os valores reais (None = erro)
          demote_rx = Some(spawn_swapoff(&nbd_dev));
      }
  }
  ```
- **Teardown (DT-12):** após `backend.zero()?;`, `let _ = canary_region.zero();` (ou
  `probe.zero()` se a região ficar encapsulada no probe).
- **Impacto:** sem uAPI/ABI; `serve()`/NBD inalterados; latência por-request preservada;
  free-floor agora com histerese; erro transiente não derruba o tier.
- **Testes:** `cargo test -p ramshared-wsl2d` (cadência + `ResidencySampler` + os 5 de
  `Canary`); `clippy --workspace -D warnings`; re-rodar `cascade-validate.sh`/
  `cascade-demote.sh` (sem regressão §14).
  - **Disciplina Kahneman:** ITEM-6 (#5/#2) — ver Mapa.

## Documentos a atualizar no commit do IMPL

- `docs/008-vram-residency-canary/IMPL.md` (Passo 3).
- `docs/vram-as-ram/SPECv3-WSL2.md` §9.4 → implementado; `ARCHITECTURE.md` (C1 resolvido);
  `docs/vram-as-ram/IMPL.md`; `MEMORY.md`.
