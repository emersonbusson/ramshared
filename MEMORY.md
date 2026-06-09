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
