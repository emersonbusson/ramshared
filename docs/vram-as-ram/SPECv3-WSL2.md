---
slug: vram-wsl2-cuda-swap
title: VRAM como tier frio numa cascata de swap no WSL2 (v3)
source_prd: PRD-2.md
supersedes: SPECv2-WSL2.md
preserves: [SPEC-WSL2.md, SPECv2-WSL2.md]
variant: WSL2/GPU-PV/CUDA
milestone: M01
status: draft-executable
audited_spec: SPECv2-WSL2.md
audit_step: SSDV3 PASSO 2.5
audit_verdict_of_v2: no-go
fase0: concluida (FASE0-FINAL.md)
active_candidate: true
impl_language: rust
reference_impl: c0deJedi/nbd-vram (MIT) — blueprint/benchmark only, NÃO entra no produto
---

# SPECv3-WSL2 — VRAM como tier frio numa cascata de swap (zram → VRAM → VHDX)

## 0. Proveniência da auditoria (regra de saída do Passo 2.5)

- **SPEC auditado:** `docs/vram-as-ram/SPECv2-WSL2.md` (preservado).
- **Veredito do v2:** `no-go` (arquitetura do MVP contrariada pela Fase 0).
- **Pedido explícito de `SPECv3`** (cláusula de exceção do Passo 2.5).
- **Findings bloqueantes endereçados:**
  - **V3-F1** (VRAM como swap quente contraria a Fase 0) → **§1** reescrita: VRAM
    é tier **frio**, nunca o swap de maior prioridade.
  - **V3-F2** (tiering estava como apêndice opcional) → **§1/§4** promovem a
    cascata `zram→VRAM→VHDX` a **arquitetura do MVP**, com evidência da Part C.
  - **V3-F3** (canário abortava em vez de demover) → **§9** redefine a ação como
    **DEMOTE gracioso** (swapoff só do tier VRAM; páginas caem pro VHDX) sem matar
    processos.
  - **V3-F4** (zram ausente do contrato) → **§5/§6/§10** incluem setup de zram,
    verificação de `CONFIG_ZRAM` e o comando `up` que monta a cascata.
  - **V3-F5** (prioridade inconsistente) → **§6.2** fixa o esquema
    `zram=200 > VRAM=100 > VHDX=−2`.
  - **V3-F6** (gate ancorado em baseline injusto) → **§3** redefine aceite por
    **swap real** (`pswpin/out` por tier, stall), não `fio` vs VHDX.
  - **V3-F7/F8** → §3 registra Fase 0 concluída; §14 usa pressão **confinada em
    cgroup** (não `stress-ng` global).
- **Carregado do v2 sem mudança material:** §0.2 Research & Reuse, modelo de I/O
  CUDA (v2 §8), backend NBD/ublk (v2 §10), recovery (v2 §13), disciplinas por
  passo (v2 §12). Citado aqui de forma condensada; o detalhe profundo continua
  válido no `SPECv2-WSL2.md`.
- **Candidato ativo** para nova auditoria. Se reprovar, atualizar **in-place**
  (não criar v4 salvo pedido explícito).
- **Auto-auditoria (Passo 2.5, 2026-06-04):** v3 reprovou por A1 (segurança do
  DEMOTE sem invariante de tier-abaixo). Corrigidos in-place: **A1** (precondição
  de rede de segurança em §6.2/§9.2), **A2** (`T_demote=30s` + math em §9.2),
  **A3** (fórmula de tamanho do zram em §11), **A4** (OQ-demote decidida).
  Resultado pós-correção: **`go`** → Passo 3 (scaffold Rust).

## 0.2 Research & Reuse (resumo; íntegra no v2 §0.2)

`c0deJedi/nbd-vram` (MIT) é a referência do daemon CUDA+NBD e **confirma** que em
GeForce consumer só o caminho NBD funciona (`nvidia_p2p_*`→`EINVAL`, BAR1 só mapeia
~16 MiB). A própria referência usa a cascata `RAM→VRAM→zram→SSD` (priorizada);
nós invertemos para `zram→VRAM` porque a §9.5 provou a VRAM latency-unsafe (zram,
RAM-comprimida, tem que ser o tier quente). NVIDIA docs (pinned limitado, UVM
ausente, WDDM) sustentam a §9. `.wslconfig` permite kernel custom (Fase B ublk).

## 1. Decisão de arquitetura (PIVÔ — resolve V3-F1, V3-F2)

**A VRAM não é swap. A VRAM é um tier FRIO numa cascata de swap por prioridade:**

```text
pressão de memória ──►  zram   (RAM comprimida, lzo-rle)   prio 200   HOT
                   └─►  VRAM   (nbd-vram, CUDA+NBD)         prio 100   COLD
                   └─►  VHDX   (/dev/sdc, swap do WSL2)      prio −2    LAST
```

**Por quê (evidência Fase 0, `FASE0-FINAL.md`):**
- A VRAM é **data-safe** (hash íntegro após eviction) mas **latency-unsafe**: uma
  leitura 4K sob VRAM cheia custou **1,18 s** (§9.5). Como swap quente, congela.
- zram (baixa latência, em RAM) absorve o **working set quente**; a VRAM pega só o
  **spill frio** (acesso raro) — escondendo a fraqueza de latência e usando a
  força (bandwidth/capacidade). **Provado (Part C):** zram encheu 1024 MiB, VRAM
  absorveu **983 MiB** de overflow, VHDX intocado.
- A cascata é por **prioridade de `swapon`** — Day-0, sem kernel custom.
  `CONFIG_ZRAM_WRITEBACK` não está setado neste kernel, então a integração por
  *writeback* (zram grava frio direto na VRAM) fica para a Fase B (kernel custom).

**O que o RamShared entrega:** orquestra a cascata e **gerencia o tier VRAM**
(daemon CUDA+NBD) com um **canário que demove a VRAM sob latência** (§9), sem
derrubar processos. zram e VHDX são mecanismos do kernel que ele só configura.

**Fora de escopo WSL2:** PRD-4 (DAMON), PRD-6 (HMM `DEVICE_PRIVATE`), BAR1/P2P.

## 2. Evidência local de plataforma (2026-06-04, confirmada na Fase 0)

```text
kernel:  6.6.114.1-microsoft-standard-WSL2
GPU:     RTX 2060, 6144 MiB (cuInit/cuMemAlloc OK — confirmado pela nbd-vram)
RAM:     15.6 GiB | swap VHDX /dev/sdc 8 GiB prio −2
zram:    CONFIG_ZRAM=m, CONFIG_ZSMALLOC=m, zramctl presente, lzo-rle default
         CONFIG_ZRAM_WRITEBACK **não** setado
nbd:     CONFIG_BLK_DEV_NBD=m (precisa modprobe; /dev/nbd* sob demanda)
ublk:    indisponível (CONFIG_BLK_DEV_UBLK not set) — Fase B
io_uring: habilitado | systemd: ativo (degraded) | cgroup v2 memory: ok
/dev/dxg, libcuda: presentes | nvidia-smi: OK
```

Tudo o que a cascata precisa existe **hoje** sem kernel custom.

## 3. Fase 0 — CONCLUÍDA (resolve V3-F6, V3-F7)

Gate original (perf vs VHDX) **superado**: a Fase 0 rodou (3 experimentos, ver
`FASE0-FINAL.md`, `FASE0-RESULTS.md`, `FASE0[B,C]-RAW.txt`). Resultados que fundam
o SPECv3:

| Experimento | Resultado | Consequência no SPECv3 |
|---|---|---|
| A) baseline justo | inconcluso (host-cache não-derrotável de dentro) | aceite passa a ser swap-real, não fio vs VHDX |
| B) eviction WDDM | data-safe; latência 4K → **1,18 s** sob pressão | §9 DEMOTE por latência; VRAM = tier frio |
| C) zram-tiering | cascata OK: zram 1G cheio, VRAM +983M, VHDX intocado | §1 arquitetura do MVP |

**Critério de aceite (redefinido, swap real):** sob pressão **confinada em
cgroup** (§14), os contadores `pswpout` por tier devem mostrar
`zram` enchendo antes da `VRAM`, e a `VRAM` antes do `VHDX`; nenhum processo
confinado deve ser morto por OOM enquanto houver capacidade na cascata.

## 4. Objetivo de implementação

Dois binários (igual v2 §4, papéis ajustados):

- `ramshared`: CLI que **orquestra a cascata** (`check`, `up`, `down`, `status`,
  `recover`, `test`).
- `ramshared-wsl2d`: daemon do **tier VRAM** (CUDA Driver API + NBD), com o
  canário de residência (§9). Núcleo de I/O = v2 §8 (inalterado).

**Linguagem: Rust (decisão FECHADA — a Fase 0 acabou).** Toda a implementação de
produção é Rust (crates da §5): CUDA via FFI sobre `libcuda.so`, protocolo NBD em
Rust, `Result<T, Error>` sem `.unwrap()/.expect()` (regra `coding.md`), `unsafe`
isolado em `ramshared-cuda` com invariantes documentadas. A `c0deJedi/nbd-vram`
(C, MIT) foi **só** (a) a régua de medição da Fase 0 e (b) o blueprint da
arquitetura e do protocolo NBD fixed-newstyle — **não** é forkada nem entra no
binário. Day-0: reescrita limpa em Rust, sem shim/fork de C.

Uso manual (sem auto-start):

```sh
sudo ramshared check
sudo ramshared up            # monta zram + VRAM + (VHDX já existe), nessa prio
sudo ramshared status        # mostra a cascata e o estado do canário
sudo ramshared down          # desmonta na ordem inversa, segura
```

## 5. Árvore de código (resolve V3-F4)

Igual ao v2 §5, com **adições para a cascata**:

```text
crates/ramshared-cli/src/commands/
    check.rs  up.rs  down.rs  status.rs  recover.rs  test.rs
crates/ramshared-tier/             # NOVO — orquestração da cascata
    Cargo.toml
    src/lib.rs
    src/zram.rs        # criar/dimensionar/mkswap/swapon zram (prio 200)
    src/cascade.rs     # esquema de prioridades, ordem up/down, verificação
    src/priority.rs    # constantes e validação de prioridade entre tiers
crates/ramshared-wsl2d/src/
    residency.rs       # canário com DEMOTE gracioso (§9)  [mudado vs v2]
# (ramshared-cuda, ramshared-block, ramshared-integrity = v2 §5, inalterados)
docs/vram-as-ram/SPECv3-WSL2.md
```

## 6. Contrato da CLI

### 6.1 `ramshared check` (v2 §6.1 + zram)

Acrescenta ao `check` do v2:
- **zram:** `CONFIG_ZRAM=y|m` e `zramctl` presente → `zram=ok`; senão
  `zram=fail`. Reportar algoritmo default (`lzo-rle`).
- **cgroup v2 memory** (para os testes): presença de `memory` em
  `/sys/fs/cgroup/cgroup.controllers`.

Saída acrescenta linha:
```text
Tiers: zram=<ok|fail>, vram=<ok|needs-modprobe|fail>, vhdx=<device,prio>
```
`Decisao: ready` exige zram **ou** vram utilizável (a cascata degrada para o que
houver). `blocked` só se nenhum tier extra for possível.

### 6.2 `ramshared up` (resolve V3-F4, V3-F5 — substitui start+swapon)

Esquema de prioridade fixo e validado (**resolve V3-F5**):
```text
ZRAM_PRIO = 200    VRAM_PRIO = 100    VHDX = mantém o existente (−2)
```
Flags: `--zram-size` (default `25%` da RAM, **OQ-zram**), `--vram-size`
(default `1G`, backoff 512 MiB, igual v2 §6.2), `--no-zram`, `--no-vram`,
`--vram-min` (default `256M`), `--force-large`.

Sequência (cada passo idempotente; abort não deixa cascata parcial — desfaz o que
montou):
1. Preflight (`check`). Abort se `blocked`.
2. **Tier zram (HOT):** `modprobe zram`; `zramctl --find --size <N> --algorithm
   lzo-rle`; `mkswap -L RAMSHARED_ZRAM`; `swapon -p 200`.
3. **Tier VRAM (COLD):** subir `ramshared-wsl2d` (CUDA alloc com backoff §6.2-v2;
   `mlockall`+`oom_score_adj=-1000`; staging; **canário armado** §9); conectar
   `nbd` (§10); `mkswap -L RAMSHARED_VRAM`; `swapon -p 100`.
4. **Tier VHDX (rede de segurança do DEMOTE — A1):** **não tocar** (já em −2).
   **Invariante de segurança:** o tier VRAM só é armado se existir um destino de
   prioridade MENOR que a VRAM para o DEMOTE (§9.2) escoar páginas — isto é, swap
   VHDX presente **OU** `MemAvailable ≥ vram_size`. Se `.wslconfig swap=0` (sem
   VHDX) **e** sem RAM suficiente, `up` **recusa** o tier VRAM (exit != 0) salvo
   `--force-no-safety-net`. Avisar se a prio do VHDX ≥ 100 (colidiria com a VRAM).
5. Publicar `/run/ramshared/cascade.json` (tiers, prios, devices, PID, sizes).
6. Imprimir a cascata resultante (igual `status`).

### 6.3 `ramshared status`
Lê `cascade.json` + `/proc/swaps` + `zramctl`; imprime a cascata, `Used` por tier,
estado do canário (`armed|demoted`), e `pswpin/pswpout` atuais.

### 6.4 `ramshared down` (ordem inversa, segura — resolve V3-F3 herdado)
1. **VRAM:** `swapoff <vram_dev>` (kernel migra páginas para zram/VHDX). Se
   falhar (ENOMEM), **não desconectar** o nbd (panic) → `recover` (§13).
2. Shutdown gracioso do daemon (drena, para o canário, zera VRAM, `nbd -d`, libera
   `CUdeviceptr`).
3. **zram:** `swapoff <zram_dev>`; `zramctl -r`.
4. VHDX permanece. Remover `/run/ramshared/*`.

### 6.5 `ramshared recover` — v2 §13 (escalonado, `wsl --terminate` último recurso).

## 7. Daemon — máquina de estados (v2 §7 + demote)

```text
Init → PreflightOk → MemoryLocked → CudaReady → VramAllocated
     → ResidencyArmed → BlockReady → SwapActive
     → Demoted        (canário disparou latência; VRAM fora do pool, sistema vivo)
     → Stopping → fim
     → Failed         (erro duro)
```
`Demoted` é **novo** e **não** é `Failed`: o tier VRAM saiu da cascata, zram/VHDX
seguem. De `Demoted` pode-se re-promover (OQ-demote) ou `down`.

## 8. I/O CUDA e atomicidade — inalterado do v2 §8

Stream ordenado, mapa de blocos em voo (sem leitura torn), durabilidade-em-VRAM
antes do complete via `cuEvent`, erros CUDA → I/O error (nunca sucesso parcial).
Detalhe completo no `SPECv2-WSL2.md §8`.

## 9. Residência com DEMOTE gracioso (resolve V3-F1, V3-F3) — evidência §9.5

Premissa e mecânica do canário = v2 §9.1/§9.2 (região canário, amostrador a cada
`T_sample`, baseline de latência). **A AÇÃO muda:**

### 9.1 Gatilho (calibrado pela Fase 0)
```text
DEMOTE-VRAM, se:
  (a) latência do canário p99 > K × baseline por ≥ M amostras consecutivas
      (default K=8, M=3; a Fase 0 viu 330× — folga enorme)   [risco DOMINANTE]
  (b) conteúdo do canário != padrão                          [não observado, mas guard]
  (c) cuMemGetInfo free < floor                              [host reavendo VRAM]
```

### 9.2 Ação: DEMOTE, não abort (a diferença-chave do v3)
```text
PRECONDIÇÃO (A1): existe tier abaixo da VRAM (VHDX) ou MemAvailable >= vram_size.
                  Garantida no `up` (§6.2 passo 4); sem ela, não se arma a VRAM.
1. swapoff <vram_dev>  (timeout T_demote=30s default; kernel migra as páginas
   VRAM-residentes para o destino de menor prio — VHDX — ou RAM. Bounded pelo
   tamanho da VRAM: pior caso ~vram_size/4KiB páginas, cada uma podendo ser lenta
   sob eviction, por isso o timeout.)
2. nbd -d; liberar CUdeviceptr; estado → Demoted.
3. Processos NÃO são mortos: zram (quente) e VHDX (frio) seguem servindo o swap.
4. Logar com número (latência observada, páginas migradas). Sem teatro (#3).
Se swapoff estourar T_demote (eviction travando a leitura de volta) → escalonar
para recover (§13), pois aí o I/O está preso no kernel.
```

Isto só é **seguro** porque a VRAM é tier intermediário: existe um tier abaixo
(VHDX) para receber as páginas. Era impossível no raw-swap do v2 (a VRAM era o
topo). **Disciplina #2 (counterfactual):** o trigger numérico de latência É a
condição de reversão; #5 (worst-case): o pior caso (host reaver VRAM) tem caminho
de saída sem perda de dado.

### 9.3 Evidência empírica (Fase 0) — ver §9.5 do v2 e `FASE0-FINAL.md`
Dado sobrevive (hash ok); latência 4K → 1,18 s sob VRAM cheia. Confirma (a) como o
gatilho relevante e justifica o DEMOTE em vez de confiar na VRAM sob pressão.

## 10. Tiers e backends

- **zram:** `ramshared-tier/zram.rs` (criar, dimensionar, mkswap, swapon 200,
  teardown). lzo-rle. Sem writeback (kernel atual).
- **VRAM/nbd:** v2 §10.1 (modprobe nbd, ioctls `NBD_SET_*`, flush só se
  implementado, `NBD_DISCONNECT` no teardown). swapon 100.
- **ublk (Fase B):** v2 §10.2 + receita de kernel custom (inalterada).
- **VHDX:** não gerenciado; só lido/validado (prioridade < VRAM).

## 11. Limites de segurança (atualizado)

- Sem auto-start. VRAM **nunca** como swap de maior prioridade (V3-F1). Esquema
  fixo `200 > 100 > −2`.
- VRAM: backoff até `--vram-min`; ≥ 1 GiB livre após reserva; ≤ 25% da VRAM livre;
  `mlockall` + `oom_score_adj=-1000` no daemon.
- zram (A3): tamanho default = `min(25% da RAM, MemAvailable − 2 GiB)`, mínimo
  `512M`; se der < `512M`, `up` avisa e segue **sem** zram (só VRAM+VHDX). Dado
  incompressível em zram ocupa **RAM real** — o teto contra `MemAvailable` evita
  swap-que-come-RAM (páginas típicas comprimem; incompressível é o pior caso).
- DEMOTE preferível a Failed; `down` desmonta VRAM antes de zram; `swapoff` antes
  de qualquer `nbd -d` (anti-panic, confirmado pela referência).
- Zerar VRAM ao alocar e liberar; daemon root-only; socket `0600`.

## 12. Disciplinas Kahneman por passo crítico (v2 §12 + cascata)

| Passo | Disciplina | Evidência mínima | Abort/Reversão |
|---|---|---|---|
| Arquitetura (cascata) | #1, #5 | `FASE0-FINAL.md` (3 experimentos) | — (decisão fundada em dado) |
| Residência VRAM (§9) | #2, #5 | canário + baseline | DEMOTE-VRAM (§9.1) |
| Ordem de tiers (§6.2) | #9 substituição→nº | prios em `/proc/swaps` | VHDX ≥ VRAM → abort montagem |
| Reserva VRAM | #3 | `cuMemGetInfo` logado | free < floor → backoff/abort |
| Aceite (§14) | #3, #13 | `pswpout` por tier sob carga real | cascata fora de ordem → falha |
| `down`/demote | #2 | swapoff ok antes de `nbd -d` | swapoff falha → recover (§13) |

## 13. Recovery — inalterado do v2 §13

Escalonado: `swapoff` (timeout) → `nbd -d`/ublk delete → liberar CUDA →
só então, com processos em `D` > `T_stuck`, sugerir `wsl --terminate` com aviso de
colateral (OQ1: distro É primária). Detalhe no v2 §13.

## 14. Testes de aceitação (swap real + cgroup — resolve V3-F6, V3-F8)

### 14.1 Detecção — `check` reporta os 3 tiers; `ready`/`blocked` pareados (v2 §14.1).
### 14.2 Integridade VRAM — `up --no-zram` + `test --integrity 30m` com blocos
sobrepostos concorrentes; hash sem divergência (v2 §14.2).
### 14.3 Cascata sob pressão (NOVO, confinada em cgroup — método da Fase 0 Part C):
```sh
sudo ramshared up
# hog INCOMPRESSÍVEL confinado: systemd-run --scope -p MemoryMax=400M memhog 2400 45
# snapshot DURANTE: swapon --show ; zramctl ; pswpout por tier
```
Aceite: `zram.Used` satura **antes** de `vram.Used` crescer; `vhdx.Used`
inalterado enquanto zram+VRAM têm espaço; nenhum kill por OOM. (Replica o
resultado já obtido: zram 1024M, VRAM 983M, VHDX intocado.)
### 14.4 DEMOTE sob latência (NOVO — o coração do v3):
```sh
sudo ramshared up
# vramhog CUDA enche a VRAM (oversubscription) -> latência do canário dispara
```
Aceite: canário detecta (a) §9.1; daemon entra `Demoted`; `swapoff` da VRAM
conclui dentro de `T_demote`; **nenhum processo morto**; zram/VHDX seguem; VRAM
some de `/proc/swaps`; `status` mostra `demoted`.
### 14.5 Falha dura (SIGKILL) → `recover` (v2 §14.5).
### 14.6 `down` deixa `/proc/swaps` só com o VHDX; zram removido; VRAM liberada.

## 15. Critérios de pronto

- `cargo fmt`/`clippy -D warnings`/`test` verdes; sem `.unwrap()` em produção.
- `check` cobre `ready` e `blocked`.
- §14.3 (cascata na ordem) e §14.4 (DEMOTE sem matar processo) verdes.
- `down`/`recover` deixam o sistema no baseline (só VHDX).
- Day-0: sem shim; cascata por prioridade nativa; kernel custom só na Fase B
  (ublk/zram-writeback), documentado.

## 16. Não objetivos e opções futuras

Não objetivos: VRAM como swap quente; emprestar VRAM ao host; módulo de kernel no
MVP; DAMON/HMM/TTM/ReBAR; auto-start.

Opções futuras (Fase B, exigem kernel custom — só com exceção Day-0 documentada):
- **zram-writeback → VRAM:** com `CONFIG_ZRAM_WRITEBACK`, zram grava páginas frias
  direto no device VRAM (`backing_dev`), eliminando o tier de prioridade separado.
- **ublk** no lugar do nbd (perf): v2 §10.2.
- **Re-promoção automática** da VRAM após cooldown de latência (OQ-demote).
- **userfaultfd** (PRD-5) se o block layer dominar overhead.

## 17. Open questions

- **OQ-pivô** — ✅ aceito (pedido de SPECv3 = aceite do pivô para a cascata).
- **OQ-zram** — tamanho default do zram (proposto 25% da RAM); confirmar política.
- **OQ-demote** — ✅ DECIDIDO: MVP = demote-and-stay-down (a VRAM sai e fica fora
  até `down`/`up`). Re-promoção automática após cooldown = opção futura (§16).
- **OQ2** — baseline justo: resolvido conceitualmente migrando aceite p/ swap real
  (§3/§14.3); fio-vs-VHDX abandonado.
- **OQ3** — teto de VRAM com GPU compartilhada (~4.2 GiB livres): backoff §6.2
  cobre; confirmar `--vram-size` default `1G`.

## 18. Rastreabilidade PRD-2 → SPECv3 (regra dura SSDV3 #4/#5)

O `PRD-2` é registro histórico (descreve VRAM como swap quente via ublk); a Fase 0
(`FASE0-FINAL.md`) revisou parte dos seus requisitos. Mapa:

| PRD-2 | Texto original | Estado no SPECv3 | Seção |
|---|---|---|---|
| **RF-1** | alocar 1–N GB de VRAM sem travar a GUI | **mantido** (backoff, teto, mlock) | §6.2, §11 |
| **RF-2** | workers `ublk` via `io_uring` | **REVISADO** → `nbd` na Fase A; `ublk` = Fase B (kernel custom) | §2, §10 |
| **RF-3** | `swapon pri=32767` (swap quente) | **REVISADO** → VRAM é tier **COLD** prio 100 atrás do zram | §1, §6.2 |
| **RNF-perf** | saturar PCIe 10–15 GB/s | **REVISADO** → claim bare-metal; GPU-PV é latency-bound | §3.2.1 |
| **RNF-estab** | `mlockall` anti-deadlock | **mantido** | §6.2, §11 |

Requisitos revisados **não** voltam ao PRD-2 (preserva histórico) — ficam
reconciliados aqui. Commits do Passo 3 citam seção do SPECv3 + RF coberto
(ex.: `feat(core): cascade priority — SPECv3 §1 / revisa RF-3`).
