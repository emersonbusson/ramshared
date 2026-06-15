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

---

## 2026-06-05 — vram-as-ram: canario #8 — Passo 3 (IMPL do SPECv3)

- **Passo 3 do SPECv3 implementado** (canário §9.4 dedicado). `ResidencySampler` puro
  com histerese: corrupção de conteúdo (`Some(false)`) demove **imediato**; free-floor
  e amostras degradadas (erro de sonda/`mem_info` = `None`) entram no `bad_streak`, só
  demovem em `>= consecutive` (default 3). `free_floor_bytes` default 0 → 64 MiB (DT-3).
- **Arquivos:** novo `crates/ramshared-wsl2d/src/canary_probe.rs` (`Cadence` +
  `CanaryProbe::check_content` + `zero`); `residency.rs` (+`ResidencySampler`, 4 testes);
  `main.rs` (aloca região-canário separada, bloco de cadência, `spawn_swapoff` unificado
  DT-8, `probe.zero()` no teardown DT-12); `lib.rs`/`Cargo.toml`. Latência por-request
  **intacta** (RF-3 não regrediu).
- **Validação:** `fmt`/`clippy --workspace -D warnings` limpos; `cargo test --workspace`
  verde (wsl2d lib 15 ok + 1 GPU ignorado: 2 cadência + 4 `ResidencySampler` novos + 5
  `Canary` intactos). **Pendente no rig (GPU+root):** `cascade-validate.sh`/
  `cascade-demote.sh` em `/home/emdev/fase0/` (§14 sem regressão).
- **Desvio do SPEC (não-ADR):** `.ok()`/`.ok().map()` no lugar do `match Ok/Err`
  (`clippy::manual_ok_err` sob `-D warnings`); semântica idêntica, DT-11 preservada.
- **C1 resolvido** (ARCHITECTURE). Docs: `docs/008-vram-residency-canary/IMPL.md`,
  SPECv3-WSL2 §9.4, `docs/vram-as-ram/IMPL.md`. **Commit/PR pendente** (não commitado).

---

## 2026-06-05 — next-fronts (#3): H1 multi-conn + hardening + typed errors + Fase B specs

Branch `feat/next-fronts-ssdv3` — 5 itens via esteira SSDV3, **um PR só**. Validado ao vivo (RTX 2060).
- **H1 — daemon NBD multi-conexão** (`feat(core)`): laço serial → acceptor + leitor/escritor por
  conexão + **worker CUDA único** (afinidade por `!Send`); canal `WMsg` (backpressure) + réplica
  ilimitada (sem deadlock); `LiveCount` (término determinístico); `CAN_MULTI_CONN`. Esteira
  PRD→SPEC→SPECv2→SPECv3 (2 no-go: lifecycle não-determinístico; válvula backpressure).
  **Lição forte (IMPL→SPEC, Kahneman #13):** o §14.3 ao vivo mostrou que medir **latência total**
  (espera na fila, DT-16) dava **falso-positivo de DEMOTE** sob carga (nbd0 511→10 MiB); revertido
  p/ **serve-only**; SPECv3 atualizado. Auditoria teórica não pegou — só o teste vivo.
- **Hardening** (`fix(core)`): `/usr/sbin/swapoff` absoluto (#2c); #2a/#6c resolvidos pelo H1
  (teardown DT-17 + threads por-conexão); #5 (mlockall) validado ao vivo.
- **Typed errors** (`refactor(core)`): `CascadeError` (zero-dep, padrão `CudaError`); **clap
  rejeitado** (seria a 1ª dep externa num projeto zero-dep/Ring-0) → registrado em LIBRARIES.md (#11).
- **Fase B (kernel-gated, DESIGN-ONLY):** `docs/zram-writeback-vram/` + `docs/ublk-backend/` —
  esteira até SPECv2 (go). WSL2 sem `CONFIG_ZRAM_WRITEBACK`/`CONFIG_BLK_DEV_UBLK` (verificado).
  zram-writeback ingênuo **rejeitado** (reentrância sob reclaim + DEMOTE sem drenagem) → kernel-side
  ou manter 2-tier; ublk threading corrigido (ring = 1 thread) + unsafe-vs-crate ao ADR. IMPL adiada.
- **Validação:** fmt/clippy --workspace -D warnings limpos; `cargo test --workspace` 61/0/2-GPU;
  §14.3 511 MiB/332.800 páginas; §14.4 480 MiB/384.000 páginas/0 corrupção; `-C 2` 2 conns íntegro.
- **C1+H1 = feito** (ROADMAP/ARCHITECTURE). PR único pendente.

---

## 2026-06-07 — Fase B prep: retomada pos-restart WSL2 e log do launcher

- **Sessao Claude2 retomada:** `/home/emdev/.claude2/projects/-home-emdev-codespace-ramshared/7698a3d5-9884-4368-85e9-390a6d062ec8.jsonl`.
- **Plano da sessao anterior:** ativar kernel WSL2 custom `6.6.123.2-microsoft-standard-WSL2+`
  com auto-revert; so depois iniciar Passo 3 da Fase B (`ublk`). `zram-writeback` ingenuo segue
  rejeitado/design-only; o caminho implementavel e `ublk` se o kernel custom estiver ativo.
- **Estado verificado pos-restart:** `uname -r = 6.6.114.1-microsoft-standard-WSL2`; kernel ativo
  da Microsoft nao tem `CONFIG_BLK_DEV_UBLK` nem `CONFIG_ZRAM_WRITEBACK`. Build custom em
  `/home/emdev/WSL2-Linux-Kernel` tem `CONFIG_BLK_DEV_UBLK=m`, `CONFIG_ZRAM_WRITEBACK=y`,
  `CONFIG_IO_URING=y`; release esperada `6.6.123.2-microsoft-standard-WSL2+`.
- **Sem evidencia anterior:** nao existia `C:\wsl\boot-ramshared.log`; logo nao da para saber se o
  comando nao foi executado ou se o launcher auto-reverteu. `.wslconfig` e backup limpos, sem
  `kernel=`.
- **Checkpoint local:** branch `feat/fase-b-prep`, commit `f5691f1 fix(scripts): persist WSL kernel boot logs (#3)`.
  Novo wrapper `scripts/kernel/boot-kernel-logged.ps1` copiado para `C:\wsl\boot-kernel-logged.ps1`.
  Dry-run do wrapper passou (`-DryRunConfig`), arm/desarm idempotente OK; log agora fica em
  `C:\wsl\boot-ramshared.log`.
- **Proximo passo seguro:** executar no PowerShell do Windows:
  `powershell -ExecutionPolicy Bypass -File C:\wsl\boot-kernel-logged.ps1`.
  Isso chama `wsl --shutdown` e encerra esta sessao. Na volta, rodar `uname -r` e
  `cat /mnt/c/wsl/boot-ramshared.log`. Se o kernel custom estiver ativo, seguir Passo 3 da Fase B
  (`docs/ublk-backend/SPECv2.md`); caso contrario, diagnosticar pelo log.

---

## 2026-06-07 — Fase B prep: preflight PowerShell sem shutdown

- **Restricao do usuario:** PowerShell liberado, mas nao executar comandos que encerrem esta sessao
  (`wsl --shutdown`/boot real) sem controle humano.
- **Novo checkpoint:** `d3a99e0 fix(scripts): add WSL kernel preflight mode (#3)` adiciona
  `-PreflightOnly` ao launcher seguro. Ele valida `kernel-ramshared`, backup limpo, `.wslconfig`
  desarmado e arm/desarm em arquivo temporario; nao chama `wsl --shutdown`.
- **Preflight executado via wrapper logado:**
  `powershell -NoProfile -ExecutionPolicy Bypass -File C:\wsl\boot-kernel-logged.ps1 -PreflightOnly`.
  Resultado em `C:\wsl\boot-ramshared.log`: `kernel-size=16027648`, `clean-config=ok`,
  `current-wslconfig=disarmed`, `arm-disarm=ok`, `active-uname=6.6.114.1-microsoft-standard-WSL2`,
  `exit=0`.
- **Gate ainda bloqueado:** kernel custom ainda nao ativo; Fase B/ublk continua pendente ate boot real.

---

## 2026-06-07 — Fase B prep: kernel custom ativo + helpers ublk seguros

- **Kernel custom ativo (humano executou o boot real):** PowerShell reportou
  `OK: kernel custom bootou (6.6.123.2-microsoft-standard-WSL2+)` e `ublk_drv carregou`.
  Validado no WSL: `uname -r = 6.6.123.2-microsoft-standard-WSL2+`,
  `CONFIG_IO_URING=y`, `CONFIG_ZRAM_WRITEBACK=y`, `CONFIG_BLK_DEV_UBLK=m`,
  `lsmod` com `ublk_drv`, `/proc/misc` com `ublk-control`, `/proc/devices` com
  `ublk-char`. `/dev/ublk-control` existe fora do sandbox como `crw------- root root 10,261`.
- **TDD DT-6 (CLI):** commits `25235a8 test(cli): add ublk transport parser RED (#3)` e
  `4fd0ad7 fix(cli): add generic swap device and ublk transport gate (#3)`.
  `ramshared-cli cascade up` agora aceita `--transport {nbd,ublk}`, `--swap-dev PATH` e
  preserva `--nbd` como alias legado. Default segue `nbd` + `/dev/nbd0`.
  `--transport ublk --connections >1` e o modo ublk real ainda sao rejeitados antes de efeitos
  colaterais (`servidor io_uring pendente`). Validados:
  `cargo test -p ramshared-cli cascade::tests`, `cargo test -p ramshared-cli`,
  `cargo clippy -p ramshared-cli -- -D warnings`.
- **TDD ublk uAPI/helpers:** commits `0e82031 test(wsl2d): add ublk uapi helper RED (#3)` e
  `782d3da fix(wsl2d): add safe ublk uapi helpers (#3)`.
  Novo `crates/ramshared-wsl2d/src/ublk.rs` espelha constantes/layouts `repr(C)` de
  `include/uapi/linux/ublk_cmd.h` e helpers puros (`IoDesc::operation/flags`,
  `io_buffer_position`, `decode_io_buffer_position`). Sem `unsafe`, sem `io_uring`, sem dep nova.
  Unions C foram representadas como campos `*_or_*` para preservar layout sem leitura unsafe.
- **Evidencia:** RED falhou por `no ublk in the root`; GREEN:
  `cargo test -p ramshared-wsl2d --test ublk_uapi` (5/5),
  `cargo test -p ramshared-wsl2d` (21 ok, 1 GPU ignorado, 5 ublk ok),
  `cargo clippy -p ramshared-wsl2d -- -D warnings`.
- **Proximo recorte seguro:** implementar/validar gate runtime DT-5 no `check` (Kconfig ublk +
  io_uring funcional + `/dev/ublk-control`) antes de adicionar crate `io-uring` ou servidor ublk.

---

## 2026-06-07 — Fase B prep: gate runtime ublk/io_uring no check

- **TDD DT-5 (`check`):** commits `4626e78 test(cli): add io_uring runtime gate RED (#3)` e
  `d530959 fix(cli): require io_uring runtime for ublk check (#3)`.
- **Mudanca:** `ramshared check` agora le `/proc/sys/kernel/io_uring_disabled` e so marca
  `ublk=ok` quando `CONFIG_BLK_DEV_UBLK` esta habilitado, `/dev/ublk-control` existe,
  `CONFIG_IO_URING` esta habilitado **e** `kernel.io_uring_disabled=0`.
  Valores `1`, `2` ou desconhecido rebaixam `ublk` para `fail` com detalhe explicito
  (`kernel.io_uring_disabled=<n>` ou `unknown`). JSON ganhou `kernel.io_uring_disabled`.
- **Evidencia:** RED falhou por simbolos/campo ausentes; GREEN:
  `cargo test -p ramshared-cli` (10/10), `cargo clippy -p ramshared-cli -- -D warnings`.
  Execucao real via `cargo run -p ramshared-cli -- check --json` no kernel custom:
  `CONFIG_BLK_DEV_UBLK=m`, `io_uring_disabled=0`, `ublk=ok`, `decision=ready`.
- **Proximo recorte seguro:** antes de servidor ublk completo, decidir entre (a) smoke/gate de
  permissao de `/dev/ublk-control` sem criar device ou (b) fechar ADR/dep `io-uring` com numero
  de crate/lockfile e bench plan. Nao iniciar `swapon`/pressao de memoria nesse recorte.

---

## 2026-06-07 — Fase B prep: gate de permissao do ublk-control

- **TDD DT-5/permissao:** commits `0278e09 test(cli): add ublk control access RED (#3)` e
  `78738c1 fix(cli): require ublk control access in check (#3)`.
- **Mudanca:** `ramshared check` agora exige abrir `/dev/ublk-control` com `O_RDWR` para marcar
  `ublk=ok`. O probe nao executa ioctl, nao cria `/dev/ublkcN`/`/dev/ublkbN`, e nao toca swap.
  Sem permissao, detalhe: `/dev/ublk-control not openable; run check as root`.
- **Evidencia:** `cargo test -p ramshared-cli` (11/11), `cargo clippy -p ramshared-cli -- -D warnings`.
  `cargo run -p ramshared-cli -- check --json` como usuario normal: `ublk=fail` por permissao,
  `decision=ready` via NBD. `sudo -n target/debug/ramshared check --json`: `ublk=ok`,
  `io_uring_disabled=0`, `decision=ready`.
- **Descoberta local:** nenhuma ferramenta `ublk`/`ublksrv` instalada no PATH; existe somente
  `/dev/ublk-control` (`crw------- root root 10,261`), sem devices `ublkc*`/`ublkb*` criados.
  Crate candidata atual: `io-uring 0.7.12` (MIT/Apache-2.0, repo tokio-rs/io-uring).

---

## 2026-06-07 — Fase B prep: mapper puro IoDesc ublk -> Request

- **TDD mapper ublk:** commits `4787d63 test(wsl2d): add ublk io desc mapper RED (#3)` e
  `0ff3a24 fix(wsl2d): map ublk io descriptors to block requests (#3)`.
- **Mudanca:** `IoDesc::to_block_request(tag)` converte descritores ublk para
  `ramshared_block::Request` sem `io_uring`/FDs: setores ublk de 512 B viram offset/len em bytes,
  `READ`/`WRITE`/`DISCARD` viram `Read`/`Write`/`Trim`, `FLUSH` vira request sem faixa,
  `tag` vira `handle` interno. Overflows de offset/len e ops sem equivalencia segura
  (ex.: `WRITE_ZEROES`) retornam `IoRequestError`.
- **Evidencia:** RED falhou por `to_block_request`/`IoRequestError` ausentes; GREEN:
  `cargo test -p ramshared-wsl2d --test ublk_uapi` (8/8),
  `cargo test -p ramshared-wsl2d` (21 ok, 1 GPU ignorado, 8 ublk ok),
  `cargo clippy -p ramshared-wsl2d -- -D warnings`.
- **Proximo recorte seguro:** introduzir uma fila/ponte ublk-thread -> worker usando tipos puros
  (sem crate `io-uring`) OU fechar a entrada da crate `io-uring 0.7.12` em ADR/LIBRARIES antes do
  primeiro smoke de ring.

---

## 2026-06-07 — Fase B prep: ponte ublk-thread -> worker sem io_uring

- **TDD ponte pura:** commits `01d1a3b test(wsl2d): add pure ublk bridge RED (#3)` e
  `8a473aa fix(wsl2d): add pure ublk work bridge (#3)`.
- **Mudanca:** `IoWork` carrega `qid`, `tag`, `buffer_addr`, `Request` e payload para a futura
  ponte ublk-thread -> worker. `IoCompletion` gera `IoCmd` de commit (`OK=0`) e traduz erros de
  request para errno negativo (`UBLK_IO_RES_EINVAL=-22`). Tudo segue sem `unsafe`, sem crate
  `io-uring`, sem abrir FDs, sem criar `/dev/ublkbN` e sem tocar swap.
- **Evidencia:** RED falhou por `IoWork`/`IoCompletion`/`UBLK_IO_RES_EINVAL` ausentes; GREEN:
  `cargo test -p ramshared-wsl2d --test ublk_uapi` (11/11),
  `cargo test -p ramshared-wsl2d` (21 ok, 1 GPU ignorado, 11 ublk ok),
  `cargo clippy -p ramshared-wsl2d -- -D warnings`.
- **Proximo recorte seguro:** agora a fronteira pura acabou. Para o primeiro smoke de ring,
  fechar antes a excecao `io-uring 0.7.12` em ADR/LIBRARIES/Cargo.lock e manter `--transport ublk`
  gated ate bench ublk vs NBD provar ganho.

---

## 2026-06-07 — Fase B prep: ADR io-uring aceita e IMPL criado

- **Checkpoint documental:** commit `8255d6b docs: accept gated io-uring dependency for ublk (#3)`.
- **Mudanca:** ADR-0004 saiu de `Proposed` para `Accepted`: usar `io-uring 0.7.12`
  (MIT/Apache-2.0, repo tokio-rs/io-uring) no userspace/Fase B, em vez de FFI hand-rolled.
  A excecao quebra zero-dep apenas no caminho ublk e permanece gated por bench ublk vs NBD.
- **Docs sincronizados no escopo:** `docs/LIBRARIES.md` registra a excecao gated; `README.md`
  troca "zero deps externas" absoluto por "caminho atual zero deps externas"; `SPECv2.md` e
  `PRD.md` deixam a decisao como fechada; `SPEC.md` antigo fica marcado como superseded/no-go;
  novo `docs/ublk-backend/IMPL.md` fixa a sequencia segura.
- **Sem codigo novo neste checkpoint:** nenhum `Cargo.toml` alterado, nenhum `Cargo.lock` alterado,
  nenhum FD/device/swap tocado. Proximo recorte tecnico: adicionar `io-uring 0.7.12` em smoke
  minimo de ring sem ublk device e sem swap.

---

## 2026-06-07 — Fase B prep: smoke minimo io_uring

- **TDD smoke ring:** commits `f08f4d8 test(wsl2d): add io_uring smoke RED (#3)` e
  `a52a2bb fix(wsl2d): add io_uring smoke gate (#3)`.
- **Mudanca:** `ramshared-wsl2d` agora depende de `io-uring 0.7.12`; `Cargo.lock` adicionou
  `io-uring 0.7.12`, `libc 0.2.186`, `bitflags 2.13.0`, `cfg-if 1.0.4`.
  Novo `uring_smoke::run(entries)` cria um ring e chama `submit()` sem SQEs, validando
  `io_uring_setup` + `io_uring_enter` sem `/dev/ublk-control`, sem `/dev/ublkcN`, sem
  `/dev/ublkbN`, sem `swapon` e sem swap.
- **Evidencia:** RED falhou por `no uring_smoke in the root`; GREEN:
  `cargo test -p ramshared-wsl2d --test uring_smoke` (1/1),
  `cargo test -p ramshared-wsl2d` (21 ok, 1 GPU ignorado, 11 ublk ok, 1 uring ok),
  `cargo clippy -p ramshared-wsl2d -- -D warnings`,
  `cargo tree -p ramshared-wsl2d` confirmou apenas as 3 transitivas citadas.
- **Docs:** `README.md`, `docs/LIBRARIES.md` e `docs/ublk-backend/IMPL.md` atualizados para
  refletir que a excecao entrou no smoke e continua gated por bench ublk vs NBD.
- **Proximo recorte seguro:** smoke ublk-control/char device sem `swapon` ou integrar o loop
  ublk real apenas ate criar `/dev/ublkbN` e removê-lo, mantendo `--transport ublk` gated.

---

## 2026-06-07 — Fase B prep: io_uring isolado em ramshared-uring

- **Descoberta ao preparar `URING_CMD`:** a crate `io-uring 0.7.12` expõe
  `SubmissionQueue::push` como `unsafe` (invariante de validade/lifetime do SQE/buffers). Logo,
  manter `ramshared-wsl2d` com `#![forbid(unsafe_code)]` exige uma fronteira propria para
  operações reais de SQE.
- **Refactor:** commit `d15aa32 refactor(wsl2d): isolate io_uring behind wrapper crate (#3)`.
  Novo crate `ramshared-uring` depende de `io-uring 0.7.12` e concentra o futuro `unsafe` de ring;
  `ramshared-wsl2d` depende de `ramshared-uring` e continua sem `unsafe`.
- **Docs:** README agora lista 7 crates; ADR-0004/LIBRARIES/PRD/SPECv2/IMPL foram ajustados para
  "wrapper `ramshared-uring` + crate externa `io-uring`", nao FFI hand-rolled e nao `unsafe` no
  daemon. O `SPEC.md` antigo continua marcado superseded/no-go historico.
- **Evidencia:** `cargo test -p ramshared-uring -p ramshared-wsl2d` passou
  (wsl2d: 21 ok, 1 GPU ignorado, 11 ublk ok, 1 uring ok; ramshared-uring doctest/unit sem testes);
  `cargo clippy -p ramshared-uring -p ramshared-wsl2d -- -D warnings` passou.
- **Proximo recorte seguro:** implementar no `ramshared-uring` um wrapper de `UringCmd80` para
  `UBLK_U_CMD_GET_FEATURES` contra `/dev/ublk-control`, com `// SAFETY:` restrito e teste/smoke
  root, ainda sem `ADD_DEV`, sem `/dev/ublkbN` e sem `swapon`.

---

## 2026-06-07 — Fase B prep: smoke ublk GET_FEATURES sem criar device

- **TDD smoke ublk-control:** commits `6cdc14f test(wsl2d): add ublk control features RED (#3)` e
  `8680bac fix(wsl2d): add ublk control features smoke (#3)`.
- **Mudanca:** `ramshared-uring` ganhou `ublk_get_features(fd)` usando `UringCmd80`/SQE 128 e
  `IORING_OP_URING_CMD` fixo para `UBLK_U_CMD_GET_FEATURES` (`0x80207513`). O unico `unsafe`
  continua no wrapper e documenta a vida do ponteiro de stack ate o CQE. `ramshared-wsl2d`
  ganhou `ublk_control::get_features(path)` e segue com `#![forbid(unsafe_code)]`.
- **Limites mantidos:** o smoke abre somente `/dev/ublk-control`, consulta 8 bytes de features,
  nao chama `ADD_DEV`, nao cria `/dev/ublkcN`/`/dev/ublkbN`, nao executa `swapon` e nao toca swap.
- **Evidencia:** RED falhou por `no ublk_control in the root`; GREEN:
  `cargo test -p ramshared-wsl2d --test ublk_control_smoke --no-run`,
  `sudo -n target/debug/deps/ublk_control_smoke-41db707307e662ad --ignored --nocapture` (1/1),
  `/dev` antes/depois: apenas `ublk-control 600 root root`,
  `cargo test -p ramshared-uring -p ramshared-wsl2d` passou
  (wsl2d: 21 ok, 1 GPU ignorado, 11 ublk ok, 1 uring ok, 1 ublk_control ignorado no modo normal),
  `cargo clippy -p ramshared-uring -p ramshared-wsl2d -- -D warnings` passou.
- **Proximo recorte seguro:** decidir entre smoke `ADD_DEV`+`DEL_DEV` controlado (criando e removendo
  `/dev/ublkbN` sem `swapon`) ou preparar o loop ublk thread/worker ainda gated por `--transport ublk`.

---

## 2026-06-07 — Fase B prep: ADD_DEV/DEL_DEV sem START_DEV

- **TDD lifecycle ublk-control:** commits `e875705 test(wsl2d): add ublk add delete RED (#3)` e
  `1bae47f fix(wsl2d): add ublk add delete smoke (#3)`.
- **Mudanca:** `ramshared-uring` ganhou wrappers fixos `ublk_add_dev(fd, dev_id, &mut [u8; 64])`
  e `ublk_del_dev(fd, dev_id)`, ainda com `UringCmd80`/SQE 128 e `unsafe` confinado ao `push`.
  `ramshared-wsl2d` ganhou `DeviceSpec::smoke_auto()`, `DeviceReport`, encoding/decoding manual
  de `CtrlDevInfo` sem `unsafe`, e constantes UAPI `UBLK_U_CMD_ADD_DEV`/`DEL_DEV`/`DEV_ID_AUTO`.
- **Limites mantidos:** o smoke root cria somente `/dev/ublkcN` via `ADD_DEV`, confirma ausencia
  de `/dev/ublkbN`, chama `DEL_DEV`, espera cleanup e compara `/dev` antes/depois. Nao chama
  `START_DEV`, nao executa `swapon` e nao toca swap.
- **Evidencia:** RED falhou por `DeviceSpec`, `add_device`, `delete_device` e constantes ausentes;
  GREEN: `cargo test -p ramshared-wsl2d --test ublk_control_smoke --test ublk_uapi --no-run`,
  `sudo -n target/debug/deps/ublk_control_smoke-41db707307e662ad --ignored --nocapture` (2/2),
  `/dev` antes/depois: apenas `ublk-control 600 root root`,
  `cargo test -p ramshared-uring -p ramshared-wsl2d` passou
  (wsl2d: 21 ok, 1 GPU ignorado, 11 ublk ok, 1 uring ok, 2 ublk_control ignorados no modo normal),
  `cargo clippy -p ramshared-uring -p ramshared-wsl2d -- -D warnings` passou.
- **Proximo recorte seguro:** antes de `START_DEV`, implementar a preparacao da fila ublk
  (abrir `/dev/ublkcN`, ring de IO, mmap/descritores e `FETCH_REQ`) para evitar deadlock no
  `wait_for_completion_interruptible` do driver; continuar sem `swapon`.

---

## 2026-06-07 — Fase B prep: encoding puro do FETCH io_cmd

- **TDD io_cmd FETCH:** commits `5a2f32a test(wsl2d): add ublk fetch io cmd RED (#3)` e
  `237aff9 fix(wsl2d): encode ublk fetch io cmd (#3)`.
- **Mudanca:** `ublk.rs` ganhou as ops de IO **codificadas** `UBLK_U_IO_FETCH_REQ=0xc0107520`,
  `UBLK_U_IO_COMMIT_AND_FETCH_REQ=0xc0107521`, `UBLK_U_IO_NEED_GET_DATA=0xc0107522` (exigidas
  porque o device usa `UBLK_F_CMD_IOCTL_ENCODE`; o ring so tinha as ops de controle codificadas).
  Novos `IoCmd::fetch(q_id, tag, buffer_addr)` e `IoCmd::to_bytes() -> [u8; 16]` (layout
  `ublksrv_io_cmd`), prontos para copia direta no `cmd` da SQE. Tudo puro: sem `unsafe`, sem
  device, sem ring, sem mmap, sem swap.
- **Provenance:** valores conferidos via `cc` contra
  `/home/emdev/WSL2-Linux-Kernel/include/uapi/linux/ublk_cmd.h` (macro `UBLK_U_IO_*` e
  `_IOWR('u', nr, struct ublksrv_io_cmd)` batem; `sizeof(ublksrv_io_cmd)=16`).
- **Evidencia:** RED falhou por constantes ausentes + `IoCmd::fetch`/`to_bytes` inexistentes;
  GREEN: `cargo test -p ramshared-wsl2d --test ublk_uapi` (13/13), `cargo test -p ramshared-wsl2d`
  (lib 21 ok, 1 GPU ignorado, 13 ublk_uapi ok, 2 ublk_control ignorados, 1 uring ok),
  `cargo fmt --check` e `cargo clippy -p ramshared-wsl2d -- -D warnings` limpos.
- **Fronteira pura esgotada:** o proximo passo sai da faixa segura puramente testavel. O smoke
  `FETCH_REQ` no char device exige `mmap` de `/dev/ublkcN` (nova superficie `unsafe` em
  `ramshared-uring`) + ring persistente com submissao **sem** esperar CQE (o driver deixa o
  FETCH pendente em `-EIOCBQUEUED`). Decisao pendente do dono: (a) seguir para esse smoke, (b)
  fechar SPEC/IMPL SSDV3 do loop de ring antes, ou (c) abrir o PR da Fase B prep ja acumulada.

---

## 2026-06-07 — Fase B prep: SPEC SSDV3 do ring loop fechado

- **Decisao do dono:** "SSDV3 SPEC first". Criado `docs/ublk-backend/SPEC-ring-loop.md` (PASSO 2),
  linkado do `SPECv2.md` (DT-3) e do `IMPL.md`. So docs, sem codigo novo.
- **Fatos verificados lendo `ublk_drv.c` (6.6.123.2):** o mmap de io-desc por fila e **READ-ONLY**
  (`VM_WRITE` -> `-EPERM`, 1413-1414) — invariante novo pro IMPL; `offset = q_id *
  ublk_max_cmd_buf_size()`, `len = round_up(q_depth*24, PAGE)`; `sizeof(ublksrv_io_desc)=24`,
  indexado por `tag` (`&buf[tag*24]`, 704-709). `FETCH_REQ` retorna `-EIOCBQUEUED` (estacionado).
  Teardown: `ublk_cancel_queue` (1523-1545) entrega
  `io_uring_cmd_done(cmd, UBLK_IO_RES_ABORT=-ENODEV)` aos FETCH estacionados -> a thread de ring
  NAO trava.
- **Crate `io-uring 0.7.12`:** `submit()`==`submit_and_wait(0)` nao bloqueia; `completion().next()`
  drena sem bloquear; `submission().push` e unsafe. Ring owner usa `submit()`+drain, nunca
  `submit_and_wait` sobre FETCH (anti-deadlock, DT-R2).
- **Fronteira unsafe:** `mmap` do io-desc = `PROT_READ`; todo `unsafe` (push do SQE + mmap/munmap
  RAII) fica em `ramshared-uring`; daemon segue `#![forbid(unsafe_code)]`. mmap via `libc`
  (ja transitiva), nao `memmap2` (DT-R1).
- **Proximos recortes TDD (SPEC §8):** M1 = `mmap` read-only (ADD_DEV->mmap->ler io-desc->munmap->
  DEL_DEV); M2 = submeter FETCH para todas as tags sem esperar CQE, DEL_DEV gera CQEs ABORT. Ambos
  smoke root, sem `START_DEV`, sem `/dev/ublkbN`, sem `swapon`. M3 (START_DEV + loop) fica gated por
  bench.

---

## 2026-06-07 — Fase B prep: M1 mmap read-only do io-desc

- **TDD M1 (SPEC-ring-loop §8):** commits `db896f1 test(wsl2d): add ublk io-desc mmap RED (#3)` e
  `c6e2890 fix(wsl2d): mmap ublk io-desc buffer read-only (#3)`.
- **Mudanca:** `ramshared-uring` ganhou `MmapRo` (RAII: `mmap` `PROT_READ`/`MAP_SHARED` + `munmap`
  no Drop), `page_size()` e `round_up_to_page()`; todo `unsafe` novo (sysconf, mmap, munmap,
  `from_raw_parts`) com `// SAFETY:`. Dep direta `libc = "0.2"` (ja transitiva; `Cargo.lock` so
  marcou `libc` como dep de `ramshared-uring`). `ublk.rs` ganhou `UBLK_IO_DESC_SIZE=24` e
  `IoDesc::from_ne_bytes`. Novo modulo `ublk_queue::read_io_desc(path, q_depth, tag)` mapeia a
  fila 0 (offset 0) read-only e decodifica o io-desc.
- **Invariante unsafe:** unsafe novo 100% em `ramshared-uring`. O unico unsafe no wsl2d e o
  `mlockall` **PRE-EXISTENTE** em `main.rs` (binario, fora do `forbid` da lib) — nao deste recorte;
  a lib (incl. `ublk_queue`) compila sob `#![forbid(unsafe_code)]`.
- **Evidencia:** RED falhou por `UBLK_IO_DESC_SIZE`/`from_ne_bytes`/`ublk_queue` ausentes; GREEN:
  `cargo test -p ramshared-wsl2d --test ublk_uapi` (14/14), `sudo -n .../ublk_control_smoke
  --ignored --nocapture` (3/3, inclui `mmap`), `/dev` antes==depois (so `ublk-control`),
  `cargo test -p ramshared-uring -p ramshared-wsl2d` verde, `cargo fmt --check` +
  `cargo clippy -p ramshared-uring -p ramshared-wsl2d -- -D warnings` limpos.
- **Limites mantidos:** mmap read-only (kernel proibe `VM_WRITE`); sem `START_DEV`, sem
  `/dev/ublkbN`, sem `swapon`. io-desc[0] lido == `IoDesc::default()` (zerado, sem I/O).
- **Proximo recorte (SPEC §8 M2):** submeter `FETCH_REQ` para todas as tags (push de SQE
  `UringCmd80` com `IoCmd::fetch`) sem esperar CQE; `DEL_DEV` gera CQEs `UBLK_IO_RES_ABORT(-ENODEV)`.
  Ainda sem `START_DEV`/`swapon`.

---

## 2026-06-07 — Fase B prep: M2 FETCH_REQ no-wait + teardown

- **TDD M2 (SPEC-ring-loop §8):** commits `29d0479 test(wsl2d): add ublk fetch submit RED (#3)` e
  `8a325a3 fix(wsl2d): submit ublk FETCH_REQ without waiting (#3)`.
- **Mudanca:** `ramshared-uring` ganhou `UblkFetchRing` (ring io_uring persistente: push de
  `UringCmd80` FETCH por tag, `submit()` want=0, `drain()` nao-bloqueante via `completion()`),
  `UblkCompletion {tag, result}` e `fetch_cmd80` (empacota `ublksrv_io_cmd`). `unsafe push`
  isolado; dono dos buffers de dados. `ublk_queue::FetchSession` segura char device + ring;
  `ramshared-wsl2d` segue `#![forbid(unsafe_code)]`.
- **DEADLOCK descoberto e resolvido (lido no driver):** `ublk_ctrl_del_dev` chama
  `ublk_cancel_dev` (posta `io_uring_cmd_done(ABORT=-ENODEV)`) e depois **bloqueia** em
  `wait_event(idr_freed)` (ublk_drv.c:2523) ate o char fechar; o char so fecha quando os FETCH
  (que seguram `fget` via io_uring) sao cancelados. Teardown single-thread (DEL_DEV bloqueante +
  drain depois) **trava** (confirmado: timeout exit 124). Correto: drenar o ring numa **thread
  propria** (DT-3) em paralelo ao DEL_DEV — ela coleta os aborts e fecha o char, desbloqueando.
- **Evidencia:** RED falhou por `fetch_cmd80`/`FetchSession` ausentes; GREEN:
  `cargo test -p ramshared-uring --lib` (1/1 fetch_cmd80),
  `sudo -n timeout 60 .../ublk_control_smoke --ignored --test-threads=1` (4/4, inclui FETCH),
  `/dev` antes==depois (so `ublk-control`), `cargo test -p ramshared-uring -p ramshared-wsl2d`
  verde, `cargo fmt --check` + `cargo clippy -p ramshared-uring -p ramshared-wsl2d -- -D warnings`.
- **Operacional:** os smokes root checam `/dev` global → rodar com **`--test-threads=1`** (em
  paralelo um teste ve o device do outro). Sempre usar `sudo -n timeout <n>` para nao pendurar.
- **Limites mantidos:** FETCH estacionado sem I/O; sem `START_DEV`, sem `/dev/ublkbN`, sem `swapon`.
- **Proximo: M3 (gated por bench).** `START_DEV` + loop ring↔worker H1 (thread dona do ring drena
  FETCH→IoWork→worker→COMMIT_AND_FETCH). Fora do prep; exige PRD/bench ublk vs NBD.

---

## 2026-06-07 — Fase B M3 inicio: SET_PARAMS (pre-START_DEV)

- **Decisao do dono:** continuar a implementacao ate o ublk **funcionar**; PR so no fim (nao agora).
  Ver [[feedback-batch-local-single-pr]].
- **TDD SET_PARAMS:** commits `f2eddca test(wsl2d): add ublk set_params RED (#3)` e
  `883de60 fix(wsl2d): add ublk set/get params control (#3)`.
- **Mudanca:** constantes `UBLK_U_CMD_{START_DEV,STOP_DEV,SET_PARAMS,GET_PARAMS}` (cc:
  `0xc0207506/07/08`; `GET_PARAMS=0x80207509`, e `_IOR`). `Params::basic_disk(dev_sectors,
  logical_bs_shift, physical_bs_shift)` + `Params::to_bytes`/`from_bytes` (layout 112 B; offsets
  via cc: basic@8, dev_sectors@24, discard@40, devt@60, zoned@76, padding@108). Wrappers
  `ublk_set_params`/`ublk_get_params` (control) em ramshared-uring; `ublk_control::set_params`/
  `get_params`. Tudo control-only (sem char/FETCH/START → DEL_DEV nao deadlocka).
- **Evidencia:** RED falhou por constantes/`basic_disk`/`to_bytes`/`set_params` ausentes; GREEN:
  `cargo test -p ramshared-wsl2d --test ublk_uapi` (15/15, +round-trip puro), smoke root
  `set_params_roundtrips` ok (GET confirma `dev_sectors=2048`, bs 9/12, types BASIC), **5/5 smokes
  root** single-thread, `/dev` so `ublk-control`, clippy/fmt limpos.
- **Proximo (M3 nucleo, maior risco):** `START_DEV` cria `/dev/ublkbN` (block device) e exige as
  filas ready (FETCH submetido) + **thread dona do ring servindo I/O** (drena FETCH → io-desc via
  mmap → backend → COMMIT_AND_FETCH). Validar com **backend de RAM (Vec)** + I/O de teste (dd) no
  block device ANTES de VRAM/swap. `swapon` continua sendo o passo final separado.

---

## 2026-06-07 — Fase B M3b: ublk FUNCIONAL (block device + I/O via backend RAM)

- **TDD M3b (2 recortes):** RamBackend `dd58baa` (RED `fe4c076`); loop servidor + I/O
  `79065f6 fix(wsl2d): serve ublk block io with ram backend (#3)` (RED `bb1df01`).
- **Investigacao do driver (agentes):** START_DEV exige SET_PARAMS + filas ready; com daemon
  privilegiado (root) o `add_disk` faz **partition scan** (le setor 0) → thread servidora
  obrigatoria durante START_DEV. WRITE: kernel copia bio→buffer ANTES do CQE. READ: servidor
  preenche buffer, COMMIT copia **exatamente `result` bytes** (READ `result=0` vira -EIO).
  `result>=0` = bytes, `<0` = -errno. COMMIT_AND_FETCH = 1 ioctl completa+re-arma (re-fornecer addr).
- **Mudanca:** `ramshared-uring` ganhou `UblkServer` (ring + mmap io-desc + buffers; submit FETCH,
  `io_desc_bytes`, `buffer_mut`, `commit_and_fetch`; `unsafe` isolado + `unsafe impl Send for
  MmapRo`), `io_cmd80`, `ublk_start_dev`/`ublk_stop_dev`. `ramshared-wsl2d` ganhou
  `ublk_server::{RamBackend, serve_request, spawn_server}` e `ublk_control::start_dev`/`stop_dev`.
  Daemon-lib segue `#![forbid(unsafe_code)]`.
- **Padrao do loop (DT-3):** `spawn_server` roda o loop numa thread propria; START_DEV/STOP_DEV
  (control, bloqueantes) na thread principal. A servidora drena FETCH→serve→COMMIT e os aborts;
  sem ela em paralelo, START/STOP travam (mesmo deadlock do M2).
- **Evidencia:** `cargo test ublk_server` (2/2 puro: round-trip WRITE/READ); smoke root
  `serves_read_from_ram_backend_over_block_device` ok (0.07s): cria `/dev/ublkbN`, READ setor 100
  devolve o padrao do backend, STOP+DEL limpam. **6/6 smokes root** (2 binarios, `--test-threads=1`),
  `/dev` antes==depois, clippy/fmt limpos.
- **Proximo: M3c (gated por bench).** Ligar `serve_request`/loop ao `VramBackend`/worker H1 (em vez
  do RamBackend); bench latencia ublk vs NBD (p50/p99). `swapon`/pressao de memoria SO depois do
  ganho provado. So ha smoke de READ (WRITE end-to-end via block device e candidato).

---

## 2026-06-07 — Fase B: WRITE end-to-end + plano M3c

- **TDD WRITE smoke:** commits `ccb1994 test(wsl2d): add ublk write io smoke RED (#3)` e
  `698604b fix(wsl2d): return backend from ublk server loop (#3)`.
- **Mudanca:** `run_server_loop`/`ServerHandle::join` devolvem o `RamBackend` ao terminar (no
  abort), permitindo inspecao direta sem page cache. Smoke
  `serves_write_into_ram_backend_over_block_device`: escreve via `/dev/ublkbN` + `fsync` (forca
  writeback → WRITE request → loop → backend), STOP, confere o backend devolvido. buf por tag =
  disco inteiro (qualquer writeback cabe no buffer).
- **Evidencia:** 2/2 smokes I/O root (READ + WRITE) ok (0.14s), `/dev` antes==depois, clippy/fmt limpos.
- **Estado:** ublk FUNCIONAL com READ+WRITE via backend RAM. Plano M3c fechado em
  `SPEC-ring-loop.md` §12: (1) trait `BlockBackend`, (2) adapter VramBackend/worker H1 (DT-3: loop
  NAO toca CUDA, manda `IoWork` ao worker), (3) smoke I/O vs VRAM sem swap, (4) bench p50/p99 ublk
  vs NBD, (5) `swapon` so com ganho provado.
- **Proximo (M3c, gated/decisao):** comecar pela (1) trait `BlockBackend` (refactor seguro) e (2)
  investigar a API do `VramBackend` antes de qualquer CUDA/VRAM real.

---

## 2026-06-07 — Fase B M3c prep: reuso BlockBackend + design DT-3 fechado

- **Disciplina (reuso, SSDV3 #1):** investigacao (agente) revelou que o trait
  `ramshared_block::BlockBackend` JA existe e o `VramBackend` JA o implementa (`backend.rs:24`).
  Meu `RamBackend`/`serve_request` duplicavam. **Corrigido:** commit
  `baf2203 refactor(wsl2d): reuse BlockBackend trait in ublk serve (#3)` — `RamBackend` implementa
  `BlockBackend`, `serve_request` generico sobre o trait → o loop ublk serve qualquer backend,
  incl. `VramBackend`, sem mudanca. Mantido o serve in-place (sem alloc, DT-8). 2/2 smokes I/O
  root + unit verdes apos o refactor.
- **Design DT-3 fechado no `SPEC-ring-loop.md` §12** (verificado no driver/cuda): worker e a unica
  thread CUDA (`DeviceMem` !Send, copias sincronas `cuMemcpy*`); `spawn_ublk_worker` cria o stack
  Cuda/Context/DeviceMem/VramBackend NA propria thread (nao recebe pronto). Canais mpsc:
  ring→worker `SyncSender<IoWork>` (bounded CHAN_CAP), worker→ring `Sender<WorkerReply>`
  (unbounded, DT-7). **Gap do READ resolvido:** novo `WorkerReply { qid, tag, result, read_data }`;
  o ring owner copia `read_data` no buffer da tag antes do COMMIT. WRITE vai como `IoWork.payload`
  owned (nunca ponteiro cru cross-thread).
- **Estado:** loop single-thread (M3b) valida a mecanica do ring com RAM; o M3c separa
  ring owner/worker (DT-3) para CUDA. `serve_request`/`VramBackend` reusados verbatim; so o wrapper
  de worker e novo. O worker H1 NBD esta inlined em `main.rs::run()` — criar `spawn_ublk_worker`.
- **Proximo (M3c IMPL, gated/decisao):** `spawn_ublk_worker` + canais + ring owner DT-3, validar
  com RamBackend (sem CUDA), depois plugar `VramBackend` (smoke GPU). Bench e `swapon` depois.

---

## 2026-06-07 — Fase B M3c: worker DT-3 (metade da arquitetura, sem GPU)

- **TDD worker DT-3:** commits `4ba0a76 test(wsl2d): add ublk dt-3 worker RED (#3)` e
  `9575d8f fix(wsl2d): add ublk dt-3 worker over channels (#3)`.
- **Mudanca:** `serve_request` **unificado** em torno de `Request` (era `IoDesc`) — reusa o trait
  `BlockBackend` tanto no loop single-thread (M3b) quanto no worker; `run_server_loop` converte
  `IoDesc`→`Request` via `to_block_request`. Novo `spawn_ublk_worker<B: BlockBackend + Send +
  'static>(backend, work_rx, reply_tx)`: thread dona do backend (unica a tocar VRAM/CUDA, DT-3),
  loopa `IoWork`→`serve_request`→`WorkerReply{qid,tag,result,read_data}`; encerra quando o canal
  fecha e devolve o backend. READ aloca buffer no worker; WRITE/FLUSH usam o `payload`.
- **Evidencia:** teste puro `ublk_worker` (1/1, sem root): READ devolve dados, WRITE persiste,
  roundtrip ok, backend devolvido no join. `ublk_server` unit 2/2 (Request). Smokes I/O root 2/2
  preservados (run_server_loop refatorado). clippy/fmt limpos.
- **Falta para o ublk-VRAM (M3c, gated):** (1) **ring owner DT-3** — drena CQE ublk → envia IoWork
  → recebe WorkerReply → copia read_data no buffer da tag → COMMIT; multiplexa CQE+reply, teardown
  cuidadoso (risco de deadlock como M2/M3b). (2) **VramBackend nao e `'static`** (`DeviceMem<'c,'a>`
  borrows Context) → `spawn_ublk_worker` precisa de uma variante **factory** (cria o stack CUDA NA
  thread) em vez de receber o backend pronto. (3) smoke GPU. (4) bench. (5) `swapon`.

---

## 2026-06-07 — Fase B M3c: ring owner DT-3 (arquitetura ALVO completa, sem GPU)

- **TDD ring owner DT-3:** commits `90a9c21 test(wsl2d): add ublk dt-3 ring owner RED (#3)` e
  `ffc542f fix(wsl2d): add ublk dt-3 ring owner loop (#3)`.
- **Mudanca:** `spawn_server_dt3` sobe 2 threads: ring owner (dona do `UblkServer`) + worker (dona
  do backend). Ring owner: drena CQE → `IoWork` (copia payload do WRITE do buffer da tag) →
  `work_tx` → recebe `WorkerReply` → copia `read_data` na tag → `commit_and_fetch`. Teardown: no
  abort retorna, dropa `work_tx`, worker encerra (devolve backend). `ServerHandleDt3::join` une os dois.
- **Evidencia:** smoke DT-3 root `dt3_serves_read_from_ram_backend_over_block_device` ok (0.07s,
  **SEM deadlock** — o teardown coordenado funcionou de primeira); **3/3 smokes I/O** (single-thread
  READ/WRITE + DT-3) verdes; `/dev` antes==depois; clippy/fmt limpos.
- **Marco:** a **arquitetura ALVO DT-3 esta funcionando** com RamBackend (ring owner + worker
  separados, do jeito que o VRAM exige). O VramBackend pluga pela MESMA via.
- **Falta SO para o ublk-VRAM:** (1) `VramBackend` e `!Send`/`!'static` (`DeviceMem<'c,'a>` borrows
  `Context`) → `spawn_server_dt3`/`spawn_ublk_worker` precisam de variante **factory** que cria o
  stack Cuda/Context/DeviceMem NA thread do worker (investigar `ramshared-cuda/src/driver.rs`).
  (2) smoke GPU. (3) bench p50/p99 ublk vs NBD. (4) `swapon`.

---

## 2026-06-07 — Fase B M3c: ublk SERVINDO VRAM (objetivo central da Fase B)

- **TDD VRAM:** commits `d5a5a71 test(wsl2d): add ublk dt-3 vram smoke RED (#3)` e
  `ac915f7 fix(wsl2d): serve ublk from vram via dt-3 worker (#3)`.
- **Lifetime resolvido:** `spawn_server_dt3_vram` — o worker cria `Cuda::load()` → `device(0)` →
  `create_context` (vira corrente na thread) → `alloc` → `VramBackend::new` NA propria thread e
  roda `worker_loop(&mut backend, ...)` ali. Assim o `VramBackend<'c,'a>` (!Send/!'static por causa
  do borrow `DeviceMem`/`Context`) nunca cruza thread. `worker_loop` passou a receber `&mut B`.
- **Evidencia (GPU real, RTX 2060):** smoke root `dt3_serves_io_from_vram_over_block_device` ok:
  bs 4096, WRITE bloco → `cuMemcpyHtoD`, `sync`+`/proc/sys/vm/drop_caches`, READ → `cuMemcpyDtoH`
  confere o bloco. **4/4 smokes I/O** (RAM READ/WRITE + DT-3 RAM + DT-3 VRAM) single-thread, `/dev`
  antes==depois, clippy/fmt limpos. `Cuda::load` funciona como root no WSL2.
- **MARCO:** o ublk serve a **VRAM** end-to-end como block device `/dev/ublkbN` — o objetivo
  central da Fase B (ublk no lugar do NBD para o transporte de swap da VRAM).
- **Falta SO:** (1) **bench** ublk vs NBD (p50/p99, mesma carga). (2) **`swapon`** `/dev/ublkbN` —
  o passo final, com pressao de memoria; cuidado (pode travar WSL2 — ver
  [[feedback-wsl2-cargo-build-caution]]). DEMOTE segue `swapoff` (SPECv2 DT-6).

---

## 2026-06-07 — Fase B M3c: VRAM-as-RAM via swap por ublk (CAPSTONE)

- **Validacao swap:** commit `2561c9b test(wsl2d): validate vram-ublk as swap device (#3)`.
- **O que prova:** `vram_ublk_round_trips_as_swap_device` (root+GPU) faz `mkswap` → `swapon`
  (sem `-p`) → confere `/proc/swaps` → `swapoff` sobre o `/dev/ublkbN` servido pela VRAM (DT-3).
  `mkswap` escreve o header na VRAM (`cuMemcpyHtoD` via ublk); `swapon` le o header (ublk READ); o
  kernel registra a area de swap. **A VRAM serve como swap (RAM) atraves do ublk** — o objetivo
  central da Fase B (ublk no lugar do NBD).
- **Seguranca:** ciclo limitado e reversivel; device 128 MiB; SEM pressao de memoria (9.6 GiB RAM
  livre → kernel nao pagina na janela de ms); `swapon` sem `-p` (prioridade auto baixa);
  `SwapGuard` faz `swapoff` antes de stop/del. Evidencia: 0.62s, `/proc/swaps` antes==depois (sem
  residuo), `/dev` limpo. `mkswap`/`swapon`/`swapoff` rodam como root no WSL2.
- **Estado:** **5/5 smokes I/O** (RAM READ/WRITE + DT-3 RAM + DT-3 VRAM + swap) verdes
  single-thread, `/proc/swaps` e `/dev` limpos antes==depois.
- **Falta SO (nao-funcional):** **bench** ublk vs NBD (p50/p99) — justificativa de adocao
  (anti-halo #11). A funcionalidade esta provada end-to-end. **Para producao sob pressao real:**
  `mlockall` no daemon + caminho do worker sem alloc (`worker_loop` aloca um `Vec` por READ — ok
  no smoke sem pressao, mas hazard sob swap real).

---

## 2026-06-07 — Fase B M3c: bench de latencia + ring owner bloqueante

- **Bench:** commit `5196466 test(wsl2d): add ublk-vram read latency bench (#3)`. Leitura 4KB
  `O_DIRECT` (offsets pseudo-aleatorios, p50/p90/p99) no `/dev/ublkbN` servido pela VRAM.
- **Perf (guiado pelo bench):** commit `b5032aa perf(wsl2d): block instead of poll in ublk dt-3
  ring owner (#3)`. O ring owner trocou o poll (sleep 200us) por espera bloqueante: ocioso bloqueia
  no proximo CQE (`UblkServer::wait_and_drain` = `submit_and_wait(1)`); com request em voo bloqueia
  no `recv` da resposta do worker. Helpers `commit_reply`/`dispatch_request`.
- **Resultado (RTX 2060, 4KB READ O_DIRECT):** p50 **628us → 231us** (2.7x), p99 820us → 400us,
  max ~1.3ms. O residual (~231us) e o custo do DT-3 (2 saltos de thread por I/O) + escalonamento do
  WSL2 — nao mais o poll nem o `cuMemcpy` (us). 6/6 smokes I/O verdes apos a mudanca, swap limpo.
- **Falta para 'tudo' (antes do PR):** (1) **bench ublk vs NBD** — comparacao com o transporte NBD
  (sobe o daemon `main.rs` + `nbd-client`; o lado ublk ja esta medido em 231us p50). (2) **no-alloc
  no worker** — `worker_loop` aloca um `Vec` por READ; hazard sob swap pesado (mitigacao: pool de
  buffers ciclado ring owner ↔ worker). NAO validavel sem gerar pressao (que pode travar WSL2).

---

## 2026-06-07 — Fase B M3c: GATE passado — ublk vence o NBD (bench fio)

- **Comparacao fio (RTX 2060, 4KB randread `O_DIRECT` iodepth=1):**
  - **ublk-VRAM:** p50=**241us** p99=461us IOPS=3911 (commit `15d1090`, teste `fio_bench_vram_ublk`).
  - **NBD-VRAM** (daemon `main.rs` + `nbd-client`, medido a parte): p50=**326us** p99=635us IOPS=2900.
  - → **ublk ~26% mais rapido em p50, ~27% em p99, ~35% mais IOPS** (io_uring vs round-trip de
    socket NBD).
- **Gate anti-halo #11 SATISFEITO:** ublk < NBD por ~26% → adocao do ublk justificada por bench.
- **Harness NBD (one-off, nao commitado):** `cargo build -p ramshared-wsl2d --bin ramshared-wsl2d`;
  `sudo modprobe nbd nbds_max=2`; daemon `--size 64 --sock <s> --nbd /dev/nbd0` (bg);
  `nbd-client -unix <s> /dev/nbd0`; `fio`; `nbd-client -d /dev/nbd0`; `pkill -f 'ramshared-wsl2d
  --size'`. Cleanup confirmado: nbd0 livre, sem daemon, `/dev` so `ublk-control`.
- **Estado: funcionalmente TUDO works** (ublk serve VRAM, swap validado, bench vence NBD). **Falta
  so (producao):** no-alloc no `worker_loop` — hardening para swap sob pressao, NAO validavel aqui
  (pressao pode travar WSL2); `mlockall` e do daemon integrador (`main.rs` ja faz).

---

## 2026-06-09 — Fase B: no-alloc DT-8 feito — ultimo item antes do PR

- **No-alloc do worker (DT-8) implementado** (commit `aa2f060`, `perf(wsl2d)`). Antes: READ alocava
  `vec![0u8; len]` no worker e WRITE `.to_vec()` no ring owner; `read_data`/`payload` dropados (free)
  por request. Alocar no caminho de I/O = hazard de deadlock sob pressao de swap (alloc -> reclaim ->
  swap -> precisa do worker).
- **Desenho:** ring owner mantem um **pool de buffers pre-aquecido** (`queue_depth` buffers de
  `buf_size`, montado em `run_ring_owner`). `dispatch_request` da `pop()` no pool e `resize(len)`;
  worker serve **in-place** no buffer cedido (READ inclusive) e o devolve em `WorkerReply.buf`;
  `commit_reply` copia (READ) para a tag e **recicla** (`clear()` preserva capacidade, push no pool).
  Em regime: **zero malloc/free no hot path**. Invariante `pool.len() + in_flight == queue_depth`
  -> pool nunca esvazia (pop sempre serve). `unwrap_or_default` no pop e so defensivo (aquecimento).
- **Contrato mudou:** `WorkerReply` trocou `read_data: Vec<u8>` por `buf: Vec<u8>` + `is_read: bool`.
  Unico consumidor era `tests/ublk_worker.rs` (o RamShared `ServeOutcome.read_data` do caminho NBD e
  outro struct, intocado). `worker_loop` agora serve sempre in-place no `work.payload`.
- **TDD:** `tests/ublk_worker.rs` reescrito como RED (campos `buf`/`is_read` inexistentes -> compile
  fail), depois GREEN. `run_ring_owner` ganhou params `queue_depth`/`buf_size` (2 call sites: DT-3
  RAM e DT-3 VRAM).
- **Validado (RTX 2060, root):** worker unit GREEN; smokes DT-3 RAM, DT-3 VRAM e VRAM-as-swap todos
  verdes; clippy lib `-D warnings` limpo; 40 testes nao-root verdes; `/dev` e `/proc/swaps` limpos
  (so o swap do sistema `/dev/sdb`). Latencia inalterada dentro do ruido (p50 ~250us vs 231-241us
  anteriores) — o no-alloc e sobre seguranca contra deadlock, nao velocidade.
- **Estado: Fase B funcionalmente completa + ultimo item de producao feito -> PRONTA PARA O PR.**
  Branch `feat/fase-b-prep`, ~82 commits. PR consolida tudo (corpo PT-BR, tabela de commits por
  governance.md). `mlockall` ja e do daemon integrador (`main.rs`).

---

## 2026-06-09 — Fase B: queue_depth>1 validado (paralelismo de swap)

- **qd>1 era estruturalmente suportado mas nao-validado** (todos os smokes usavam queue_depth=1).
  O `UblkServer` ja dimensiona o mmap de io-desc a `queue_depth*24`, aloca um buffer por tag e
  submete FETCH por tag com o **endereco proprio de cada tag** (`self.buffers[tag]` em
  submit_initial_fetch:365 e commit_and_fetch:405); o pool no-alloc pre-aquece `queue_depth`
  buffers. Faltava so provar end-to-end sob concorrencia.
- **Teste novo** (commit `444354a`): `dt3_vram_serves_concurrent_io_with_queue_depth_gt1`. Device
  `queue_depth=4` (fila unica — so servimos a fila 0), escreve 16 blocos com padrao distinto por
  indice (`block_pattern`: ~+5/byte mod 251 entre blocos), dropa page cache, dispara 4 threads
  lendo round-robin via `O_DIRECT` (64 rodadas cada) conferindo integridade por bloco. Aliasing/
  troca de buffer entre tags ou underflow do pool -> corrupcao (assert) ou deadlock.
- **Resultado (RTX 2060):** ~4096 leituras concorrentes, integridade OK, sem deadlock, `/dev` limpo
  (3.11s). **Pool no-alloc correto com `in_flight>1`** (invariante `pool.len()+in_flight==qd`).
- **Fora de escopo:** `nr_hw_queues > 1` (multi-fila) exigiria um ring + char-region por fila ->
  novo SPEC. A fila unica com qd>4..N ja da paralelismo suficiente pro swap no MVP.
- **Estado:** Fase B = VRAM + swap + bench(>NBD) + no-alloc + qd>1, tudo em hardware. PR seguro
  ainda (usuario: "nada de PR agora, continue"). Branch `feat/fase-b-prep`, ~84 commits.

---

## 2026-06-09 — Fase B: WRITE concorrente qd>1 + achado do cap de 4KB

- **Cobertura de WRITE sob qd>1** (commit `736f5a5`): o smoke qd>1 anterior era so leitura. Novo
  `dt3_vram_serves_concurrent_writes_with_queue_depth_gt1`: 4 threads donas de blocos disjuntos, 32
  rodadas WRITE(padrao novo por rodada)+READ-verify via O_DIRECT. Exercita o caminho de WRITE do
  pool no-alloc (`dispatch_request` copia tag_buf->buffer do pool; worker `write_at` na VRAM) com
  `in_flight>1`. ~512 ciclos concorrentes, integridade OK, sem deadlock (RTX 2060). Helper
  `keyed_pattern(seed,bs)` parametriza padrao por (bloco,rodada); `block_pattern` delega a ele.
- **ACHADO (kernel): o device so faz requests de 4KB.** `DeviceSpec::smoke_auto` seta
  `max_io_buf_bytes=4096`; `ublk_drv.c:307` faz `min(bufsize, max_hw_sectors<<9)` -> kernel limita
  TODO request a 4KB. Linha 546 `blk_queue_max_hw_sectors(q, p->max_sectors)` e validacao
  `max_sectors <= max_io_buf_bytes>>9` (581) -> `max_sectors` (em `params.basic`, hoje 0 via
  `..default()`) acopla com `max_io_buf_bytes`. **NAO e bug**: e seguro (request <= buf_size sempre)
  e casa com swap-in (1 pagina). **Custo:** swap clustering/writeback fatiado em 4KB.
- **Futuro (throughput, nao-bug):** pra requests multi-pagina, acoplar `max_io_buf_bytes`(ADD_DEV)
  ↔ `max_sectors`(SET_PARAMS) ↔ `buf_size`(servidor) e testar I/O grande. Nao feito no MVP (ganho
  incerto no WSL2; atual correto). Documentado em SPEC §12 / IMPL.
- **Estado:** Fase B = VRAM+swap+bench(>NBD)+no-alloc+qd>1(read&write). Branch `feat/fase-b-prep`,
  ~86 commits. Usuario: "nada de PR agora, continue" — seguir endurecendo, sem propor PR.

---

## 2026-06-09 — Fase B Frente A: requests multi-pagina (feito, TDD)

- **Frente A do "continue todas as frentes": requests >4KB.** `Params::with_max_sectors(n)` (builder
  imutavel, commit `3cf690e`) seta `basic.max_sectors` -> kernel usa como `max_hw_sectors`
  (`ublk_drv.c:546`), validado `<= max_io_buf_bytes>>9` (581). Acopla os 3 knobs:
  `max_io_buf_bytes`(ADD_DEV) ↔ `max_sectors`(SET_PARAMS) ↔ `buf_size`(servidor por-tag). Invariante
  dura: `buf_size >= max_sectors*512`. `smoke_auto` mantem 4KB como default seguro.
- **TDD RED->GREEN:** teste `dt3_vram_serves_multipage_request` usava `.with_max_sectors` (nao
  existia -> compile fail RED), depois GREEN. Device 128KB -> `max_hw_sectors_kb=128`; WRITE+READ
  O_DIRECT de **64KB real** (len=65536 > 4096) servido da VRAM num request, integridade OK (RTX
  2060). Pool no-alloc ja dimensiona o buffer por tag a buf_size -> request grande passa sem alloc.
- **GOTCHA real (custou 1 timeout 124):** o teste travou no TEARDOWN, nao na feature (markers
  provaram que write/read 64KB passaram). Causa: `del_gendisk` (no STOP_DEV) **bloqueia ate todos
  os openers do block device fecharem**; o teste mantinha o fd O_DIRECT aberto. Fix: `drop(file)`
  antes do `stop_dev`. Os outros smokes fecham o File dentro do helper (read_block/write_block), por
  isso nunca bateram nisso. **Regra:** fechar fd de /dev/ublkbN antes do STOP_DEV.
- **Frente B (integracao no daemon):** PROXIMA, via SSDV3 PRD (impl direto e proibido pela
  disciplina; ha conflito de contexto CUDA worker DT-3 vs canario/residencia do main.rs).
- **Estado:** Fase B = VRAM+swap+bench(>NBD)+no-alloc+qd>1(r/w)+multipagina. ~88 commits.

---

## 2026-06-09 — Fase B Frente B: PRD+SPEC da integracao no daemon (gate SSDV3)

- **Frente B = integrar o ublk no daemon `main.rs`** (hoje NBD-only; ublk so roda em teste). Mudanca
  ESTRUTURAL -> a disciplina SSDV3 PROIBE impl direto; entreguei PRD+SPEC e PAREI no gate de
  aprovacao (IMPL sem SPEC aprovado e Don't).
- **Decisao central (PRD, commit `f3a5f7a`):** conflito de afinidade CUDA. No NBD a thread que serve
  E dona do contexto (canario/residencia trivial). No ublk DT-3 a dona do contexto e a thread
  WORKER, mas o loop de canario/demote vive na thread principal do main.rs. **Opcao 1 (recomendada):
  mover a maquina de residencia para DENTRO do worker DT-3** (a thread dona do ctx serve E se
  auto-monitora). Rejeitadas: Opcao 2 (refazer lifetimes do ramshared-cuda, grande/arriscado),
  Opcao 3 (2o contexto so p/ canario, incoerente com o sinal de latencia que nasce no serve).
- **SPEC (commit a seguir):** docs/ublk-daemon-integration/{PRD,SPEC}.md. F1: novo
  `spawn_server_dt3_vram_with_residency` + `worker_loop_with_residency` (reusa Canary/
  ResidencySampler/CanaryProbe/Cadence/spawn_swapoff; refactor: extrair `spawn_swapoff` do main.rs
  p/ `src/swap.rs`). F2: `--transport ublk` no main.rs. F3: swap e2e pelo daemon + bench. Parte
  sensivel: gatilho DETERMINISTICO de DEMOTE no smoke (preferir ResidencyConfig com limiar explicito
  a depender de eviction WDDM real).
- **PROXIMO PASSO (quando retomar):** implementar F1 via TDD (RED: smoke que forca DEMOTE sintetico;
  GREEN: worker-com-residencia). So depois F2/F3. Nao comecar F1 sem o gate, e melhor numa sessao
  focada (contexto ja longo).
- **Estado Fase B:** VRAM+swap+bench(>NBD)+no-alloc+qd>1(r/w)+multipagina TODOS feitos/validados em
  hardware; integracao no daemon DESENHADA (PRD+SPEC). ~90 commits. Usuario: "nada de PR agora".

---

## 2026-06-09 — Frente B F1: worker DT-3 com residencia (feito, TDD)

- **F1 da integracao no daemon FEITO** (commit `31f8395`, RF-3). Opcao 1 do PRD: a maquina de
  residencia (canario §9 latencia + sonda §9.4 conteudo/free + DEMOTE/swapoff) roda DENTRO do worker
  DT-3, que ja e a thread dona do contexto CUDA (DeviceMem !Send) -> resolve afinidade, zero CUDA
  cross-thread.
- **Refactor de reuso:** `src/swap.rs` (novo) extrai `spawn_swapoff`/`swapoff_bin` do `main.rs`;
  ambos transportes usam `crate::swap`. Caminho NBD intocado.
- **API nova (`ublk_server.rs`):** `spawn_server_dt3_vram_with_residency(char_path, qd, buf_size,
  vram_bytes, block_size, swap_dev:String, residency:ResidencyConfig) -> ServerHandleDt3VramResidency`.
  O worker constroi canary_region+CanaryProbe e roda o loop inline (espelha o worker NBD do main.rs).
  `ServerHandleDt3VramResidency::demote_count()` (Arc<AtomicU32>) torna o DEMOTE observavel SEM swap
  real.
- **Gatilho sintetico determinISTICO de DEMOTE** (reusavel): `ResidencyConfig{latency_mult:0,
  consecutive:1, free_floor_bytes:0}` -> limiar=baseline*0=0, todo serve real (lat>0) dispara apos a
  baseline (16 amostras). Smoke `dt3_vram_residency_triggers_demote_synthetic` (root+GPU): swapoff de
  swap_dev inexistente falha (esperado) mas demote_count>=1. /dev limpo.
- **PROXIMO: F2** = `--transport ublk` no main.rs (mlockall+oom reuso -> ADD/SET_PARAMS ->
  spawn_..._with_residency -> START -> aguarda SINAL (SIGINT/SIGTERM via flag) -> fecha fds -> STOP
  -> join -> DEL). Ponto sensivel: ciclo de vida do daemon (sinal) + teardown ordenado (del_gendisk
  espera openers). Depois F3 (swap e2e pelo daemon + bench). Nao comecado: melhor em sessao focada
  (signal handling + smoke a nivel de processo).
- **Estado Fase B:** A(multipagina)+B-F1 feitos/validados; B-F2/F3 desenhados. ~93 commits.

---

## 2026-06-09 — Frente B F2: daemon ublk CONGELOU o WSL2; travado por 2 gates

- **F2 (`--transport ublk` no main.rs) escrito mas NAO validado.** Rodar o smoke de processo
  (`daemon_ublk_serves_and_terminates_on_signal`, sobe o daemon + SIGTERM) **CONGELOU o WSL2** (~8min
  hang -> reboot forcado). Mecanismo: teardown nao fechou limpo -> `kill` deixou `/dev/ublkbN` SEM
  servidor com I/O em voo -> D-state no caminho de writeback + `mlockall(MCL_FUTURE)` + `drop_caches`
  -> stall global. Causa-raiz (bug STOP_DEV/join vs corrida SIGTERM-tarde->SIGKILL) so depuravel em
  qemu.
- **Pos-incidente:** WSL2 reiniciou limpo (sem device/daemon/D-state, swap zerado). O reboot tambem
  corrompeu artefatos de `target/` (E0786 "invalid metadata") -> `cargo clean -p ramshared-wsl2d -p
  ramshared-uring` + rebuild `-j2` (5.5s) resolveu. **Build nunca travou; so a EXECUCAO do daemon.**
- **DUAS TRAVAS independentes (default = trancado):** (1) teste pula sem
  `RAMSHARED_DANGEROUS_DAEMON_SMOKE=1`; (2) `run_ublk` chama `guard_not_wsl2()` que RECUSA servir se
  osrelease tem `microsoft`/`wsl`, a menos de `RAMSHARED_ALLOW_UBLK_ON_WSL2=1`. Smoke perdeu o
  `drop_page_cache()`. Memoria auto: [[feedback-no-standalone-daemon-smoke-wsl2]].
- **Tambem:** `wait_and_drain` agora faz retry em EINTR (sinal na thread do ring owner). NBD
  inalterado; `spawn_swapoff` ja extraido pra `swap.rs` (F1).
- **REGRA DURA:** nunca rodar o daemon ublk standalone / smoke de processo no WSL2. Validar F2/F3 so
  em qemu. Smokes in-process (DeviceGuard, <1s) seguem seguros.
- **Estado:** A(multipagina)+B-F1 validados em hw; B-F2 codigo escrito+travado, valida em qemu.

---

## 2026-06-09 — Frente B F2: modo --backend ram (pre-req pra validar em qemu)

- **`--backend {vram,ram}` no daemon ublk** (commit a seguir). VRAM = caminho atual (worker dono do
  ctx CUDA + residencia). RAM = `spawn_server_dt3` com `RamBackend` (sem GPU, sem residencia) —
  existe so pra validar o **ciclo de vida/teardown** do daemon em **qemu** (sem GPU; o bug de
  teardown e independente do backend). `run_ublk` faz branch via `BackendKind`/`UblkHandle` (une os
  dois tipos de handle pro teardown unico stop_dev->join->del). `--transport ublk --backend ram`.
- **Seguro:** so codigo, compila+clippy -j2 OK, 40 nao-root verdes, NAO rodado no WSL2 (gated por
  guard_not_wsl2 + a trava do smoke). Build nunca trava; so a execucao do daemon.
- **Falta pra fechar F2:** so o rootfs/harness qemu (estender qemu-validate.sh com o daemon
  RAM-backed + insmod ublk_drv + script de ciclo dd+SIGTERM dentro da VM). Recipe no IMPL.md.
- **Estado:** A+F1 validados em hw; F2 codigo-completo (+ RAM mode) e travado/analisado; falta qemu.
  ~95 commits. Regra dura: nada de daemon standalone no WSL2 [[feedback-no-standalone-daemon-smoke-wsl2]].

---

## 2026-06-09 — Frente B F2: VALIDADO em qemu (teardown limpo; freeze era do harness)

- **`scripts/kernel/qemu-ublk-daemon.sh` rodou e PASSOU.** Boota uma VM efemera (RAM-only, sem disco)
  com o kernel WSL2 + initramfs throwaway: insmod ublk_drv -> daemon `--backend ram` sobe -> cria
  /dev/ublkb0 -> serve dd 4KB (KTEST-SERVED=ok) -> **SIGTERM -> teardown limpo** (KTEST-TERMINATED=ok)
  -> device removido (KTEST-DEVICE-REMOVED=ok). Host intacto (uptime sem reboot, /dev limpo).
- **ACHADO PRINCIPAL: o teardown do daemon F2 e SOLIDO.** O freeze do WSL2 NAO foi bug do daemon —
  foi a **corrida SIGKILL do harness de teste** (`wait_child(15s)` -> `child.kill()`). Com SIGTERM
  limpo na VM, STOP_DEV->join->DEL_DEV fecha certo. Confirma a analise por inspecao.
- **Viabilidade qemu (confirmada):** `ramshared-cuda` carrega o driver via **dlopen** (ffi.rs); `ldd`
  do daemon = so libc/libgcc/ld-linux (ZERO CUDA no load). Logo `--backend ram` roda em VM sem GPU.
  Pre-reqs presentes: qemu-system-x86_64, busybox, /dev/kvm, bzImage, ublk_drv.ko.
- **Harness e NAO-DESTRUTIVO** (mesmo padrao do qemu-validate.sh): so `-kernel`+`-initrd` (sem -hda),
  bzImage/ublk_drv.ko so lidos, mktemp+trap limpa, kernel tree git-limpo. **Nao toca a VM real, o
  kernel armado no .wslconfig, nem CI.** Rodar via `sudo bash scripts/kernel/qemu-ublk-daemon.sh`.
- **Estado:** Fase B — VRAM/swap/bench/no-alloc/qd>1/multipagina (host) + F1 + **F2 ciclo validado
  em qemu**. Resta F3 (swap e2e + bench, no mesmo harness estendido). ~96 commits. Daemon segue gated
  no WSL2 (SIGKILL/crash ainda orfanaria) [[feedback-no-standalone-daemon-smoke-wsl2]].

---

## 2026-06-09 — Memory Broker: PRD unificado final (consolida arbiter + Fase C + visao)

- **Contexto novo (conversa Emerson↔Alex Santos):** Fase C = RAM-as-VRAM pro DCC (Blender/Cycles).
  Dor real do tester (Alex, artista 3D, Windows): cena > VRAM -> dias otimizando a mao com RAM
  ociosa. Mercado: addon Blender (SuperHive). Alex confirmado como tester ("eu testo de boa").
- **Pedidos do usuario na sequencia:** (1) ramshared na civm como no wsl2 + "quem precisa mais"
  -> PRD vram-arbiter (`439b461`); (2) "leve tudo em consideracao pro PRD e vamos discutir" ->
  PRD dcc-out-of-core; (3) "uma coisa so que resolva tudo (windows, vm, wsl2)" + "instalavel/exe"
  + "qualquer placa de video" -> VISION; (4) **"avalie tudo gerado e crie um unico final de onde
  sai a SPEC"** -> `docs/memory-broker/PRD.md` (PRD UNIFICADO FINAL).
- **Decisoes do PRD unificado:** plataforma protocol-first (um broker/arbitro por host, agentes por
  ambiente); primitivo = **lease de VRAM revogavel** (revogacao = DEMOTE ja construido — e o que
  une swap-tier e DCC); mecanismos nativos por consumidor (Linux=block device pronto; DCC=out-of-
  core; sem driver Windows de swap por ora); **qualquer GPU** via trait `VramProvider` (CUDA pronto
  -> Vulkan -> D3D12/dxg pesquisa); produto instalavel (exe/winget + deb/systemd + addon).
  Personas: dev (EMEDEV: wsl2+civm, cerebro no WSL2) e artista (Alex: Windows puro, cerebro =
  servico Windows). Fases P0(medicao)->P1(broker linux<->linux)->P2(ponte windows+MVP addon)->
  P3(vulkan)->P4(gated: interposer v2 etc), com gates anti-halo numericos.
- **Riscos top:** R2 D-state em tenant remoto com broker morto (ja nos mordeu; RNF-1 watchdog +
  prioridade + drill obrigatorio no P1 em qemu); R1 NAT do WSL2 (P0 mede; Tailscale).
- **PROXIMO PASSO: SPEC** (`docs/memory-broker/SPEC.md`) a partir do PRD unificado, apos discussao/
  aprovacao do usuario. F3 do ublk-daemon (swap no harness qemu) segue na fila tambem.
- Avaliacao critica dos docs de origem: Anexo A do PRD unificado. PRDs de origem marcados ABSORVIDOS.

---

## 2026-06-09 — Memory Broker: SPEC (SSDV3 Passo 2) gerado de docs/memory-broker/PRD.md

- **Artefato:** `docs/memory-broker/SPEC.md` (primeira execucao do Passo 2 — nao havia SPEC
  anterior, sem SPECv2). Escopo fechado: **P0 (medicao, gate) + P1 (broker core linux<->linux)**;
  P2/P3/P4 explicitamente fora (RF-W*, RF-G*, RF-P1, RF-P3/TOML adiado — DT-11).
- **Decisoes tecnicas chave (DT-1..15):** protocolo agente<->broker = **JSON-lines/TCP**
  (serde/serde_json; exige ADR-0005 + LIBRARIES.md, disciplina #11); broker **in-process** no
  daemon via `--arbiter-listen`; **slices = exports NBD nomeados** s0..sK-1 (ublk fica
  single-device — guard_not_wsl2/incidente 2026-06-09); worker CUDA unico permanece, slice
  resolvida via `SliceView` novo em backend.rs (DeviceMem ja aceita offset — RF-L1 respondido);
  binario renomeado **ramsharedd** ([[bin]], crate dir fica); atribuicao inicial round-robin
  (kernel gateia uso via prioridade de swap); swapon remoto **sem -p** (prioridade negativa
  abaixo do swap local — RNF-1); nbd-client sempre `-timeout 30`, nunca `-persist`; estado do
  broker em memoria, reconciliado no Register; agente exige euid==0.
- **Crates novos:** `ramshared-broker` (protocol/model/slices/arbiter puro, forbid unsafe) e
  `ramshared-agent` (psi/swap/watchdog, forbid unsafe). Modifica: handshake.rs (exports
  nomeados, assinatura `&[Export]` -> indice), conn.rs (streams genericos + acceptor TCP +
  Job.export), main.rs (flags --slices/--slice-mb/--listen-nbd/--arbiter-listen; recusa
  0.0.0.0; --backend ram no caminho NBD p/ drill sem GPU).
- **Gates:** P0-RESULTS.md preenchido (n>=3 rodadas) antes de qualquer codigo P1; drill
  D-state em qemu (`scripts/kernel/qemu-broker-drill.sh`, watchdog <5s) e e2e real WSL2<->civm
  fecham P1. Defaults do arbitro sao provisorios (delta_psi=15, streak=5, cooldown=60s) e
  **P0 calibra** (recalibracao = update do SPEC).
- **PROXIMO PASSO:** revisao/aprovacao do SPEC pelo usuario (ou Passo 2.5 auditoria) -> IMPL
  comeca pelo ITEM-1 (scripts p0). F3 do ublk-daemon segue na fila.

---

## 2026-06-09 — Memory Broker: Passo 2.5 sobre SPEC.md → NO-GO → SPECv2

- **Auditoria pre-implementacao do `docs/memory-broker/SPEC.md` = no-go.** 17 findings
  (2 CRITICAL, 5 HIGH, 4 MEDIUM, 6 LOW), verificados contra o codebase (nao so leitura do doc):
  - **F1 CRITICAL: agente sem `mkswap`** — `swapon` exige assinatura; todo caminho validado do
    repo roda mkswap antes (`cascade.rs:310`, `ublk_io_smoke.rs:671`); o fluxo SwapOn do SPEC
    nunca funcionaria e o drill (gate P1) nunca passaria.
  - **F2 CRITICAL: slice re-atribuida sem zerar** vaza paginas swapped (memoria anonima) entre
    tenants; `DeviceMem::zero()` e whole-buffer (`driver.rs:237`) → zero por slice via write_at.
  - HIGH: heartbeat do watchdog nao obrigatorio (Ack por Psi indefinido → watchdog expiraria
    sempre); drill validava happy path #13 (slice "ativa" com used_kb=0; initramfs sem
    nbd.ko/nbd-client/`lo up`); lease sem estado (round-robin re-atribuiria slices arrendadas);
    tenant ausente → slice presa em Draining; shutdown ordenado sem teste algum.
- **`docs/memory-broker/SPECv2.md` criado no mesmo turno** (regra do Passo 2.5), SPEC.md
  preservado. Novos DT-16..DT-26: mkswap obrigatorio; higiene de slice (WMsg::ZeroExport no
  worker, Draining→zero→Free); Ack por Psi (1 Hz, deadline 3s = 3 perdidos); SliceState::Leased;
  tenant ausente = slices congeladas fora da view do arbitro; endpoints Unix+TCP por transport;
  counterfactual com piso psi_floor; writer thread por sessao (core nunca faz IO de socket);
  contrato `{base}{N}` da reconciliacao; euid via /proc/self/status (zero-dep). Drill virou
  3 fases (graceful SIGTERM / kill swap-vazio <5s / kill swap-USADO: attempt<5s + sem D-state
  >10s + echo<2s — swapoff PODE falhar com EIO, objetivo e bounded sem D-state). e2e ganhou
  shutdown/heartbeat/higiene/ausencia.
- **Verificado nesta sessao:** CONFIG_PSI=y default-enabled no kernel custom + /proc/pressure
  legivel no WSL2; CONFIG_BLK_DEV_NBD=m com nbd.ko ja compilado; host tem nbd-client+fio,
  NAO tem nbdkit/nbd-server (preflight no measure-nbd-tcp.sh).
- **Candidato ativo: SPECv2.md → re-auditoria (Passo 2.5) antes do IMPL.** Nada commitado.

---

## 2026-06-13 — Memory Broker: re-auditoria do SPECv2 (Opus 4.8) → NO-GO → update in-place

- **Passo 2.5 sobre `docs/memory-broker/SPECv2.md` (candidato ativo) = no-go.** 6 findings novos
  (R1..R6), aterrados no codigo (conn.rs/main.rs/driver.rs), nao so no texto:
  - **R1 HIGH: agente executava comando (nbd_connect/mkswap/swapon) SINCRONO no loop de
    heartbeat** → um SwapOn >3s (latencia de mkswap/nbd_connect sobre TCP e NAO medida) starva
    o Psi/Ack e dispara o watchdog → swapoff espurio da slice recem-montada (anti-RNF-1/RNF-3).
    Fix DT-27: exec em thread propria (espelha spawn_swapoff); watchdog mede liveness do broker,
    nao duracao de comando. ITEM-9 reescrito com 2 threads.
  - **R2 MEDIUM: corrida de reserva do lease** — durante revogacao multi-tick, slice liberada
    (Free) podia ser pega pelo round-robin (passo 5) antes do GrantLease → lease nunca acumula.
    Fix DT-19 emendado: reserva incremental (Free→Leased na hora) + passo (5) suprimido sob
    pending_lease; ITEM-4/ITEM-10 exigem teste de lease QUE PRECISA REVOGAR (nao so de Free).
  - **R3 MEDIUM: worker sem geometria** — Export{name,size} nao carrega base; worker precisa de
    base p/ SliceView (Job E ZeroExport). Fix: worker mantem geom: Vec<(base,len)> de SliceMap;
    block::Export fica so name+size.
  - **R4 MEDIUM: ZeroExport via try_send (jobs bound 64) sem retry se cheio** → slice presa em
    Draining. Fix DT-17 emendado: try_send falho = Draining + retry no proximo tick.
  - **R5/R6 LOW: drill/runbook** (socket Unix default /run/ramshared inexistente no initramfs →
    bind falha; nbds_max p/ /dev/nbdN; parse estado-D apos ultimo `)` de /proc/stat) + worker
    bloqueado no zero de slice grande (aceitavel).
- **SPECv2.md atualizado IN-PLACE no mesmo turno** (regra de saida do Passo 2.5 p/ SPECv2 no-go):
  registro das 2 auditorias no topo (F1..F17 + R1..R6), +DT-27, emendas DT-17/DT-19 e
  ITEM-4/7/8/9/11/12. 27 DTs no total. SPEC.md original intacto.
- **Aterramento desta sessao:** WMsg = {Opened, Job(Job), Closed} (conn.rs:42); worker NBD =
  `while let Ok(msg)=jobs_rx.recv()` (main.rs:229) + try_recv no demote (260) → valida o idiom
  pending_zeros; --sock tem default /run/ramshared/wsl2d.sock (main.rs:106); DeviceMem::zero e
  whole-buffer (driver.rs:237) → zero por slice via write_at.
- **Candidato ativo: SPECv2.md (atualizado) → nova re-auditoria antes do IMPL.** Nada commitado.

---

## 2026-06-13 — Memory Broker: 3ª auditoria do SPECv2 → NO-GO → update in-place (R7 grave)

- **3ª passada (Passo 2.5) sobre SPECv2 atualizado = no-go.** 3 findings novos (R7..R9), foco no
  ciclo de vida do worker — aterrado lendo conn.rs/main.rs:
  - **R7 HIGH (grave): worker do broker morreria após um DemoteAll/idle.** O worker reusa o
    pipeline H1 e encerra via `LiveCount::on_close` quando `live==0 && opened`
    (conn.rs:70-73, main.rs:235-238 — DT-15 determinismo do modo single). Em modo broker o
    daemon e PERSISTENTE: as conexoes NBD caem a zero a cada `DemoteAll` (canario/GPU) ou quando
    todas as slices ficam Free (idle) → o worker encerraria e o daemon pararia de servir qualquer
    SwapOn futuro = **falha permanente apos um demote normal**. Nenhuma das 2 auditorias anteriores
    pegou (e um reuso de lifecycle num contexto novo). Fix DT-28: em modo broker o worker ignora o
    break do LiveCount, usa recv_timeout (tick) p/ checar shutdown, e so encerra no fechamento do
    canal `jobs` (shutdown ordenado: acceptors param + broker dropa o SyncSender). ITEM-8(e) +
    ITEM-10 cenario (j) (injeta DemoteAll, prova worker vivo).
  - **R8 MEDIUM:** agente com 2 threads (DT-27) sem escritor unico do socket → SwapOnDone (exec)
    e Psi (main) intercalariam bytes → JSON-lines corrompido. Fix: exec devolve resultado por
    canal; loop principal e o unico escritor.
  - **R9 LOW:** rebalanco (passos 2/4 do tick) nao suprimido sob lease pendente → churn. Fix:
    suprimir 2/4 enquanto ha pending_lease.
- **SPECv2.md atualizado IN-PLACE de novo** (regra de saida, sem pedido de SPECv3): topo com as 3
  auditorias (F1..F17, R1..R6, R7..R9), +DT-28, emendas DT-27/ITEM-4/8/10. **28 DTs no total.**
- **Padrao observado:** 3 no-go consecutivos, mas cada um com achados reais e distintos so
  visiveis com grounding no codigo (1ª: mkswap/zero/heartbeat/drill; 2ª: command-starvation/
  lease-race/geom; 3ª: worker-lifecycle/socket-writer). Convergencia esperada na proxima passada.
- **Candidato ativo: SPECv2.md → 4ª re-auditoria antes do IMPL.** Nada commitado.

---

## 2026-06-13 — Memory Broker: 4ª auditoria = GO + Passo 3 ITEM-1 (P0) iniciado

- **4ª passada (Passo 2.5) sobre SPECv2 = GO.** Só 2 findings LOW (clarificações, sem decisão
  arquitetural nova), dobrados in-place:
  - **R10 LOW:** shutdown ambíguo (ZeroExport por slice vs zero whole-buffer do teardown). Fix
    DT-17: shutdown usa o zero whole-buffer do teardown (F2), sem ZeroExport por slice → worker
    encerra limpo (DT-28); 10s = backstop.
  - **R11 LOW:** reconciliação (DT-21) assumia `--nbd-dev-base` que o Register não carrega
    (default /dev/nbd já funcionava; override quebraria → risco de re-atribuir slice montada).
    Fix DT-21: broker reconcilia pelo **inteiro final** de SwapEntry.dev (= id da slice),
    agnóstico ao prefixo, sem novo campo no protocolo.
- **Convergência real:** severidade caiu a cada passada (2 CRIT+5 HIGH → 1 HIGH+3 MED → 1 HIGH+1
  MED+1 LOW → 2 LOW). SPECv2 final: **28 DTs**, 11 findings (R1..R11) endereçados, 4 tabelas de
  auditoria no topo. SPEC.md original intacto.
- **Passo 3 INICIADO — ITEM-1 (gate P0), o único item liberado antes de fechar o gate:**
  criados `scripts/p0/{measure-psi.sh,measure-net.sh,measure-nbd-tcp.sh,measure-render-vram.ps1}`
  (set -euo pipefail, log prefix, preflights F17/R5) + `docs/memory-broker/P0-RESULTS.md`
  (template do gate). measure-psi.sh **rodado no WSL2 (6s)**: idle some.avg10=0.00 → confirma
  psi_floor=5.0 com folga + valida o harness. Gate **FECHADO** (preliminar só; faltam ≥3×300s,
  civm, rede, NBD/TCP, render).
- **Gate anti-halo:** ITEM-3+ (código P1, crate ramshared-broker etc.) **não** começa até
  P0-RESULTS fechar. ITEM-2 (ADR-0005 serde) pode andar em paralelo (é doc). Execução de P0:
  WSL2 carga (cargo build -j4, escopado), civm (SSH/Tailscale — confirmar PSI + PAGE_SIZE),
  NBD/TCP (precisa `apt install nbdkit`), render (host EMEDEV → tester Alex).
- **Nada commitado** (batch local, [[feedback-batch-local-single-pr]]). Branch feat/fase-b-prep.

---

## 2026-06-13 — Memory Broker P0: execução (R1 medido, civm confirmada) + ITEM-2 (ADR-0005)

- **R1 (conectividade WSL2↔civm) MEDIDO e decidido** — números reais (scripts/p0/measure-net.sh +
  ssh read-only na civm `gha-ubuntu-2404`):
  - WSL2→civm **LAN** (192.168.0.50): RTT p50=0.375ms p99=0.849ms (apertado), TCP ok.
  - WSL2→civm **Tailscale** (100.123.103.106): p50=1.02ms **p99=430ms** (cauda péssima).
  - civm→WSL2 **NAT (172.31.230.209): 100% perda** (confirmado por ping; `ip route get` na civm
    manda 172.31.x pro gateway LAN). **WSL2 NÃO é nó Tailscale** (nenhum IP TS por nenhum método).
  - **DECISÃO R1 = port-forward no host Windows** (`netsh portproxy` LAN:porta→172.31.230.209:porta
    p/ --arbiter-listen e --listen-nbd). Tailscale-no-host inviável p/ data-plane (430ms vs swap
    241-326µs Fase B). WSL2 gw/host vNIC = 172.31.224.1. Alimenta DT-25 + ITEM-12.
- **civm confirmada (read-only via ssh):** kernel 6.8.0-124, **PSI habilitado** (some/full
  legíveis; some.avg10 0.5–7.8 conforme carga de CI; full.avg10 chegou a 7.75 = stall real),
  **PAGE_SIZE=4096** (= WSL2). civm acessível por ssh sem senha (BatchMode ok).
- **Calibração (achado #3):** civm sob CI ~7.8 vs WSL2 idle ~0 ⇒ Δ≈7.8 < `delta_psi=15` default
  → árbitro **nunca moveria** sob carga típica de CI. **delta_psi=15 suspeito de ALTO**; confirmar
  com PSI-sob-carga antes de fixar (registrado em P0-RESULTS §5).
- **ITEM-2 FEITO** (doc, paralelo ao gate): `docs/decisions/ADR-0005-broker-protocol-jsonl.md`
  (JSON-lines via serde/serde_json; rollback trigger: >64KiB/msg ou >100msg/s/tenant → bincode) +
  linha em LIBRARIES.md. Versões reais do registry: **serde 1.0.228 / serde_json 1.0.150**
  (MIT OR Apache-2.0). Pin exato + transitivas entram no Cargo.lock no ITEM-3.
- **Coletas concluídas (números no P0-RESULTS):** PSI idle **WSL2 ~0.00–0.05** (3×300s, max 0.55);
  PSI **civm idle/CI méd 1.4–2.1, burst 19.4** (full.avg10 7.75 = stall CI real); **NBD/TCP
  loopback** randread p50≈174µs/p99 285–578µs, randwrite p50≈202µs (3 rodadas) — já bate NBD-Unix
  326µs da Fase B, mas é loopback. **Cross-host civm↔WSL2 sobre port-forward = o R4 de verdade,
  ainda falta.**

---

## 2026-06-13 — Memory Broker P0: PSI sob carga (cgroup hog seguro) + calibração delta_psi

- **PSI-sob-carga do WSL2 medido com segurança** via novo `scripts/p0/measure-psi-load.sh` (hog
  anônimo confinado em cgroup v2: `memory.high=64M`/`memory.max=512M` teto/`swap.max=0`). Rodou
  40s, **0 OOM, cgroup limpo, host vivo** (8.6 GB livres depois). Resultado: **WSL2 sob carga
  system some.avg10 média=14.25 max=22.54** (full 10.2/18.3). cgroup ficou throttled em ~72 MB
  (3029 throttles) — pressão real e contida. NADA de swap/daemon/block-device (longe do freeze
  de 2026-06-09). **Achado de metodologia:** o "cargo build -j4" do SPEC é CPU-bound, NÃO gera
  PSI de memória → substituído pelo hog confinado (documentado no P0-RESULTS §1).
- **Idle final (3 rodadas):** WSL2 some.avg10 **0.011** (831 am.); civm **1.237** (806 am., max
  19.44 = burst de CI).
- **Calibração delta_psi (P0-RESULTS §5):** WSL2 carga 14.25 vs civm idle 1.2 ⇒ Δ≈13 → o default
  **delta_psi=15 NÃO moveria** sob pressão clara. **Proposta: delta_psi=10** (+ streak=5 filtra
  os bursts transientes de CI da civm; psi_floor=5 separa idle<5 de carga≥14). Validar no e2e P1
  (ITEM-12). Caveat #1: carga confinada é lower bound do PSI real do host (real ≥14). A troca do
  default no SPEC acontece no ITEM-4 (quando o árbitro for codado), citando este P0-RESULTS.
- **Gate P0 quase fechado:** §1 (PSI idle+carga), §2 (rede+port-forward), §3 loopback, §5
  calibração = FEITOS. **Faltam só §3 NBD/TCP cross-host (precisa netsh portproxy no host Windows)
  e §4 render (tester Alex).** ITEM-3+ (código P1) segue bloqueado até esses dois.
- Novo script P0: `scripts/p0/measure-psi-load.sh` (reusável; serve p/ civm também se preciso).

---

## 2026-06-13 — Memory Broker P0: §3 cross-host (R4) medido via port-forward + §4 script validado

- **Usuário liberou PowerShell admin do host** (UAC off; `sudo.exe` em modo Embutido/inline eleva
  sem prompt — nível S-1-16-12288). Host LAN = **192.168.0.250**; WSL2 NAT = 172.31.230.209.
- **§3 NBD/TCP cross-host MEDIDO (o R4 real, antes só Inferência):** montei nbdkit no WSL2
  (userspace) + `netsh portproxy 0.0.0.0:10810→172.31.230.209:10810` + regra de firewall no host,
  e rodei nbd-client+fio **na civm** (3 rodadas, `-timeout 30`). **randread 4k p50=644µs p99~1.0–1.5ms;
  randwrite p50=644µs; IOPS ~1400.** = loopback (~174µs) + ~470µs do virt-switch (≈1 RTT LAN/op).
  644µs << swap em disco saturado (ms+) → **civm lucra usando VRAM remota como swap** (confirmado).
  Loopback NBD/TCP: randread p50~174µs (bate NBD-Unix 326µs da Fase B).
- **SEGURANÇA (não travou o WSL2):** as partes que podem dar D-state (nbd-client, /dev/nbd0, fio)
  rodaram **na civm**, não no WSL2; no WSL2 só nbdkit (userspace). **Tudo limpo após:** nbdkit morto,
  portproxy+firewall removidos, civm nbd0 size=0, sem nbd em /proc/swaps. (1 exit-144 no meio = só
  artefato de comando bash+powershell+ssh aninhado; estado verificado limpo, refeito passo-a-passo.)
- **§4 render: script validado no host + BUG corrigido** (regra "rodar no host primeiro", #13):
  `Get-Counter '\Memory\Available MBytes'` é localizado e **quebra em Windows pt-BR** → troquei por
  CIM `Win32_OperatingSystem.FreePhysicalMemory` (neutro). Agora captura VRAM/RAM ok. Falta só a
  cena real do Alex (gate §4 depende do tester).
- **Gate P0: falta SÓ §4 render** (precisa da cena do tester). §1/§2/§3/§5 fechados. Calibração:
  delta_psi=10 proposto. **ITEM-3+ (código P1) ainda bloqueado** até §4 (ou decisão de abrir o gate
  com §4 parcial, já que é da Fase C/P2 e o P1 é Linux↔Linux). civm tem nbd-client+fio instalados agora.

---

## 2026-06-13 — Memory Broker P1: gate P1 ABERTO + ITEM-3 e ITEM-4a (slices) implementados (TDD)

- **Gate P1 ABERTO** (decisão, autorizada por "continue todas as frentes"): os números que o P1
  precisa (§1/§2/§3/§5) estão completos; **§4 render é insumo da P2** (DCC/out-of-core), rastreado
  p/ quando o Alex mandar a cena — não bloqueia o P1 (Linux↔Linux). Marcado no P0-RESULTS.
- **Crate novo `ramshared-broker`** (workspace member; serde/serde_json 1.x via ADR-0005),
  `#![forbid(unsafe_code)]`, lints unwrap/expect=deny (testes com `#![allow(...)]`):
  - **ITEM-3 — `model.rs` + `protocol.rs`:** tipos do PRD §7 (SliceState c/ `Leased`, Slice,
    PsiSample, TransportKind, Lease) + JSON-lines (Msg internamente-etiquetado por `type`/snake_case,
    NbdEndpoint, SwapEntry, TenantStatus c/ `present`, `write_msg`/`read_msg` c/ teto 64 KiB
    anti-DoS via `take`). **Correção forçada:** `Slice` precisou de `PartialEq, Eq` (Msg deriva
    PartialEq e embute `Vec<Slice>`) — SPEC inconsistente, corrigido no código E no SPECv2.
  - **ITEM-4a — `slices.rs`:** `SliceMap` máquina de estados (new/total_bytes/get/assign/drain/
    release/lease/unlease/exports); transições ilegais rejeitadas (assign em Active **e em Leased**;
    DT-17 release só de Draining; DT-19 lease/unlease).
  - **Validado:** clippy `-D warnings` limpo, **20 testes** verdes (12 protocol/model + 8 slices),
    fmt ok. Build escopado `-p ramshared-broker -j4` (sem --release; não travou WSL2).
- **delta_psi recalibrado 15→10 no SPECv2** (citando P0-RESULTS §5; era a regra "recalibração =
  update do SPEC no ITEM-4").
- **PAREI antes do `arbiter.rs` de propósito** (cuidado, contexto longo): achei uma questão de
  design real — o `tick` é **puro** e recebe `TenantView{psi}`, **sem `used_kb` por slice**, então
  o "mais ociosas primeiro" do DT-19 não é implementável como escrito. Decisão registrada no SPECv2:
  P1 usa **`psi.avg10` do dono ascendente** como proxy; o critério por `used_kb` fica adiado
  (precisaria o core passar used_kb ao tick; DT-10: sem holder real de lease em P1). A reserva R2
  é do core (GrantLease→lease()) + supressão do passo 5 no árbitro.
- **PRÓXIMO:** `arbiter.rs` (ITEM-4b) — núcleo do RF-B2/B3 + counterfactual #2 (clock injetado,
  testes adversariais: histerese, cooldown, nunca-zero, RevertMove c/ piso DT-23, lease drena além
  do nunca-zero, round-robin só entre presentes). Merece sessão focada. Depois ITEM-5..12.
- **Nada commitado** (working tree; batch local, harness "commit só quando pedir"). Crate compila/testa.

---

## 2026-06-13 — Memory Broker P1: ITEM-4b arbiter.rs FEITO (crate ramshared-broker completo)

- **`arbiter.rs` implementado** (a questão de design foi resolvida, então segui no mesmo "continue"):
  `tick` puro (clock `Instant` injetado) com a ordem da SPECv2 — (1) lease (prioridade, suprime
  2/4/5), (2) counterfactual c/ **piso** (DT-23: só reverte se psi do drenado >2× E >psi_floor),
  (3) cooldown, (4) diferencial c/ histerese (streak) + **nunca-zero** (não drena donor pressionado
  a 0), (5) round-robin de Free (DT-6). `last_move` zera após revert (sem oscilação); `cooldown_until`
  separado (move→60s, revert→300s). **delta_psi=10** default (P0). Revogação de lease por **psi do
  dono ascendente** (proxy; sem used_kb no tick puro).
- **30 testes verdes** no crate (12 protocol/model + 8 slices + **10 arbiter**): histerese, cooldown,
  nunca-zero, counterfactual reverte E o caso ruído-abaixo-do-piso NÃO reverte (DT-23), lease drena
  além do nunca-zero (DT-8), lease concede de Free sem round-robin + segura Free durante revogação
  (R2), round-robin distribui Free, sem-diferencial-não-move. clippy `-D warnings` limpo, fmt ok.
- **1 refactor clippy** (let-chains, rustc 1.93) — sem mudança de lógica. **Correção SPEC** já
  registrada: Slice PartialEq/Eq; delta_psi 15→10.
- **Crate `ramshared-broker` COMPLETO** (ITEM-3 + ITEM-4): protocol/model/slices/arbiter, lib pura
  `#![forbid(unsafe_code)]`, **zero código existente tocado** (sem risco de regressão até aqui).
- **PRÓXIMO: ITEM-5** (`ramshared-block/handshake.rs`: exports nomeados, assinatura `export_size`→
  `&[Export]`→índice). É **mudança de contrato em código Fase-B existente** com risco de regressão
  (RNF-4: wire byte-idêntico p/ cliente sem `-N`; abort trigger = teste existente quebrar). Merece
  passo focado (não no fim de um contexto já enorme). Depois ITEM-6 (SliceView) → 7 (conn) → 8
  (broker_srv+main) → 9 (agent) → 10 (e2e) → 11 (drill qemu) → 12 (runbook).

---

## 2026-06-13 — Memory Broker P1: ITEM-5 (handshake exports nomeados) FEITO, workspace verde

- **ITEM-5 — `ramshared-block/src/handshake.rs`:** `server_handshake` agora resolve o export
  **pelo nome** e devolve o índice. Assinatura `(.., export_size: u64, ..) -> Result<()>` →
  `(.., exports: &[Export], ..) -> Result<usize>`. Novo `pub struct Export { name, size }`,
  const `NBD_REP_ERR_UNKNOWN=0x8000_0006`, helpers `go_export_name`/`name_utf8`/`find_export`.
  Resolução: **nome vazio → exports[0]** (wire byte-idêntico Fase B, RNF-4); GO/INFO nome
  desconhecido → `ERR_UNKNOWN` e **segue** negociando; EXPORT_NAME desconhecido → fecha (Io);
  nome não-UTF-8 → Io. GO+INFO unificados num braço (GO transiciona, INFO não).
- **Caller `conn.rs` (patch mínimo, ITEM-7 fará a tabela real):** passa
  `[Export{name:"default", size:device_size}]` e ignora o índice — mantém o crate compilando.
- **TDD da regressão (RNF-4):** os 5 testes existentes do handshake adaptados **só na assinatura**,
  asserts de bytes **intactos** (greeting/export-name/GO byte-idênticos). +5 testes novos: GO
  nomeado devolve índice+size, GO desconhecido → ERR_UNKNOWN+continua, EXPORT_NAME desconhecido
  fecha, nome não-UTF-8 erra, nome vazio → primeiro export.
- **TESTE TUDO (pedido do usuário) — `cargo test --workspace -j4` (debug): 0 falhas em todo o
  workspace.** block 23, broker 30, cli 11, wsl2d-lib 21 (+1 GPU ign), cuda 2 (+1 ign), tier 7,
  integrity 8, uring/ublk smokes root corretamente ignorados (5+12). clippy `-D warnings` limpo
  (block + wsl2d-lib), fmt ok. **Zero regressão** — o caminho NBD da Fase B intacto.
- **Progresso P1: ITEM-1(gate)/2/3/4/5 FEITOS.** Falta ITEM-6 (SliceView em backend.rs + mover
  RamBackend de ublk_server.rs) → 7 (conn genérico+TCP+ZeroExport) → 8 (broker_srv+main) → 9
  (agent) → 10 (e2e) → 11 (drill qemu) → 12 (runbook). ITEM-6 mexe em código existente (mover
  RamBackend) — risco de regressão moderado, melhor em passo focado. Nada commitado (working tree).

---

## 2026-06-13 — Memory Broker P1: ITEM-6 (SliceView + move RamBackend) FEITO, workspace verde

- **ITEM-6 — `ramshared-wsl2d/src/backend.rs`:**
  - **`SliceView<'b, B: BlockBackend>`** (RF-L1/DT-4): janela `[base,base+len)` sobre um backend;
    `size_bytes()=len` (o bounds-check de `serve()` passa a valer por slice de graça); read/write
    somam `base` com `checked_add` (overflow→IoError). O worker constrói uma `SliceView` por `Job`
    sobre o backend único, sem tocar CUDA.
  - **`RamBackend` MOVIDO** de `ublk_server.rs` para `backend.rs` (compartilhado NBD+ublk). Day-0
    sem alias de compat: os **8 usos** atualizados p/ o caminho canônico (`pub use
    backend::{RamBackend, SliceView, VramBackend}` no lib.rs → `ramshared_wsl2d::RamBackend`):
    main.rs ×2, tests/ublk_{worker,server,io_smoke}.rs ×6. `ublk_server.rs` importa de
    `crate::backend` e perdeu `IoError` do import (só o RamBackend usava).
- **3 testes novos de SliceView** (backend.rs): isolamento entre slices vizinhas (escrita na s1
  não vaza p/ s0), `serve` rejeita fora da janela (EINVAL), `new` panica em debug se a janela
  excede o backend.
- **TESTE TUDO: `cargo test --workspace` 0 falhas.** wsl2d-lib 21→**24** (3 SliceView), demais
  intactos (block 23, broker 30, cli 11, ...); ublk integration tests compilam com o novo caminho
  do RamBackend e os não-root passam; root/GPU ignorados. clippy lib+bin limpo, meus crates fmt ok.
- **Achados PRÉ-EXISTENTES (não meus, não corrigidos — fora do escopo do ITEM-6):** (1)
  `tests/ublk_control_smoke.rs` tem 15 `clippy::expect_used` que só aparecem sob
  `clippy --all-targets` (o gate do projeto sempre foi lib+bin, então nunca pegou); (2)
  `ramshared-cli/src/main.rs:1036` tem um diff de `cargo fmt` pré-existente. Ambos são gaps
  latentes a tratar separadamente.
- **Progresso P1: ITEM-1/2/3/4/5/6 FEITOS.** Falta ITEM-7 (conn.rs genérico sobre stream + acceptor
  TCP + `Job.export` + `WMsg::ZeroExport`) → 8 (broker_srv+main) → 9 (agent) → 10 (e2e) → 11 (drill)
  → 12 (runbook). ITEM-7 mexe no pipeline multi-conn H1 (worker/backpressure/LiveCount) — alto risco
  de regressão, passo focado. Nada commitado.

---

## 2026-06-13 — Memory Broker P1: ITEM-7 (conn.rs genérico Unix+TCP) FEITO, workspace verde

- **ITEM-7 — `ramshared-wsl2d/src/conn.rs`:** reader/writer/acceptor agora **genéricos sobre o
  stream** (Unix **e** TCP, RF-L2). Mudanças:
  - `spawn_writer<S: Write+Send+'static>`, `spawn_reader<S: Read+..., W2: Write+...>` (o `hs_writer`
    do handshake agora é passado pelo acceptor — Unix/TCP não têm trait comum de `try_clone`).
  - `spawn_reader` recebe `Arc<Vec<Export>>`, negocia o export, guarda o **índice** e o anti-DoS de
    WRITE passa a usar `exports[idx].size` (não mais `device_size`). `Job` ganhou `export: usize`.
  - Helper `wire_conn<RS,WS>` (Opened+canais+writer+reader) compartilhado pelos 2 acceptors.
  - **`spawn_acceptor`** (Unix) agora recebe `exports: Arc<Vec<Export>>`; **`spawn_acceptor_tcp`**
    novo (TcpListener, `TCP_NODELAY` por conexão, MESMO canal `jobs` — worker único).
  - `main.rs run_nbd`: passa `Arc::new(vec![Export{"default", device_size}])` ao `spawn_acceptor`
    (1 export; o broker passará a tabela do SliceMap no ITEM-8).
- **`WMsg::ZeroExport` DEFERIDO para o ITEM-8** (desvio do SPEC, registrado): é produzido pelo
  broker_srv e consumido pelo worker com SliceView — adicioná-lo agora exigiria um stub no worker
  single-mode que mente (diz "zerado" sem zerar). Vai junto no ITEM-8, onde produtor+consumidor
  existem. WMsg segue {Opened, Job, Closed}.
- **TESTE TUDO: `cargo test --workspace` 0 falhas.** Os testes do H1 (`LiveCount` término
  determinístico, `chan_cap` backpressure, `slow_writer_does_not_deadlock`, `job_reply_roundtrip`)
  **preservados** com os genéricos + `Job.export=0`. wsl2d-lib 24, demais intactos. clippy lib+bin
  limpo, meus crates fmt ok. Zero regressão no pipeline multi-conn.
- **Progresso P1: ITEM-1..7 FEITOS** (gate + protocol/model + slices/arbiter + handshake + SliceView/
  RamBackend + conn genérico). Falta **ITEM-8** (broker_srv.rs + run_nbd rework + WMsg::ZeroExport +
  --slices/--slice-mb/--listen-nbd/--arbiter-listen + SliceView por Job + rename `ramsharedd` +
  scripts/docs) — o capstone de integração, GRANDE, multi-passo, sessão focada. Depois 9 (agent) →
  10 (e2e) → 11 (drill) → 12 (runbook). Nada commitado (working tree; tudo compila/testa verde).

---

## 2026-06-13 — Memory Broker P1: ITEM-8 sub-peça 1 (validação de flags do broker) FEITA

- **ITEM-8 é o capstone (multi-arquivo, ~300+ linhas de concorrência)** — fatiado. **Sub-peça 1
  (a parte pura/testável + crítica de segurança):** `main.rs` ganhou as flags `--slices`,
  `--slice-mb`, `--listen-nbd`, `--arbiter-listen` (parsing manual) + 2 funções puras:
  - `parse_private_listen(&str) -> Result<SocketAddr,String>`: aceita `tcp://IP:PORT`, **recusa
    unspecified (0.0.0.0/::) ANTES de qualquer bind()** — RNF-2 / abort trigger #5.
  - `validate_slice_flags(slices, slice_mb, is_ublk)`: ublk+slices → Err (DT-3); slices>0 sem
    slice-mb → Err.
- **Gate WIP honesto:** se `--slices>0` ou `--arbiter-listen`/`--listen-nbd` forem dados, retorna
  erro claro "modo broker ainda não conectado ao runtime (ITEM-8 em progresso)" — sem stub que
  finge funcionar. O caminho single-mode (sem essas flags) segue **inalterado**.
- **5 testes novos** no binário (`mod tests` em main.rs, allow unwrap): loopback/LAN ok,
  0.0.0.0/`[::]`/`tcp://0.0.0.0` recusados, garbage/sem-porta recusados, ublk+slices recusado,
  slice-mb obrigatório. clippy lib+bin limpo, `cargo test --workspace` 0 falhas, fmt ok.
- **Falta do ITEM-8 (próximos recortes focados, precisam de mais contexto):** (1) `broker_srv.rs`
  — o core single-thread (CoreEvent loop, writer-thread por sessão/DT-24, forwarder de demote,
  heartbeat Ack, lease, reconciliação) — só validável de verdade com o e2e (ITEM-10), então fazer
  os dois juntos; (2) `WMsg::ZeroExport` em conn.rs + handling no worker do run_nbd (com SliceView
  por Job + geometria); (3) rework do run_nbd (spawn_broker, demote routing, BackendKind::Ram no
  NBD); (4) rename binário→`ramsharedd` ([[bin]] + log + qemu-ublk-daemon.sh + IMPL.md); (5)
  README/ARCHITECTURE/DEGRADATION-MATRIX.
- **Estado:** ITEM-1..7 + ITEM-8(parcial: flags) feitos/validados; broker lib completa; data-plane
  do daemon completo; CLI do broker validada. Nada commitado. Branch feat/fase-b-prep.

---

## 2026-06-13 — Memory Broker P1: ITEM-8 — BrokerCore (núcleo puro do broker) FEITO + testado

- **Decisão de design (disciplina #13):** separei o broker em **core PURO** (`BrokerCore`,
  testável sem threads/sockets/GPU, igual ao árbitro) + futura fina camada de IO (`spawn_broker`).
  Assim valido a LÓGICA de decisão com testes determinísticos — não só "compila".
- **`crates/ramshared-wsl2d/src/broker_srv.rs` (novo)** + dep `ramshared-broker` no wsl2d.
  `BrokerCore::handle(CoreEvent, now) -> Vec<Outbound>`:
  - `CoreEvent` = {Msg(sid,Msg), Disconnected(sid), ZeroDone(slice,ok), Demote(reason), Tick};
    `Outbound` = {ToSession(sid,Msg), CloseSession(sid), ZeroSlice{slice,base,len}, Log}.
  - Sessões: Register (id estável por nome/DT-22, duplicado→Error+Close, proto≠→Close); Psi→**Ack**
    (DT-18) + reconciliação no 1º Psi (DT-9/21, `dev_to_slice` pega inteiro final, agnóstico ao
    prefixo); Status→StatusReply; Disconnected→present=false (DT-20, slices congeladas).
  - Rebalanço: `Tick` chama `arbiter.tick` (só presentes + slices visíveis/DT-20) → AssignFree→
    SwapOn (round-robin), MoveSlice/RevertMove→drain+SwapOff+pending_dest. **Higiene DT-17:**
    SwapOffDone{ok}→ZeroSlice; ZeroDone{ok}→release→Free→(se pending_dest) assign+SwapOn ao destino.
  - Demote→DemoteAll a todos presentes. Endpoint por transporte (DT-25). `BTreeMap` p/ round-robin
    determinístico.
  - **Lease (RF-B3) DEFERIDO** p/ próximo recorte: `LeaseRequest`→`LeaseDenied` honesto.
- **11 testes puros** (register/dup/proto/psi-ack/round-robin-assign/swapoff→zero→release/
  move→swapon-no-destino/disconnect-congela/demote-broadcast/status/dev-parse). 1 refactor clippy
  (let-chains). clippy lib+bin limpo, **workspace 0 falhas**, wsl2d-lib 35 (24+11), fmt ok.
- **Falta do ITEM-8 (próximos recortes):** (1) camada IO `spawn_broker` (acceptor TCP + reader/
  writer por sessão + loop chamando BrokerCore + forwarder de demote + shutdown) — validável pelo
  e2e ITEM-10; (2) lease no BrokerCore; (3) `WMsg::ZeroExport` em conn.rs + worker do run_nbd
  executando o zero via SliceView + run_nbd com slices/spawn_broker/demote-routing; (4) rename
  `ramsharedd`; (5) scripts/docs. Depois ITEM-9 (agent), 10 (e2e), 11 (drill), 12 (runbook).
- **Estado:** broker lib + **núcleo de decisão do daemon** completos e testados; falta a fiação de
  IO/runtime + lease + agent + drill + runbook. Nada commitado. Branch feat/fase-b-prep.

---

## 2026-06-13 — Memory Broker P1: ITEM-8 — lease (RF-B3) no BrokerCore; decisão completa

- **Lease revogável (RF-B3/DT-19) no `BrokerCore`** (puro, testável): `LeaseRequest` → pending
  (negado se já há lease em andamento → `lease_em_andamento`, ou bytes > capacidade →
  `acima_da_capacidade`); `on_tick` passa `pending_lease` ao árbitro → `RevokeForLease` (drena+
  SwapOff; no ZeroDone vira Free **sem** pending_dest, e o árbitro o conta p/ o lease — R2) e
  `GrantLease` (Free→`lease()`→Leased + `LeaseGranted` ao holder); `LeaseRelease` → unlease todas
  (Free, round-robin re-arrenda); holder/requester desconecta → release automático (DT-19). O id
  do lease vem do árbitro (removi um campo `next_lease_id` que ficaria morto no core).
- **+6 testes de lease:** concede de Free; negado em-andamento; negado capacidade; release devolve;
  **revoga Active→zero→grant** (R2, o fluxo completo); release no disconnect do holder.
- **`BrokerCore` agora cobre TODA a decisão do broker (RF-B1/B2/B3/B4)** — **17 testes** puros
  determinísticos. clippy lib+bin limpo, `cargo test --workspace` 0 falhas, wsl2d-lib 41, fmt ok.
- **Decisão de disciplina:** fiz o lease (puro, 100% validável) em vez da camada de IO
  (`spawn_broker`, threading) porque no fim de um contexto enorme o certo é a peça determinística;
  a fiação de IO/threads merece contexto fresco.
- **Falta do ITEM-8:** (1) camada IO `spawn_broker` (acceptor TCP + reader/writer por sessão +
  loop chamando BrokerCore + forwarder demote/zero-done + shutdown) — validável pelo e2e (ITEM-10);
  (2) `WMsg::ZeroExport{base,len,done}` em conn.rs + worker do run_nbd zerando a janela via
  write_at (DT-17, sem stub) + run_nbd com slices/spawn_broker/demote-routing; (3) rename
  `ramsharedd`; (4) scripts/docs. Depois ITEM-9 (agent), 10 (e2e), 11 (drill), 12 (runbook).
- **Estado:** decisão do broker 100% feita+testada; resta fiação IO/runtime + agent + drill +
  runbook. Nada commitado. Branch feat/fase-b-prep.

---

## 2026-06-14 — Memory Broker P1: ITEM-8 spawn_broker (IO) + ITEM-10 (e2e) FEITOS

- **Camada de IO `spawn_broker`** (em broker_srv.rs): a casca de threads que roda o `BrokerCore`
  puro. `BrokerConfig{listen, endpoints, swap_prio, arbiter, tick}` + `spawn_broker(cfg, slice_map,
  demote_rx, jobs, shutdown: Arc<AtomicBool>) -> (JoinHandle, SocketAddr)` (devolve o addr ligado,
  útil c/ porta 0 em teste). Threads: acceptor TCP (nonblocking+poll, para no shutdown), reader/
  writer por sessão (writer = canal bounded 64/DT-24; ao fechar o canal, dá `shutdown(Both)` no
  socket → reader vê EOF), forwarder de DEMOTE→CoreEvent, e o core-loop (`recv_timeout(tick)`=Tick;
  dispatch: ToSession via try_send/derruba sessão se cheio, CloseSession, ZeroSlice→WMsg::ZeroExport
  no canal jobs + forwarder de zero-done→CoreEvent::ZeroDone, Log→eprintln). io_tx do core mantém o
  canal vivo → core só sai no shutdown.
- **`WMsg::ZeroExport{base,len,done}`** em conn.rs + **worker do run_nbd** (main.rs) zera a janela
  via novo `zero_window` (write_at em chunks de 1 MiB) — higiene DT-17 **real, sem stub** (vale
  single e broker mode; em single nunca é enviado).
- **ITEM-10 e2e** (`tests/broker_e2e.rs`): broker real + agentes falsos por TCP loopback + worker
  drenador; **3 testes verdes** (register→Ack/DT-18; tick→AssignFree→SwapOn pelo socket — fiação IO
  completa; registro duplicado→Error+CloseSession). In-process (threads+loopback), seguro no WSL2;
  rodado com `timeout` (0.04s). Gotcha do teste: agente NbdTcp exige `nbd_tcp` configurado no
  EndpointCfg (DT-25 — o broker corretamente recusa SwapOn sem endpoint do transporte).
- **Validação:** clippy lib+bin limpo, `cargo test --workspace` 0 falhas (23 bins de teste), fmt ok.
- **Falta do ITEM-8:** só (1) rework do `run_nbd` no main.rs — `--slices` constrói SliceMap +
  `spawn_broker` + `spawn_acceptor_tcp` + SliceView por Job + roteamento de DEMOTE pro broker
  (hoje há o gate WIP); (2) rename `ramsharedd`; (3) scripts/docs. Depois ITEM-9 (agent), 11
  (drill qemu), 12 (runbook). O **broker (core+IO) está completo e validado**; resta plugá-lo no
  daemon + o agente + os gates de ambiente. Nada commitado. Branch feat/fase-b-prep.

---

## 2026-06-14 — Memory Broker P1: auditoria + validação + COMMITS (5)

- **Auditoria (disciplina):** limpa — 0 TODO/FIXME/`todo!`/`unimplemented!`, 0 secrets,
  `#![forbid(unsafe_code)]` nos crates novos (0 unsafe), unwrap só em `#[cfg(test)]`. Único
  marcador: o **gate WIP honesto** em main.rs (flags `--slices/--arbiter-listen` → erro claro
  "ainda não conectado ao runtime") — estado incremental documentado do `run_nbd`, não stub
  silencioso; aceitável em branch de feature, a resolver quando o run_nbd for plugado.
- **Validação:** `cargo clippy --workspace -D warnings` limpo; `cargo test --workspace` **0
  falhas** (broker 30, broker_e2e 3, wsl2d-lib 41, block 23, cli 11, bin 5, demais intactos;
  root/GPU ignorados); fmt dos crates novos ok.
- **5 commits atômicos** (branch feat/fase-b-prep; sem Co-Authored-By por CLAUDE.md): `09fb1ea`
  docs SPECv2+P0+ADR-0005; `54fc596` scripts P0; `49d37fc` crate ramshared-broker; `e3518b4`
  wsl2d P1 (handshake nomeado + SliceView + conn TCP + broker_srv core+IO + e2e + flags);
  MEMORY. Árvore limpa, HEAD verde.
- **NÃO commitado / não feito ainda (precisa de qemu/civm p/ validar — por isso não foi commitado
  sem validação):** (1) fiação do broker no `run_nbd` (remover o gate WIP; --slices → SliceMap +
  spawn_broker + spawn_acceptor_tcp + SliceView por Job + demote routing) — o daemon vivo só
  valida em qemu (smoke standalone proibido no WSL2); (2) rename `ramsharedd`; (3) ITEM-9 agente
  (crate novo); (4) ITEM-11 drill qemu; (5) ITEM-12 e2e real WSL2↔civm + runbook.
- **Estado:** broker completo como componente (lógica+IO, e2e-validado) e **commitado**; resta a
  integração no binário + agente + gates de ambiente. Branch feat/fase-b-prep.
- **P0-RESULTS.md** atualizado: §2 rede completa + decisão; civm PSI/pagesize; §1 WSL2 idle r1;
  §5 delta_psi. **Gate segue FECHADO** (faltam: PSI sob carga WSL2+civm, NBD/TCP cross-host,
  render do Alex). **Não iniciar ITEM-3+ (código P1) até o gate abrir.**
- **PSI sob carga:** precisa de pressão de MEMÓRIA (cgroup-confined hog, técnica da Fase B), não
  `cargo build` (que é CPU); fazer com cuidado p/ não congelar WSL2 [[feedback-wsl2-cargo-build-caution]].

---

## 2026-06-14 — Memory Broker P1: ITEM-8/9/11/12 + drill qemu PASS (+8 commits)

- **ITEM-9 agente** (`cd15ba4`): crate novo `ramshared-agent` (psi/swap/watchdog puros + testados;
  main DT-27 = 3 threads, escritor único). 25 testes. `#![forbid(unsafe_code)]`.
- **ITEM-8 capstone** (`b0bae97`): fiação do broker no daemon — `run_broker` (remove o gate WIP);
  `--slices/--slice-mb/--arbiter-listen/--listen-nbd`; geom+exports do SliceMap; spawn_broker +
  acceptors Unix/TCP no MESMO canal jobs; worker serve via SliceView; DT-28 (não quebra por
  LiveCount); demote → broker DemoteAll. Helper `residency_check` compartilhado com run_nbd single.
- **`--backend ram` p/ broker** (`4b14070`): refactor `broker_setup` (control-plane agnóstico) +
  `serve_broker_jobs<B>` (worker genérico) + `run_broker`/`run_broker_ram`. Habilita o drill GPU-free.
- **ITEM-11 drill qemu** (`18f5cbf`): `scripts/kernel/qemu-broker-drill.sh` — broker RAM + agente
  reais numa VM isolada. **RODADO = PASS**: KTEST-SWAP-ACTIVE=ok (broker assina slice → agente
  nbd-client+mkswap+swapon → swap ativo via NBD), KTEST-SWAPOFF=ok, KTEST-DAEMON-TERMINATED=ok.
  Disciplina 13: pegou 2 bugs reais antes do PASS (loopback DOWN→ENETUNREACH; `grep -c||echo`).
  **qemu é seguro** (daemon roda DENTRO da VM; sem órfão no host) — diferente de rodar no WSL2.
- **ITEM-12 prep** (`02faf6b` + `129c177`): `--advertise-nbd HOST:PORT` (endpoint TCP anunciado ≠
  bind, p/ civm via port-forward, DT-25) + runbook `docs/memory-broker/CIVM-TENANT.md` (topologia
  R1, Fase A RAM/Fase B VRAM, netsh, ordem de teardown). Execução civm = gate operacional.
- **Docs** (`8863a8c`): SPECv2 ganhou bloco "Estado de implementação"; `IMPL.md` criado (Passo 3
  SSDV3, rastreabilidade ITEM→commit + validação).
- **Validação:** workspace verde (~210 testes, 0 falhas); clippy -D warnings limpo (broker/agent/
  wsl2d lib+bin); fmt. Drill PASS é a evidência de runtime do P1 Linux↔Linux.
- **Estado:** P1 broker **Linux↔Linux completo e runtime-validado** (drill). Resta: ITEM-12 ao vivo
  no civm (operador), DT-5 rename `ramsharedd` (mecânico, deferido), P0 §4 render (tester).

---

## 2026-06-15 — rename `ramsharedd` (DT-5) + DT-29 (fronteira servidor-only) + lição de validação

- **DT-5 rename** feito (`3650008` + `f3a6dff`): binário `ramshared-wsl2d` → `ramsharedd`
  (pacote/lib/dir seguem `ramshared-wsl2d`). Superfície viva: `[[bin]] name`, prefixos `[wsl2d]`→
  `[ramsharedd]` (main.rs/conn.rs), 2 scripts qemu, doc F12, **CLI `cascade.rs`** (gerencia o daemon
  por nome: path + `pgrep`/`pkill -x` + default) e **`CARGO_BIN_EXE_ramsharedd`** no teste de
  integração. Drill re-rodado = PASS.
- **DT-29** (`651360b`): fronteira de **segurança servidor-only**. O freeze de 2026-06-09 foi
  WSL2-**consumidor** (swapon em device morto → D-state). No e2e civm o WSL2 é **só broker/servidor**
  (`run_broker` nunca faz swapon) → o vetor de D-state cai no **civm** (VM isolada), não no host;
  exposição do WSL2 = userspace (matável). Invariante: **nada de agente local no WSL2** nessa
  topologia (isso é qemu-only). Registrado em SPECv2 + `CIVM-TENANT.md`. Corrige ênfase exagerada.
- **LIÇÃO (validação):** `cargo clippy -p X` (lib+bin) e o drill **NÃO** compilam os testes de
  integração nem exercitam o CLI. O rename passou nesses checks mas quebrou `cargo test --workspace`
  (CARGO_BIN_EXE) e o `ramshared up/down` (cascade.rs spawnava binário inexistente). **Só o
  `cargo test --workspace` pegou.** Para mudança que toca nome de binário/processo: rodar o workspace
  inteiro + grep em `*.rs` (não só scripts/docs). [[feedback-batch-local-single-pr]]

---

## 2026-06-15 — ITEM-12 Fase A (cross-host civm) = PASS + fix de tick starvation (DT-30)

- **Smoke broker VRAM no WSL2 (server-only) = OK** (RTX 2060): sobe, aloca VRAM, escuta, SIGTERM
  zera a VRAM e sai limpo — sem travar. Confirma DT-29 empírico (servidor-only no WSL2 real é seguro).
- **ITEM-12 Fase A (RAM, cross-host) = PASS:** broker RAM no WSL2 servindo swap ao civm
  (`gha-ubuntu-2404`). civm ativou `/dev/nbd0`+`nbd1` (broker: `swapon ok s0+s1`), teardown limpo
  (verificação independente: 0 swaps, 0 agentes). commit `32d2911`.
- **TÉCNICA de orquestração (sem netsh/admin):** WSL2→civm SSH funciona (o NAT só bloqueia
  civm→WSL2). Usei **túnel reverso** `ssh -R 127.0.0.1:7000/10809` → o civm acessa o broker do WSL2
  pelo próprio loopback. Broker bind loopback + `--advertise-nbd 127.0.0.1:10809`. Zero admin, zero
  host. (netsh é o caminho de PRODUÇÃO do runbook; o túnel é p/ validação autônoma.) Script em
  `/tmp/civm-drill.sh` (trap de teardown; `pkill -x` não `-f`; swapoff sem var no ssh aninhado).
- **BUG DE PRODUTO (DT-30) — tick starvation:** `core_loop` só emitia `Tick` no `Err(Timeout)` do
  `recv_timeout(tick)`; sob `Psi` normal (~1/s/tenant) as msgs resetavam o timeout → `AssignFree`
  nunca rodava → **nenhum SwapOn**. Travaria a arbitragem sob carga real. Fix: Tick por **deadline de
  wall-clock**. O drill qemu (loopback) mascarava por timing; só o cross-host (jitter) expôs.
  Regressão: `e2e_psi_flood_does_not_starve_arbiter_tick`.
- **civm:** sudo nopass, glibc 2.39 (== WSL2, binário portável), PSI on, já tem `/swap.img` (nbd entra
  em prio menor, aditivo). SSH via `~/.ssh/config` (Host gha-ubuntu-2404). Falta: Fase B (VRAM
  cross-host) + deploy de produção via netsh.

---

## 2026-06-15 — ITEM-12 Fase B (VRAM real cross-host) = PASS

- **Fase B PASS:** broker `--backend vram` na RTX 2060 no WSL2 servindo swap ao civm pelo túnel
  reverso SSH. civm ativou `/dev/nbd0`+`nbd1` **backed por GPU VRAM real sobre a rede**. O **canário
  de residência §9/§9.4 armou (baseline=125µs) e ficou quieto** (`DEMOTE-count=0` → integridade da
  VRAM ok sob o I/O do swap), e o teardown **zerou a VRAM** ("broker VRAM encerrado (VRAM zerada)").
  Verificação independente: civm 0 swaps/0 agentes, WSL2 0 daemon, VRAM liberada. Script:
  `/tmp/civm-drill-vram.sh` (broker com sudo p/ mlockall; teardown `sudo pkill -TERM -x ramsharedd`
  espera o zero da VRAM). NÃO forcei page-out (exigiria pressão real no civm = risco OOM; o header
  do swap já trafega write+read pela VRAM, provando o data path).
- **P1 broker validado ponta-a-ponta:** same-host (drill qemu) + cross-host RAM (Fase A) + cross-host
  VRAM (Fase B). Só docs commitados (sem mudança de código nesta etapa). Falta no ITEM-12: só o deploy
  de PRODUÇÃO via netsh (o software está provado). Próximo natural: abrir o PR de `feat/fase-b-prep`.

---

## 2026-06-15 — INTEGRIDADE da VRAM PROVADA + canário recalibrado (DT-31)

- **Cobrança do user ("nada foi validado / nada funciona") procedia:** eu tinha provado só "anexa"
  (swap active), NÃO que pagina com integridade. Corrigido agora.
- **PROVA real:** com `MADV_PAGEOUT` (page-out determinístico, sem cgroup/pressão/thrash) o civm
  forçou **64 MiB pra VRAM** (broker RTX 2060, túnel SSH) e releu **16384 páginas byte-a-byte: BAD=0**,
  0 DEMOTE, VRAM zerada no teardown. → VRAM-as-swap **funciona** (page-out + page-in íntegro cross-host).
- **DT-31 (bug real achado no caminho):** o canário de latência (`latency_mult=8×`) **false-positivava
  sob carga** — a latência de serve sob page-out/in pesado bate ~17× o baseline, e o `DemoteAll`
  derrubava o swap no meio (auto-sabotagem sob a própria carga). Recalibrado **8×→64×** (entre 17×
  carga e 330× eviction); a sonda de conteúdo §9.4 é o detector autoritativo. Regressão:
  `load_spike_below_threshold_stays_ok`. Sem o fix, o verify nem completava.
- **LIÇÃO de método:** swap-thrash sobre o túnel SSH é lento demais p/ verificar (ms/op). `MADV_PAGEOUT`
  (1 page-out + 1 page-in, sem re-eviction) é o jeito de provar integridade rápido. Derrubar o teste
  no meio do thrash degradou o civm transitoriamente (recuperou via `-timeout 30` do nbd, DT-14).
- workspace verde (26 ok). Falta no ITEM-12: só o deploy de produção via netsh.

---

## 2026-06-15 — Auditoria dos commits do P1 + fix de consistência (pré-merge)

- **Achado #1 (MÉDIA, corrigido):** o broker usava `tick=1s` (`main.rs`), mas o SPEC especifica
  **2s** em 5 lugares (DT-24, tabela de config, comentários) e o `ArbiterConfig` comenta "5 ticks →
  10s". Eu introduzi 1s no `run_broker` sem DT (viola "zero criatividade no IMPL"). Fix: `tick=2s`
  (alinha código↔SPEC; streak=5 → janela de 10s). Re-validado: drill qemu PASS + e2e VRAM cross-host
  PASS (0/16384 páginas ruins) com 2s.
- **Achado #2 (BAIXA, histórico):** o commit `f134dfa` afirma "validado ponta-a-ponta / integridade
  da VRAM ok", mas ali a integridade era inferida de `0 DEMOTE` — a prova byte-a-byte só veio em
  `bbf76ec`. Docs atuais já corrigem; mensagem do commit fica no histórico (não reescrever c/ force-push).
- **#3 (resolvido):** rename levou 2 commits (`3650008` deixou refs vivas → `f3a6dff`); CI verde agora.
- **Limpo:** 0 WIP/stub, 0 `[wsl2d]`, `latency_mult=64` e `delta_psi=10` consistentes código↔docs,
  DTs únicos.
- **Lição de harness:** o `/tmp/ramshared-agent` do civm some entre rodadas (limpeza de /tmp da VM de
  CI); o script de teste deve **re-copiar o binário** (auto-contido) — senão dá falso "swap não ativou".

---

## 2026-06-15 — Frentes pós-P1 (plano 1→4 aprovado): F1 feito, F4 SPEC; branch `feat/p1-hardening`

- **Frente 1 (hardening) FEITA + validada** (branch `feat/p1-hardening`):
  - `7787fc2` feat: backoff exponencial de reconexão no agente (era fixo 2s → 2→4→…→60s, reseta
    pós-sessão produtiva; `next_backoff` puro + teste `backoff_doubles_up_to_cap`).
  - `79f9ce6` fix: retry R4 do zero de slice presa em Draining (try_send cheio → sem ZeroDone →
    presa). `pending_zero` + retry no tick (carência 1 + ERROR após 5); só re-zera slices já
    swapped-off (não corrompe). Teste `stuck_draining_zero_is_retried_on_tick`.
  - Validado: workspace 26 ok, clippy --all-targets, fmt, drill qemu PASS.
- **Frente 2 (PRD P2): ACHADO — PRD já existe.** O `docs/memory-broker/PRD.md` unificado já cobre a
  P2 (RF-W1..W3, RF-P1/P3, Anexo B com perguntas do Alex). Escrever PRD novo = duplicar (SSDV3 reuso).
  O que falta é o **SPEC** da P2, mas ele é design-heavy + **gated no input do Alex** (cenas .blend)
  → adiado. Usuário escolheu **pular pra Frente 4**.
- **Frente 4 (VramProvider): SPEC FEITO** (`c3ae8d5`, `docs/vram-provider/SPEC.md`). Design resolve a
  cadeia de lifetimes `Cuda→Context<'a>→DeviceMem<'c,'a>` (auto-ref): **Arc no Context (DT-V1) + Mem
  como GAT (DT-V2)**. **IMPL DIFERIDA p/ sessão fresca** (refactor invasivo multi-arquivo; performance.md
  proíbe refactor grande fundo no contexto). Mapa de acoplamento no SPEC §5.
- **Pendente:** abrir PR de `feat/p1-hardening` (F1 + SPEC); F4 IMPL (sessão fresca); F2 SPEC + F3
  deploy gated no Alex/ops. Vulkan (RF-G2) = subsistema novo, PRD próprio.

---

## 2026-06-15 — Frente 4 IMPL: VramProvider extraído (passos 1-3), daemon-genericização diferida

- **"continue" → executei a IMPL da F4** incrementalmente + verde a cada passo (branch `feat/p1-hardening`):
  - `d898488` crate `ramshared-vram` (traits `VramProvider`/`VramMemory`, GAT).
  - `74f4052` impl CUDA (`VramProvider for Context` via GAT `Mem<'p>=DeviceMem<'p,'a>` + `VramMemory
    for DeviceMem` + `From<CudaError> for VramError`).
  - `ca5194d` `VramBackend<M>`/`CanaryProbe<M>`/`residency_check<M>` genéricos; call sites inferem
    M=DeviceMem (comportamento idêntico). clippy --all-targets + test 28 ok + drill PASS.
- **DT-V1 do SPEC revisado:** o `Arc` no Context **não** foi preciso — GAT nos tipos existentes é
  mais limpo (sem ripple no cuda crate). SPEC `docs/vram-provider/SPEC.md` atualizado com o estado.
- **Falta (próxima sessão, fresca):** o daemon ainda aloca via `cuda::Context` direto; genericizar
  `run_nbd`/`run_broker`/`ublk_server` sobre `P: VramProvider` (provider criado no shell CUDA do
  `run()`) é a parte INVASIVA (assinaturas das fns grandes) → **diferida** por performance.md (refactor
  grande no fim do contexto; valida só em host/qemu). Só então `VramProvider` é consumido genericamente.
- **PR:** nenhum (regra do usuário: PR só no fim de TUDO implementado+validado, e só quando ele pedir).
  Branch `feat/p1-hardening` acumula: F1 (backoff+R4) + SPEC + F4 passos 1-3.
