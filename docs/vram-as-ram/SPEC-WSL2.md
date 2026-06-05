---
slug: vram-wsl2-cuda-swap
title: VRAM emprestada como swap seguro no WSL2 via CUDA
source_prd: PRD-2.md
variant: WSL2/GPU-PV/CUDA
milestone: M01
status: draft-executable
---

# SPEC-WSL2 — VRAM emprestada como swap seguro no WSL2 via CUDA

## Decisao de convergencia

Esta SPEC adapta o `PRD-2` para WSL2. No WSL2, a GPU e exposta por
GPU-PV e pelo device paravirtualizado `/dev/dxg`; o guest Linux nao controla a
placa como bare metal via DRM/TTM/ReBAR. Portanto:

- **Escolha implementavel para MVP:** daemon em userspace, backend CUDA,
  dispositivo de bloco e `swapon`.
- **Fase A sem kernel customizado:** backend `nbd`, quando
  `CONFIG_BLK_DEV_NBD` existir.
- **Fase B de performance:** backend `ublk`, somente quando o kernel WSL2
  customizado habilitar `CONFIG_BLK_DEV_UBLK`.
- **Fora do escopo WSL2 inicial:** `PRD-4` com DAMON/tiering e `PRD-6` com HMM
  `DEVICE_PRIVATE`, porque dependem de integracao direta com driver DRM e
  controle de memoria de dispositivo pelo Linux guest.

Fontes normativas usadas para esta variante:

- Microsoft WSL `.wslconfig` e swap: <https://learn.microsoft.com/windows/wsl/wsl-config>
- NVIDIA CUDA on WSL: <https://docs.nvidia.com/cuda/wsl-user-guide/index.html>
- Microsoft GPU paravirtualization: <https://learn.microsoft.com/windows-hardware/drivers/display/gpu-paravirtualization>

## Evidencia local de plataforma

Sondagem desta maquina de desenvolvimento:

```text
kernel: Linux 6.6.87.2-microsoft-standard-WSL2
CONFIG_SWAP=y
CONFIG_IO_URING=y
CONFIG_BLK_DEV_NBD=m
CONFIG_BLK_DEV_UBLK is not set
swap atual: /dev/sdc, 8 GiB, prioridade -2
libcuda: /usr/lib/wsl/lib/libcuda.so presente
/dev/dxg: ausente nesta sessao
nvidia-smi: GPU access blocked by the operating system
```

Implicacao: nesta maquina, o MVP deve selecionar `nbd` como unico backend de
bloco possivel sem kernel customizado, mas o `ramshared check` deve abortar a
execucao enquanto `/dev/dxg` estiver ausente ou CUDA/NVML reportar GPU bloqueada
pelo sistema operacional.

## Objetivo de implementacao

Criar dois binarios:

- `ramshared`: CLI operacional para preflight, start, swapon, stop, recovery e
  testes.
- `ramshared-wsl2d`: daemon userspace que reserva VRAM via CUDA e atende I/O de
  bloco por `nbd` ou `ublk`.

O primeiro alvo de uso e manual:

```sh
sudo ramshared check
sudo ramshared start --size 512M --backend nbd
sudo ramshared swapon --priority 32767
sudo ramshared stop
```

Nao instalar servico auto-start nesta fase.

## Arvore de codigo a criar

```text
Cargo.toml
crates/ramshared-cli/
  Cargo.toml
  src/main.rs
crates/ramshared-wsl2d/
  Cargo.toml
  src/main.rs
  src/config.rs
  src/daemon.rs
  src/preflight.rs
  src/state.rs
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
crates/ramshared-integrity/
  Cargo.toml
  src/lib.rs
  src/pattern.rs
  src/hash.rs
docs/vram-as-ram/SPEC-WSL2.md
```

Rust deve ser usado para o controle principal. Chamadas CUDA devem usar a Driver
API via FFI (`libcuda.so`) e nao depender de toolkit instalado alem da biblioteca
stubada pelo WSL. Qualquer wrapper inseguro deve ficar isolado em
`ramshared-cuda`, com invariantes documentadas em comentarios curtos.

## Contrato da CLI

### `ramshared check`

Responsabilidades:

1. Confirmar WSL2:
   - `uname -r` contem `microsoft-standard-WSL2` ou `/proc/sys/kernel/osrelease`
     contem `WSL2`.
   - `/proc/version` deve ser coletado para diagnostico.
2. Confirmar swap atual:
   - ler `/proc/swaps` e `swapon --show --bytes`;
   - reportar device, tamanho, uso e prioridade;
   - avisar quando swap VHDX ja estiver muito usado.
3. Confirmar GPU-PV:
   - exigir `/dev/dxg`;
   - exigir `/usr/lib/wsl/lib/libcuda.so` ou resolucao equivalente via loader;
   - executar `cuInit(0)` e listar ao menos um `CUdevice`;
   - coletar VRAM total/livre via `cuMemGetInfo`.
4. Confirmar `nvidia-smi` quando disponivel:
   - se retornar "GPU access blocked by the operating system", abortar;
   - se NVML estiver indisponivel mas CUDA funcionar, emitir aviso e continuar;
   - se NVML reportar reset, MIG/reconfiguracao ou memoria insuficiente, abortar.
5. Confirmar backend de bloco:
   - `nbd`: aceitar `CONFIG_BLK_DEV_NBD=y|m` e `/dev/nbd*` disponivel, ou
     `modprobe nbd` possivel;
   - `ublk`: aceitar somente com `CONFIG_BLK_DEV_UBLK=y|m`,
     `/dev/ublk-control` e `io_uring` funcional.

Saida texto obrigatoria:

```text
WSL2: ok|fail
CUDA: ok|fail
GPU: <nome>, total=<MiB>, livre=<MiB>
Swap atual: <device>, size=<MiB>, used=<MiB>, prio=<N>
Backends: nbd=<ok|fail>, ublk=<ok|fail>
Decisao: ready|blocked
```

Tambem implementar `--json` com os mesmos campos para automacao.

### `ramshared start`

Flags:

```text
--size <SIZE>          obrigatorio; MVP recomendado: 512M ou 1G
--backend <auto|nbd|ublk>
--device <path>        opcional; ex: /dev/nbd0
--block-size <bytes>   default: 4096
--queue-depth <N>      default: 32 para nbd, 64 para ublk
--debug-checksum       ativa checksum por bloco
--foreground           nao daemoniza
```

Sequencia obrigatoria:

1. Rodar o mesmo preflight de `ramshared check`.
2. Aplicar limite conservador:
   - default operacional: `512M`;
   - maximo sem `--force-large`: `1G`;
   - apos alocar, manter pelo menos `1G` de VRAM livre;
   - nunca alocar mais que 25% da VRAM livre em modo MVP.
3. Elevar resiliencia do daemon:
   - chamar `setrlimit(RLIMIT_MEMLOCK)` quando permissao permitir;
   - chamar `mlockall(MCL_CURRENT | MCL_FUTURE)`;
   - verificar retorno de `mlockall` e abortar em erro;
   - escrever `-1000` em `/proc/self/oom_score_adj` e abortar se falhar sem
     override explicito.
4. Inicializar CUDA:
   - `cuInit(0)`;
   - selecionar device 0 por default;
   - criar contexto;
   - obter memoria livre/total;
   - reservar `CUdeviceptr` com `cuMemAlloc(size)`;
   - limpar a regiao com `cuMemsetD8Async(..., 0, size)` e sincronizar.
5. Pre-alocar staging:
   - ring de buffers host pinned com `cuMemHostAlloc`;
   - nenhum `malloc`, `Vec::push` sem reserva, ou alocacao de heap no hot path;
   - toda falha de alocacao antes do device ficar visivel ao kernel.
6. Criar backend de bloco:
   - `nbd`: configurar `/dev/nbdX` com tamanho, block size e socketpair;
   - `ublk`: criar device via ublk control e registrar filas.
7. Publicar estado:
   - criar `/run/ramshared/wsl2d.json`;
   - gravar PID, backend, device, size, block size, CUDA device UUID quando
     disponivel e estado `BlockReady`.
8. Imprimir o comando `ramshared swapon` correspondente.

Abortar antes do passo 6 se qualquer requisito de CUDA, VRAM, memoria travada ou
backend falhar. Nao deixar device parcialmente criado.

### `ramshared swapon`

Responsabilidades:

1. Ler `/run/ramshared/wsl2d.json`.
2. Conferir que o daemon esta vivo.
3. Rodar `mkswap <device>` somente se o device ainda nao tiver assinatura
   RamShared valida.
4. Rodar:

```sh
swapon --priority 32767 <device>
```

5. Confirmar em `/proc/swaps` que a prioridade do RamShared e maior que a do
   swap VHDX do WSL2.

### `ramshared stop`

Sequencia:

1. Se o device estiver em `/proc/swaps`, rodar `swapoff <device>`.
2. Enviar shutdown gracioso ao daemon via Unix socket
   `/run/ramshared/wsl2d.sock`.
3. O daemon deve:
   - parar de aceitar novas requisicoes;
   - concluir ou rejeitar requisicoes pendentes;
   - zerar a regiao de VRAM com `cuMemsetD8Async`;
   - sincronizar;
   - desconectar `nbd` ou remover `ublk`;
   - liberar staging host pinned;
   - liberar `CUdeviceptr`.
4. Remover arquivos de `/run/ramshared`.

### `ramshared recover`

Comando para falhas duras.

1. Mostrar estado conhecido de `/run/ramshared/wsl2d.json`.
2. Tentar `swapoff <device>`.
3. Para `nbd`, executar o ioctl de disconnect equivalente a
   `nbd-client -d <device>`.
4. Para `ublk`, chamar delete device no ublk control.
5. Se I/O travar, imprimir instrucao explicita para o host:

```powershell
wsl --terminate <DistroName>
```

O recovery deve deixar claro que isso reinicia o guest WSL, nao o Windows host.

## Daemon `ramshared-wsl2d`

### Estado

Estados permitidos:

```text
Init
PreflightOk
MemoryLocked
CudaReady
VramAllocated
BlockReady
SwapActive
Stopping
Failed
```

Transicoes invalidas devem abortar o processo. Exemplo: `BlockReady` nao pode
ser alcancado antes de `VramAllocated`.

### Modelo de enderecamento

- Tamanho do device deve ser multiplo de 4096.
- `logical_block_size = 4096`.
- Offset de bloco `off` mapeia para `vram_base + off`.
- Requisicao fora de faixa retorna erro de I/O.
- Requisicao desalinhada deve ser rejeitada no backend antes de tocar CUDA.

### I/O CUDA

Escrita de bloco:

1. Copiar payload do backend para staging pinned pre-alocado.
2. Enfileirar `cuMemcpyHtoDAsync(vram_base + off, staging, len, stream)`.
3. Se `--debug-checksum`, calcular hash do payload e armazenar em tabela
   pre-alocada por indice de bloco.
4. Completar I/O do block layer somente apos evento CUDA confirmar sucesso.

Leitura de bloco:

1. Enfileirar `cuMemcpyDtoHAsync(staging, vram_base + off, len, stream)`.
2. Aguardar evento CUDA.
3. Se `--debug-checksum`, validar hash e retornar erro de I/O em divergencia.
4. Copiar staging para o buffer do backend e completar I/O.

Erros CUDA:

- `CUDA_ERROR_OUT_OF_MEMORY`: abortar antes de expor device se ocorrer em
  inicializacao; retornar I/O error e marcar `Failed` se ocorrer no hot path.
- `CUDA_ERROR_CONTEXT_IS_DESTROYED`, reset, device lost ou GPU bloqueada:
  marcar `Failed`, parar filas e exigir `ramshared recover`.
- Qualquer erro desconhecido no hot path deve virar I/O error, nunca sucesso
  parcial.

### Backend NBD

Implementacao alvo:

- Abrir `/dev/nbdX` com privilegio.
- Criar `socketpair(AF_UNIX, SOCK_STREAM)`.
- Configurar `NBD_SET_SOCK`, `NBD_SET_BLKSIZE`, `NBD_SET_SIZE_BLOCKS` e
  flags de flush/trim somente se suportadas.
- Rodar loop de protocolo NBD no lado userspace do socket.
- Usar `NBD_DO_IT` em thread dedicada.
- Em stop/recover, executar `NBD_DISCONNECT`, `NBD_CLEAR_SOCK` e fechar FDs.

Se `modprobe nbd` for necessario, a CLI deve pedir permissao root e falhar com
mensagem objetiva quando o modulo nao puder ser carregado.

### Backend ublk

Implementacao alvo de fase B:

- Exigir `/dev/ublk-control`.
- Exigir `CONFIG_BLK_DEV_UBLK=y|m`.
- Usar `io_uring` para filas userspace.
- Preferir `libublksrv` se o binding Rust puro nao cobrir todos os ioctls.
- Manter a mesma interface `BlockBackend` usada pelo `nbd`.

Nao bloquear MVP em `ublk`: se `ublk` faltar e `nbd` existir, `--backend auto`
deve escolher `nbd`.

## Limites de seguranca

- Sem auto-start no boot.
- Sem uso dos 6 GiB inteiros de uma GPU de 6 GiB no MVP.
- Default documentado: `512M`; teto sem override: `1G`.
- Exigir margem minima de `1G` de VRAM livre apos reserva.
- Exigir `mlockall(MCL_CURRENT | MCL_FUTURE)` antes de qualquer fila de I/O.
- Exigir `oom_score_adj=-1000` ou abortar.
- Exigir `swapoff` antes de encerrar de forma graciosa.
- Nao aceitar `SIGTERM` como shutdown imediato quando swap estiver ativo; trocar
  para fluxo `Stopping` e executar `swapoff` via CLI. `SIGKILL` permanece uma
  falha dura recuperavel apenas por `ramshared recover` ou reinicio do guest.
- Zerar VRAM ao alocar e ao liberar.
- Nao guardar dados persistentes de swap fora da VRAM.

## Disciplinas Kahneman aplicadas

### Disciplina 2 — Sobrevivencia ao hardware

Pergunta: o que acontece se a GPU-PV desaparecer ou for bloqueada em 2 ms?

Resposta obrigatoria: o daemon nao promete preservar paginas se a GPU sumir
enquanto o device esta em swap. Ele deve:

- detectar falta de `/dev/dxg`, `cuInit` falho, reset ou GPU bloqueada no
  preflight;
- abortar antes de `swapon` nesses casos;
- no hot path, retornar I/O error e marcar `Failed`;
- documentar que processos com paginas apenas nesse swap podem morrer dentro do
  guest WSL.

### Disciplina 3 — Prevencao de deadlock de memoria

Evidencia obrigatoria no codigo:

```rust
// Deve ocorrer antes de criar filas NBD/ublk ou expor device de swap.
mlockall(MCL_CURRENT | MCL_FUTURE)
```

O retorno deve ser checado. Falha aborta inicializacao. Staging, tabelas de
checksum e filas devem ser pre-alocados antes do device ficar visivel.

### Disciplina 4 — Isolamento entre processos

Mesmo sendo swap de bloco, o daemon ve conteudo de memoria de processos do
guest. Mitigacoes obrigatorias:

- processo root-only;
- socket em `/run/ramshared` com permissao `0600`;
- zerar VRAM na alocacao e liberacao;
- nao expor API de leitura arbitraria por offset a usuarios nao-root;
- modo debug de checksum nao deve persistir payloads.

## Testes de aceitacao

### 1. Deteccao

Comandos:

```sh
sudo ramshared check
sudo ramshared check --json
```

Aceite:

- reporta kernel WSL2;
- reporta swap atual;
- reporta CUDA disponivel e VRAM total/usada/livre;
- reporta `nbd` e `ublk` separadamente;
- em maquina com `nvidia-smi` bloqueado ou `/dev/dxg` ausente, retorna
  `Decisao: blocked` e exit code diferente de zero.

### 2. Integridade sem swap

Comando alvo:

```sh
sudo ramshared start --size 512M --backend nbd --debug-checksum
sudo ramshared test-integrity --duration 30m --pattern random
sudo ramshared stop
```

Aceite:

- escreve e le padroes aleatorios por pelo menos 30 minutos;
- compara hash por bloco;
- zero divergencias;
- zero erros CUDA;
- stop limpa device e VRAM.

### 3. Swap real

Comandos alvo:

```sh
sudo ramshared start --size 512M --backend nbd
sudo ramshared swapon --priority 32767
swapon --show
grep -E 'pswpin|pswpout' /proc/vmstat
stress-ng --vm 2 --vm-bytes 75% --timeout 10m
grep -E 'pswpin|pswpout' /proc/vmstat
sudo ramshared stop
```

Aceite:

- `/proc/swaps` mostra o device RamShared com prioridade maior que o swap VHDX;
- contadores de swap mudam durante pressao de memoria;
- RamShared e usado antes do swap VHDX;
- nenhum erro de I/O aparece em `dmesg`.

### 4. Falha controlada

Comandos alvo:

```sh
sudo ramshared start --size 512M --backend nbd
sudo ramshared swapon --priority 32767
sudo kill -TERM "$(cat /run/ramshared/wsl2d.pid)"
sudo ramshared stop
```

Aceite:

- `SIGTERM` inicia shutdown gracioso;
- `swapoff` ocorre antes de liberar VRAM;
- device some de `/proc/swaps`;
- daemon sai com status 0.

Teste de falha dura:

```sh
sudo ramshared start --size 512M --backend nbd
sudo ramshared swapon --priority 32767
sudo kill -KILL "$(cat /run/ramshared/wsl2d.pid)"
sudo ramshared recover
```

Aceite:

- recovery tenta `swapoff`;
- se o kernel bloquear em I/O, a mensagem final instrui `wsl --terminate`;
- Windows host nao e afetado.

### 5. Performance

Comandos alvo:

```sh
sudo ramshared bench --backend nbd --size 512M --seq --rand
```

Metricas obrigatorias:

- throughput sequencial leitura/escrita;
- IOPS aleatorio 4K;
- latencia p50/p95/p99;
- comparativo contra swap VHDX atual.

Aceite MVP: estabilidade e integridade prevalecem sobre performance. O MVP pode
ser aceito antes de superar o VHDX se passar deteccao, integridade, swap real e
falha controlada.

## Criterios de pronto

- `cargo fmt --all` sem diff.
- `cargo clippy --workspace --all-targets -- -D warnings` verde.
- `cargo test --workspace` verde.
- `sudo ramshared check` cobre caminho `ready` em ambiente com GPU-PV funcional
  e caminho `blocked` no ambiente atual sem `/dev/dxg`.
- `ramshared test-integrity` verde por 30 minutos com `512M`.
- `ramshared stop` deixa `/proc/swaps` sem o device RamShared.
- Documentacao de recovery testada ao menos uma vez com kill do daemon.

## Nao objetivos

- Nao emprestar VRAM ao Windows host.
- Nao criar modulo de kernel no MVP.
- Nao usar DAMON, HMM, TTM, ReBAR ou NUMA fake dentro do WSL2.
- Nao iniciar automaticamente no boot.
- Nao prometer preservacao de processos do guest se a GPU-PV falhar enquanto
  paginas estiverem exclusivamente no swap RamShared.
