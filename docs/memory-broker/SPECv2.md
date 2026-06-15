# SPECv2 — RamShared Memory Broker (P0 + P1)

> SSDV3 PASSO 2 **revisado**, gerado de [`docs/memory-broker/PRD.md`](PRD.md). Slug: `memory-broker`.
> Escopo: **P0 (medição) + P1 (broker core Linux↔Linux)** — fases do PRD §10. P2/P3/P4 ficam
> explicitamente fora e terão SPECs próprios quando os gates abrirem.
> Disciplinas: links obrigatórios para [`docs/methodology/KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md)
> em toda etapa crítica (mapa na seção própria).

## Registro de auditoria (Passo 2.5)

- **1ª auditoria — SPEC auditado:** [`docs/memory-broker/SPEC.md`](SPEC.md) (2026-06-09).
  **Resultado: no-go.** Findings F1..F17 (tabela abaixo); endereçados ao criar este SPECv2.
- **2ª auditoria — SPECv2 (este arquivo) re-auditado** (2026-06-13, Opus 4.8). **Resultado:
  no-go** → atualizado **in-place** no mesmo turno (regra de saída do Passo 2.5 para SPECv2).
  Findings R1..R6 (2ª tabela); endereçados por DT-27 + emendas a DT-17/DT-19 e ITEM-4/7/8/9/11/12.
- **3ª auditoria — SPECv2 re-auditado** (2026-06-13, Opus 4.8; foco no ciclo de vida do worker
  em modo broker, que reusa o pipeline H1). **Resultado: no-go** → atualizado **in-place**.
  Findings R7..R9 (3ª tabela); endereçados por DT-28 + emendas a DT-27/ITEM-4/ITEM-8/ITEM-10.
- **4ª auditoria — SPECv2 re-auditado** (2026-06-13, Opus 4.8; foco no que R7..R9 introduziram +
  reconciliação e shutdown). **Resultado: go.** Só 2 findings **LOW** (R10/R11), clarificações
  sem decisão arquitetural nova, dobradas in-place em DT-17/DT-21; severidade convergiu ao longo
  das passadas (2 CRITICAL+5 HIGH → 1 HIGH+3 MEDIUM → 1 HIGH+2 → 2 LOW). **SPECv2 pronto para o
  Passo 3 (IMPL); gate em ITEM-1 (P0) antes de qualquer código P1.**
- **Este arquivo (`SPECv2.md`) é o candidato ativo** para nova auditoria/implementação.
  `SPEC.md` original preservado como histórico.
- **Findings da 1ª auditoria (sobre SPEC.md) endereçados** (CRITICAL/HIGH; menores junto):

| Finding | Severidade | Correção nesta versão |
| --- | --- | --- |
| F1 — agente sem `mkswap`: fluxo SwapOn inexecutável (`swapon` exige assinatura; evidência: `cascade.rs:310`, `ublk_io_smoke.rs:671`) | CRITICAL | DT-16; ITEM-9 (`swap.rs::mk_swap`, ordem do loop); drill/e2e atualizados |
| F2 — slice re-atribuída sem zerar vaza páginas swapped entre tenants | CRITICAL | DT-17; `WMsg::ZeroExport` (ITEM-7), fiação no core (ITEM-8); fronteira de atomicidade ganha o passo de zero |
| F3 — watchdog sem fonte de heartbeat obrigatória (broker não era obrigado a responder nada em regime) | HIGH | DT-18: `Ack` obrigatório por `Psi`; e2e asserta cadência |
| F4 — drill validava happy path (#13): "slice ativa" ≠ páginas residentes; resultado do swapoff com device morto indefinido; initramfs sem `nbd.ko`/`nbd-client`/`lo up` | HIGH | ITEM-11 reescrito: 3 fases (graceful / kill com swap vazio / kill com swap usado), critérios numéricos, pré-requisitos de initramfs explícitos |
| F5 — lease sem estado: `AssignFree` round-robin re-atribuiria slices arrendadas no tick seguinte | HIGH | DT-19: `SliceState::Leased` + regras de grant/deny/release |
| F6 — tenant ausente (EOF) com slices Active: árbitro podia emitir ação inexecutável → slice presa em Draining | HIGH | DT-20: nenhuma Action com alvo ausente; slices congeladas e visíveis no Status; reconciliação no re-Register |
| F7 — shutdown ordenado (rollback de app) sem evidência em teste algum | HIGH | e2e cenário (f) + drill fase 0 (SIGTERM graceful) |
| F8 — `BrokerConfig.nbd_endpoint` único vs `TransportKind` por tenant | MEDIUM | DT-25: endpoints Unix e TCP opcionais; escolha pelo `Register{transport}` |
| F9 — counterfactual multiplicativo sem piso: `RevertMove` por ruído em baseline ~0 | MEDIUM | DT-23: guarda `psi(from) > psi_floor` além do fator 2× |
| F10 — escrita outbound do core sem dono (agente travado bloquearia o broker) | MEDIUM | DT-24: writer thread por sessão, canal bounded 64, `try_send`, desconexão por backpressure |
| F11 — protocolo subespecificado: `Status` sem `Register`; `Register` duplicado; estabilidade de `TenantId`; contrato `{base}{N}` da reconciliação | MEDIUM | DT-21/DT-22 |
| F12 — varredura do rename `ramsharedd` incompleta (`docs/ublk-daemon-integration/IMPL.md:102` é doc vivo) | LOW | ITEM-8: critério observável de grep + arquivo adicionado aos MODIFICAR |
| F13 — "uma decisão por tick" vs `-> Vec<Action>` | LOW | ITEM-4: no máximo 1 Move/Revert por tick; Assign/lease coexistem |
| F14 — "a VM segue responsiva" sem critério observável | LOW | ITEM-11: echo <2s ×3 + nenhum processo em D >10s |
| F15 — DT-13 (euid==0) sob `forbid(unsafe_code)` sem caminho (risco de dep nova sem ADR) | LOW | DT-26: parse de `/proc/self/status`, zero-dep |
| F16 — runbook civm sem `modprobe nbd`/persistência | LOW | ITEM-12 |
| F17 — `measure-nbd-tcp.sh` sem check de deps (verificado: host sem `nbdkit`/`nbd-server`) | LOW | ITEM-1: preflight de dependências com mensagem de instalação |

- **Findings da 2ª auditoria (sobre este SPECv2) endereçados:**

| Finding | Severidade | Correção in-place |
| --- | --- | --- |
| R1 — agente executa comandos (nbd_connect/mkswap/swapon) **síncrono no loop de heartbeat**: um `SwapOn` >3s starva o envio de `Psi`/leitura de `Ack` e dispara o watchdog → swapoff espúrio da slice recém-montada (viola RNF-1/RNF-3). `mkswap`/`nbd_connect` sobre NBD/TCP têm latência **não medida** (Inferência) | HIGH | DT-27: execução de swap fora do loop de heartbeat (thread de exec, espelha `spawn_swapoff`); o watchdog passa a medir liveness do broker, não latência de comando; ITEM-9 lógica reescrita |
| R2 — corrida de reserva do lease: durante revogação multi-tick uma slice liberada (`Free`) podia ser pega pelo round-robin (passo 5) antes do `GrantLease` → lease nunca acumula (starvation de RF-B3) | MEDIUM | DT-19 emendado: sob `pending_lease`, slice liberada vira `Leased` **incrementalmente** (não fica `Free`) e o passo (5) é suprimido; ITEM-4/ITEM-10 exigem teste de lease **que precise revogar** (não só conceder de `Free`) |
| R3 — geometria da slice indisponível ao worker: `Export{name,size}` não carrega `base`; o worker precisa de `base` p/ `SliceView` (Job **e** ZeroExport) | MEDIUM | ITEM-8: worker mantém `geom: Vec<(u64,u64)>` (base,len por export, de `SliceMap`); `block::Export` fica só name+size (não acopla o crate de bloco ao layout de slice) |
| R4 — `ZeroExport` enfileirado por `try_send` no canal `jobs` (bound 64): canal cheio → falha de envio sem retry definido (só `ZeroDone{ok:false}` tinha retry) → slice presa em `Draining` | MEDIUM | DT-17 emendado: falha de `try_send` mantém a slice `Draining` e re-tenta `ZeroExport` no próximo tick (mesmo caminho do `ok:false`) |
| R5 — drill/runbook: socket Unix default (`/run/ramshared/`) pode não existir no initramfs (bind falha → daemon não sobe); `/dev/nbdN` exige `nbds_max`; parse de estado-D em `/proc/*/stat` quebra com `comm` contendo parênteses | LOW | ITEM-11: `--sock /tmp/d.sock` no setup + `modprobe nbd nbds_max=8`; parsing lê o estado **após o último `)`**; ITEM-12: `modprobe nbd nbds_max=…` no runbook |
| R6 — worker bloqueado durante o zero de slice grande (`--slice-mb` alto → centenas de chunks de 1 MiB serializam atrás do I/O) | LOW (note) | DT-17: aceitável (rebalanço raro, cooldown 60s); registrado como limite conhecido |

- **Findings da 3ª auditoria (sobre este SPECv2) endereçados:**

| Finding | Severidade | Correção in-place |
| --- | --- | --- |
| R7 — **ciclo de vida do worker em modo broker**: o worker reusa o pipeline H1 e encerra via `LiveCount` quando `live==0 && opened` (`conn.rs:70-73`, `main.rs:235-238`, DT-15). Em modo broker o daemon é persistente; as conexões NBD caem a zero a cada `DemoteAll` (canário/GPU) ou quando todas as slices ficam `Free` (idle) → o worker **encerra e o daemon para de servir** qualquer `SwapOn` futuro = falha permanente após um demote normal | HIGH | DT-28: em modo broker o worker **ignora o break por `LiveCount`** (DT-15 vale só no single) e só encerra no fechamento do canal `jobs` (shutdown ordenado); ITEM-8 (e) + ITEM-10 cenário (j) |
| R8 — agente com 2 threads (DT-27) sem **escritor único** do socket: `SwapOnDone` (thread de exec) e `Psi` (loop principal) podem intercalar bytes → linha JSON-lines corrompida → broker responde `Error`/desconecta | MEDIUM | DT-27 emendado: a thread de exec devolve o resultado ao loop principal por canal; o **loop principal é o único escritor** do socket |
| R9 — rebalanço (passos 2/4 do `tick`) não suprimido durante lease pendente → `MoveSlice`/`RevertMove` concorrendo com a revogação do lease = churn e disputa do worker | LOW | ITEM-4: passos (2) e (4) também suprimidos enquanto há `pending_lease` (só ações de lease + nunca-zero) |

- **Findings da 4ª auditoria (sobre este SPECv2) — LOW, dobrados como clarificações (verdito go):**

| Finding | Severidade | Correção in-place |
| --- | --- | --- |
| R10 — shutdown ambíguo: usa `ZeroExport` por slice (exige worker vivo) ou o zero whole-buffer do teardown? Uma implementação ZeroExport-no-shutdown poderia esperar `ZeroDone` após já dropar o sender (degrada ao timeout de 10s, não trava — mas é ambíguo) | LOW | DT-17: no shutdown a higiene vem do zero whole-buffer do teardown (F2), **sem** `ZeroExport` por slice → o worker encerra limpo (DT-28); os 10s são backstop |
| R11 — reconciliação (DT-21) assumia o `--nbd-dev-base` que o `Register` **não** carrega; um base não-default quebraria o reconcile → risco de re-atribuir slice ainda montada (corrupção). O default `/dev/nbd` (usado pelo runbook P1) já funcionava | LOW | DT-21: broker reconcilia pelo **inteiro final** de `SwapEntry.dev` (= id da slice por construção), **agnóstico ao prefixo** — sem novo campo no protocolo |

- **Day-0:** auditoria não encontrou violação (DT-3/DT-5/DT-11 documentam adiamentos; `--backend ram`
  é harness de validação, mesmo padrão F2; modo single é o produto da Fase B; ITEM-6 recusa alias).

## Escopo fechado desta implementação

**Entra agora:**

- **P0** — scripts de medição (sem código de produto) + template de resultados que é o **gate
  numérico** de P1: PSI idle/carga (WSL2, civm, host), alcançabilidade/RTT VM↔WSL2, p50/p99
  NBD/TCP cru no virt-switch, medição de VRAM/RAM durante render (script p/ o tester).
- **P1** — RF-B1, RF-B2, RF-B3, RF-B4, RF-L1, RF-L2, RF-L3, RF-L4, RF-P2 (parcial: NBD como
  fallback universal; ublk inalterado), RNF-1..RNF-6:
  - crate novo `ramshared-broker` (protocolo JSON-lines, modelo, árbitro puro, mapa de slices);
  - crate novo `ramshared-agent` (binário do tenant: PSI, nbd-client + mkswap + swapon/swapoff,
    watchdog);
  - daemon (`crates/ramshared-wsl2d`) ganha `--slices/--slice-mb`, `--listen-nbd tcp://`,
    `--arbiter-listen`, `--backend ram` no caminho NBD, higiene de slice (zero na devolução),
    e binário renomeado `ramsharedd`;
  - export NBD nomeado por slice em `ramshared-block::server_handshake`;
  - drill de D-state em qemu (`scripts/kernel/qemu-broker-drill.sh`, 3 fases);
  - runbook civm copiável (`docs/runbooks/CIVM-TENANT.md`).

**Fica explicitamente fora agora:**

- P2: RF-W1..W4 (agente Windows, addon Blender, lease bridge, interposer), RF-P1 (instaladores),
  RF-P3 (config TOML — ver DT-11).
- P3/P4: RF-G1..G3 (trait `VramProvider`, Vulkan, D3D12). **Não** extrair trait agora: o tier
  continua CUDA-direto via `VramBackend`.
- Counterfactual do lease do PRD §14 (uso <50% em 5min): exige telemetria NVML do RF-W1 → P2
  (DT-10). Em P1 o lease é concedido/revogado/logado, sem medição de uso do holder.
- Auth/criptografia no protocolo e no NBD/TCP (PRD §12): mitigação é bind privado (RNF-2).
- Persistência de estado do broker (DT-9).

**Dependências já assumidas como prontas** (Confirmado no codebase, Fase B):

- `ramshared_block::BlockBackend` + `serve()` + NBD fixed-newstyle multi-conexão (Unix socket).
- Máquina de DEMOTE: `Canary`/`ResidencySampler`/`CanaryProbe` (`crates/ramshared-wsl2d/src/residency.rs`,
  `canary_probe.rs`) e `spawn_swapoff` (`crates/ramshared-wsl2d/src/swap.rs`).
- CUDA via dlopen (`ramshared-cuda`: `Cuda`, `Context::alloc`, `DeviceMem::{read_at,write_at,zero}`;
  **`zero()` é whole-buffer** — confirmado em `driver.rs:237`; zero por slice usa `write_at` em
  chunks, DT-17).
- Teardown ublk validado em qemu (F2, `scripts/kernel/qemu-ublk-daemon.sh`) + `RamBackend`.
- VM civm `gha-ubuntu-2404` no host `EMEDEV`, alcançável por SSH/Tailscale (Confirmado em docs).
- **Confirmado nesta auditoria (host/kernel):** kernel custom WSL2 tem `CONFIG_PSI=y` sem
  `PSI_DEFAULT_DISABLED` e `/proc/pressure/memory` legível; `CONFIG_BLK_DEV_NBD=m` com
  `nbd.ko` já compilado em `/home/emdev/WSL2-Linux-Kernel/drivers/block/nbd.ko`; host tem
  `nbd-client` (`/usr/sbin/nbd-client`) e `fio`, **não** tem `nbdkit`/`nbd-server` (ITEM-1 checa).

## Matriz de rastreabilidade PRD → SPEC

| PRD | Implementação no SPEC |
| --- | --- |
| P0 (§10) / R1 / R4 | ITEM-1 (scripts p0 + `P0-RESULTS.md`) |
| RF-B1 | ITEM-2 (ADR-0005), ITEM-3 (`protocol.rs`), ITEM-8 (`broker_srv.rs`), ITEM-9 (agente) |
| RF-B2 | ITEM-4 (`arbiter.rs`: histerese+cooldown+nunca-zero; counterfactual com piso DT-23) |
| RF-B3 | ITEM-4 (ações de lease + estado `Leased`, DT-19), ITEM-8 (revogação = SwapOff/demote per-slice) |
| RF-B4 | ITEM-8 (log de decisão com PSI dos dois lados; `StatusReply` com presença), ITEM-9 (`--status`) |
| RF-L1 | ITEM-4 (`slices.rs`), ITEM-5 (handshake por export), ITEM-6 (`SliceView`), ITEM-7, ITEM-8 |
| RF-L2 | ITEM-7 (streams genéricos), ITEM-8 (`--listen-nbd tcp://`, recusa 0.0.0.0) |
| RF-L3 | ITEM-9 (`psi.rs`, `swap.rs` incl. `mk_swap` DT-16) |
| RF-L4 | ITEM-12 (`docs/runbooks/CIVM-TENANT.md`) |
| RF-P2 (parcial) | ITEM-8 (NBD fallback universal; ublk single-device intacto) |
| RNF-1 | ITEM-9 (watchdog + heartbeat DT-18), ITEM-11 (drill 3 fases), DT-7, DT-14 |
| RNF-2 | ITEM-8 (recusa bind 0.0.0.0), ITEM-12 (runbook só rede privada/Tailscale) |
| RNF-3 | ITEM-4 (histerese+cooldown; defaults provisórios calibrados por P0; piso do counterfactual DT-23) |
| RNF-4 | ITEM-10 (suíte existente verde; modo single inalterado), checklist de validação |
| RNF-5 | ITEM-3/ITEM-9 (`#![forbid(unsafe_code)]` nos crates novos; euid via `/proc`, DT-26) |
| RNF-6 | DT-1..DT-26 (decisões únicas, sem dual-path) |

RF-W1..W4, RF-G1..G3, RF-P1, RF-P3: **fora** (ver escopo).

## Decisões técnicas

Decisões tomadas que não estavam explícitas no PRD (cada uma é a solução única Day-0).
DT-1..DT-15 vêm do SPEC auditado (inalteradas exceto onde indicado); DT-16..DT-26 corrigem os
findings do Passo 2.5.

| # | Decisão | Justificativa |
| --- | --- | --- |
| DT-1 | Protocolo agente↔broker = **JSON-lines** (1 objeto JSON por linha, `\n`, UTF-8) sobre TCP, via `serde`/`serde_json` | Control-plane de baixa taxa (1 msg/s/tenant): debugável com `nc`/`jq`, evolução por campo opcional. Length-prefixed só ganharia em data-plane binário, que aqui é o NBD. Dep nova exige ADR (disciplina #11) → ADR-0005 + `docs/LIBRARIES.md` |
| DT-2 | Broker **in-process no daemon** (`--arbiter-listen`), não binário separado | Anexo A.4 do PRD: um único dono da verdade sobre a VRAM. O daemon já é `mlockall`+`oom_score_adj=-1000`; broker separado recriaria a disputa cega que o lease resolve |
| DT-3 | Slices em P1 = **exports NBD nomeados** (`s0..s{K-1}`), Unix e TCP; ublk permanece single-device; `--transport ublk` + `--slices` → erro de CLI | O tenant local da persona dev é WSL2, onde `guard_not_wsl2()` recusa ublk (incidente de congelamento 2026-06-09, `crates/ramshared-wsl2d/src/main.rs`). NBD já tem export name no handshake. Slices ublk entram quando existir tenant local não-WSL2 (sem dual-path agora) |
| DT-4 | **Worker CUDA único permanece**; a slice é resolvida no worker via `SliceView` (offset/len sobre o backend único) | Mantém a afinidade de thread (H1) e a sincronicidade `cuMemcpy*` de que o `flush()` no-op depende (`backend.rs`). `DeviceMem` único = zero mudança no modelo CUDA |
| DT-5 | Binário renomeado **`ramsharedd`** via `[[bin]]` no `Cargo.toml` do crate; prefixo de log `[ramsharedd]`; diretório do crate **não** renomeia agora | PRD §8 nomeia `ramsharedd`; o runbook civm grava o nome em systemd unit (renomear depois quebraria runbook = anti-Day-0). Rename do diretório é fatia ortogonal separada (disciplina #14) |
| DT-6 | Atribuição inicial: slices `Free` são distribuídas **round-robin** entre tenants registrados **presentes**; o árbitro só **move** sob diferencial de pressão | Swap ocioso não custa nada: o kernel só usa a slice sob pressão (gate natural via prioridade de swap). Evita política de admissão extra e torna o drill determinístico |
| DT-7 | Prioridade de swap remoto: default **sem `-p`** no `swapon` (kernel atribui prioridade negativa decrescente, sempre abaixo do swap local pré-existente); `--swap-prio` só para override explícito | `swapon -p` do util-linux não aceita negativos; sem `-p`, a slice entra com prioridade menor que qualquer swap já ativo — exatamente o que RNF-1 exige. Evidência: coluna `Priority` em `/proc/swaps` |
| DT-8 | Invariante "nunca zero slices para tenant sob pressão" (RF-B2) vale **só para rebalanceamento**; revogação por **lease pode drenar tudo** | RF-B3: pedido explícito de VRAM > swap tier. O tenant drenado mantém o swap local (RNF-1); a VRAM emprestada é best-effort por definição de lease revogável |
| DT-9 | Estado do broker é **em memória**, reconstruído no `Register` (agente reporta `/proc/swaps` atual); zero persistência em P1 | Broker morre → watchdog limpa (RNF-1) → no restart os agentes re-registram e reportam o estado real. Persistir duplicaria a fonte da verdade |
| DT-10 | Counterfactual do lease (PRD §14, uso <50% em 5min) **adiado para P2** | Exige telemetria de uso de VRAM do holder (NVML, RF-W1). Em P1 não há holder real de lease (DCC é P2); grant/release/revoke são implementados e logados |
| DT-11 | Config TOML (RF-P3) adiada para P2 (junto de RF-P1); superfície P1 = **flags CLI** | TOML é a superfície do produto instalável. As flags mapeiam 1:1 para o TOML futuro (não é shim: continuam válidas como override, padrão systemd) |
| DT-12 | Sem métricas Prometheus em P1; observabilidade = **logs estruturados em stderr + `Status`** | O daemon é `eprintln`-based hoje; exporter entra com o produto instalável (P2+). RF-B4 é satisfeito por log de decisão + `StatusReply` |
| DT-13 | `ramshared-agent` exige **euid 0** no startup (erro claro caso contrário) | Equivalente userspace do gate `capable(CAP_SYS_ADMIN)`: `swapon`/`swapoff`/`mkswap`/`nbd-client` exigem root. Falhar cedo > falhar no primeiro comando. Implementação em DT-26 |
| DT-14 | `nbd-client` sempre com **`-timeout 30`** e **nunca `-persist`** | Broker morto deve virar EIO bounded, não D-state eterno; `-persist` tentaria reconectar a um servidor morto, prolongando o hang (RNF-1, R2). Precisão: conexão **fechada** (processo morto) erra I/O imediatamente; o `-timeout 30` cobre o caso conexão-aberta-mas-travada |
| DT-15 | PSI: arbitragem usa a linha **`some`** (`avg10`); `full` apenas logado | `some` captura "alguém estagnou" (sinal de rebalanceamento); `full` é estagnação total, tarde demais para agir. P0 valida a escolha com números |
| DT-16 | **`mkswap` é passo obrigatório do agente em todo `SwapOn`**, entre `nbd_connect` e `swapon` | `swapon` exige assinatura de swap no device; a slice chega **zerada** (DT-17), logo sem assinatura — `mkswap` é sempre necessário e idempotente. Page size do header = o do tenant que usa (consistente por construção). Evidência no repo: todo caminho de swap validado roda `mkswap` antes (`crates/ramshared-cli/src/cascade.rs:310`, `crates/ramshared-wsl2d/tests/ublk_io_smoke.rs:671`). Corrige F1 |
| DT-17 | **Higiene de slice**: o daemon **zera o conteúdo da slice** na devolução (`Draining` → zero → `Free`), antes de qualquer re-atribuição/lease | Páginas swapped são memória anônima de processos do tenant (chaves, tokens); sem zero, root no tenant seguinte lê o conteúdo cru via `/dev/nbdN`. Implementação sem nova API CUDA: variante `WMsg::ZeroExport` processada pelo **worker** (única thread CUDA), `write_at` de zeros em chunks de 1 MiB via `SliceView` — fora do hot path (rebalanço é raro, cooldown 60s). Falha de `try_send` do `ZeroExport` (canal `jobs` cheio) → slice permanece `Draining`, retry no próximo tick (R4). Zero de slice grande bloqueia o worker por ~chunks×cuMemcpy (aceitável, R6). No **shutdown** a higiene vem do zero whole-buffer do teardown (F2), **sem** `ZeroExport` por slice → o worker encerra sem depender de `ZeroDone` (DT-28; os 10s da espera são backstop — R10). Corrige F2 (emendado R4/R6/R10) |
| DT-18 | **Heartbeat**: o broker responde `Ack` a **cada** `Psi` recebido (1 Hz/agente); o watchdog do agente (deadline 3s) conta = 3 heartbeats perdidos | Sem resposta obrigatória em regime, o watchdog expiraria sempre (swapoff espúrio + reconexão em loop). `Ack` não é logado (ruído 1 Hz × N). Corrige F3 |
| DT-19 | `SliceState` ganha **`Leased`**: `Free → Leased` no grant, `Leased → Free` no release. Em P1 há **no máximo 1 lease ativo ou pendente**; segundo pedido → `LeaseDenied{reason:"lease_em_andamento"}`; `bytes` acima da capacidade → `LeaseDenied{reason:"acima_da_capacidade"}`; EOF do holder → release automático. **Reserva incremental (R2):** enquanto há `pending_lease`, cada slice que chega a `Free` (após drenar+zerar uma `Active` revogada) é imediatamente movida a `Leased` reservada ao pedido — **não fica `Free`** — e o round-robin (passo 5 do tick) é **suprimido**; `GrantLease` é emitido quando a contagem reservada fecha | Sem estado próprio, o passo round-robin re-atribuiria as slices arrendadas no tick seguinte, anulando RF-B3; sem a reserva incremental, uma slice liberada no meio da revogação multi-tick vazaria pro round-robin e o lease nunca acumularia (starvation). "Mais ociosas primeiro" = slices `Active` ordenadas por `used_kb` ascendente (último `Psi{swaps}` do dono, mapeado por DT-21), desempate por menor `psi.avg10` do dono. Corrige F5 (emendado R2) |
| DT-20 | **Tenant ausente** (EOF/queda): nenhuma `Action` pode ter alvo ausente. O core passa ao árbitro só tenants presentes e **exclui da view as slices cujo dono está ausente** (congeladas); `StatusReply` expõe `present:false`; limpeza manual = runbook; re-`Register` + primeiro `Psi{swaps}` reconciliam (DT-9) | O plano de dados NBD continua funcionando sem o agente (a conexão é do kernel); mover uma slice de dono ausente emitiria `SwapOff` sem executor → slice presa em `Draining` para sempre. Corrige F6 |
| DT-21 | **Contrato de reconciliação**: o device da slice `sN` no tenant é `{nbd-dev-base}{N}` (default `/dev/nbdN`), com o **sufixo numérico = id da slice** por construção (o agente usa o `slice` do `SwapOn` como número do device). O broker reconcilia extraindo o **inteiro final** de `SwapEntry.dev` → `sN`, **agnóstico ao prefixo** (não depende de conhecer o `--nbd-dev-base` de cada tenant, que o `Register` não carrega — R11); entradas sem inteiro final que case um id de slice conhecido são ignoradas | A reconciliação DT-9 e o "mais ociosas" do DT-19 dependem do mapeamento; o parse agnóstico evita que um `--nbd-dev-base` não-default quebre o reconcile e re-atribua slice ainda montada (corrupção). Corrige F11d (emendado R11) |
| DT-22 | Protocolo: `Status` é aceito **sem** `Register` (one-shot read-only). `Register` com nome de tenant já em sessão viva → `Error{reason:"tenant_duplicado"}` + desconexão da conexão nova. `TenantId` é alocado incrementalmente e **estável por nome durante a vida do broker** (sobrevive a reconexão; some no restart — DT-9, sem efeito externo) | Remove as três ambiguidades de sessão (F11a-c); a estabilidade do id é pré-condição da reconciliação |
| DT-23 | Counterfactual com **piso**: `RevertMove` exige `psi(from).avg10 > cf_factor × psi_no_momento_do_move` **e** `psi(from).avg10 > psi_floor` | O doador é, por construção, o tenant de PSI baixa; multiplicador sobre baseline ~0 dispara com ruído de idle (0,2→0,5 = 2,5×) e congelaria o árbitro com `cf_cooldown` espúrios. O trigger do PRD §14 (2× em 60s) é preservado; o piso só exige que a piora seja pressão real. Corrige F9 |
| DT-24 | **IO do core**: por sessão, uma **writer thread** dona da metade de escrita drena um canal **bounded (cap 64)**; o core usa `try_send` — canal cheio = sessão não-drenante → core fecha a sessão e marca o tenant ausente (log ERROR). O core consome um único canal de `CoreEvent` com `recv_timeout(2s)` (= tick); `demote_rx` é drenado por uma thread forwarder (→ `CoreEvent::Demote`); confirmações de zero chegam por `pending_zeros: Vec<(SliceId, Receiver<bool>)>` com `try_recv` por iteração (zeros são raros; latência extra ≤ 1 tick) | O core nunca faz IO de socket: um agente travado não pode bloquear o broker (a disputa cega que o broker existe para resolver). Zero locks preservado: só canais + ownership. Corrige F10 |
| DT-25 | `BrokerConfig` carrega **ambos endpoints opcionais** (`nbd_unix: Option<String>`, `nbd_tcp: Option<SocketAddr>`); o `SwapOn.endpoint` é escolhido pelo `transport` do `Register`; transporte pedido e não configurado → `Error{reason:"transporte_indisponivel"}` | Tenant local (WSL2) usa Unix; civm usa TCP — um endpoint único não serve os dois e tornava `TransportKind` letra morta. Corrige F8 |
| DT-26 | Checagem de euid sem `unsafe` e sem dep nova: parse da linha `Uid:` de **`/proc/self/status`** | `geteuid()` é FFI (violaria `forbid(unsafe_code)`) e `libc`/`nix` seriam dep nova sem ADR (#11). Leitura de `/proc` é zero-dep e já é padrão do agente (PSI/swaps). Corrige F15 |
| DT-27 | **Execução de comando do agente fora do loop de heartbeat**: `nbd_connect`/`mk_swap`/`swap_on`/`swap_off` rodam numa **thread de exec** dedicada (fila serial 1 comando por vez); o loop principal só envia `Psi` (1 Hz), lê `Ack`/comandos (`touch` no watchdog) e despacha. `SwapOnDone`/`SwapOffDone` voltam ao loop principal por canal — o **loop principal é o único escritor** do socket (R8) | Mesmo desenho anti-bloqueio do `spawn_swapoff` (daemon): um `SwapOn` lento (mkswap/nbd_connect sobre TCP, latência **não medida**) não pode starvar o heartbeat e disparar o watchdog (swapoff espúrio = anti-RNF-3). O watchdog passa a medir **liveness do broker**, não a duração do comando local. Escritor único evita intercalar JSON-lines de `Psi` e dos resultados (R8). Corrige R1 (emendado R8) |
| DT-28 | **Ciclo de vida do worker depende do modo**: no modo single (Fase B) o worker encerra por `LiveCount` quando a conexão fecha (DT-15, RNF-4 intacto); no **modo broker** o worker **ignora o break por `LiveCount`** e roda enquanto o daemon vive, encerrando só no **fechamento do canal `jobs`** (shutdown ordenado: acceptors param + broker dropa seu `SyncSender`) | Em modo broker as conexões NBD caem a zero a cada `DemoteAll` ou idle (todas as slices `Free`); manter o break do single mataria o daemon após um demote normal (R7). O worker passa a `recv_timeout` (tick) para checar a flag de shutdown e poder encerrar mesmo ocioso. Corrige R7 |
| DT-30 | **Tick do árbitro por deadline, não por timeout de recv**: o `core_loop` do broker mantém um `next_tick` de wall-clock e espera `next_tick - now`; o `Tick` dispara quando o deadline passa, qualquer que seja a taxa de mensagens. NÃO usar `recv_timeout(tick)` puro (só dispara o Tick no `Err(Timeout)`) | Sob o fluxo NORMAL de `Psi` (~1/s por tenant, + jitter de rede) as mensagens resetavam o `recv_timeout` antes de expirar → o `Tick` nunca disparava → `AssignFree`/rebalanço nunca rodava (o broker travaria a arbitragem sob carga real). Bug pego no e2e cross-host civm (o drill qemu passava por sorte de timing, loopback). Regressão: `e2e_psi_flood_does_not_starve_arbiter_tick` |
| DT-29 | **Fronteira de segurança servidor-only (WSL2)**: no e2e cross-host (ITEM-12) o WSL2 roda **só o broker/servidor** — `run_broker`/`run_broker_ram` **nunca** fazem `swapon`; quem consome o swap é o tenant (civm). Invariante: **nenhum agente local no WSL2** nessa topologia. O isolamento por qemu é exigido apenas para **WSL2-como-consumidor** (smoke ublk/swap local, regra `[[no-standalone-daemon-smoke-wsl2]]`); WSL2-como-servidor pode rodar no host real | O freeze que travou o WSL2 (2026-06-09) foi **WSL2-consumidor**: `swapon` num device cujo backend morreu → I/O do kernel em **D-state** ininterruptível. Esse vetor mora no **consumidor**; com WSL2 servidor-only ele cai no civm (VM Hyper-V isolada, recuperável por reboot), e a exposição do WSL2 fica em **userspace** (processo matável + fechamento de socket + free da VRAM no exit), não em D-state de kernel. Mitigações: DT-14 (`-timeout 30`, sem `-persist` → o nbd do tenant **erra** em vez de pendurar), ordem de teardown do runbook (swapoff no tenant **antes** de derrubar o broker), Fase A (`--backend ram`) primeiro. Corrige a ênfase exagerada de risco do WSL2 no e2e cross-host (#5 availability/ #11 halo do incidente ublk) |

## Fronteira de atomicidade e política de rollback

- **Fronteira de atomicidade desta implementação:**
  - **Atômico (garantido pelo broker):** uma slice nunca está `Active` em dois tenants ao mesmo
    tempo, e nunca é re-atribuída **sem ter sido zerada**. Sequência de movimento:
    `SwapOff(from)` → aguarda `SwapOffDone{ok:true}` → `ZeroExport(slice)` → aguarda
    `ZeroDone{ok:true}` → slice `Free` → só então `SwapOn(to)`. O broker é single-threaded no
    estado (mesmo padrão do worker CUDA), então não há corrida interna.
  - **Fora da atomicidade:** o par swapoff/swapon **não** é transacional entre hosts. Estados
    parciais aceitos nesta fase: slice `Draining` (swapoff em voo), slice `Draining` aguardando
    zero (`ZeroDone{ok:false}` → permanece `Draining`, log ERROR, retry de `ZeroExport` no
    próximo tick), slice `Free` sem dono entre os passos, `SwapOn` que falha no destino (slice
    volta a `Free`, retry no próximo tick, logado). Broker morto no meio do movimento: agentes
    mantêm seu estado de swap local; o watchdog limpa slices remotas; no restart o `Register`
    reconcilia (DT-9). Comando em voo para sessão que caiu: slice fica no estado intermediário,
    congelada pela regra DT-20 até re-`Register`.
- **Política de rollback:**
  - **Rollback de app:** parar agente(s) e daemon = `DemoteAll` no shutdown (SIGTERM) → agentes
    confirmam swapoff → daemon zera a VRAM (fluxo 4 do PRD; reuso do teardown F2 validado);
    espera bounded de 10s — no estouro, loga `shutdown_timeout` e sai assim mesmo (o residual é
    o cenário broker-morto, coberto pelo watchdog + drill). **Evidência:** cenário (f) do e2e
    (ITEM-10) + fase 0 do drill (ITEM-11). Reverter o código = `git revert`; o modo single
    (Fase B) permanece intacto (RNF-4), então o rollback de app degrada para o estado atual do
    repo, sem resíduo.
  - **Rollback de migration:** **N/A** — não há schema, banco ou migration nesta implementação.
  - **Rollback de dados:** páginas swapped em VRAM/NBD são **voláteis por design**. Matar o
    daemon com swap remoto ativo = perda das páginas swapped daquela slice; o swapoff de
    recuperação **pode falhar** (EIO no read-back de device morto) e a área pode permanecer
    listada em `/proc/swaps` — processos do tenant podem ser mortos ao tomar EIO. O objetivo de
    RNF-1 é **EIO bounded sem D-state global**, não recuperação das páginas. Mitigação:
    prioridade baixa (DT-7) mantém pouquíssimas páginas lá; drill fase 2 valida o pior caso
    **somente em qemu**.
  - **Proibido em produção (hosts reais EMEDEV/civm):** matar o daemon com swap remoto ativo sem
    `DemoteAll` prévio, exceto no drill em VM descartável; bind de NBD/TCP ou árbitro em
    `0.0.0.0` ou interface pública (o daemon recusa — ITEM-8).
  - **Forward-only:** N/A (sem dados persistentes).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina Kahneman | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (P0, gate) | #3 Número não adjetivo; #1 WYSIATI | [`docs/methodology/KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) §3, §1 | Os números de PSI/RTT/NBD-TCP existem, com unidade, n de rodadas e ambiente descrito? | `docs/memory-broker/P0-RESULTS.md` preenchido pela execução de `scripts/p0/*.sh` (CSV bruto commitado ou linkado), ≥3 rodadas por métrica | Qualquer célula do template vazia ou "estimado" ⇒ **nenhum item de P1 inicia** (gate anti-halo §14 do PRD) |
| ITEM-4 (árbitro) | #2 Counterfactual | idem, §2 | O que me faria desfazer um rebalanço? | `cargo test -p ramshared-broker` cobrindo: streak incompleto não move; cooldown bloqueia; nunca-zero respeitado; **PSI do drenado >2× em 60s E acima de `psi_floor` ⇒ `RevertMove` + cooldown longo** (teste com clock falso, incl. caso ruído-abaixo-do-piso que NÃO reverte) | Teste do counterfactual ausente ou falhando ⇒ não integra ITEM-8 |
| ITEM-5/ITEM-7 (contrato NBD por export) | #2 Counterfactual | idem, §2 | Cliente antigo (`nbd-client` sem `-N`, Fase B) continua funcionando byte a byte? | `cargo test -p ramshared-block` (handshake com nome vazio → export default, mesmo wire format dos testes atuais) + suíte existente verde | Qualquer teste existente de `handshake.rs`/`conn.rs` quebrar ⇒ reverter a mudança de assinatura, redesenhar |
| ITEM-8 (bind TCP, RNF-2) | #5 Availability heuristic | idem, §5 | O que acontece se o daemon subir com bind público por engano? | Teste unitário: `--listen-nbd tcp://0.0.0.0:10809` e `--arbiter-listen 0.0.0.0:7777` retornam `Err` antes de qualquer `bind()`; mensagem cita RNF-2 | Daemon aceita bind unspecified ⇒ bloqueia merge (CRITICAL) |
| ITEM-8 (higiene de slice, DT-17) | #5 Worst-case | idem, §5 | O que o tenant B consegue ler da slice que acabou de receber? | Unit: zero antes de `Free` na máquina de estados; e2e (h2): conteúdo escrito via export `s0` pelo "tenant A" não é legível após movimento para B (leitura NBD devolve zeros) | Slice re-atribuída sem `ZeroDone{ok:true}` ⇒ bloqueia merge (CRITICAL) |
| ITEM-9 (watchdog, RNF-1/R2) | #5 Availability heuristic; #13 Ilusão de validade | idem, §5, §13 | O teste do watchdog pode falhar pelo motivo certo (broker realmente morto, swap realmente **usado**), ou só valida um mock? | Unit: `Watchdog::expired` com clock falso. Integração real: ITEM-11 **fase 2** (kill -9 com `used_kb>0` na slice) — o unit e a fase 1 (swap vazio) **não** contam como evidência de RNF-1 sozinhos | Drill não executado, fase 2 sem `used_kb>0` verificado, ou swapoff não iniciado <5s após morte do broker ⇒ P1 não fecha (gate §10 do PRD) |
| ITEM-11 (drill D-state) | #5 Worst-case (PRD §14); #13 | idem, §5, §13 | O drill roda num ambiente onde um stall é recuperável, e exercita páginas residentes num device morto? | `scripts/kernel/qemu-broker-drill.sh` imprime os marcadores das 3 fases (`DRILL-GRACEFUL=ok`, `DRILL-IDLE-SWAPOFF=<N>s` com N<5, `DRILL-USED: attempt<5s, d_state=none>10s, echo<2s×3`) rodando **dentro do qemu**; log commitado em `docs/memory-broker/` no IMPL | Qualquer tentativa de rodar o drill no WSL2/host ⇒ abortar (repete o incidente 2026-06-09); stall na VM >60s ⇒ investigar antes de re-rodar; fase 2 com `used_kb==0` ⇒ resultado inválido, não conta |
| ITEM-2 (dep serde) | #11 Halo effect | idem, §11 | A dep nova tem ADR e entrada em LIBRARIES.md? | `docs/decisions/ADR-0005-broker-protocol-jsonl.md` + linha em `docs/LIBRARIES.md` no mesmo commit que adiciona a dep | Dep no `Cargo.toml` sem ADR ⇒ bloqueia commit |
| ITEM-12 (rollout civm) | #1 WYSIATI | idem, §1 | O runbook declara o que NÃO foi testado no ambiente do peer? | Runbook tem seção "Validado em / Não validado em"; e2e P1 executado uma vez seguindo o runbook literalmente (logs anexados no IMPL) | Passo do runbook que exige automação no host civm (política: peer copia template, zero automação) ⇒ reescrever o passo |

## Checklist de segurança (pré-implementação)

- [x] Isolamento de endereçamento: todo acesso por offset valida contra os limites da **slice**
      (`SliceView` reusa o bounds-check de `ramshared_block::serve` com `size_bytes()` = len da
      slice); anti-DoS de WRITE em `spawn_reader` passa a usar o tamanho do export negociado
- [x] Isolamento de conteúdo: slice é **zerada antes de qualquer re-atribuição** (DT-17);
      nenhuma página de um tenant é legível pelo próximo dono
- [x] Buffer overflow / OOB: `read_msg` impõe teto de linha (64 KiB) antes de alocar (mesmo
      padrão do `MAX_OPT_LEN` em `handshake.rs`); `copy_{from,to}_user` N/A (sem código kernel —
      PRD §8: nenhuma uAPI nova)
- [x] Permissões: `capable(CAP_SYS_ADMIN)` N/A em kernel; equivalente userspace: `ramshared-agent`
      checa `euid==0` no startup via `/proc/self/status` (DT-13/DT-26); daemon segue exigindo
      root para `mlockall` (ou `--force`)
- [x] Permissões: `permission.RequirePermission(...)` N/A (não há backend HTTP neste repo)
- [x] Preemption / IRQ flooding: N/A kernel; análogo userspace: árbitro fora do hot path
      (thread própria, tick 2s); canal `WMsg` segue como único ponto de backpressure do data
      plane; control plane tem backpressure próprio (DT-24)
- [x] Input validation: todo `Msg` recebido é validado (serde rejeita shape errado; `Register`
      com `proto != PROTO_VERSION` → `Error` + desconexão; nomes de export inexistentes →
      recusa no handshake; `Register` duplicado → `Error`, DT-22)
- [x] Ponteiros: nenhum endereço de kernel/GPU logado (logs usam ids de slice/tenant; regra
      pré-existente de não vazar KASLR em `MEMORY.md` mantida)
- [x] Nenhuma credencial hardcoded (não há credenciais; RNF-2 = bind privado, sem auth própria)
- [x] Erros internos não vazam detalhe: `Msg::Error{reason}` carrega frase curta; detalhe
      (errno, stderr do swapon) fica no log local do lado que falhou

## Estado de implementação (P1) — atualizado 2026-06-14

Detalhe e rastreabilidade em [`IMPL.md`](IMPL.md).

| ITEM | Estado | Evidência |
| --- | --- | --- |
| 1 — P0 (gate) | ✅ fechado | `P0-RESULTS.md` (§4 render pendente do tester, vira input P2) |
| 2 — ADR-0005 | ✅ | `docs/decisions/ADR-0005-broker-protocol-jsonl.md` |
| 3 — `protocol.rs` | ✅ | crate `ramshared-broker` (roundtrip por variante) |
| 4 — `slices.rs` + árbitro | ✅ | `ramshared-broker` (30+ testes; lease com revogação) |
| 5 — handshake por export | ✅ | `ramshared-block::handshake` (23 testes) |
| 6 — `SliceView` | ✅ | `ramshared-wsl2d::backend` |
| 7 — streams genéricos + TCP | ✅ | `conn.rs` (`spawn_acceptor_tcp`, `ZeroExport`) |
| 8 — `broker_srv` + fiação `run_broker` | ✅ | commits da fiação + `--backend ram`; drill PASS |
| 9 — agente | ✅ | crate `ramshared-agent` (25 testes; DT-27) |
| 10 — e2e in-process | ✅ | `tests/broker_e2e.rs` (3 testes) |
| 11 — drill qemu | ✅ **PASS** | `scripts/kernel/qemu-broker-drill.sh` (rodado: swap ativo via NBD + teardown limpo) |
| 12 — e2e civm | 🟢 **Fase A (RAM, cross-host) PASS** | broker RAM no WSL2 servindo swap ao civm (via túnel reverso SSH); civm ativou `/dev/nbd0`+`nbd1` (swapon ok s0+s1), teardown limpo. Falta: Fase B (VRAM cross-host) + deploy de produção via `netsh` ([`CIVM-TENANT.md`](CIVM-TENANT.md)) |

**Pendências:** ITEM-12 Fase B (VRAM cross-host) + deploy de produção via `netsh`. **Feito desde
então:** DT-5 rename `ramsharedd`; DT-29 (fronteira servidor-only); DT-30 (tick por deadline, fix de
starvation); `--advertise-nbd`; **ITEM-12 Fase A (RAM cross-host) = PASS**.

## Arquivos a CRIAR

### ITEM-1 — P0 (gate; sem código de produto)

**`scripts/p0/measure-psi.sh`**

- **Propósito**: amostrar `/proc/pressure/memory` (linhas `some` e `full`) a cada 1s por N
  segundos, em CSV (`ts,kind,avg10,avg60,avg300,total_us`).
- **Requisitos cobertos**: P0 (§10), R1 contexto.
- **Funções**: shell puro (`while read`-loop sobre `/proc/pressure/memory`); args: `DURATION`
  (default 300) e `OUT` (CSV). Sem dependências além de coreutils. Preflight: arquivo
  `/proc/pressure/memory` legível, senão erro citando `CONFIG_PSI` (WSL2 já confirmado ok;
  civm a confirmar na primeira execução).
- **Padrão de referência**: `scripts/kernel/qemu-ublk-daemon.sh` (estrutura `set -euo pipefail`,
  log com prefixo).
- **Testes requeridos**: execução manual nos 3 ambientes (WSL2, civm, idle e sob carga
  `cargo build -j4` / action real); CSVs viram tabelas do `P0-RESULTS.md`.

**`scripts/p0/measure-net.sh`**

- **Propósito**: matriz de alcançabilidade e RTT VM↔WSL2: `ping -c 100` (p50/p99 via sort),
  teste de porta TCP (`nc -z`) nos dois sentidos, com e sem Tailscale.
- **Requisitos cobertos**: P0, R1 (conectividade NAT — Inferência do PRD a validar).
- **Testes requeridos**: saída anexada ao `P0-RESULTS.md`; decide Tailscale-no-WSL2 vs
  port-forward (decisão registrada no próprio P0-RESULTS).

**`scripts/p0/measure-nbd-tcp.sh`**

- **Propósito**: p50/p99 de NBD/TCP **cru** no virt-switch, sem código nosso: servidor
  `nbdkit memory 1G` (ou `nbd-server` se nbdkit ausente) + cliente `nbd-client` + `fio`
  (randread/randwrite 4k, `--lat_percentiles=1`, 3 rodadas).
- **Preflight obrigatório**: checa `nbdkit`/`nbd-server`, `nbd-client` e `fio` com `command -v`
  e **falha cedo** com a linha de instalação (`sudo apt install nbdkit nbd-client fio`).
  Verificado nesta auditoria: o host hoje tem `nbd-client` e `fio`, **não** tem
  `nbdkit`/`nbd-server`; a civm precisa do mesmo preflight.
- **Requisitos cobertos**: P0, R4 (latência virt-switch).
- **Dependências externas**: `nbdkit`/`nbd-server`, `nbd-client`, `fio` (apenas nos ambientes de
  medição; nada entra no produto).
- **Testes requeridos**: 3 rodadas, números com stddev no `P0-RESULTS.md` (disciplina #3:
  comparáveis com p50 241µs ublk / 326µs NBD-Unix da Fase B).

**`scripts/p0/measure-render-vram.ps1`**

- **Propósito**: script Windows (PowerShell) para o tester: poll de 1s de
  `nvidia-smi --query-gpu=memory.used,memory.total --format=csv,noheader` + RAM
  (`Get-Counter '\Memory\Available MBytes'`) durante um render; CSV. **Não altera a cena**
  (Anexo B.5 do PRD).
- **Requisitos cobertos**: P0 (comportamento do out-of-core nativo; alimenta gate de P2).
- **Testes requeridos**: rodado primeiro no host EMEDEV (RTX 2060) antes de enviar ao tester.

**`docs/memory-broker/P0-RESULTS.md`**

- **Propósito**: template de evidência do gate P0 — uma tabela por métrica (PSI por ambiente
  idle/carga; RTT/portas; NBD/TCP p50/p99; render VRAM/RAM), célula = número + unidade + n de
  rodadas + data; seção final "Gate P1: ABERTO/FECHADO" + calibração dos defaults do árbitro
  (ver ITEM-4). Inclui também: confirmação de PSI habilitado na civm e page size dos dois
  tenants (`getconf PAGE_SIZE`; o `mkswap` por tenant do DT-16 já tolera divergência, mas o
  número fica registrado).
- **Requisitos cobertos**: P0, gate anti-halo (#11 do PRD §14).
- **Disciplina Kahneman**: ver linha ITEM-1 do mapa.

### ITEM-2 — Decisão de protocolo (ADR + deps)

**`docs/decisions/ADR-0005-broker-protocol-jsonl.md`**

- **Propósito**: registrar DT-1 (JSON-lines vs length-prefixed) com counterfactual numérico.
- **Requisitos cobertos**: RF-B1, DT-1.
- **Conteúdo mínimo**: contexto (control-plane 1 msg/s/tenant), alternativas (length-prefixed
  + bincode), decisão, **rollback trigger**: "se o protocolo precisar transportar payload de
  dados (>64 KiB/msg) ou >100 msg/s/tenant, migrar para length-prefixed via ADR superseding";
  deps `serde`/`serde_json` citando disciplina #11.
- **Padrão de referência**: `docs/decisions/ADR-0004-ublk-io-uring-crate.md` (formato).

### ITEM-3 — Crate `ramshared-broker`: protocolo e modelo

**`crates/ramshared-broker/Cargo.toml`**

- **Propósito**: lib de protocolo + política. Sem binário.
- **Dependências externas**: `serde = { version = "1", features = ["derive"] }`,
  `serde_json = "1"` (ADR-0005). Nada mais (sem tokio: threads std, padrão do workspace).
- **Requisitos cobertos**: RF-B1, RNF-5.

**`crates/ramshared-broker/src/lib.rs`**

- **Propósito**: `#![forbid(unsafe_code)]` + re-exports (`model`, `protocol`, `arbiter`, `slices`).
- **Requisitos cobertos**: RNF-5.

**`crates/ramshared-broker/src/model.rs`**

- **Propósito**: tipos do PRD §7, exatamente um lugar.
- **Requisitos cobertos**: RF-B1, RF-L1 (modelo de slice), RF-B3 (estado `Leased`, DT-19).
- **Structs/Types** (assinaturas exatas):

```rust
pub type TenantId = u32;
pub type SliceId = u16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum SliceState { Free, Active, Draining, Leased }

// PartialEq/Eq: `protocol::Msg` deriva PartialEq (testes de roundtrip) e embute `Vec<Slice>`
// em `StatusReply` → todos os campos da Slice são Eq (correção forçada no ITEM-3).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Slice {
    pub id: SliceId,
    pub offset: u64,
    pub len: u64,
    pub tenant: Option<TenantId>,
    pub state: SliceState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PsiSample {
    pub avg10: f32,
    pub avg60: f32,
    pub stall_us: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum TransportKind { NbdUnix, NbdTcp }

#[derive(Clone, Debug)]
pub struct Lease {
    pub id: u32,
    pub holder: TenantId,
    pub bytes: u64,
    pub slices: Vec<SliceId>,
    pub revocable: bool,
}
```

- **Padrão de referência**: enums de estado em `crates/ramshared-wsl2d/src/state.rs`.
- **Testes requeridos**: roundtrip serde de cada tipo (inline `#[cfg(test)]`).

**`crates/ramshared-broker/src/protocol.rs`**

- **Propósito**: wire format JSON-lines (DT-1) — mensagens RF-B1 + codec.
- **Requisitos cobertos**: RF-B1, DT-1, DT-18, DT-22.
- **Structs/Funções** (assinaturas exatas):

```rust
pub const PROTO_VERSION: u32 = 1;
pub const MAX_LINE_BYTES: usize = 64 * 1024; // anti-DoS, espelha MAX_OPT_LEN do handshake NBD

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Msg {
    // agente/cliente → broker
    Register { proto: u32, tenant: String, transport: TransportKind },
    Psi { sample: PsiSample, swaps: Vec<SwapEntry> },
    SwapOnDone { slice: SliceId, ok: bool, detail: String },
    SwapOffDone { slice: SliceId, ok: bool, detail: String },
    LeaseRequest { bytes: u64 },
    LeaseRelease { lease: u32 },
    Status,
    // broker → agente/cliente
    Registered { tenant_id: TenantId },
    Ack,
    SwapOn { slice: SliceId, export: String, endpoint: NbdEndpoint, swap_prio: Option<i32> },
    SwapOff { slice: SliceId },
    DemoteAll,
    LeaseGranted { lease: u32, bytes: u64 },
    LeaseDenied { reason: String },
    StatusReply { tenants: Vec<TenantStatus>, slices: Vec<Slice>, last_rebalance_secs: Option<u64> },
    Error { reason: String },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NbdEndpoint {
    Unix { path: String },
    Tcp { host: String, port: u16 },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SwapEntry { pub dev: String, pub prio: i32, pub size_kb: u64, pub used_kb: u64 }

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TenantStatus {
    pub id: TenantId,
    pub name: String,
    pub psi: PsiSample,
    pub slices: Vec<SliceId>,
    pub present: bool, // DT-20: false = sessão caída, slices congeladas
}

/// Serializa `msg` + '\n' e dá flush (uma mensagem por linha).
pub fn write_msg<W: std::io::Write>(w: &mut W, msg: &Msg) -> std::io::Result<()>;

/// Lê uma linha (teto MAX_LINE_BYTES) e desserializa. Ok(None) em EOF limpo;
/// Err em linha gigante, JSON inválido ou shape desconhecido.
pub fn read_msg<R: std::io::BufRead>(r: &mut R) -> std::io::Result<Option<Msg>>;
```

- **Semântica de sessão (DT-18/DT-22, normativa):** broker responde `Ack` a **cada** `Psi`;
  `Status` é aceito sem `Register` prévio; `Register` com `tenant` já em sessão viva →
  `Error{reason:"tenant_duplicado"}` + desconexão; `proto != PROTO_VERSION` → `Error` +
  desconexão (teste no ITEM-8).
- **Padrão de referência**: `crates/ramshared-block/src/handshake.rs` (codec genérico sobre
  `Read`/`Write`, testável com `Cursor`, teto anti-alloc).
- **Testes requeridos** (inline): roundtrip de cada variante; linha > `MAX_LINE_BYTES` falha
  **antes** de alocar; JSON com `type` desconhecido → `Err`.

### ITEM-4 — Crate `ramshared-broker`: slices e árbitro (lógica pura)

**`crates/ramshared-broker/src/slices.rs`**

- **Propósito**: partição estática da VRAM em K slices e mapa dinâmico slice→tenant (RF-L1),
  incluindo o estado `Leased` (DT-19).
- **Requisitos cobertos**: RF-L1, RF-B3.
- **Structs/Funções** (assinaturas exatas):

```rust
pub struct SliceMap { slices: Vec<Slice> }

impl SliceMap {
    /// K slices de `slice_bytes`, offsets `i * slice_bytes`, todas Free.
    pub fn new(k: u16, slice_bytes: u64) -> Self;
    pub fn total_bytes(&self) -> u64;
    pub fn get(&self, id: SliceId) -> Option<&Slice>;
    pub fn slices(&self) -> &[Slice];
    /// Free → Active(tenant). Err se não-Free (invariante de atomicidade; Leased também recusa).
    pub fn assign(&mut self, id: SliceId, tenant: TenantId) -> Result<(), SliceError>;
    /// Active → Draining. Err se não-Active.
    pub fn drain(&mut self, id: SliceId) -> Result<(), SliceError>;
    /// Draining → Free. SÓ pode ser chamada após SwapOffDone{ok} E ZeroDone{ok} (DT-17).
    pub fn release(&mut self, id: SliceId) -> Result<(), SliceError>;
    /// Free → Leased (grant de lease, DT-19). Err se não-Free.
    pub fn lease(&mut self, id: SliceId) -> Result<(), SliceError>;
    /// Leased → Free (release de lease). Err se não-Leased.
    pub fn unlease(&mut self, id: SliceId) -> Result<(), SliceError>;
    /// Nomes de export NBD: ("s0", len), ("s1", len)...
    pub fn exports(&self) -> Vec<(String, u64)>;
}

#[derive(Debug, PartialEq)]
pub enum SliceError { UnknownSlice, BadState { have: SliceState } }
```

- **Padrão de referência**: máquina de estados de `crates/ramshared-wsl2d/src/state.rs`
  (transições ilegais rejeitadas, testes `illegal_jumps_rejected`).
- **Testes requeridos**: transições legais (incl. `lease`/`unlease`); ilegais rejeitadas
  (`assign` em Active **e em Leased** falha — teste da fronteira de atomicidade e do DT-19);
  offsets disjuntos cobrem `total_bytes` sem gap.

**`crates/ramshared-broker/src/arbiter.rs`**

- **Propósito**: política do árbitro **pura** (sem IO, clock injetado) — RF-B2, RF-B3, RNF-3 e
  counterfactual §14 do PRD com piso (DT-23).
- **Requisitos cobertos**: RF-B2, RF-B3, RNF-3, DT-6, DT-8, DT-19, DT-20, DT-23.
- **Structs/Funções** (assinaturas exatas):

```rust
#[derive(Clone, Copy, Debug)]
pub struct ArbiterConfig {
    pub delta_psi: f32,                 // diferencial some.avg10 para mover
    pub streak: u32,                    // ticks consecutivos acima do delta
    pub cooldown: std::time::Duration,  // pós-movimento
    pub psi_floor: f32,                 // "sob pressão" (nunca-zero) e piso do counterfactual (DT-23)
    pub cf_window: std::time::Duration, // janela do counterfactual
    pub cf_factor: f32,                 // piora do drenado que dispara revert
    pub cf_cooldown: std::time::Duration, // cooldown longo pós-revert
}
impl Default for ArbiterConfig {} // ver tabela de defaults abaixo

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TenantView { pub id: TenantId, pub psi: PsiSample, pub slices: u16 }

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    AssignFree { slice: SliceId, to: TenantId },                    // DT-6 round-robin
    MoveSlice { slice: SliceId, from: TenantId, to: TenantId },     // RF-B2
    RevertMove { slice: SliceId, from: TenantId, to: TenantId },    // counterfactual §14 + DT-23
    RevokeForLease { slice: SliceId, from: TenantId, lease: u32 },  // RF-B3
    GrantLease { lease: u32, holder: TenantId, slices: Vec<SliceId> },
}

pub struct Arbiter { /* cfg, streak, last_move: Option<MoveRecord>, rr_cursor, next_lease_id */ }

impl Arbiter {
    pub fn new(cfg: ArbiterConfig) -> Self;
    /// `now` injetado (testável). CONTRATO (DT-20): o chamador passa em `tenants` SÓ os
    /// presentes, e em `slices` SÓ as slices cujo dono está presente ou que estão Free/Leased —
    /// portanto nenhuma Action emitida pode ter alvo ausente. Nunca emite MoveSlice que deixe
    /// tenant com psi.avg10 > psi_floor com zero slices (DT-8: não vale para lease).
    pub fn tick(
        &mut self,
        now: std::time::Instant,
        tenants: &[TenantView],
        slices: &[Slice],
        pending_lease: Option<(TenantId, u64)>,
    ) -> Vec<Action>;
}
```

- **Lógica resumida do `tick`** (ordem; **no máximo um `MoveSlice` OU `RevertMove` por tick**;
  **enquanto há `pending_lease`, os passos (2) e (4) ficam suprimidos** — só ações de lease +
  nunca-zero, para não competir com a revogação em curso, R9; `AssignFree` e ações de lease
  podem coexistir no mesmo `Vec`):
  (1) lease pendente tem prioridade — calcula ⌈bytes/slice_len⌉ slices; emite `RevokeForLease`
  para as `Active` que faltarem e `GrantLease` **uma única vez** quando `Free`+`Leased` ≥ need.
  A **reserva (DT-19/R2)** é realizada pelo **core** (ITEM-8) aplicando `GrantLease`→`lease()`; o
  árbitro puro não muta `SliceMap`, só garante a reserva **suprimindo o passo (5) enquanto há
  `pending_lease`** (slices `Free` não são round-robinadas, ficam disponíveis para o grant).
  **Ordem de revogação (decisão de impl ITEM-4/arbiter):** o `tick` é **puro** e recebe
  `TenantView{psi}` — **não tem `used_kb` por slice**. Logo, "mais ociosas primeiro" usa o
  **`psi.avg10` do dono, ascendente** (tenant menos pressionado primeiro) como proxy
  implementável; o critério por `used_kb` do DT-19 exigiria o core passar `used_kb` por slice ao
  `tick` e fica **adiado** (refinamento, não bloqueia P1 — DT-10: sem holder real de lease em P1);
  (2) counterfactual: se `last_move` dentro de `cf_window` e `psi(from).avg10 >
  cf_factor × psi_no_momento_do_move` **e** `psi(from).avg10 > psi_floor` (DT-23) →
  `RevertMove` + `cf_cooldown`;
  (3) cooldown ativo → nada de movimento;
  (4) diferencial: maior PSI − menor PSI > `delta_psi` por `streak` ticks → `MoveSlice` de uma
  slice do menos pressionado (respeitando nunca-zero);
  (5) **só quando não há lease pendente** (R2 — senão o round-robin roubaria slices reservadas):
  slices `Free` (nunca `Leased` — o estado resolve estruturalmente) → `AssignFree` round-robin
  entre presentes.
- **Defaults provisórios** (constantes em `impl Default`; **P0 calibra** — recalibração é
  atualização deste SPEC + commit citando `P0-RESULTS.md`):

| Parâmetro | Default provisório | Calibrado por |
| --- | --- | --- |
| `delta_psi` | **10.0** (pontos some.avg10; era 15.0) | **Recalibrado por P0** (P0-RESULTS §5): WSL2 carga 14.25 vs civm idle ~1.2 ⇒ Δ≈13; 15 não moveria. Validar no e2e P1 |
| `streak` | 5 ticks (tick = 2s → 10s) | idem |
| `cooldown` | 60s | PRD §14 |
| `psi_floor` | 5.0 | PSI idle medido |
| `cf_window` / `cf_factor` / `cf_cooldown` | 60s / 2.0 / 300s | PRD §14 (fator fixo: trigger do PRD; piso = `psi_floor`, DT-23) |

- **Padrão de referência**: `ResidencySampler` em `crates/ramshared-wsl2d/src/residency.rs`
  (histerese por streak, lógica pura testável — reuso de padrão exigido por RF-B2).
- **Testes requeridos**: ver mapa Kahneman (linha ITEM-4) — histerese, cooldown, nunca-zero,
  counterfactual com clock falso **incl. ruído abaixo do piso que NÃO reverte** (DT-23), lease
  drena além do nunca-zero (DT-8), lease não re-atribui `Leased` (DT-19), **lease que precisa
  revogar `Active` (não só conceder de `Free`): slice liberada vira `Leased` reservada e NÃO
  volta ao round-robin durante a revogação multi-tick — R2**, segundo lease → negado,
  round-robin estável e só entre presentes.
- **Disciplina Kahneman**: #2 Counterfactual — ver mapa.

### ITEM-8 (parte CRIAR) — Integração do broker no daemon

**`crates/ramshared-wsl2d/src/broker_srv.rs`**

- **Propósito**: listener TCP do árbitro + sessões de agente + loop de decisão; ponte com o
  worker NBD (demote + zero de slice) — RF-B1..B4 "vivos" no daemon.
- **Requisitos cobertos**: RF-B1, RF-B2, RF-B3, RF-B4, RNF-2, RNF-3, DT-2, DT-17..DT-25.
- **Structs/Funções** (assinaturas exatas):

```rust
pub struct BrokerConfig {
    pub listen: std::net::SocketAddr,         // já validado não-unspecified (main.rs)
    pub nbd_unix: Option<String>,             // DT-25: endpoint p/ tenants NbdUnix
    pub nbd_tcp: Option<std::net::SocketAddr>,// DT-25: endpoint p/ tenants NbdTcp
    pub arbiter: ArbiterConfig,
}

/// Eventos consumidos pelo core single-threaded (DT-24).
enum CoreEvent {
    Agent(TenantId, Msg),       // readers por conexão
    AgentGone(TenantId),        // EOF/erro do reader
    Demote(DemoteReason),       // forwarder do demote_rx (canário §9/§9.4) → DemoteAll
    Tick,                       // implícito: recv_timeout(2s) esgotado
}

/// Sobe acceptor + core single-threaded do broker. `demote_rx` é drenado por uma thread
/// forwarder (→ CoreEvent::Demote → DemoteAll: SwapOff em todas as Active).
/// `jobs` é o canal do worker NBD: o core envia WMsg::ZeroExport nele (DT-17) e coleta as
/// confirmações em pending_zeros (try_recv por iteração).
/// `shutdown` é a flag SIGTERM: dispara DemoteAll + espera Done (bounded 10s; no estouro
/// loga `shutdown_timeout` e segue) antes de sair.
pub fn spawn_broker(
    cfg: BrokerConfig,
    slice_map: SliceMap,
    demote_rx: std::sync::mpsc::Receiver<DemoteReason>,
    jobs: std::sync::mpsc::SyncSender<WMsg>,
    shutdown: &'static std::sync::atomic::AtomicBool,
) -> std::io::Result<std::thread::JoinHandle<()>>;
```

- **Desenho interno** (mesmo padrão do worker CUDA único / `conn.rs`; zero locks, só canais):
  - acceptor TCP spawna, por conexão, um **reader** (desserializa `Msg` → `CoreEvent::Agent`)
    e uma **writer thread** dona da metade de escrita, drenando um canal **bounded (cap 64)**;
  - o **core** (uma thread, dono de `SliceMap`+`Arbiter`+tabela de sessões+`pending_zeros`+
    `pending_lease`+mapa nome→`TenantId`) consome `CoreEvent` com `recv_timeout(2s)` —
    timeout = tick do árbitro; outbound via `try_send` no canal da sessão — **cheio = sessão
    não-drenante → fecha a sessão, marca ausente, log ERROR** (DT-24);
  - **heartbeat (DT-18)**: todo `Psi` recebido responde `Ack` imediatamente;
  - **ausência (DT-20)**: `AgentGone` marca o tenant ausente (slices congeladas — fora da view
    do árbitro); nenhuma Action com alvo ausente é possível por construção do contrato do
    `tick`;
  - **movimento com higiene (DT-17)**: `SwapOffDone{ok:true}` → `try_send(WMsg::ZeroExport{
    export, done})` → guarda `(SliceId, Receiver<bool>)` em `pending_zeros` → `ZeroDone{ok:true}`
    (via `try_recv` por iteração) → `release()` → `Free`. `ZeroDone{ok:false}` **ou** `try_send`
    falho (canal `jobs` cheio) → slice permanece `Draining`, log ERROR, retry de `ZeroExport`
    no próximo tick (R4);
  - **lease (DT-19)**: no máximo 1 pendente/ativo; `LeaseRequest` extra → `LeaseDenied
    {reason:"lease_em_andamento"}`; `bytes` > `total_bytes()` → `LeaseDenied
    {reason:"acima_da_capacidade"}`; EOF do holder → release automático (unlease de todas);
  - **endpoint por transporte (DT-25)**: `SwapOn.endpoint` = `nbd_unix`/`nbd_tcp` conforme o
    `transport` registrado; transporte não configurado → `Error{reason:"transporte_indisponivel"}`.
- **Log de decisão (RF-B4)**, uma linha por decisão, chave=valor:
  `[ramsharedd] arbiter move slice=s1 from=civm(psi10=32.1) to=wsl2(psi10=4.2) streak=5 cooldown=60s`
  (sempre as pressões **dos dois lados**). `Ack` não é logado (DT-18).
- **Reconciliação (DT-9/DT-21)**: no `Register`, o primeiro `Psi{swaps}` é comparado aos
  exports — `SwapEntry.dev` que casa `{nbd-dev-base}{N}` mapeia para a slice `sN`; slice que o
  agente já tem em `/proc/swaps` re-`assign` sem comando; entradas fora do padrão são ignoradas.
- **Dependências internas**: `ramshared_broker::{protocol, slices, arbiter, model}`,
  `crate::conn::WMsg` (variante `ZeroExport`).
- **Padrão de referência**: `spawn_acceptor`/canais de `crates/ramshared-wsl2d/src/conn.rs`.
- **Testes requeridos**: ITEM-10 (e2e in-process com agentes falsos); proto errado → `Error` +
  desconexão; `Register` duplicado → `Error` (DT-22); agente que some (EOF) → tenant ausente,
  slices congeladas, nenhuma Action o referencia (cenário (i)); `Status` sem `Register`
  respondido (DT-22).
- **Disciplina Kahneman**: #5 — ver mapa (bind e higiene de slice) e ITEM-9/11 (worst-case).

### ITEM-9 — Crate `ramshared-agent`

**`crates/ramshared-agent/Cargo.toml`**

- **Propósito**: binário `ramshared-agent` (PRD §8). Deps: `ramshared-broker` (path),
  `serde`/`serde_json` (transitivo). `#![forbid(unsafe_code)]`.
- **Requisitos cobertos**: RF-L3, RNF-1, RNF-5.

**`crates/ramshared-agent/src/main.rs`**

- **Propósito**: CLI + loop principal: conectar/reconectar ao broker, reportar PSI a 1 Hz,
  executar comandos, watchdog.
- **Requisitos cobertos**: RF-L3, RF-B1 (lado agente), RNF-1, DT-13, DT-16, DT-26.
- **Flags CLI** (parsing manual, mesmo padrão do `run()` em `crates/ramshared-wsl2d/src/main.rs`):
  `--broker IP:PORT` (obrigatória) · `--tenant NAME` (obrigatória) · `--swap-prio P`
  (opcional, DT-7) · `--nbd-dev-base PATH` (default `/dev/nbd`; device da slice `sN` =
  `{base}{N}` — **contrato DT-21**) · `--status` (modo one-shot: envia `Status` sem `Register`,
  imprime a linha JSON do `StatusReply`, sai — DT-22).
- **Lógica resumida** (duas threads, DT-27 — espelha o `spawn_swapoff` do daemon): (1) checa
  `euid==0` lendo `/proc/self/status` (DT-13/DT-26; `--status` dispensa); (2) conecta TCP,
  `set_read_timeout(1s)`, `Register`; (3) **thread de exec** (fila serial, 1 comando por vez):
  recebe comandos do loop principal e executa — `SwapOn` → `nbd_connect` + **`mk_swap`
  (DT-16)** + `swap_on`, depois emite `SwapOnDone`; `SwapOff`/`DemoteAll` → `swap_off` +
  `nbd_disconnect`, depois `SwapOffDone`; (4) **loop principal** (heartbeat — nunca bloqueia
  em comando): a cada 1s envia `Psi` (`psi.rs`); lê `Msg` do broker (comandos vão para a fila
  de exec); qualquer byte do broker (o `Ack` por `Psi` garante ≥1 Hz — DT-18) alimenta
  `Watchdog::touch`; (5) `Watchdog::expired` (broker silencioso ≥ deadline, **independente da
  duração de qualquer comando** — R1) → **cleanup best-effort**: `swap_off` (pode falhar com
  EIO se houver páginas no device morto — esperado, logado) + `nbd_disconnect` de toda slice
  ativa, depois loop de reconexão com backoff (1s..30s).
- **Testes requeridos**: dispatch de comandos com transporte `Cursor` (sem rede), asserta a
  ordem `nbd_connect → mk_swap → swap_on`; cleanup chamado exatamente uma vez por expiração;
  **comando de exec lento (stub que dorme > deadline) NÃO expira o watchdog enquanto o broker
  segue mandando `Ack` — R1** (clock falso + thread de exec stub).
- **Disciplina Kahneman** (RNF-1): #5/#13 — ver mapa (linha ITEM-9).

**`crates/ramshared-agent/src/psi.rs`**

- **Propósito**: parsers de `/proc/pressure/memory`, `/proc/swaps` e `/proc/self/status`
  (RF-L3, DT-26).
- **Funções** (assinaturas exatas):

```rust
/// Parse da linha `some` (DT-15): avg10/avg60 + total (us) → stall_us. `full` é logada.
pub fn read_psi() -> std::io::Result<PsiSample>;
pub fn parse_psi(content: &str) -> Option<PsiSample>;          // puro, testável
pub fn read_swaps() -> std::io::Result<Vec<SwapEntry>>;
pub fn parse_swaps(content: &str) -> Vec<SwapEntry>;           // puro, testável
/// euid via /proc/self/status, linha "Uid:\treal\teffective\t..." (DT-26). Zero-dep, sem unsafe.
pub fn read_euid() -> std::io::Result<u32>;
pub fn parse_euid(status: &str) -> Option<u32>;                // puro, testável
```

- **Testes requeridos**: fixtures literais de `/proc/pressure/memory`, `/proc/swaps` e
  `/proc/self/status` reais (WSL2 e civm, coletadas no P0); entrada truncada/corrompida →
  `None`/vazio, nunca panic.

**`crates/ramshared-agent/src/swap.rs`**

- **Propósito**: execução de `nbd-client`/`mkswap`/`swapon`/`swapoff` (generalização do padrão
  `spawn_swapoff` de `crates/ramshared-wsl2d/src/swap.rs`).
- **Requisitos cobertos**: RF-L3, RNF-1, DT-7, DT-14, DT-16.
- **Funções** (assinaturas exatas):

```rust
/// nbd-client -N <export> (-unix <path> | <host> <port>) <dev> -timeout 30  — nunca -persist (DT-14).
pub fn nbd_connect(endpoint: &NbdEndpoint, export: &str, dev: &str) -> std::io::Result<()>;
pub fn nbd_disconnect(dev: &str) -> bool;                       // nbd-client -d, best-effort
/// mkswap <dev> — obrigatório antes de todo swap_on (DT-16; a slice chega zerada, DT-17).
pub fn mk_swap(dev: &str) -> std::io::Result<()>;
/// swapon [<-p prio>] <dev>. prio=None ⇒ sem -p (DT-7).
pub fn swap_on(dev: &str, prio: Option<i32>) -> std::io::Result<()>;
/// swapoff em thread separada (pode bloquear) — mesmo desenho anti-deadlock de spawn_swapoff.
pub fn spawn_swap_off(dev: &str) -> std::sync::mpsc::Receiver<bool>;
```

- **Padrão de referência**: `spawn_swapoff` (`crates/ramshared-wsl2d/src/swap.rs:23`) — o
  original do daemon **não** muda nem move (o caminho de DEMOTE single-mode continua dono dele).
- **Testes requeridos**: montagem de argv pura (função `fn nbd_args(...) -> Vec<String>`
  separada do spawn, testável): `-timeout 30` sempre presente, `-persist` nunca; `-p` só com
  `Some`; `mkswap` sem label.

**`crates/ramshared-agent/src/watchdog.rs`**

- **Propósito**: detecção de broker morto (RNF-1, R2/R7) — lógica pura, clock injetado.
- **Funções** (assinaturas exatas):

```rust
pub struct Watchdog { deadline: std::time::Duration, last: std::time::Instant }

impl Watchdog {
    pub fn new(deadline: std::time::Duration, now: std::time::Instant) -> Self; // default 3s
    pub fn touch(&mut self, now: std::time::Instant);
    pub fn expired(&self, now: std::time::Instant) -> bool;
}
```

- **Dimensionamento**: heartbeat garantido 1 Hz (o `Ack` por `Psi`, DT-18) e deadline 3s = 3
  heartbeats perdidos ⇒ detecção ≤3s + início do swapoff best-effort ⇒ **tentativa <5s**, o
  gate do PRD §10/P1 (sucesso do swapoff depende de haver páginas no device morto — ver
  rollback de dados e drill fase 2).
- **Testes requeridos**: não expira com touch em dia; expira após deadline; touch pós-expiração
  re-arma (reconexão).
- **Disciplina Kahneman**: #13 — o unit test usa clock falso e **não substitui** o drill
  (ITEM-11, fase 2) como evidência de RNF-1; ver mapa.

### ITEM-10 — Teste e2e in-process

**`crates/ramshared-wsl2d/tests/broker_e2e.rs`**

- **Propósito**: e2e sem root/GPU/swap real: broker real (`spawn_broker` com `SliceMap` 2×64MiB
  e worker NBD com `RamBackend`) + 2 agentes falsos (TcpStream falando `protocol::Msg`,
  respondendo `SwapOnDone`/`SwapOffDone` imediatos) em `127.0.0.1:0`.
- **Requisitos cobertos**: RF-B1, RF-B2, RF-B3, RF-B4 (integração), RNF-4 (não toca o modo
  single), DT-17..DT-22.
- **Cenários mínimos**:
  (a) registro + round-robin inicial (DT-6);
  (b) PSI desequilibrado por streak ticks → observa `SwapOff` no doador e `SwapOn` no receptor
  **nessa ordem**, com a slice só re-atribuída **após** o zero (fronteira de atomicidade +
  DT-17);
  (c) `Status` reflete slices/tenant/PSI/presença;
  (d) `LeaseRequest` que **precisa revogar** uma slice `Active` (não só conceder de `Free` —
  R2): observa `RevokeForLease` → SwapOff → Zero → a slice liberada vira `Leased` reservada (não
  volta ao round-robin durante a revogação) → `GrantLease`; `LeaseRelease` devolve; durante o
  lease, **nenhum tick re-atribui as slices `Leased`** (DT-19); segundo `LeaseRequest` →
  `LeaseDenied`;
  (e) proto errado → `Error`; `Register` duplicado → `Error` (DT-22);
  (f) **shutdown ordenado**: seta a flag → broker emite `DemoteAll` → agentes confirmam →
  `JoinHandle` retorna ≤10s (evidência do rollback de app — F7);
  (g) **heartbeat**: todo `Psi` é respondido com `Ack` (cadência observada por N ciclos, DT-18);
  (h2) **higiene**: "tenant A" escreve padrão via export `s0` (conexão NBD in-process ao worker
  `RamBackend`), slice move para B → leitura via `s0` devolve zeros (DT-17);
  (i) **ausência**: derruba a conexão de um agente falso com slice Active → por ≥3 ticks o
  broker não emite nenhuma mensagem referenciando aquele tenant; reconecta + `Psi{swaps}`
  coerente → reconciliação re-assign sem novo `SwapOn` (DT-20/DT-9/DT-21);
  (j) **worker persistente (R7)**: injeta `DemoteAll` (via `demote_rx`) → todos os agentes falsos
  desconectam o NBD (`live` cai a 0) → o **worker NÃO encerra**; um desequilíbrio de PSI (ou
  `LeaseRequest`) subsequente ainda gera `SwapOn` servido — prova que o daemon segue vivo após
  um demote normal.
- **Restrição operacional**: tudo in-process (threads), **nenhum daemon standalone spawnado** —
  smoke de daemon só em VM/qemu (incidente WSL2; regra de sessão).
- **Disciplina Kahneman**: #13 — este teste valida protocolo+política; o modo de falha real
  (swap usado, broker morto) é o ITEM-11.

### ITEM-11 — Drill de D-state em qemu (3 fases)

**`scripts/kernel/qemu-broker-drill.sh`**

- **Propósito**: worst-case obrigatório do PRD §14, **dentro da VM qemu**, com critérios
  numéricos por fase. Setup comum: `ramsharedd --backend ram --slices 2 --slice-mb 64
  --sock /tmp/d.sock --listen-nbd tcp://127.0.0.1:10809 --arbiter-listen 127.0.0.1:7777` +
  `ramshared-agent --broker 127.0.0.1:7777 --tenant vm`; espera slice ativa em `/proc/swaps`
  (via `nbd-client` + `mkswap` + `swapon` comandados pelo broker — DT-16). A VM **não tem swap
  local** e sobe com `-m` apertado (ex.: 256M) para a fase 2 empurrar páginas à slice.
- **Pré-requisitos do initramfs** (deltas sobre o harness F2 — verificados nesta auditoria):
  - `nbd.ko` copiado de `/home/emdev/WSL2-Linux-Kernel/drivers/block/nbd.ko`
    (`CONFIG_BLK_DEV_NBD=m` confirmado) + `insmod /modules/nbd.ko nbds_max=8` no `/init`
    (sem `nbds_max` suficiente, o `/dev/nbdN` da slice pode não existir — R5);
  - binário `nbd-client` do host (`/usr/sbin/nbd-client`) + libs via `ldd` (mesmo método já
    usado para o daemon);
  - `ip link set lo up` no `/init` (o harness F2 **não** sobe loopback — verificado; TCP
    127.0.0.1 morto sem isso);
  - `--sock /tmp/d.sock` no daemon (o default `/run/ramshared/wsl2d.sock` exige diretório
    inexistente no initramfs throwaway → `bind` falha e o daemon não sobe; `/tmp` é montado
    pelo `/init` — R5);
  - `mkswap`/`swapon`/`swapoff`/`awk` são applets do busybox já presente; o sampler de
    estado-D lê o caractere de estado **após o último `)`** de `/proc/<pid>/stat` (o `comm`
    pode conter `)` — R5).
- **Fase 0 — graceful (evidência do fluxo 4 / rollback de app):** SIGTERM no daemon → espera:
  agente recebe `DemoteAll`, swapoff confirmado, daemon sai limpo ≤10s, `/proc/swaps` vazio.
  Marcador: `DRILL-GRACEFUL=ok`.
- **Fase 1 — kill -9 com swap vazio:** re-sobe tudo; slice ativa com `used_kb==0` → `kill -9`
  no daemon → PASS se a entrada some de `/proc/swaps` em **<5s**. Marcador:
  `DRILL-IDLE-SWAPOFF=<N>s`.
- **Fase 2 — kill -9 com swap USADO (o worst case real, #13):** hog de memória (busybox
  `awk 'BEGIN{s="x"; while(length(s)<N) s=s s}'`) até `/proc/swaps` mostrar **`used_kb ≥
  1024`** na slice (pré-condição verificada; sem ela a fase é inválida) → `kill -9` no daemon
  → resultado esperado **definido**: o swapoff de recuperação **pode falhar** (EIO no
  read-back; a área pode permanecer listada); PASS =
  (a) tentativa de swapoff iniciada **<5s** pós-morte (log do agente),
  (b) **nenhum processo em estado D por >10s** (amostra de `/proc/*/stat` campo 3 a 1 Hz),
  (c) shell da VM responde: `echo DRILL-ALIVE` ecoa em **<2s**, 3 medições consecutivas,
  (d) `nbd-client -d` tentado (best-effort, logado).
  Marcador: `DRILL-USED: attempt=<N>s d_state=none echo=ok`.
- **Requisitos cobertos**: RNF-1, R2/R7, gate P1, fluxo 4 do PRD.
- **Padrão de referência**: `scripts/kernel/qemu-ublk-daemon.sh` (harness F2: mesma VM, mesmo
  mecanismo de injeção/coleta por marcadores).
- **Testes requeridos**: o script **é** o teste; log das 3 fases vai para o IMPL.
- **Disciplina Kahneman**: #5/#13 — ver mapa (proibido rodar fora do qemu; fase 2 sem
  `used_kb>0` não conta como evidência).

### ITEM-12 — Runbook civm

**`docs/runbooks/CIVM-TENANT.md`**

- **Propósito**: RF-L4 — provisionamento copiável do tenant civm: instalar `nbd-client`,
  **carregar o módulo (`sudo modprobe nbd nbds_max=8` + persistência: `nbd` em
  `/etc/modules-load.d/ramshared.conf` e `options nbd nbds_max=8` em
  `/etc/modprobe.d/ramshared.conf` — R5)**, copiar binário `ramshared-agent`, unit systemd
  (template literal no doc), conectividade (resultado do `measure-net.sh` decide Tailscale vs
  port-forward), verificação (`/proc/swaps` com prioridade negativa, `--status`), **runbook de
  remoção** (RNF-1: swapoff → nbd-client -d → disable unit; nota: se o broker estiver morto, o
  swapoff pode falhar com EIO — seguir mesmo assim e reiniciar a VM se a área ficar presa) e
  seção "Validado em / Não validado em" (#1 WYSIATI).
- **Requisitos cobertos**: RF-L4, RNF-1 (remoção), RNF-2 (bind privado).
- **Restrição**: política do civm — peer copia template; **zero automação de host** no repo.
- **Padrão de referência**: `docs/runbooks/FASE-B-KERNEL.md`.

## Arquivos a MODIFICAR

### ITEM-5 — `crates/ramshared-block/src/handshake.rs`

- **O que muda**: `server_handshake` passa a **resolver o export pelo nome** (hoje ignora o
  nome em `NBD_OPT_EXPORT_NAME`/`NBD_OPT_GO` e responde sempre `export_size`).
- **Requisitos cobertos**: RF-L1, DT-3.
- **Função/bloco afetado**: `server_handshake`, `write_export_info`, braços
  `NBD_OPT_EXPORT_NAME`/`NBD_OPT_GO`/`NBD_OPT_INFO`.
- **Antes**:

```rust
pub fn server_handshake<R: Read, W: Write>(
    r: &mut R, w: &mut W, export_size: u64, tx_flags: u16,
) -> Result<(), HandshakeError>
```

- **Depois**:

```rust
/// Um export disponível para negociação. `name == ""` nunca aparece na tabela;
/// nome vazio do cliente resolve para `exports[0]` (default NBD; compat Fase B).
pub struct Export { pub name: String, pub size: u64 }

/// Retorna o índice do export negociado em `exports`.
pub fn server_handshake<R: Read, W: Write>(
    r: &mut R, w: &mut W, exports: &[Export], tx_flags: u16,
) -> Result<usize, HandshakeError>
```

  Resolução: extrai o nome (payload de `EXPORT_NAME`; campo nome de `GO`/`INFO` — `len u32 +
  bytes`, validado UTF-8); vazio → índice 0; não encontrado → `GO`/`INFO` respondem
  `NBD_REP_ERR_UNKNOWN` (`0x8000_0006`, constante nova) e seguem negociando; `EXPORT_NAME`
  desconhecido → `HandshakeError::Io` (o protocolo manda fechar). `HandshakeError` ganha a
  variante `UnknownExport(String)` apenas se necessário para log — caso contrário manter `Io`.
- **Por quê**: RF-L1 — slice = export nomeado; é o único ponto onde o cliente escolhe.
- **Impacto**: quebra a assinatura → callers: `spawn_reader` (`conn.rs:124`, ITEM-7) e os
  testes do próprio arquivo. Não há outros usuários (Confirmado: grep `server_handshake`).
  Wire format para cliente sem `-N` permanece **byte-idêntico** (RNF-4).
- **Testes requeridos**: existentes adaptados (1 export, nome vazio) **sem mudar os asserts de
  bytes**; novos: `GO` com nome `s1` → size da `s1`; `GO` com nome inexistente → `ERR_UNKNOWN`
  e a negociação continua; `EXPORT_NAME` inexistente → `Err`; nome não-UTF-8 → `Err`.
- **Disciplina Kahneman**: #2 — ver mapa (linha ITEM-5/7).

### ITEM-6 — `crates/ramshared-wsl2d/src/backend.rs`

- **O que muda**: (a) entra `SliceView`, um `BlockBackend` que projeta uma janela
  `(base, len)` sobre outro `BlockBackend`; (b) `RamBackend` **muda para cá** vindo de
  `ublk_server.rs` (vira compartilhado entre os caminhos ublk e NBD).
- **Requisitos cobertos**: RF-L1 (a pergunta aberta do PRD: `VramBackend` **não** comporta
  view nativamente — `DeviceMem::{read_at,write_at}` aceitam offset, então a view é uma
  projeção fina por cima, sem tocar CUDA), DT-4, DT-17 (o zero de slice escreve via
  `SliceView`).
- **Função/bloco afetado**: novo tipo; `VramBackend` **não muda**.
- **Depois** (assinatura exata):

```rust
/// Janela [base, base+len) sobre um BlockBackend. serve() valida contra size_bytes()
/// = len da slice (bounds da slice de graça); read/write somam base.
pub struct SliceView<'b, B: BlockBackend> { inner: &'b mut B, base: u64, len: u64 }

impl<'b, B: BlockBackend> SliceView<'b, B> {
    /// Panica em debug se base+len excede inner.size_bytes() (construção é interna ao worker).
    pub fn new(inner: &'b mut B, base: u64, len: u64) -> Self;
}
impl<B: BlockBackend> BlockBackend for SliceView<'_, B> { /* size=len; off+base delega */ }
```

- **Por quê**: RF-L1 com reuso máximo — o bounds-check existente de `ramshared_block::serve`
  passa a valer por slice (checklist de segurança).
- **Impacto**: sem quebra; `ublk_server.rs` ajusta o import de `RamBackend` (sem alias de
  compat: atualizar os usos para `crate::backend::RamBackend`, Day-0).
- **Testes requeridos**: `SliceView` sobre `RamBackend`: leitura/escrita em slices vizinhas
  não vazam (offsets disjuntos); offset além do len da slice → `serve` devolve EINVAL (reuso
  do teste `out_of_range_is_einval_not_corruption` de `request.rs` como referência);
  `base+len > inner` panica em debug.

### ITEM-7 — `crates/ramshared-wsl2d/src/conn.rs`

- **O que muda**: (a) reader/writer/acceptor ficam **genéricos sobre o stream** (Unix e TCP);
  (b) handshake passa a negociar export e o `Job` carrega o índice; (c) anti-DoS de WRITE usa
  o tamanho do export negociado; (d) `WMsg` ganha a variante **`ZeroExport`** (DT-17).
- **Requisitos cobertos**: RF-L1, RF-L2, DT-17.
- **Função/bloco afetado**: `Job`, `WMsg`, `spawn_reader`, `spawn_writer`, `spawn_acceptor`.
- **Antes**: `Job { req, payload, reply }`; `spawn_reader(stream: UnixStream, device_size: u64, ...)`;
  `spawn_writer(stream: UnixStream, ...)`; `spawn_acceptor(listener: UnixListener, device_size: u64, tx_flags: u16, jobs: SyncSender<WMsg>)`.
- **Depois** (assinaturas exatas):

```rust
pub struct Job {
    pub export: usize,            // índice na tabela de exports (slice)
    pub req: Request,
    pub payload: Vec<u8>,
    pub reply: Sender<Reply>,
}

// WMsg ganha (Opened/Closed/Job intactos; o balanceamento Opened/Closed por conexão não é
// afetado — ZeroExport não é conexão):
//   ZeroExport { export: usize, done: Sender<bool> }   // DT-17: worker zera a slice

pub fn spawn_writer<S: Write + Send + 'static>(stream: S, replies: Receiver<Reply>) -> JoinHandle<()>;

/// `wstream` é o try_clone feito pelo acceptor (UnixStream e TcpStream têm try_clone
/// inerente mas nenhum trait comum — o clone fica no acceptor concreto).
pub fn spawn_reader<S: Read + Send + 'static, W2: Write + Send + 'static>(
    stream: S, hs_writer: W2,
    exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
    tx_flags: u16, jobs: SyncSender<WMsg>, reply_tx: Sender<Reply>,
) -> JoinHandle<()>;

pub fn spawn_acceptor(
    listener: UnixListener,
    exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
    tx_flags: u16, jobs: SyncSender<WMsg>,
) -> JoinHandle<()>;

/// Mesmo desenho do acceptor Unix, sobre TcpListener; alimenta o MESMO jobs channel
/// (SyncSender clona). TCP_NODELAY ligado por conexão (latência de swap).
pub fn spawn_acceptor_tcp(
    listener: std::net::TcpListener,
    exports: std::sync::Arc<Vec<ramshared_block::handshake::Export>>,
    tx_flags: u16, jobs: SyncSender<WMsg>,
) -> JoinHandle<()>;
```

  Dentro do reader: `server_handshake` devolve o índice; o anti-DoS muda de `device_size` para
  `exports[idx].size`; cada `Job` sai com `export: idx`. `Reply`/`LiveCount`/`CHAN_CAP`
  **não mudam** (o término determinístico DT-15 e o backpressure DT-7 da multiconn ficam
  intactos — os dois acceptors compartilham o mesmo canal, e `Opened`/`Closed` continuam
  balanceados por conexão).
- **Por quê**: RF-L2 (TCP) e RF-L1 (slice por conexão) sem duplicar o pipeline H1; DT-17
  precisa de um caminho para o worker (única thread CUDA) executar o zero.
- **Impacto**: quebra assinaturas internas → callers: `run_nbd` (ITEM-8) e testes do módulo.
  uAPI/ABI: nenhuma. Wire NBD: idêntico para cliente sem `-N`.
- **Testes requeridos**: existentes preservados; novos: dois acceptors (Unix+TCP em loopback)
  alimentando um worker — `live_count` balanceado e jobs de ambos servidos (in-process);
  `ZeroExport` zera exatamente a janela da slice (vizinha intacta) e responde `done`.
- **Disciplina Kahneman**: #2 — ver mapa (linha ITEM-5/7).

### ITEM-8 — `crates/ramshared-wsl2d/src/main.rs` (+ `Cargo.toml`, `lib.rs`)

- **O que muda**:
  1. **`Cargo.toml`**: `[[bin]] name = "ramsharedd"`, `path = "src/main.rs"` (DT-5); dep
     `ramshared-broker = { path = "../ramshared-broker" }`.
  2. **Flags novas** no `run()` (parsing manual existente): `--slices K` (u16, default 0 =
     modo single Fase B), `--slice-mb N` (u64, obrigatória se `--slices > 0`),
     `--listen-nbd tcp://IP:PORT` (opcional, adicional ao `--sock`), `--arbiter-listen IP:PORT`
     (opcional, habilita broker). Validações: `--transport ublk` com `--slices > 0` → `Err`
     (DT-3); IP unspecified (`0.0.0.0`/`::`) em qualquer listen → `Err` citando RNF-2 —
     **antes** de qualquer `bind()`.
  3. **`run_nbd` reescrito** para: (a) backend selecionável — `BackendKind::Vram` (atual) ou
     `BackendKind::Ram` (reuso do enum existente do caminho ublk; `RamBackend` de ITEM-6; pula
     CUDA, canário §9.4 fica `None`) — necessário para o drill ITEM-11 e e2e sem GPU; (b)
     tamanho = `--slices × --slice-mb` MiB (uma alocação única `ctx.alloc`, como hoje) ou
     `--size` no modo single; (c) tabela `Vec<Export>` vinda de `SliceMap::exports()` (modo
     single: 1 export `"default"`, resolvido também por nome vazio); (d) sobe `spawn_acceptor`
     (Unix) e, se `--listen-nbd`, `spawn_acceptor_tcp` no mesmo canal; (e) **worker**: mantém
     `geom: Vec<(u64,u64)>` (base,len por export, derivado de `SliceMap`; R3 — `block::Export`
     fica só name+size, sem acoplar o crate de bloco ao layout de slice); para cada `Job`,
     `let (base,len) = geom[job.export]` → `SliceView::new(&mut backend, base, len)` →
     `serve(&job.req, &job.payload, &mut view)`; para `WMsg::ZeroExport{export,done}`, escreve
     zeros em chunks de 1 MiB via `SliceView` sobre `geom[export]` e responde `done` (DT-17) —
     canário §9 (latência
     serve-only) e sonda §9.4 continuam globais, sem mudança de semântica (eviction WDDM é
     GPU-wide); **ciclo de vida (DT-28/R7)**: em modo broker o worker **não** encerra por
     `LiveCount` (as conexões NBD caem a zero a cada `DemoteAll`/idle) — usa `recv_timeout`
     (tick) e só sai no fechamento do canal `jobs` no shutdown ordenado; (f) **roteamento do
     DEMOTE**: com broker ativo, `Verdict::Demote(reason)` envia
     o `DemoteReason` pelo canal ao broker (vira `DemoteAll` nos agentes) em vez de
     `spawn_swapoff` local; sem broker, caminho atual intacto (RNF-4); (g) registra
     `handle_shutdown` (handler existente, `main.rs:90`) p/ SIGINT/SIGTERM também no caminho
     NBD quando broker ativo (shutdown ordenado = fluxo 4 do PRD; evidência: e2e (f) + drill
     fase 0); (h) se `--arbiter-listen`, chama `spawn_broker` (ITEM-8/CRIAR) com `nbd_unix` =
     `--sock` e `nbd_tcp` = `--listen-nbd` (DT-25).
  4. **Prefixo de log**: `[wsl2d]` → `[ramsharedd]` em todas as `eprintln!` do binário (DT-5).
  5. **`lib.rs`**: exporta `broker_srv` e os novos tipos públicos.
- **Requisitos cobertos**: RF-B1..B4 (fiação), RF-L1, RF-L2, RF-P2 (parcial), RNF-2, RNF-4,
  DT-2..DT-5, DT-17, DT-25.
- **Antes/Depois (resumo do shape)**: `run_nbd(size, sock, force, nbd_dev)` (Confirmado,
  `main.rs:169`) → `run_nbd(cfg: NbdRunConfig)` com `struct NbdRunConfig { slices: u16,
  slice_mb: u64, size: u64, sock: String, listen_nbd: Option<std::net::SocketAddr>,
  arbiter: Option<std::net::SocketAddr>, force: bool, nbd_dev: String, backend: BackendKind }`
  (mantém `run()` <80 linhas por função — regra de coding).
- **Por quê**: PRD §8 — superfície CLI definitiva do P1.
- **Impacto**: binário muda de nome → `scripts/kernel/qemu-ublk-daemon.sh` ajusta (abaixo);
  `docs/ublk-daemon-integration/IMPL.md` (recipe F3 vivo, linha 102) ajusta; docs
  (`README.md`/`ARCHITECTURE.md`) no mesmo commit. **Critério observável do rename:**
  `grep -rn "ramshared-wsl2d" --include="*.sh" --include="*.yml" scripts .github` sem hits, e
  nenhum hit em docs vivos (docs históricos/superseded ficam). uAPI kernel: nenhuma.
- **Testes requeridos**: parsing/validação das flags (recusa 0.0.0.0; ublk+slices; slice-mb
  ausente) extraído para função pura testável; `run_nbd` com `--backend ram --slices 2` coberto
  pelo e2e ITEM-10 (transitivamente) e drill ITEM-11.
- **Disciplina Kahneman**: #5 (bind, higiene) — ver mapa.

### `crates/ramshared-wsl2d/src/ublk_server.rs`

- **O que muda**: `RamBackend` sai daqui (movido para `backend.rs`, ITEM-6); imports/usos
  atualizados (`spawn_server_dt3`, testes, `UblkHandle::Ram` no main). Nenhuma mudança
  funcional no caminho ublk (RNF-4; DT-3).
- **Requisitos cobertos**: DT-3, RNF-4.
- **Testes requeridos**: suíte ublk existente verde sem edição de asserts.

### `scripts/kernel/qemu-ublk-daemon.sh`

- **O que muda**: nome do binário `ramshared-wsl2d` → `ramsharedd` (DT-5). Só isso.
- **Testes requeridos**: rodar o harness F2 uma vez pós-rename (PASS igual ao baseline).

### `docs/ublk-daemon-integration/IMPL.md`

- **O que muda**: recipe do harness F3 (linha 102) atualizada para o binário `ramsharedd`
  (doc vivo — F3 pendente usa a recipe literalmente).
- **Requisitos cobertos**: DT-5 (F12 da auditoria).

### `Cargo.toml` (raiz do workspace)

- **O que muda**: `members` ganha `"crates/ramshared-broker"` e `"crates/ramshared-agent"`.
- **Impacto**: CI (`cargo test --workspace`) cobre os crates novos sem mudança no workflow
  (Confirmado: `.github/workflows/ci.yml` não referencia nomes de binário).

### `docs/LIBRARIES.md`

- **O que muda**: entradas `serde`/`serde_json` (versão, motivo, link ADR-0005) — mesmo commit
  da dep (disciplina #11).

### `docs/reliability/DEGRADATION-MATRIX.md`

- **O que muda**: novos modos de falha: broker morto com swap remoto ativo (detecção:
  watchdog 3s via heartbeat DT-18; unwind: tentativa de swapoff best-effort — **pode falhar
  com EIO se houver páginas residentes**, área pode permanecer listada — + EIO bounded via
  conexão fechada/`-timeout 30`; validação: drill ITEM-11 fases 1 e 2) · `wsl --shutdown` mata
  broker (R7; = broker morto) · agente morto com slice ativa (detecção: EOF no broker; unwind:
  slice **congelada** — DT-20 — visível no `Status` com `present:false`; runbook de remoção;
  re-`Register` reconcilia) · `SwapOn` falha no destino (unwind: slice `Free`, retry no tick,
  log) · zero de slice falha (unwind: slice fica `Draining`, retry no tick, log ERROR — slice
  não re-atribuível até zerar, DT-17).
- **Requisitos cobertos**: RNF-1, R2/R7 (disciplina #5: matrix atualizada na feature).

### `README.md` / `ARCHITECTURE.md`

- **O que muda**: seção da plataforma broker (1 cérebro/host, lease, tenants) + rename do
  binário; no mesmo commit do ITEM-8 (regra: mudança estrutural atualiza docs junto).

### `MEMORY.md`

- **O que muda**: entrada de sessão (append-only) ao fim de cada checkpoint relevante (regra
  do repo).

## Arquivos a DELETAR

| Arquivo | Motivo |
| --- | --- |
| — | Nenhum. Não há arquivo de compatibilidade temporária neste escopo (Day-0 checado: o modo single não é shim — é o produto da Fase B servindo o host sem broker). |

## Observabilidade

**Métricas Prometheus**: N/A em P1 (DT-12). Reavaliar no SPEC de P2 (produto instalável).

**Logs estruturados** (stderr, prefixo `[ramsharedd]`/`[agent]`, chave=valor, uma linha por evento):

| Evento | Level | Campos |
| --- | --- | --- |
| Decisão do árbitro (move/assign/revert) | Info | `slice`, `from`, `to`, `psi10_from`, `psi10_to`, `streak`, `cooldown_s`, `reason` |
| Lease grant/revoke/release/denied | Info | `lease`, `holder`, `bytes`, `slices`, `revoked_from`, `reason` (denied) |
| DemoteAll (residência ou shutdown) | Info | `reason` (`Latency\|Corruption\|FreeFloor\|shutdown`), `slices_active` |
| Zero de slice (DT-17) | Info | `slice`, `ms`, `ok` |
| Tenant registrado/perdido/duplicado | Info/Error | `tenant`, `transport`, `slices_reconciliadas`, `motivo` |
| Sessão derrubada por backpressure (DT-24) | Error | `tenant`, `motivo=canal_cheio` |
| Shutdown: espera de DemoteAll estourou | Error | `motivo=shutdown_timeout`, `slices_pendentes` |
| Watchdog expirado (agente) | Error | `broker`, `idle_s`, `slices_limpas`, `swapoff_ok` |
| SwapOn/SwapOff/mkswap executado (agente) | Info | `slice`, `dev`, `prio`, `ok`, `detail` |
| Bind recusado (RNF-2) | Error | `addr`, `motivo=rnf2_bind_privado` |

`Ack` (DT-18) **não** é logado (1 Hz × N tenants = ruído). `Status` (RF-B4): `StatusReply` com
slices/tenant, PSI por tenant, presença e `last_rebalance_secs` — acessível via
`ramshared-agent --status` ("cada um sabe quem está precisando mais").

## Contratos e documentação viva

| Documento | Atualização necessária | Motivo |
| --- | --- | --- |
| `Documentation/` (uAPI/ABI) | N/A | Nenhuma uAPI de kernel nova (PRD §8); ublk/NBD existentes |
| `Kconfig` (help) | N/A | Sem novo CONFIG_/module param |
| `CLAUDE.md` | N/A | Nenhum padrão estrutural de trabalho muda |
| `.claude/rules/*.md` | N/A | Nenhuma convenção nova |
| `docs/decisions/ADR-0005-broker-protocol-jsonl.md` | Criar | DT-1 + dep serde (disciplina #11) |
| `docs/methodology/KAHNEMAN-DISCIPLINES.md` | N/A | Disciplinas existentes cobrem (nenhuma âncora nova) |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alterar | 5 modos de falha novos (ver MODIFICAR) |
| `docs/LIBRARIES.md` | Alterar | serde/serde_json |
| `docs/runbooks/CIVM-TENANT.md` | Criar | RF-L4 |
| `docs/ublk-daemon-integration/IMPL.md` | Alterar | recipe F3 cita o binário (rename DT-5) |
| `README.md` / `ARCHITECTURE.md` | Alterar | plataforma + rename `ramsharedd` (mesmo commit do ITEM-8) |
| PRDs de origem (`vram-arbiter`, `dcc-out-of-core`, `VISION.md`) | Alterar (banner) | marcar como histórico, fonte = este PRD/SPEC (PRD §11) |

## Ordem de implementação

Gate: **ITEM-1 termina e `P0-RESULTS.md` fecha o gate antes de qualquer item ≥3** (ITEM-2 é
doc e pode andar em paralelo ao P0).

1. **ITEM-1** — `scripts/p0/{measure-psi.sh,measure-net.sh,measure-nbd-tcp.sh,measure-render-vram.ps1}` + `docs/memory-broker/P0-RESULTS.md`; executar nos 3 ambientes; calibrar defaults do ITEM-4. **[GATE]**
2. **ITEM-2** — `docs/decisions/ADR-0005-broker-protocol-jsonl.md` + `docs/LIBRARIES.md`.
3. **ITEM-3** — `ramshared-broker`: `Cargo.toml`, `lib.rs`, `model.rs`, `protocol.rs` (+ workspace `Cargo.toml`).
4. **ITEM-4** — `ramshared-broker`: `slices.rs`, `arbiter.rs` (defaults calibrados por P0).
5. **ITEM-5** — `ramshared-block/src/handshake.rs`: exports nomeados (testes byte-compat primeiro — TDD da regressão RNF-4).
6. **ITEM-6** — `ramshared-wsl2d/src/backend.rs`: `SliceView` + `RamBackend` movido (+ ajuste `ublk_server.rs`).
7. **ITEM-7** — `ramshared-wsl2d/src/conn.rs`: streams genéricos, export no `Job`, acceptor TCP, `WMsg::ZeroExport`.
8. **ITEM-8** — `ramshared-wsl2d`: `broker_srv.rs`, `main.rs` (flags, `run_nbd`, demote routing, rename `ramsharedd`), `lib.rs`, `scripts/kernel/qemu-ublk-daemon.sh`, `docs/ublk-daemon-integration/IMPL.md`, `README.md`/`ARCHITECTURE.md`.
9. **ITEM-9** — `ramshared-agent`: `psi.rs`, `swap.rs` (incl. `mk_swap`), `watchdog.rs`, `main.rs`.
10. **ITEM-10** — `crates/ramshared-wsl2d/tests/broker_e2e.rs` (cenários a–i).
11. **ITEM-11** — `scripts/kernel/qemu-broker-drill.sh` + execução das 3 fases (marcadores commitados no IMPL). **[GATE P1: fase 0 ok; fase 1 <5s; fase 2 attempt<5s, sem D-state >10s, echo<2s×3]**
12. **ITEM-12** — `docs/runbooks/CIVM-TENANT.md` + `DEGRADATION-MATRIX.md` + e2e real WSL2↔civm (cenário 1 do PRD: action na civm + build no WSL2, logs de rebalanço → IMPL). **[GATE P1: cenário 1 demonstrado]**

Checkpoints de commit: 1 commit por item (atômico, revisável), rastreando `RF-*`/`DT-*` no body;
itens 5–8 carregam `Rollback trigger:` (mudança de contrato/estrutura — disciplina #2).

## Plano de testes

**Backend (crates Rust)**

- Unitários: `protocol.rs` (roundtrip por variante, teto de linha, shape inválido);
  `slices.rs` (transições incl. `Leased`, offsets disjuntos); `arbiter.rs` (histerese,
  cooldown, nunca-zero, counterfactual com clock falso + piso DT-23, lease > nunca-zero,
  lease não re-atribui `Leased`, lease duplicado negado, round-robin só entre presentes);
  `psi.rs` (fixtures reais WSL2/civm, entrada corrompida, `parse_euid`); `watchdog.rs`
  (clock falso); `swap.rs` (argv puro: `-timeout` sempre, `-persist` nunca, `-p` condicional,
  `mkswap` presente e ordenado antes do `swapon` no dispatch); `SliceView` (isolamento entre
  slices, EINVAL fora da janela); `ZeroExport` (zera só a janela); parsing de flags do daemon
  (0.0.0.0, ublk+slices).
- Integração: `broker_e2e.rs` (ITEM-10, in-process, cenários a–i incl. shutdown, heartbeat,
  higiene e ausência); dois acceptors no mesmo worker (ITEM-7); suíte existente intacta
  (`handshake`, `conn`, `ublk_*`, `residency`, `state`) — RNF-4.
- Isolamento de ring: N/A kernel (Ring 0 não muda); análogo coberto por: bounds por slice,
  zero por slice (DT-17), euid-gate do agente, bind privado.
- Concorrência / atomicidade: `slow_writer_does_not_deadlock` preservado; `assign` em slice
  não-Free falha (fronteira de atomicidade); ordem SwapOff→Done→Zero→Free→SwapOn observada no
  e2e.

**Drivers (drm/amd/nouveau)**: N/A — nenhum código de kernel neste escopo.

**Manuais**

- Smoke GPU (host EMEDEV, sudo ok): `ramsharedd --slices 2 --slice-mb 128` + `nbd-client -unix
  ... -N s0 /dev/nbd0` e `-N s1 /dev/nbd1` + `mkswap/swapon` nos dois; verificação em
  `/proc/swaps` (prioridades distintas das locais — DT-7).
- Cenários de erro: export inexistente (`-N s9` → conexão recusada); broker com bind 0.0.0.0
  (recusa); agente sem root (erro claro via `/proc/self/status`); `SwapOn` com `nbd-client`
  ou `mkswap` ausente (`SwapOnDone{ok:false}` + retry).
- Evidências objetivas das etapas críticas (mapa Kahneman): `P0-RESULTS.md` preenchido;
  marcadores das 3 fases do drill qemu; logs do árbitro com PSI dos dois lados no e2e real
  WSL2↔civm (gates P1, critérios de aceitação 1 e 4 do PRD §13).

## Checklist de validação

**Backend**

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (builds no WSL2 sempre escopados: `-j 4`, nunca `--release` —
      restrição de estabilidade do host)
- [ ] `./scripts/checkpatch.pl -f …` — N/A (nenhum arquivo C neste escopo)
- [ ] `make W=1 C=1` / `make modules` — N/A (sem código de kernel)

**Drivers (drm/amd/nouveau)**

- [ ] `cargo clippy` / `rustfmt` — cobertos acima
- [ ] `sparse` / `make kselftest` — N/A (sem C/kernel; o harness de VM é
      `scripts/kernel/qemu-broker-drill.sh` + `qemu-ublk-daemon.sh` pós-rename)

**Docs**

- [ ] `make htmldocs` / `make pdfdocs` / `make cleandocs` — N/A (docs são Markdown puro neste
      repo; validação = links relativos resolvem e tabelas renderizam no GitHub)

**Gates cognitivos**

- [ ] Cada etapa crítica aponta para `docs/methodology/KAHNEMAN-DISCIPLINES.md` (mapa preenchido)
- [ ] Cada etapa crítica tem pergunta obrigatória, evidência mínima executável e abort trigger
- [ ] Nenhum ponto crítico com linguagem vaga: os triggers são numéricos (swapoff iniciado <5s;
      2× PSI em 60s acima de `psi_floor`; 0.0.0.0 recusado; P0 com n≥3 rodadas; fase 2 com
      `used_kb ≥ 1024`; sem processo em D >10s; echo <2s ×3; slice só re-atribuída após
      `ZeroDone{ok:true}`)
