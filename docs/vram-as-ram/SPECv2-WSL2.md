---
slug: vram-wsl2-cuda-swap
title: VRAM emprestada como swap seguro no WSL2 via CUDA (v2)
source_prd: PRD-2.md
supersedes: SPEC-WSL2.md
variant: WSL2/GPU-PV/CUDA
milestone: M01
status: superseded
superseded_by: SPECv3-WSL2.md
audited_spec: SPEC-WSL2.md
audit_step: SSDV3 PASSO 2.5
audit_verdict_of_v1: no-go
active_candidate: false
reference_impl: c0deJedi/nbd-vram (MIT) — daemon CUDA + NBD, arquitetura idêntica ao MVP
---

# SPECv2-WSL2 — VRAM emprestada como swap seguro no WSL2 via CUDA

## 0. Proveniência da auditoria (obrigatório — regra de saída do Passo 2.5)

- **SPEC auditado:** `docs/vram-as-ram/SPEC-WSL2.md` (preservado, não alterado).
- **Resultado da auditoria do v1:** `no-go`.
- **Findings bloqueantes endereçados nesta versão:**
  - **F1** — evidência de plataforma falsa e teste de detecção chaveado nela
    → §2 reescrita com sondagem real e datada; §14.1 testa caminho `ready`
    **e** `blocked` em vez de afirmar `blocked` nesta máquina.
  - **F2** — eviction WDDM/GPU-PV de alocação viva não tratada
    → nova **§9 Residência e eviction WDDM/GPU-PV** com canário, detecção e
    abort trigger numérico.
  - **F3** — viabilidade não medida vs. swap VHDX
    → nova **§3 Fase 0 — spike de viabilidade** que **gateia** todo o resto com
    abort trigger numérico contra o baseline VHDX.
  - **F8** — disciplinas Kahneman sem abort trigger nos passos críticos
    → **§12** mapeia cada passo crítico para disciplina + evidência mínima +
    abort trigger.
  - **F6** — `wsl --terminate` como `Down` inseguro em máquina compartilhada
    → **§13** recovery escalonado, com `wsl --terminate` como último recurso
    quantificado e aviso de colateral.
- **Demais findings (HIGH/MEDIUM/LOW)** F4, F5, F7, F9, F10, F11, F12
  endereçados nas seções §8, §10, §6, §5, §10, §6, e na grafia PT-BR deste doc.
- **Esta versão é o candidato ativo** para nova auditoria (Passo 2.5). Se
  reprovar, atualizar **in-place** (não criar v3 salvo pedido explícito).

## 0.2 Research & Reuse (Passo 0 do development-workflow)

Pesquisa em `microsoft/WSL` (issues), documentação Microsoft/NVIDIA e GitHub
em 2026-06-04. Conclusões que mudam o SPEC:

**Implementação de referência — `c0deJedi/nbd-vram` (MIT, 389★, atualizado
2026-06-05).** É um daemon C (~18 KiB) que faz **exatamente** o nosso MVP:
aloca VRAM via CUDA Driver API (`dlopen libcuda.so.1`, `cuMemAlloc_v2`,
`cuMemcpyHtoD/DtoH_v2` síncronos) e serve como block device por **NBD
fixed-newstyle sobre socket Unix**; o `nbd.ko` do kernel expõe `/dev/nbdX` e
vira swap. Data path idêntico ao da §8. Implicações (regra dura SSDV3: reuso
antes de criação):
- **Convergência PRD-2/NBD confirmada por terceiro.** A referência documenta
  *por que* as alternativas falham em GeForce consumer (nosso caso, RTX 2060):
  `nvidia_p2p_get_pages_persistent` retorna `EINVAL` (gated p/ Quadro/datacenter);
  `ioremap_wc` do BAR1 só mapeia ~16 MiB (framebuffer), resto lê zero — `mkswap`
  finge sucesso e `swapon` falha. Isso **sepulta PRD-1 e o caminho BAR1/P2P** em
  hardware consumer e reforça NBD como rota correta.
- **Reuso na Fase 0:** usar a referência como spike e seus scripts de benchmark
  como ferramenta do GATE-PERF (ver §3). Não construir spike novo.
- **Backoff de alocação** (recuar 512 MiB se a GPU está cheia) — adotado na §6.2.
- **Licença MIT** permite fork/port. Decisão port-Rust vs. fork-C fica para
  **depois** da Fase 0 (WYSIATI: ainda não sabemos se roda em WSL2).
- **Limite da referência:** validada em **bare-metal** (Pop!_OS, kernel 6.17),
  **não** em WSL2. Ela não trata eviction WDDM (§9) — esse é o valor agregado
  desta variante.

**Documentação Microsoft (`.wslconfig`):**
- Swap do WSL2 = VHDX em `%Temp%\swap.vhdx` (no C:, tipicamente NVMe), default
  25% da RAM da VM. É o baseline do GATE-PERF (OQ2). `swap=0` desliga.
- `autoMemoryReclaim=dropCache` (default) reclama cache agressivamente — interage
  com pressão de memória/swap.
- **`kernel=` / `kernelModules=` / `kernelCommandLine=`** permitem kernel custom
  → habilita a Fase B (ublk). Receita concreta na §10.2.

**Documentação NVIDIA (CUDA on WSL) — confirma premissas da §9:**
- "Pinned system memory availability ... is **limited**" → restringe o ring de
  staging pinned (§6.2/§8); a referência evita o problema usando cópia **síncrona**
  sem staging pinned.
- "Unified/Managed Memory **not supported**" → sepulta PRD-1/HMM no WSL2.
- "Concurrent CPU/GPU access **not supported**" → nosso modelo de staging+cópia
  explícita é compatível; acesso concorrente direto não é.
- Limitações atribuídas explicitamente à **arquitetura WDDM** → base da §9.

**Issues `microsoft/WSL` relevantes:** #11050 (WSL2 **não respeita** a *CUDA
Sysmem Fallback Policy* — reforça §9), #9962 ("GPU access blocked by the OS" — o
erro do v1, transitório, hoje resolvido nesta máquina), #7162/#9099/#13370/#13769
(GPU-PV quebra entre updates de WSL/driver — reforça Disciplina 2 / #5 worst-case).

Fontes: <https://github.com/c0deJedi/nbd-vram> ·
<https://wiki.archlinux.org/title/Swap_on_video_RAM> ·
<https://learn.microsoft.com/windows/wsl/wsl-config> ·
<https://docs.nvidia.com/cuda/wsl-user-guide/index.html> ·
<https://github.com/microsoft/WSL2-Linux-Kernel> ·
<https://github.com/microsoft/WSL/issues/11050>

## 1. Decisão de convergência

Mantida a direção do `PRD-2`: no WSL2 a GPU é exposta por GPU-PV e `/dev/dxg`;
o guest Linux não controla a placa via DRM/TTM/ReBAR. Logo:

- **MVP:** daemon userspace, backend CUDA (Driver API via `libcuda.so`),
  device de bloco e `swapon`.
- **Fase A (sem kernel custom):** backend `nbd` (único viável neste kernel).
- **Fase B (performance):** backend `ublk`, **somente** com kernel WSL2 custom
  habilitando `CONFIG_BLK_DEV_UBLK`.
- **Fora de escopo WSL2:** `PRD-4` (DAMON) e `PRD-6` (HMM `DEVICE_PRIVATE`).

**Mudança estrutural vs. v1:** a construção do daemon completo passa a ser
**gateada pela Fase 0** (§3). O PRD-2 prometia "saturar PCIe 10–15 GB/s" — claim
de bare-metal. Em GPU-PV, swap é I/O 4 KiB aleatório, latency-bound, com
round-trips de paravirtualização. Não comprometemos o build completo antes de
medir (Kahneman #1 WYSIATI, #3 número não adjetivo, #5 worst-case).

Fontes normativas:
- WSL `.wslconfig` e swap: <https://learn.microsoft.com/windows/wsl/wsl-config>
- NVIDIA CUDA on WSL: <https://docs.nvidia.com/cuda/wsl-user-guide/index.html>
- GPU paravirtualization (WDDM): <https://learn.microsoft.com/windows-hardware/drivers/display/gpu-paravirtualization>

## 2. Evidência local de plataforma (real, datada)

Sondagem desta máquina de desenvolvimento em **2026-06-04**:

```text
kernel:        6.6.114.1-microsoft-standard-WSL2  (osrelease confirma WSL2)
CONFIG_SWAP=y
CONFIG_IO_URING=y          (kernel.io_uring_disabled = 0)
CONFIG_BLK_DEV_NBD=m       (nbd.ko presente, NÃO carregado; /dev/nbd* ausente)
CONFIG_BLK_DEV_UBLK        ausente do /proc/config.gz; modulo ublk_drv inexiste;
                           /dev/ublk-control ausente
RAM:           MemTotal 16376924 kB (~15.6 GiB)
swap atual:    /dev/sdc, 8 GiB, usado ~1.28 GiB no momento, prioridade -2
GPU:           NVIDIA GeForce RTX 2060, 6144 MiB total, ~1862 MiB em uso,
               ~4.2 GiB livres, util 21%, Disp.A On (desktop Windows vivo)
driver:        NVIDIA-SMI 610.43.02, KMD 610.47, CUDA UMD 13.3
/dev/dxg:      presente (char 10,125)
libcuda:       /usr/lib/wsl/lib/libcuda.so presente
/dev/nvidia*:  ausente (esperado no WSL2 — acesso via /dev/dxg + libcuda)
nvidia-smi:    OK (não mais bloqueado)
```

**WYSIATI (#1) — o que NÃO foi verificado ainda:** `nvidia-smi` funcionar
**não** prova que `cuInit(0)` + `cuMemAlloc` + `cuMemGetInfo` funcionam de dentro
de um processo nosso. A verificação de contexto CUDA é responsabilidade do
`ramshared check`/Fase 0 (§3, §6.1) e deve ser executada antes de qualquer
conclusão de prontidão. Confiança calibrada (#6): GPU disponível com ~85% de
probabilidade de `cuInit` ok dado nvidia-smi+`/dev/dxg`+libcuda+UMD 13.3; os 15%
restantes são o motivo de a Fase 0 existir.

**Implicação:** nesta máquina o MVP seleciona `nbd` como único backend possível
sem kernel custom, e `nbd.ko` precisa de `modprobe` explícito (§5, §10). A GPU
está **compartilhada com o desktop Windows vivo** — premissa central da §9.

## 3. Fase 0 — Spike de viabilidade (GATE; resolve F3)

> **Esta fase é um portão.** Nenhum código das §4–§14 é construído antes de a
> Fase 0 passar. O objetivo é provar, com número, que VRAM-swap via GPU-PV é
> uma troca **estritamente melhor** que o swap VHDX que ele substituiria —
> mesmo papel, então pior latência = pior produto.

### 3.1 Artefato — rodar a referência, não construir spike (Research & Reuse)

**Não construir spike novo.** O caminho mais barato e honesto (WYSIATI) é medir
com a referência provada `c0deJedi/nbd-vram` (§0.2), que já é daemon CUDA + NBD:

1. **Provar que roda em WSL2/GPU-PV** (a referência só foi validada em bare-metal):
   `sudo modprobe nbd nbds_max=1`; `git clone` + `make` (`gcc -O2 ... -ldl`);
   `sudo bash test-nbd.sh` (aloca VRAM, conecta `/dev/nbd0`, write/readback de
   1 MiB, ativa swap). Falha aqui = sinal precoce de incompatibilidade GPU-PV;
   registrar e reavaliar antes de qualquer linha de código nova.
2. **Medir vs. baseline VHDX** com os próprios scripts da referência, que já
   rodam NVMe primeiro e VRAM depois — exatamente o comparativo do GATE-PERF:
   `bench-latency.sh` (`ioping` 4K), `bench-iops.sh` (`fio` libaio iodepth=32),
   `bench-throughput.sh` (`dd` O_DIRECT, 2 GiB). **(OQ2)**
3. **Probe de residência (§9.4):** durante a medição, induzir pressão de VRAM no
   host (abrir app GPU pesada no Windows) e observar integridade/latência.

### 3.2 Abort trigger numérico (Kahneman #2, #3, #5)

```text
GATE-PERF:
  Seja  W99_vram  = p99 de escrita 4 KiB HtoD via CUDA/GPU-PV
  Seja  W99_vhdx  = p99 de escrita 4 KiB no /dev/sdc (baseline)
  Se    W99_vram > W99_vhdx          -> ABORT da linha de swap-cru.
  (margem: exigir W99_vram <= 0.8 * W99_vhdx para "go" confortável;
   0.8*W99_vhdx < W99_vram <= W99_vhdx => "go condicional", revisar com usuário.)

GATE-RESIDENCIA (ver §9):
  Rodar o cenário de pressão de host (§9.3) durante o spike.
  Se a alocação for evictada SEM sinal detectável -> ABORT (risco de corrupção
  silenciosa não mitigável no MVP).
```

### 3.2.1 Expectativa ancorada na referência (anti-anchoring, #4)

A `nbd-vram` reporta, em bare-metal RTX 3070 vs. NVMe cryptswap PCIe 4.0:
latência ~**27× melhor** que NVMe para acesso esporádico, mas **throughput
sequencial pior** (overhead de socket Unix + cuMemcpy). Em WSL2 espere **pior que
bare-metal** pelo round-trip GPU-PV — por isso o gate mede *nesta* máquina e não
confia no número da referência (confiança calibrada, #6). Se o resultado WSL2
ficar próximo do VHDX, é o `dropCache`/`autoMemoryReclaim` e o paravirt comendo a
vantagem; nesse caso seguir para o galho zram-tiering (§16).

### 3.3 Saída da Fase 0

- `go`  → seguir para §4 (build do daemon).
- `abort por GATE-PERF` → **não** construir o daemon de swap cru. Registrar
  números e abrir decisão de pivô (galhos documentados em §16): (a) PRD novo de
  **zram com writeback para VRAM** (amortiza round-trips), (b) PRD-5/userfaultfd
  adaptado. Voltar ao **Passo 1 (PRD)** com os dados medidos como evidência.
- `abort por GATE-RESIDENCIA` → idem; o problema é de segurança de dados, não de
  performance.

Toda conclusão da Fase 0 é registrada em `docs/vram-as-ram/FASE0-RESULTS.md`
(ambiente: CPU, RAM, kernel, n de rodadas, stddev — contra "métrica sem
contexto", contra-exemplo #3 das disciplinas).

**RESULTADO 2026-06-04 (1ª rodada, ver `FASE0-RESULTS.md`): `go` CONDICIONAL.**
VRAM venceu o caminho de swap-out (escrita): p99 14.7 ms vs VHDX 183 ms (12.5×),
IOPS 2.6×, seq 3.5–6.3×. GATE-PERF passou confortável (`14.7 ≤ 0.8·183`). Leitura
do baseline foi cache-inflada (rever com `drop_caches`).

**CONSOLIDADO 2026-06-04 (ver `FASE0-FINAL.md`): `go` COM PIVÔ ARQUITETURAL.**
Eviction WDDM (§9): dado **sobrevive** (hash ok) mas latência 4K saltou a **1,18 s**
sob VRAM cheia → VRAM é **data-safe, latency-unsafe**; não serve como swap quente
único. Tiering (Part C) **provado**: zram(prio200)→VRAM(prio100)→VHDX, VRAM
absorveu 983 MiB de spill com VHDX intocado. **Recomendação: mudar o MVP** de
"VRAM swap cru prio 32767" para a **cascata zram(hot)→VRAM(cold)→VHDX** — decisão
de arquitetura para o usuário (volta ao Passo 2 se aceita).

## 4. Objetivo de implementação (após `go` da Fase 0)

Dois binários:

- `ramshared`: CLI operacional (`check`, `start`, `swapon`, `stop`, `recover`,
  `test-integrity`, `bench`).
- `ramshared-wsl2d`: daemon userspace que reserva VRAM via CUDA e atende I/O de
  bloco por `nbd` (Fase A) ou `ublk` (Fase B).

Alvo de uso manual (sem auto-start):

```sh
sudo ramshared check
sudo ramshared start --size 512M --backend nbd
sudo ramshared swapon --priority 32767
sudo ramshared stop
```

## 5. Árvore de código a criar (corrige F9)

```text
Cargo.toml
crates/ramshared-cli/
  Cargo.toml
  src/main.rs
  src/commands/
    mod.rs
    check.rs
    start.rs
    swapon.rs
    stop.rs
    recover.rs
    test_integrity.rs
    bench.rs
crates/ramshared-wsl2d/
  Cargo.toml
  src/main.rs
  src/config.rs
  src/daemon.rs
  src/preflight.rs
  src/state.rs
  src/residency.rs        # NOVO — canário e detecção de eviction WDDM (§9)
crates/ramshared-cuda/
  Cargo.toml
  build.rs
  src/lib.rs
  src/driver.rs
  src/pool.rs
  src/staging.rs
crates/ramshared-block/
  Cargo.toml
  src/lib.rs
  src/nbd.rs
  src/ublk.rs
  src/request.rs
  src/inflight.rs         # NOVO — mapa de blocos em voo p/ atomicidade (§8)
crates/ramshared-integrity/
  Cargo.toml
  src/lib.rs
  src/pattern.rs
  src/hash.rs
docs/vram-as-ram/SPECv2-WSL2.md
docs/vram-as-ram/FASE0-RESULTS.md   # produzido pela Fase 0
```

Controle principal em Rust. CUDA via Driver API FFI (`libcuda.so`), sem depender
de toolkit. Wrappers `unsafe` isolados em `ramshared-cuda` com invariantes em
comentários curtos. **Sem `.unwrap()`/`.expect()` em código de produção** (regra
`coding.md`); erros via `Result<T, Error>`.

## 6. Contrato da CLI (vagueza eliminada — corrige F5, F7, F9, F11)

### 6.1 `ramshared check`

1. **WSL2:** `osrelease`/`uname -r` contém `microsoft-standard-WSL2`; coletar
   `/proc/version` para diagnóstico.
2. **Swap atual:** ler `/proc/swaps` + `swapon --show --bytes`; reportar device,
   size, used, prio. **Critério observável (corrige F7):** se
   `used/size >= 0.90` no swap VHDX, emitir `WARN swap-pressao` no relatório.
3. **GPU-PV:** exigir `/dev/dxg`; resolver `libcuda.so` via
   `/usr/lib/wsl/lib/libcuda.so` **ou** `dlopen("libcuda.so.1")` (critério de
   "resolução via loader", corrige F7); `cuInit(0)`; listar ≥ 1 `CUdevice`;
   `cuMemGetInfo` (total/livre).
4. **`nvidia-smi` quando presente:** se `--query` retornar
   `"blocked by the operating system"` → abortar; se NVML indisponível mas CUDA
   ok → `WARN nvml-ausente` e continuar; se NVML reportar reset/MIG/insuficiente
   → abortar.
5. **Backend de bloco:**
   - `nbd`: `CONFIG_BLK_DEV_NBD=y|m`. Se `=m` e não carregado, marcar
     `nbd=needs-modprobe` (não é falha). Não tentar `modprobe` em `check`.
   - `ublk`: `ok` **somente** com `CONFIG_BLK_DEV_UBLK=y|m` **e**
     `/dev/ublk-control` **e** `io_uring_disabled=0`. Neste kernel: `fail`.

Saída texto obrigatória (e `--json` com os mesmos campos):

```text
WSL2: ok|fail
CUDA: ok|fail (cuInit + cuMemGetInfo)
GPU: <nome>, total=<MiB>, livre=<MiB>
Swap atual: <device>, size=<MiB>, used=<MiB>, prio=<N>[, WARN swap-pressao]
Backends: nbd=<ok|needs-modprobe|fail>, ublk=<ok|fail>
Decisao: ready|blocked
```

`Decisao: blocked` ⇔ qualquer um: WSL2 fail, CUDA fail, `/dev/dxg` ausente,
nvidia-smi bloqueado, ou nenhum backend utilizável. Exit code != 0 quando
`blocked`.

### 6.2 `ramshared start`

Flags: `--size`, `--backend <auto|nbd|ublk>`, `--device`, `--block-size`
(default 4096), `--queue-depth` (default 32/nbd, 64/ublk), `--debug-checksum`,
`--foreground`, `--force-large`.

Sequência obrigatória (abortar e **não deixar device parcial** se qualquer passo
falhar antes do passo 7):

1. Rodar o preflight de `check`. Abort se `blocked`.
2. **Alocação com backoff (adotado da referência, §0.2):** tentar `--size`; se
   CUDA retornar out-of-memory, **recuar em passos de 512 MiB** até um piso
   (`--min-size`, default `256M`) antes de abortar — a GPU é compartilhada com o
   desktop vivo e a VRAM livre flutua (OQ3). Invariantes mantidas: teto sem
   `--force-large` `1G`; ≥ `1G` de VRAM livre após a reserva efetiva; nunca > 25%
   da VRAM livre. Registrar o tamanho **efetivamente** alocado no estado e no log.
3. **Resiliência (corrige F7 — sem "se necessário"):**
   - `setrlimit(RLIMIT_MEMLOCK, infinity)`; abort se falhar e não houver
     `--force-large`.
   - `mlockall(MCL_CURRENT | MCL_FUTURE)`; **checar retorno; falha = abort**.
   - escrever `-1000` em `/proc/self/oom_score_adj`; **falha = abort** (sem
     override silencioso).
4. **CUDA:** `cuInit(0)`; device 0; criar contexto; `cuMemGetInfo`;
   `cuMemAlloc(size)`; `cuMemsetD8Async(...,0,size)` + sync.
5. **Staging:** ring de buffers host pinned via `cuMemHostAlloc`,
   pré-alocado. **Zero `malloc`/`Vec::push` sem reserva no hot path.**
6. **Residência (§9):** alocar região canário, gravar padrão, iniciar amostrador.
7. **Backend de bloco:**
   - `nbd`: garantir módulo (§10) → configurar `/dev/nbdX` (size, blksize,
     socketpair).
   - `ublk`: criar via ublk control + filas.
8. **Publicar estado:** criar `/run/ramshared/wsl2d.json` **e
   `/run/ramshared/wsl2d.pid`** (corrige F9), perms `0600`; gravar PID, backend,
   device, size, blksize, CUDA device UUID (quando disponível), estado
   `BlockReady`.
9. Imprimir o comando `ramshared swapon` correspondente.

### 6.3 `ramshared swapon`

1. Ler `wsl2d.json`; confirmar daemon vivo (PID + socket).
2. `mkswap <device>` **somente se** o device não tiver assinatura de swap com o
   label `RAMSHARED` (corrige F11 — checar via `blkid`/leitura do superblock de
   swap; o label é gravado por nós no `mkswap -L RAMSHARED`).
3. `swapon --priority 32767 <device>`.
4. Confirmar em `/proc/swaps` que a prioridade > prioridade do swap VHDX.

### 6.4 `ramshared stop` (rollback em camadas — corrige F6)

Ordem inversa da alocação, **três camadas explícitas**:

1. **Camada swap:** se device em `/proc/swaps` → `swapoff <device>` (bloqueante;
   o kernel migra páginas de volta). Timeout observável: ver §13.
2. **Camada daemon:** shutdown gracioso via Unix socket
   `/run/ramshared/wsl2d.sock`: parar de aceitar requisições; drenar pendentes
   (limite: §13); parar amostrador de residência.
3. **Camada dados-em-VRAM:** `cuMemsetD8Async(...,0,size)` + sync; desconectar
   `nbd`/remover `ublk`; liberar staging pinned; liberar `CUdeviceptr`.
4. Remover `/run/ramshared/*` (json, pid, sock).

### 6.5 `ramshared recover`

Falhas duras — ver fluxo escalonado completo na **§13**.

## 7. Daemon — máquina de estados

```text
Init -> PreflightOk -> MemoryLocked -> CudaReady -> VramAllocated
     -> ResidencyArmed -> BlockReady -> SwapActive
     -> Stopping -> (Init|fim)
     -> Failed  (de qualquer estado)
```

Transições inválidas abortam o processo (ex.: `BlockReady` antes de
`VramAllocated`, ou `SwapActive` sem `ResidencyArmed`).

## 8. Modelo de endereçamento e I/O CUDA (atomicidade explícita — corrige F4)

- Size do device múltiplo de 4096; `logical_block_size = 4096`.
- Offset `off` → `vram_base + off`. Fora de faixa = erro de I/O. Desalinhado =
  rejeitado no backend antes de tocar CUDA.

### 8.1 Modelo de atomicidade/ordenação (MVP, Day-0 seguro)

- **Stream único ordenado** para o hot path no MVP (QD pipeliniza blocos
  **distintos**, não reordena conclusão dentro do mesmo bloco).
- **Mapa de blocos em voo** (`ramshared-block/inflight.rs`): requisição a um
  bloco com operação em voo no **mesmo** bloco é serializada atrás dela. Garante
  ausência de leitura torn e ausência de write-after-write reordenada.
- **Staging por slot:** cada requisição em voo possui slot de staging próprio; a
  requisição bloqueia até haver slot livre (sem wrap destrutivo do ring).
- **Durabilidade-em-VRAM antes do complete:** escrita = `cuMemcpyHtoDAsync` →
  `cuEventRecord` → completar o I/O do block layer **somente** após o evento ser
  observado retirado (`cuEventSynchronize`/poll). Leitura = `cuMemcpyDtoHAsync`
  → evento → copiar staging→backend → complete.
- `--debug-checksum`: hash por bloco em tabela pré-alocada por índice; em leitura,
  divergência = erro de I/O (não persiste payload — §11).

### 8.2 Erros CUDA

- `CUDA_ERROR_OUT_OF_MEMORY` na init → abortar antes de expor device; no hot path
  → I/O error + `Failed`.
- `CONTEXT_IS_DESTROYED`/reset/device lost/GPU bloqueada → `Failed`, parar filas,
  exigir `recover`.
- Erro desconhecido no hot path → I/O error. **Nunca** sucesso parcial.

## 9. Residência e eviction WDDM/GPU-PV (NOVO — resolve F2)

**Premissa (calibrada, #6):** no WSL2 a VRAM é virtualizada pelo WDDM no host.
Sob pressão de VRAM no Windows (jogo, compositor, outra app GPU), o host **pode
migrar/evictar uma alocação CUDA viva** para o pagefile do Windows **sem** gerar
erro CUDA. Para swap-de-guest isso significa swap-sobre-swap (latência
catastrófica) ou, se a região for invalidada, **corrupção silenciosa** de páginas
swapadas → processo do guest corrompido/morto. CUDA no WSL **não** oferece um
"pin resident" garantido; pior, o WSL2 **não respeita** a *CUDA Sysmem Fallback
Policy* (microsoft/WSL #11050), então não controlamos se/quando uma alocação
transborda para sysmem. Logo mitigamos por **detecção + abort**, não por promessa.
A referência `nbd-vram` roda em bare-metal e **não** trata este modo — §9 é o
valor agregado da variante WSL2.

### 9.1 Canário

Na alocação (§6.2 passo 6), reservar uma região canário pequena (ex.: 1 MiB)
dentro da VRAM, gravar um padrão conhecido + timestamp lógico.

### 9.2 Amostrador

Thread dedicada amostra o canário a cada `T_sample` (default 250 ms):
`cuMemcpyDtoH` do canário, medir latência e verificar conteúdo. Coletar baseline
de latência do canário logo após `VramAllocated` (mediana de N amostras).

### 9.3 Abort trigger (Kahneman #2, #5)

```text
RESIDENCIA-ABORT, se qualquer:
  (a) conteúdo do canário != padrão gravado           -> eviction/invalidação
  (b) latência do canário p99 > 8x baseline por >= 3   -> suspeita de page-out
      amostras consecutivas                               host
  (c) cuMemGetInfo free cai abaixo de floor reservado  -> host reaver VRAM
Ação: marcar Failed -> entrar Stopping -> swapoff -> NÃO confiar em dados.
```

(Os multiplicadores `8x`/`3 amostras`/`floor` são parametrizáveis e calibrados
na Fase 0 §3 com o GATE-RESIDENCIA; valores aqui são default inicial.)

### 9.4 Cenário de pressão (rodado na Fase 0 e como teste — §14.6)

Com a alocação ativa, induzir pressão de VRAM no host (abrir app GPU pesada **ou**
alocar VRAM concorrente) e observar se (a)/(b)/(c) disparam **antes** de qualquer
corrupção observável. Se a eviction ocorrer sem sinal detectável → **GATE-
RESIDENCIA reprova** (§3.2): o MVP de swap cru é inseguro nesta plataforma.

### 9.5 Resultado empírico (Fase 0, 2026-06-04)

Probe executado: 1 GiB alocado pela `nbd-vram`, canário de 256 MiB, e um
`vramhog` CUDA forçando a VRAM a **0 MiB livre** (alocou +4096 MiB). Observado:

- **Dado SOBREVIVE:** verificação final de 256 MiB com hash **idêntico** — sem
  corrupção. O host paginou a alocação para sysmem e trouxe de volta.
- **Latência EXPLODE:** uma leitura 4K do canário sob pressão saltou para
  **1 183 094 µs (~1,18 s)** vs. ~3–4 ms normal (≈330×), recuperando logo após.

Conclusão para a §9: no WSL2 o risco dominante **não é corrupção, é latência** —
uma página em swap na VRAM pode custar **>1 s** para retornar se o host estiver
sob pressão de VRAM (jogo/compositor). Para swap, um stall de 1,2 s por página
**congela** o processo. Logo:

- O canário deve disparar `RESIDENCIA-ABORT` também por **latência** (gatilho (b)
  da §9.3 confirmado como o relevante), não só por corrupção.
- VRAM-as-swap-cru é **data-safe** porém **latency-unsafe** sob contenção de GPU
  do host. Reforça o tiering (§16): VRAM como tier de **páginas frias** atrás de
  zram, nunca como único swap quente. Ver `FASE0-FINAL.md`.

## 10. Backends

### 10.1 NBD (Fase A — corrige F5, F10)

- **Módulo:** se `nbd` não carregado, a CLI executa `modprobe nbd nbds_max=1`
  (root), registra que **nós** carregamos (para descarregar no stop só nesse
  caso). Falha de `modprobe` = mensagem objetiva + abort. Em WSL2 com udev
  mínimo, criar o node `/dev/nbd0` via `mknod` se ausente após modprobe.
- Abrir `/dev/nbdX`; `socketpair(AF_UNIX, SOCK_STREAM)`; `NBD_SET_SOCK`,
  `NBD_SET_BLKSIZE`, `NBD_SET_SIZE_BLOCKS`. **Flush/trim:** anunciar
  `NBD_FLAG_SEND_FLUSH`/`TRIM` **somente** se o handler os implementar — critério
  observável, não "se suportadas" (corrige F7). `NBD_DO_IT` em thread dedicada.
- Stop/recover: `NBD_DISCONNECT`, `NBD_CLEAR_SOCK`, fechar FDs, e `rmmod nbd` só
  se nós carregamos.

### 10.2 ublk (Fase B)

- Exigir `CONFIG_BLK_DEV_UBLK=y|m` + `/dev/ublk-control` + `io_uring`. **Neste
  kernel: indisponível.** Mesma interface `BlockBackend` do `nbd`.
- `--backend auto`: se `ublk` ausente e `nbd` presente → escolher `nbd` **e
  logar** `INFO backend-downgrade: ublk ausente (CONFIG_BLK_DEV_UBLK not set),
  usando nbd` (corrige F10, Day-0 sem dual-path silencioso).

**Receita concreta de habilitação (confirmada por pesquisa, §0.2):**

```sh
git clone --depth=1 --branch linux-msft-wsl-6.6.y \
  https://github.com/microsoft/WSL2-Linux-Kernel.git
# adicionar  CONFIG_BLK_DEV_UBLK=y  em Microsoft/config-wsl
make -j"$(nproc)" KCONFIG_CONFIG=Microsoft/config-wsl
cp arch/x86/boot/bzImage /mnt/c/Users/<user>/bzImage-ublk
#  %USERPROFILE%\.wslconfig  →  [wsl2]  kernel=C:\\Users\\<user>\\bzImage-ublk
#  wsl --shutdown
```

Alternativa sem trocar a imagem inteira: compilar `ublk_drv` como módulo e servir
via `.wslconfig kernelModules=<modules.vhdx>` (gerado por `gen_modules_vhdx.sh`).
Day-0: versionar o `config-wsl` custom no repo e registrar a versão do kernel.

## 11. Limites de segurança

- Sem auto-start no boot. Sem usar os 6 GiB inteiros. Default `512M`; teto sem
  `--force-large` `1G`; margem ≥ `1G` de VRAM livre após reserva.
- `mlockall` antes de qualquer fila de I/O; `oom_score_adj=-1000` ou abort.
- `swapoff` antes de encerrar graciosamente. `SIGTERM` com swap ativo → fluxo
  `Stopping` (não shutdown imediato). `SIGKILL` = falha dura recuperável só por
  `recover` ou reinício do guest.
- Zerar VRAM ao alocar e ao liberar. **Isolamento (#4):** processo root-only;
  socket `0600`; sem API de leitura arbitrária por offset a não-root; checksum
  debug não persiste payload.
- Não guardar dados de swap fora da VRAM.

## 12. Disciplinas Kahneman por passo crítico (resolve F8)

| Passo crítico | Disciplina | Evidência mínima | Abort trigger |
|---|---|---|---|
| Reserva de VRAM (§6.2.4) | #3 número | `cuMemGetInfo` antes/depois logado | falha de alloc ou free < floor → abort |
| Lock de memória (§6.2.3) | #3 substituição→prova | retorno de `mlockall` checado | retorno != 0 → abort |
| Residência (§9) | #2 counterfactual, #5 worst-case | canário + baseline de latência | RESIDENCIA-ABORT (§9.3) |
| Viabilidade (§3) | #1 WYSIATI, #3, #5 | `FASE0-RESULTS.md` com n rodadas + stddev | GATE-PERF / GATE-RESIDENCIA (§3.2) |
| `swapon` (§6.3) | #2 counterfactual | prioridade em `/proc/swaps` | prio RamShared <= prio VHDX → reverter |
| Hot path I/O (§8) | #13 ilusão de validade | teste de integridade com modo de falha real | divergência de hash → I/O error |
| Detecção (§6.1) | #13 | teste `ready` **e** `blocked` (§14.1) | — |

## 13. Recovery em ambiente compartilhado (resolve F6)

`wsl --terminate` mata **toda** a distro (shells, editores, builds). É último
recurso, com colateral explícito. Escalonamento:

1. Mostrar estado de `wsl2d.json`/`.pid`.
2. `swapoff <device>` com timeout observável: se não retornar em **`T_swapoff`
   (default 120 s)**, seguir ao passo 3 (não esperar indefinidamente).
3. `nbd`: ioctl de disconnect (equivalente a `nbd-client -d`); `ublk`: delete no
   control.
4. Liberar VRAM/staging quando o contexto CUDA ainda responder.
5. **Critério de "I/O travado" (corrige F7):** se após o passo 2/3 houver
   processos em `D` (uninterruptible sleep) referenciando o device por
   **> `T_stuck` (default 60 s)** e/ou `dmesg` mostrar `nbd: ... timed out`,
   então — e só então — imprimir, com aviso de colateral:

```powershell
# ÚLTIMO RECURSO: reinicia o guest WSL inteiro (mata TODOS os processos da distro).
# NÃO afeta o Windows host. Salve trabalho aberto antes.
wsl --terminate <DistroName>
```

**(OQ1 CONFIRMADO 2026-06-04: esta distro É a sessão primária → colateral de
`wsl --terminate` é alto (mata editores/builds em curso). Recovery DEVE esgotar
opções in-guest e só então sugerir o terminate, com aviso explícito.)**

## 14. Testes de aceitação (corrige F1, F4, #13)

### 14.1 Detecção (corrige F1)
- `ramshared check` e `--json`. Aceite: reporta kernel WSL2, swap atual, CUDA +
  VRAM total/livre, `nbd`/`ublk` separados.
- **Pareado (#13):** nesta máquina (GPU disponível) → `Decisao: ready`, exit 0.
  Simular indisponibilidade (ex.: `CUDA_VISIBLE_DEVICES=""` ou `/dev/dxg`
  inacessível em sandbox) → `Decisao: blocked`, exit != 0. **Ambos** devem passar.

### 14.2 Integridade sem swap (corrige F4)
- `start --size 512M --backend nbd --debug-checksum` →
  `test-integrity --duration 30m --pattern random` → `stop`.
- Aceite: hash por bloco sem divergência; **incluir padrão de
  blocos sobrepostos concorrentes** (QD>1 no mesmo bloco) para exercitar §8.1;
  zero erro CUDA; stop limpa device e VRAM.

### 14.3 Swap real
- `start` → `swapon --priority 32767` → `stress-ng --vm 2 --vm-bytes 75%
  --timeout 10m`, comparando `/proc/vmstat pswpin/pswpout` antes/depois.
- Aceite: device RamShared em `/proc/swaps` com prio > VHDX; contadores mudam;
  RamShared usado antes do VHDX; sem I/O error em `dmesg`.

### 14.4 Falha controlada (SIGTERM)
- Aceite: `SIGTERM` inicia `Stopping`; `swapoff` antes de liberar VRAM; device
  some de `/proc/swaps`; daemon sai 0.

### 14.5 Falha dura (SIGKILL)
- `SIGKILL` no daemon → `recover`. Aceite: recover tenta `swapoff`; se kernel
  travar em I/O, segue o escalonamento §13; Windows host intacto.

### 14.6 Eviction de residência (NOVO, #13)
- Cenário §9.4. Aceite: ao induzir pressão de VRAM no host, um dos sinais
  (a)/(b)/(c) de §9.3 dispara **antes** de qualquer corrupção observável, levando
  a `Failed`+`Stopping`. Se não disparar e houver corrupção → teste **falha** (e
  GATE-RESIDENCIA da Fase 0 deveria ter barrado).

### 14.7 Performance
- `bench --backend nbd --size 512M --seq --rand`: throughput seq R/W, IOPS 4K
  aleatório, latência p50/p95/p99, comparativo vs. VHDX. Reportar ambiente.

## 15. Critérios de pronto

- Fase 0 (§3) com `go` registrado em `FASE0-RESULTS.md`.
- `cargo fmt --all` sem diff; `cargo clippy --workspace --all-targets -- -D
  warnings` verde; `cargo test --workspace` verde.
- `ramshared check` cobre `ready` (GPU disponível) **e** `blocked` (simulado).
- `test-integrity` verde 30 min com `512M`, incluindo blocos sobrepostos.
- Teste de residência §14.6 verde.
- `stop` deixa `/proc/swaps` sem o device; recovery §13 testado ao menos uma vez.

## 16. Não objetivos e galhos de pivô documentados

Não objetivos: emprestar VRAM ao host; módulo de kernel no MVP; DAMON/HMM/TTM/
ReBAR/NUMA fake no WSL2; auto-start; prometer preservação de processos se a
GPU-PV falhar com páginas só no swap RamShared.

**Galhos de pivô (só viram PRD se a Fase 0 reprovar — não criar antes do dado):**
- **zram + writeback para VRAM:** zram comprimido em RAM como swap primário, com
  `backing_dev` = device-VRAM para spill de páginas frias/incompressíveis.
  Amortiza round-trips GPU-PV (páginas quentes ficam em RAM comprimida). Exige
  probe de `CONFIG_ZRAM`. Candidato natural se GATE-PERF reprovar por latência de
  página quente.
- **userfaultfd (PRD-5 adaptado):** granularidade de página em userspace, sem
  block layer. Candidato se o overhead do block layer dominar.
- **Tiering multi-camada por prioridade — PROVADO na Fase 0 (Part C), candidato a
  virar o MVP:** a ordem correta (corrigida pelo achado da §9.5) é
  **RAM → zram (prio alta, HOT) → VRAM (prio média, COLD) → VHDX (prio baixa)**.
  zram (RAM comprimida, baixa latência) absorve o working set quente; a VRAM pega
  só o spill frio — escondendo sua fraqueza de latência sob pressão (§9.5) e
  usando sua força (bandwidth/capacidade). Medido: zram encheu 1024 MiB e a VRAM
  absorveu 983 MiB de overflow, VHDX intocado. `CONFIG_ZRAM_WRITEBACK` **não** está
  setado neste kernel, então a integração por *writeback* exigiria kernel custom;
  a cascata por **prioridade de `swapon`** (o que foi provado) é Day-0 e não exige
  nada. Pós-MVP: `swap=0` no `.wslconfig` para o RamShared assumir o swap.

## 17. Open questions herdadas da auditoria (precisam de decisão do usuário)

- **OQ1** — ✅ RESOLVIDA (2026-06-04): distro É a sessão primária → colateral alto
  no `wsl --terminate`; recovery escalonado é mandatório (§13).
- **OQ2** — ⚠️ PARCIAL (2026-06-04): baseline medido em `/dev/sdd` (proxy do
  swap VHDX). Leitura cache-inflada (host RAM), escrita possivelmente sparse-VHDX.
  Falta baseline justo (`drop_caches` + arquivo pré-alocado). Ver `FASE0-RESULTS.md`.
- **OQ3** — Teto de VRAM aceitável com a GPU compartilhada com o desktop vivo
  (~4.2 GiB livres), §6.2/§9.
- **OQ4** — Apetite para zram-writeback (§16) como Fase 2 desde já, ou manter
  swap cru no MVP.
