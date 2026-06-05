# MEMORY — RamShared

Memória compartilhada da sessão (regra: [`.claude/rules/coding.md`](.claude/rules/coding.md) §Session Memory).
Ler de **baixo para cima** (entrada mais recente no fim). **Append-only**: nunca
apagar/reescrever entradas antigas. Nunca gravar secrets nem endereços que vazem KASLR.

---

## 2026-06-05 — vram-as-ram: SPECv3 + scaffold Rust + import de método

- **`docs/vram-as-ram/`** convergiu para **SPECv3-WSL2** (cascata
  `zram(200) → VRAM(100) → VHDX`; VRAM é tier **frio**, não swap quente; DEMOTE
  por latência §9). v1/v2 preservados como superseded. Esteira aplicada:
  SPEC → Passo 2.5 → SPECv2 → Passo 2.5 → SPECv3.
- **Fase 0** medida em GPU real (RTX 2060, WSL2/GPU-PV): eviction WDDM é
  **data-safe mas latency-unsafe** (4K → 1,18 s sob pressão); tiering **provado**
  (zram 1 GiB cheio + VRAM absorveu 983 MiB de spill, VHDX intocado).
  Ver [`docs/vram-as-ram/FASE0-FINAL.md`](docs/vram-as-ram/FASE0-FINAL.md).
- **Passo 3 (Rust):** `crates/ramshared-tier` (cascata + invariante A1) e
  `crates/ramshared-cuda` (wrapper `libcuda`, **roundtrip validado em GPU real**)
  — `fmt`/`clippy -D warnings`/testes verdes. Próximo: `ramshared-block` (NBD §8),
  depois daemon `ramshared-wsl2d`.
- **Método importado do advoq** (fechou cargo-cult de disciplinas que citavam
  arquivos inexistentes): `docs/postmortems/TEMPLATE.md` (#7),
  `docs/reliability/DEGRADATION-MATRIX.md` (#5), `docs/methodology/SUPERPROMPT.md`
  (#14). `SSDV3-PROMPTS.md` em de-web (era port superficial; **regra: o que não
  existe em kernel — SQL/DDL/migrations/endpoints/SDK — sai, não se traduz**).
- **Pendente:** terminar de-web do `SSDV3-PROMPTS.md`; #9 (gatilhos web→kernel no
  `ssdv3.md`, exige sync `CLAUDE.md`/`AGENTS.md`); `docs/decisions/` + ADRs
  (ainda não existem, citados pelas disciplinas); `docs/LIBRARIES.md`.

---

## 2026-06-05 — vram-as-ram: cascata Rust fim-a-fim VALIDADA

- **Passo 3 completo** (`faca tudo na ordem correta`): daemon com `mlockall`+
  `oom_score_adj=-1000` (Disciplina 3); **canário §9 inline** no serve loop (mede
  latência, arma `Canary` pós-baseline, dispara `swapoff <nbd>` numa thread no
  DEMOTE mantendo o read-back); `check`+zram (linha "Tiers"); 6 crates verdes
  (`fmt`/`clippy -D warnings`/testes).
- **Aceitação §14 provada no sistema vivo** (RTX 2060, WSL2), pressão confinada
  por cgroup v2 — ver [`docs/vram-as-ram/VALIDATION-CASCADE.md`](docs/vram-as-ram/VALIDATION-CASCADE.md):
  - **§14.3 spill:** `up` montou `zram(200)>nbd0(100)>sdc(-2)`; hog 1300M/cgroup
    768M → **511 MiB** na VRAM, **332.800 páginas íntegras**, canário sem
    falso-positivo sob carga.
  - **§14.4 DEMOTE:** **481 MiB vivos** migraram VRAM→VHDX via `swapoff` em 6 s,
    **384.000 páginas íntegras, 0 corrupção**, daemon serviu o read-back.
- **Harness** (fora do repo, em `/home/emdev/fase0/`): `cascade-validate.sh`,
  `cascade-demote.sh`, `cascade-hog.c` (bug corrigido: `mmap` exigia `offset=0`).
- **Pendente:** refinamentos não-bloqueantes (canário §9.4 dedicado p/
  conteúdo/free-floor; daemon multi-conexão). **PR ainda NÃO** (revisar tudo antes).

---

## 2026-06-05 — vram-as-ram: PR #2 MERGED (revisao adversarial + CI)

- **Repo remoto privado:** github.com/emersonbusson/ramshared. CI (GitHub Actions:
  `fmt` + `clippy -D warnings` + `test`) verde. Template de PR (7 secoes da governanca)
  na main. Proxy A1 da cascata validado de novo apos fixes.
- **PR #2 MERGED** (issue #1 fechada), 30 commits: cascata VRAM-as-swap + revisao.
- **Revisao adversarial pre-merge** (disciplina #13) achou bugs reais, corrigidos:
  - C2: DEMOTE engolia falha de `swapoff` + desarmava o canario incondicional → agora
    confirma por canal (mpsc) e **re-arma se falhar** (serve loop segue atendendo o read-back).
  - H3: rede A1 fraca (contava linhas) → `lower_tier_present()` checa prioridade < VRAM.
  - H4: `pkill` apos 300ms podia matar o daemon no meio do `zero()` da VRAM → poll ate 5s.
  - C4/H2/H5: `checked_mul` (overflow), log honesto de mlockall/oom sob `--force`, cap de
    WRITE antes de alocar (anti-DoS).
  - Re-validacao §14 ao vivo apos os fixes: **sem regressao** (511 MiB spill / 480 MiB DEMOTE).
- **Adiado (issue #3, follow-up):** C3 (FFI CUDA duplicada na CLI, fora do crate auditado —
  Day-0), C1-full (canario dedicado §9.4 conteudo/free), H1 (daemon multi-thread), lints
  uniformes, comentarios PT vs regra-EN.

---

## 2026-06-05 — vram-as-ram: issue #3 (debito da revisao) — 5/9 itens

Atacando a issue #3 (follow-up da revisao adversarial) via PRs gated (CI + governanca):
- **PR #4 (C3 + M1):** elimina a FFI CUDA duplicada na CLI (−161 linhas); `probe_cuda`
  reusa o crate auditado `ramshared-cuda`; `find_libcuda` so-filesystem; CLI ganha
  `#![forbid(unsafe_code)]` + lint gate unwrap/expect=deny. Verificado por **diff
  comportamental** do `check --json` em GPU real (gpu name/total/status identicos).
- **PR #5 (M2/M4/M5 + name-buffer):** `const{}` assert do fn-pointer (cuda), NUL final
  no name-buffer (cuda), cap de 4096 no len de opcao do handshake NBD + teste (block),
  validacao de `/dev/zramN` do zramctl (cli).
- **Issue #3: 5/9.** Pendentes (features/decisao, nao mecanicos): **C1-full** (canario
  dedicado §9.4 conteudo/free), **H1** (daemon multi-thread), **M3** (doc de context CUDA
  por-thread), **LOW** (decisao: comentarios PT vs regra-EN; erros tipados; clap).

---

## 2026-06-05 — vram-as-ram: issue #3 — 6/9 + idioma resolvido

- **PR #6:** regra de idioma alinhada — **comentarios de codigo em PT-BR** (coding.md +
  AGENTS.md, governance sync), encerrando o conflito PT-vs-EN apontado pela revisao
  (decisao do usuario: atualizar a regra, sem churn). + **M3** (doc de afinidade de
  thread do Context CUDA).
- **Issue #3: 6/9** (C3, M1, M2, M3, M4, M5). Decisao do usuario: deixar **C1-full**
  (canario dedicado §9.4) e **H1** (daemon multi-thread) **rastreados** no #3 (features,
  nao bloqueiam — cascata validada). LOW resta: erros tipados + clap.

---

## 2026-06-05 — vram-as-ram: docs de raiz + esteira SSDV3 do canario (#8)

- **Docs de raiz** (PR #7): README, ARCHITECTURE, ROADMAP (PT-BR).
- **Issue #8 (C1, canario dedicado §9.4)** pela esteira SSDV3:
  - PRD (Passo 1) + SPEC (Passo 2) — PR #9.
  - **Passo 2.5: SPEC.md deu no-go** (substituir a latencia por-request validada §14 pela
    sonda a cada 64 regredia a deteccao + atrasava baseline 64x) -> **SPECv2 hybrid**
    (PR #10): latencia por-request intacta + sonda de conteudo/free em cadencia, imediata,
    via `ResidencyConfig::check_residency` (DT-7) + `spawn_swapoff` unificado (DT-8).
    SPEC.md preservado. **Candidato ativo: SPECv2.md; re-auditoria = go.**
  - **Pendente: Passo 3 (IMPL)** do SPECv2. #8 segue aberta.

---

## 2026-06-05 — vram-as-ram: canario #8 — Passo 2.5 (2a) -> SPECv3

- **Passo 2.5 sobre o SPECv2 -> no-go** (free-floor/erro transiente demoviam no
  single-sample, sem histerese; semantica do free-floor no GPU-PV nao declarada).
- **SPECv3** (PR #11): `ResidencySampler` puro com **streak (consecutive)** para
  free-floor e amostras degradadas (erros transientes); **corrupcao de conteudo
  (Some(false)) imediata**; latencia por-request intacta; DT-10 declara que o free-floor
  detecta pressao GPU-wide (indicador antecedente), nao a evicção da nossa regiao; DT-12
  zera a regiao-canario no teardown. SPEC.md/SPECv2.md preservados.
- **Candidato ativo: SPECv3.md; re-auditoria = go.** Pendente: Passo 3 (IMPL). #8 aberta.
