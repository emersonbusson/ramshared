# SPEC — RamShared Memory Broker (P0 + P1)

> SSDV3 PASSO 2, gerado de [`docs/memory-broker/PRD.md`](PRD.md). Slug: `memory-broker`.
> Escopo: **P0 (medição) + P1 (broker core Linux↔Linux)** — fases do PRD §10. P2/P3/P4 ficam
> explicitamente fora e terão SPECs próprios quando os gates abrirem.
> Disciplinas: links obrigatórios para [`docs/methodology/KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md)
> em toda etapa crítica (mapa na seção própria).

## Escopo fechado desta implementação

**Entra agora:**

- **P0** — scripts de medição (sem código de produto) + template de resultados que é o **gate
  numérico** de P1: PSI idle/carga (WSL2, civm, host), alcançabilidade/RTT VM↔WSL2, p50/p99
  NBD/TCP cru no virt-switch, medição de VRAM/RAM durante render (script p/ o tester).
- **P1** — RF-B1, RF-B2, RF-B3, RF-B4, RF-L1, RF-L2, RF-L3, RF-L4, RF-P2 (parcial: NBD como
  fallback universal; ublk inalterado), RNF-1..RNF-6:
  - crate novo `ramshared-broker` (protocolo JSON-lines, modelo, árbitro puro, mapa de slices);
  - crate novo `ramshared-agent` (binário do tenant: PSI, swapon/swapoff, watchdog);
  - daemon (`crates/ramshared-wsl2d`) ganha `--slices/--slice-mb`, `--listen-nbd tcp://`,
    `--arbiter-listen`, `--backend ram` no caminho NBD, e binário renomeado `ramsharedd`;
  - export NBD nomeado por slice em `ramshared-block::server_handshake`;
  - drill de D-state em qemu (`scripts/kernel/qemu-broker-drill.sh`);
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
- CUDA via dlopen (`ramshared-cuda`: `Cuda`, `Context::alloc`, `DeviceMem::{read_at,write_at,zero}`).
- Teardown ublk validado em qemu (F2, `scripts/kernel/qemu-ublk-daemon.sh`) + `RamBackend`.
- VM civm `gha-ubuntu-2404` no host `EMEDEV`, alcançável por SSH/Tailscale (Confirmado em docs).

## Matriz de rastreabilidade PRD → SPEC

| PRD | Implementação no SPEC |
| --- | --- |
| P0 (§10) / R1 / R4 | ITEM-1 (scripts p0 + `P0-RESULTS.md`) |
| RF-B1 | ITEM-2 (ADR-0005), ITEM-3 (`protocol.rs`), ITEM-8 (`broker_srv.rs`), ITEM-9 (agente) |
| RF-B2 | ITEM-4 (`arbiter.rs`: histerese+cooldown+nunca-zero) |
| RF-B3 | ITEM-4 (ações de lease), ITEM-8 (revogação = SwapOff/demote per-slice) |
| RF-B4 | ITEM-8 (log de decisão com PSI dos dois lados; `StatusReply`), ITEM-9 (`--status`) |
| RF-L1 | ITEM-4 (`slices.rs`), ITEM-5 (handshake por export), ITEM-6 (`SliceView`), ITEM-7, ITEM-8 |
| RF-L2 | ITEM-7 (streams genéricos), ITEM-8 (`--listen-nbd tcp://`, recusa 0.0.0.0) |
| RF-L3 | ITEM-9 (`psi.rs`, `swap.rs`) |
| RF-L4 | ITEM-12 (`docs/runbooks/CIVM-TENANT.md`) |
| RF-P2 (parcial) | ITEM-8 (NBD fallback universal; ublk single-device intacto) |
| RNF-1 | ITEM-9 (watchdog), ITEM-11 (drill qemu), DT-7, DT-14 |
| RNF-2 | ITEM-8 (recusa bind 0.0.0.0), ITEM-12 (runbook só rede privada/Tailscale) |
| RNF-3 | ITEM-4 (histerese+cooldown; defaults provisórios calibrados por P0) |
| RNF-4 | ITEM-10 (suíte existente verde; modo single inalterado), checklist de validação |
| RNF-5 | ITEM-3/ITEM-9 (`#![forbid(unsafe_code)]` nos crates novos) |
| RNF-6 | DT-1..DT-15 (decisões únicas, sem dual-path) |

RF-W1..W4, RF-G1..G3, RF-P1, RF-P3: **fora** (ver escopo).

## Decisões técnicas

Decisões tomadas que não estavam explícitas no PRD (cada uma é a solução única Day-0):

| # | Decisão | Justificativa |
| --- | --- | --- |
| DT-1 | Protocolo agente↔broker = **JSON-lines** (1 objeto JSON por linha, `\n`, UTF-8) sobre TCP, via `serde`/`serde_json` | Control-plane de baixa taxa (1 msg/s/tenant): debugável com `nc`/`jq`, evolução por campo opcional. Length-prefixed só ganharia em data-plane binário, que aqui é o NBD. Dep nova exige ADR (disciplina #11) → ADR-0005 + `docs/LIBRARIES.md` |
| DT-2 | Broker **in-process no daemon** (`--arbiter-listen`), não binário separado | Anexo A.4 do PRD: um único dono da verdade sobre a VRAM. O daemon já é `mlockall`+`oom_score_adj=-1000`; broker separado recriaria a disputa cega que o lease resolve |
| DT-3 | Slices em P1 = **exports NBD nomeados** (`s0..s{K-1}`), Unix e TCP; ublk permanece single-device; `--transport ublk` + `--slices` → erro de CLI | O tenant local da persona dev é WSL2, onde `guard_not_wsl2()` recusa ublk (incidente de congelamento 2026-06-09, `crates/ramshared-wsl2d/src/main.rs`). NBD já tem export name no handshake. Slices ublk entram quando existir tenant local não-WSL2 (sem dual-path agora) |
| DT-4 | **Worker CUDA único permanece**; a slice é resolvida no worker via `SliceView` (offset/len sobre o backend único) | Mantém a afinidade de thread (H1) e a sincronicidade `cuMemcpy*` de que o `flush()` no-op depende (`backend.rs`). `DeviceMem` único = zero mudança no modelo CUDA |
| DT-5 | Binário renomeado **`ramsharedd`** via `[[bin]]` no `Cargo.toml` do crate; prefixo de log `[ramsharedd]`; diretório do crate **não** renomeia agora | PRD §8 nomeia `ramsharedd`; o runbook civm grava o nome em systemd unit (renomear depois quebraria runbook = anti-Day-0). Rename do diretório é fatia ortogonal separada (disciplina #14) |
| DT-6 | Atribuição inicial: slices `Free` são distribuídas **round-robin** entre tenants registrados; o árbitro só **move** sob diferencial de pressão | Swap ocioso não custa nada: o kernel só usa a slice sob pressão (gate natural via prioridade de swap). Evita política de admissão extra e torna o drill determinístico |
| DT-7 | Prioridade de swap remoto: default **sem `-p`** no `swapon` (kernel atribui prioridade negativa decrescente, sempre abaixo do swap local pré-existente); `--swap-prio` só para override explícito | `swapon -p` do util-linux não aceita negativos; sem `-p`, a slice entra com prioridade menor que qualquer swap já ativo — exatamente o que RNF-1 exige. Evidência: coluna `Priority` em `/proc/swaps` |
| DT-8 | Invariante "nunca zero slices para tenant sob pressão" (RF-B2) vale **só para rebalanceamento**; revogação por **lease pode drenar tudo** | RF-B3: pedido explícito de VRAM > swap tier. O tenant drenado mantém o swap local (RNF-1); a VRAM emprestada é best-effort por definição de lease revogável |
| DT-9 | Estado do broker é **em memória**, reconstruído no `Register` (agente reporta `/proc/swaps` atual); zero persistência em P1 | Broker morre → watchdog limpa (RNF-1) → no restart os agentes re-registram e reportam o estado real. Persistir duplicaria a fonte da verdade |
| DT-10 | Counterfactual do lease (PRD §14, uso <50% em 5min) **adiado para P2** | Exige telemetria de uso de VRAM do holder (NVML, RF-W1). Em P1 não há holder real de lease (DCC é P2); grant/release/revoke são implementados e logados |
| DT-11 | Config TOML (RF-P3) adiada para P2 (junto de RF-P1); superfície P1 = **flags CLI** | TOML é a superfície do produto instalável. As flags mapeiam 1:1 para o TOML futuro (não é shim: continuam válidas como override, padrão systemd) |
| DT-12 | Sem métricas Prometheus em P1; observabilidade = **logs estruturados em stderr + `Status`** | O daemon é `eprintln`-based hoje; exporter entra com o produto instalável (P2+). RF-B4 é satisfeito por log de decisão + `StatusReply` |
| DT-13 | `ramshared-agent` exige **euid 0** no startup (erro claro caso contrário) | Equivalente userspace do gate `capable(CAP_SYS_ADMIN)`: `swapon`/`swapoff`/`nbd-client` exigem root. Falhar cedo > falhar no primeiro comando |
| DT-14 | `nbd-client` sempre com **`-timeout 30`** e **nunca `-persist`** | Broker morto deve virar EIO bounded, não D-state eterno; `-persist` tentaria reconectar a um servidor morto, prolongando o hang (RNF-1, R2) |
| DT-15 | PSI: arbitragem usa a linha **`some`** (`avg10`); `full` apenas logado | `some` captura "alguém estagnou" (sinal de rebalanceamento); `full` é estagnação total, tarde demais para agir. P0 valida a escolha com números |

## Fronteira de atomicidade e política de rollback

- **Fronteira de atomicidade desta implementação:**
  - **Atômico (garantido pelo broker):** uma slice nunca está `Active` em dois tenants ao mesmo
    tempo. Sequência de movimento: `SwapOff(from)` → aguarda `SwapOffDone{ok:true}` → slice
    `Free` → só então `SwapOn(to)`. O broker é single-threaded no estado (mesmo padrão do worker
    CUDA), então não há corrida interna.
  - **Fora da atomicidade:** o par swapoff/swapon **não** é transacional entre hosts. Estados
    parciais aceitos nesta fase: slice `Draining` (swapoff em voo), slice `Free` sem dono entre
    os dois passos, `SwapOn` que falha no destino (slice volta a `Free`, retry no próximo tick,
    logado). Broker morto no meio do movimento: agentes mantêm seu estado de swap local; o
    watchdog limpa slices remotas; no restart o `Register` reconcilia (DT-9).
- **Política de rollback:**
  - **Rollback de app:** parar agente(s) e daemon = `DemoteAll` no shutdown (SIGTERM) → agentes
    confirmam swapoff → daemon zera a VRAM (fluxo 4 do PRD; reuso do teardown F2 validado).
    Reverter o código = `git revert`; o modo single (Fase B) permanece intacto (RNF-4), então o
    rollback de app degrada para o estado atual do repo, sem resíduo.
  - **Rollback de migration:** **N/A** — não há schema, banco ou migration nesta implementação.
  - **Rollback de dados:** páginas swapped em VRAM/NBD são **voláteis por design**. Matar o
    daemon com swap remoto ativo = perda das páginas swapped daquela slice (processo do tenant
    pode ser morto pelo kernel ao tomar EIO). Mitigação: prioridade baixa (DT-7) mantém pouquíssimas
    páginas lá; drill valida o pior caso **somente em qemu**.
  - **Proibido em produção (hosts reais EMEDEV/civm):** matar o daemon com swap remoto ativo sem
    `DemoteAll` prévio, exceto no drill em VM descartável; bind de NBD/TCP ou árbitro em
    `0.0.0.0` ou interface pública (o daemon recusa — ITEM-8).
  - **Forward-only:** N/A (sem dados persistentes).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina Kahneman | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (P0, gate) | #3 Número não adjetivo; #1 WYSIATI | [`docs/methodology/KAHNEMAN-DISCIPLINES.md`](../methodology/KAHNEMAN-DISCIPLINES.md) §3, §1 | Os números de PSI/RTT/NBD-TCP existem, com unidade, n de rodadas e ambiente descrito? | `docs/memory-broker/P0-RESULTS.md` preenchido pela execução de `scripts/p0/*.sh` (CSV bruto commitado ou linkado), ≥3 rodadas por métrica | Qualquer célula do template vazia ou "estimado" ⇒ **nenhum item de P1 inicia** (gate anti-halo §14 do PRD) |
| ITEM-4 (árbitro) | #2 Counterfactual | idem, §2 | O que me faria desfazer um rebalanço? | `cargo test -p ramshared-broker` cobrindo: streak incompleto não move; cooldown bloqueia; nunca-zero respeitado; **PSI do drenado >2× em 60s ⇒ `RevertMove` + cooldown longo** (teste com clock falso) | Teste do counterfactual ausente ou falhando ⇒ não integra ITEM-8 |
| ITEM-5/ITEM-7 (contrato NBD por export) | #2 Counterfactual | idem, §2 | Cliente antigo (`nbd-client` sem `-N`, Fase B) continua funcionando byte a byte? | `cargo test -p ramshared-block` (handshake com nome vazio → export default, mesmo wire format dos testes atuais) + suíte existente verde | Qualquer teste existente de `handshake.rs`/`conn.rs` quebrar ⇒ reverter a mudança de assinatura, redesenhar |
| ITEM-8 (bind TCP, RNF-2) | #5 Availability heuristic | idem, §5 | O que acontece se o daemon subir com bind público por engano? | Teste unitário: `--listen-nbd tcp://0.0.0.0:10809` e `--arbiter-listen 0.0.0.0:7777` retornam `Err` antes de qualquer `bind()`; mensagem cita RNF-2 | Daemon aceita bind unspecified ⇒ bloqueia merge (CRITICAL) |
| ITEM-9 (watchdog, RNF-1/R2) | #5 Availability heuristic; #13 Ilusão de validade | idem, §5, §13 | O teste do watchdog pode falhar pelo motivo certo (broker realmente morto, swap realmente ativo), ou só valida um mock? | Unit: `Watchdog::expired` com clock falso. Integração real: ITEM-11 (drill qemu com swap ativo e `kill -9` no broker) — o unit **não** conta como evidência de RNF-1 sozinho | Drill não executado ou swapoff >5s após morte do broker ⇒ P1 não fecha (gate §10 do PRD) |
| ITEM-11 (drill D-state) | #5 Worst-case (PRD §14) | idem, §5 | O drill roda num ambiente onde um stall é recuperável? | `scripts/kernel/qemu-broker-drill.sh` imprime `PASS: watchdog swapoff em <N>s (<5s)` rodando **dentro do qemu**; log commitado em `docs/memory-broker/` no IMPL | Qualquer tentativa de rodar o drill no WSL2/host ⇒ abortar (repete o incidente 2026-06-09); stall na VM >60s ⇒ investigar antes de re-rodar |
| ITEM-2 (dep serde) | #11 Halo effect | idem, §11 | A dep nova tem ADR e entrada em LIBRARIES.md? | `docs/decisions/ADR-0005-broker-protocol-jsonl.md` + linha em `docs/LIBRARIES.md` no mesmo commit que adiciona a dep | Dep no `Cargo.toml` sem ADR ⇒ bloqueia commit |
| ITEM-12 (rollout civm) | #1 WYSIATI | idem, §1 | O runbook declara o que NÃO foi testado no ambiente do peer? | Runbook tem seção "Validado em / Não validado em"; e2e P1 executado uma vez seguindo o runbook literalmente (logs anexados no IMPL) | Passo do runbook que exige automação no host civm (política: peer copia template, zero automação) ⇒ reescrever o passo |

## Checklist de segurança (pré-implementação)

- [x] Isolamento: todo acesso por offset valida contra os limites da **slice** (`SliceView`
      reusa o bounds-check de `ramshared_block::serve` com `size_bytes()` = len da slice);
      anti-DoS de WRITE em `spawn_reader` passa a usar o tamanho do export negociado
- [x] Buffer overflow / OOB: `read_msg` impõe teto de linha (64 KiB) antes de alocar (mesmo
      padrão do `MAX_OPT_LEN` em `handshake.rs`); `copy_{from,to}_user` N/A (sem código kernel —
      PRD §8: nenhuma uAPI nova)
- [x] Permissões: `capable(CAP_SYS_ADMIN)` N/A em kernel; equivalente userspace: `ramshared-agent`
      checa `euid==0` no startup (DT-13); daemon segue exigindo root para `mlockall` (ou `--force`)
- [x] Permissões: `permission.RequirePermission(...)` N/A (não há backend HTTP neste repo)
- [x] Preemption / IRQ flooding: N/A kernel; análogo userspace: árbitro fora do hot path
      (thread própria, tick 2s); canal `WMsg` segue como único ponto de backpressure (DT-7 da
      multiconn preservado)
- [x] Input validation: todo `Msg` recebido é validado (serde rejeita shape errado; `Register`
      com `proto != PROTO_VERSION` → `Error` + desconexão; nomes de export inexistentes →
      recusa no handshake)
- [x] Ponteiros: nenhum endereço de kernel/GPU logado (logs usam ids de slice/tenant; regra
      pré-existente de não vazar KASLR em `MEMORY.md` mantida)
- [x] Nenhuma credencial hardcoded (não há credenciais; RNF-2 = bind privado, sem auth própria)
- [x] Erros internos não vazam detalhe: `Msg::Error{reason}` carrega frase curta; detalhe
      (errno, stderr do swapon) fica no log local do lado que falhou

## Arquivos a CRIAR

### ITEM-1 — P0 (gate; sem código de produto)

**`scripts/p0/measure-psi.sh`**

- **Propósito**: amostrar `/proc/pressure/memory` (linhas `some` e `full`) a cada 1s por N
  segundos, em CSV (`ts,kind,avg10,avg60,avg300,total_us`).
- **Requisitos cobertos**: P0 (§10), R1 contexto.
- **Funções**: shell puro (`while read`-loop sobre `/proc/pressure/memory`); args: `DURATION`
  (default 300) e `OUT` (CSV). Sem dependências além de coreutils.
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
  (ver ITEM-4).
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
- **Requisitos cobertos**: RF-B1, RF-L1 (modelo de slice).
- **Structs/Types** (assinaturas exatas):

```rust
pub type TenantId = u32;
pub type SliceId = u16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum SliceState { Free, Active, Draining }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
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
- **Requisitos cobertos**: RF-B1, DT-1.
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
pub struct TenantStatus { pub id: TenantId, pub name: String, pub psi: PsiSample, pub slices: Vec<SliceId> }

/// Serializa `msg` + '\n' e dá flush (uma mensagem por linha).
pub fn write_msg<W: std::io::Write>(w: &mut W, msg: &Msg) -> std::io::Result<()>;

/// Lê uma linha (teto MAX_LINE_BYTES) e desserializa. Ok(None) em EOF limpo;
/// Err em linha gigante, JSON inválido ou shape desconhecido.
pub fn read_msg<R: std::io::BufRead>(r: &mut R) -> std::io::Result<Option<Msg>>;
```

- **Padrão de referência**: `crates/ramshared-block/src/handshake.rs` (codec genérico sobre
  `Read`/`Write`, testável com `Cursor`, teto anti-alloc).
- **Testes requeridos** (inline): roundtrip de cada variante; linha > `MAX_LINE_BYTES` falha
  **antes** de alocar; JSON com `type` desconhecido → `Err`; `Register` com proto errado é
  decisão do chamador (broker responde `Error`) — teste no ITEM-8.

### ITEM-4 — Crate `ramshared-broker`: slices e árbitro (lógica pura)

**`crates/ramshared-broker/src/slices.rs`**

- **Propósito**: partição estática da VRAM em K slices e mapa dinâmico slice→tenant (RF-L1).
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
    /// Free → Active(tenant). Err se não-Free (invariante de atomicidade).
    pub fn assign(&mut self, id: SliceId, tenant: TenantId) -> Result<(), SliceError>;
    /// Active → Draining. Err se não-Active.
    pub fn drain(&mut self, id: SliceId) -> Result<(), SliceError>;
    /// Draining → Free (após SwapOffDone{ok:true}).
    pub fn release(&mut self, id: SliceId) -> Result<(), SliceError>;
    /// Nomes de export NBD: ("s0", len), ("s1", len)...
    pub fn exports(&self) -> Vec<(String, u64)>;
}

#[derive(Debug, PartialEq)]
pub enum SliceError { UnknownSlice, BadState { have: SliceState } }
```

- **Padrão de referência**: máquina de estados de `crates/ramshared-wsl2d/src/state.rs`
  (transições ilegais rejeitadas, testes `illegal_jumps_rejected`).
- **Testes requeridos**: transições legais; ilegais rejeitadas (`assign` em Active falha —
  é o teste da fronteira de atomicidade); offsets disjuntos cobrem `total_bytes` sem gap.

**`crates/ramshared-broker/src/arbiter.rs`**

- **Propósito**: política do árbitro **pura** (sem IO, clock injetado) — RF-B2, RF-B3, RNF-3 e
  counterfactual §14 do PRD.
- **Requisitos cobertos**: RF-B2, RF-B3, RNF-3, DT-6, DT-8.
- **Structs/Funções** (assinaturas exatas):

```rust
#[derive(Clone, Copy, Debug)]
pub struct ArbiterConfig {
    pub delta_psi: f32,                 // diferencial some.avg10 para mover
    pub streak: u32,                    // ticks consecutivos acima do delta
    pub cooldown: std::time::Duration,  // pós-movimento
    pub psi_floor: f32,                 // "sob pressão" (nunca-zero)
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
    RevertMove { slice: SliceId, from: TenantId, to: TenantId },    // counterfactual §14
    RevokeForLease { slice: SliceId, from: TenantId, lease: u32 },  // RF-B3
    GrantLease { lease: u32, holder: TenantId, slices: Vec<SliceId> },
}

pub struct Arbiter { /* cfg, streak, last_move: Option<MoveRecord>, rr_cursor, next_lease_id */ }

impl Arbiter {
    pub fn new(cfg: ArbiterConfig) -> Self;
    /// Uma decisão por tick. `now` injetado (testável). Nunca emite MoveSlice que deixe
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

- **Lógica resumida do `tick`** (ordem): (1) lease pendente tem prioridade — calcula
  ⌈bytes/slice_len⌉ slices, emite `RevokeForLease` para as Active (mais ociosas primeiro) e
  `GrantLease` quando houver Free suficientes; (2) counterfactual: se `last_move` dentro de
  `cf_window` e `psi(from).avg10 > cf_factor × psi_no_momento_do_move` → `RevertMove` +
  `cf_cooldown`; (3) cooldown ativo → nada; (4) diferencial: maior PSI − menor PSI >
  `delta_psi` por `streak` ticks → `MoveSlice` de uma slice do menos pressionado (respeitando
  nunca-zero); (5) slices Free → `AssignFree` round-robin.
- **Defaults provisórios** (constantes em `impl Default`; **P0 calibra** — recalibração é
  atualização deste SPEC + commit citando `P0-RESULTS.md`):

| Parâmetro | Default provisório | Calibrado por |
| --- | --- | --- |
| `delta_psi` | 15.0 (pontos some.avg10) | PSI idle/carga medido (ITEM-1) |
| `streak` | 5 ticks (tick = 2s → 10s) | idem |
| `cooldown` | 60s | PRD §14 |
| `psi_floor` | 5.0 | PSI idle medido |
| `cf_window` / `cf_factor` / `cf_cooldown` | 60s / 2.0 / 300s | PRD §14 (fixo: trigger do PRD) |

- **Padrão de referência**: `ResidencySampler` em `crates/ramshared-wsl2d/src/residency.rs`
  (histerese por streak, lógica pura testável — reuso de padrão exigido por RF-B2).
- **Testes requeridos**: ver mapa Kahneman (linha ITEM-4) — histerese, cooldown, nunca-zero,
  counterfactual com clock falso, lease drena além do nunca-zero (DT-8), round-robin estável.
- **Disciplina Kahneman**: #2 Counterfactual — ver mapa.

### ITEM-8 (parte CRIAR) — Integração do broker no daemon

**`crates/ramshared-wsl2d/src/broker_srv.rs`**

- **Propósito**: listener TCP do árbitro + sessões de agente + loop de decisão; ponte com o
  worker NBD (demote) — RF-B1..B4 "vivos" no daemon.
- **Requisitos cobertos**: RF-B1, RF-B2, RF-B3, RF-B4, RNF-2, RNF-3, DT-2.
- **Structs/Funções** (assinaturas exatas):

```rust
pub struct BrokerConfig {
    pub listen: std::net::SocketAddr,      // já validado não-unspecified (main.rs)
    pub nbd_endpoint: NbdEndpoint,         // o que os agentes recebem no SwapOn
    pub arbiter: ArbiterConfig,
}

/// Sobe acceptor + core single-threaded do broker. `demote_rx` recebe os veredictos
/// do worker NBD (canário §9/§9.4) e vira DemoteAll (SwapOff em todas as Active).
/// `shutdown` é a flag SIGTERM: dispara DemoteAll + espera Done (bounded 10s) antes de sair.
pub fn spawn_broker(
    cfg: BrokerConfig,
    slice_map: SliceMap,
    demote_rx: std::sync::mpsc::Receiver<DemoteReason>,
    shutdown: &'static std::sync::atomic::AtomicBool,
) -> std::io::Result<std::thread::JoinHandle<()>>;
```

- **Desenho interno** (mesmo padrão do worker CUDA único / `conn.rs`): acceptor TCP spawna,
  por conexão, um reader que desserializa `Msg` e envia para o canal de eventos do core; o
  core (uma thread, dono de `SliceMap`+`Arbiter`+tabela de sessões com `Sender<Msg>` por
  agente) processa eventos com `recv_timeout(2s)` — timeout = tick do árbitro. Zero locks.
- **Log de decisão (RF-B4)**, uma linha por decisão, chave=valor:
  `[ramsharedd] arbiter move slice=s1 from=civm(psi10=32.1) to=wsl2(psi10=4.2) streak=5 cooldown=60s`
  (sempre as pressões **dos dois lados**).
- **Reconciliação (DT-9)**: no `Register`, o primeiro `Psi{swaps}` é comparado aos exports —
  slice que o agente já tem em `/proc/swaps` re-`assign` sem comando.
- **Dependências internas**: `ramshared_broker::{protocol, slices, arbiter, model}`.
- **Padrão de referência**: `spawn_acceptor`/canais de `crates/ramshared-wsl2d/src/conn.rs`.
- **Testes requeridos**: ITEM-10 (e2e in-process com agentes falsos); proto errado →
  `Error` + desconexão; agente que some (EOF) → tenant marcado ausente e slices dele
  permanecem no estado em que estão (limpeza é do watchdog do agente, não do broker).
- **Disciplina Kahneman**: #5 — ver mapa (bind) e ITEM-9/11 (worst-case).

### ITEM-9 — Crate `ramshared-agent`

**`crates/ramshared-agent/Cargo.toml`**

- **Propósito**: binário `ramshared-agent` (PRD §8). Deps: `ramshared-broker` (path),
  `serde`/`serde_json` (transitivo). `#![forbid(unsafe_code)]`.
- **Requisitos cobertos**: RF-L3, RNF-1, RNF-5.

**`crates/ramshared-agent/src/main.rs`**

- **Propósito**: CLI + loop principal: conectar/reconectar ao broker, reportar PSI a 1 Hz,
  executar comandos, watchdog.
- **Requisitos cobertos**: RF-L3, RF-B1 (lado agente), RNF-1, DT-13.
- **Flags CLI** (parsing manual, mesmo padrão do `run()` em `crates/ramshared-wsl2d/src/main.rs`):
  `--broker IP:PORT` (obrigatória) · `--tenant NAME` (obrigatória) · `--swap-prio P`
  (opcional, DT-7) · `--nbd-dev-base PATH` (default `/dev/nbd`; device da slice `sN` =
  `{base}{N}`) · `--status` (modo one-shot: envia `Status`, imprime `StatusReply` JSON, sai).
- **Lógica resumida**: (1) checa `euid==0` (DT-13; `--status` dispensa); (2) conecta TCP,
  `set_read_timeout(1s)`, `Register`; (3) loop: a cada 1s envia `Psi` (`psi.rs`); processa
  `Msg` recebidas — `SwapOn` → `nbd_connect` + `swap_on` → `SwapOnDone`; `SwapOff`/`DemoteAll`
  → `swap_off` + `nbd_disconnect` → `SwapOffDone`; qualquer byte do broker (inclusive `Ack`)
  alimenta `Watchdog::touch`; (4) `Watchdog::expired` → **cleanup best-effort**: `swap_off` +
  `nbd_disconnect` de toda slice ativa, depois loop de reconexão com backoff (1s..30s).
- **Testes requeridos**: dispatch de comandos com transporte `Cursor` (sem rede); cleanup
  chamado exatamente uma vez por expiração.
- **Disciplina Kahneman** (RNF-1): #5/#13 — ver mapa (linha ITEM-9).

**`crates/ramshared-agent/src/psi.rs`**

- **Propósito**: parsers de `/proc/pressure/memory` e `/proc/swaps` (RF-L3).
- **Funções** (assinaturas exatas):

```rust
/// Parse da linha `some` (DT-15): avg10/avg60 + total (us) → stall_us. `full` é logada.
pub fn read_psi() -> std::io::Result<PsiSample>;
pub fn parse_psi(content: &str) -> Option<PsiSample>;          // puro, testável
pub fn read_swaps() -> std::io::Result<Vec<SwapEntry>>;
pub fn parse_swaps(content: &str) -> Vec<SwapEntry>;           // puro, testável
```

- **Testes requeridos**: fixtures literais de `/proc/pressure/memory` e `/proc/swaps` reais
  (WSL2 e civm, coletadas no P0); entrada truncada/corrompida → `None`/vazio, nunca panic.

**`crates/ramshared-agent/src/swap.rs`**

- **Propósito**: execução de `nbd-client`/`swapon`/`swapoff` (generalização do padrão
  `spawn_swapoff` de `crates/ramshared-wsl2d/src/swap.rs`).
- **Requisitos cobertos**: RF-L3, RNF-1, DT-7, DT-14.
- **Funções** (assinaturas exatas):

```rust
/// nbd-client -N <export> (-unix <path> | <host> <port>) <dev> -timeout 30  — nunca -persist (DT-14).
pub fn nbd_connect(endpoint: &NbdEndpoint, export: &str, dev: &str) -> std::io::Result<()>;
pub fn nbd_disconnect(dev: &str) -> bool;                       // nbd-client -d, best-effort
/// swapon [<-p prio>] <dev>. prio=None ⇒ sem -p (DT-7).
pub fn swap_on(dev: &str, prio: Option<i32>) -> std::io::Result<()>;
/// swapoff em thread separada (pode bloquear) — mesmo desenho anti-deadlock de spawn_swapoff.
pub fn spawn_swap_off(dev: &str) -> std::sync::mpsc::Receiver<bool>;
```

- **Padrão de referência**: `spawn_swapoff` (`crates/ramshared-wsl2d/src/swap.rs:23`) — o
  original do daemon **não** muda nem move (o caminho de DEMOTE single-mode continua dono dele).
- **Testes requeridos**: montagem de argv pura (função `fn nbd_args(...) -> Vec<String>`
  separada do spawn, testável): `-timeout 30` sempre presente, `-persist` nunca; `-p` só com
  `Some`.

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

- **Dimensionamento**: heartbeat 1 Hz (o `Psi` + `Ack`) e deadline 3s ⇒ detecção ≤3s +
  swapoff best-effort ⇒ **<5s total**, o gate do PRD §10/P1.
- **Testes requeridos**: não expira com touch em dia; expira após deadline; touch pós-expiração
  re-arma (reconexão).
- **Disciplina Kahneman**: #13 — o unit test usa clock falso e **não substitui** o drill
  (ITEM-11) como evidência de RNF-1; ver mapa.

### ITEM-10 — Teste e2e in-process

**`crates/ramshared-wsl2d/tests/broker_e2e.rs`**

- **Propósito**: e2e sem root/GPU/swap real: broker real (`spawn_broker` com `SliceMap` 2×64MiB)
  + 2 agentes falsos (TcpStream falando `protocol::Msg`, respondendo `SwapOnDone`/`SwapOffDone`
  imediatos) em `127.0.0.1:0`.
- **Requisitos cobertos**: RF-B1, RF-B2, RF-B4 (integração), RNF-4 (não toca o modo single).
- **Cenários mínimos**: registro + round-robin inicial (DT-6); PSI desequilibrado por streak
  ticks → observa `SwapOff` no doador e `SwapOn` no receptor **nessa ordem** (fronteira de
  atomicidade); `Status` reflete slices/tenant/PSI; `LeaseRequest` revoga e `LeaseRelease`
  devolve; proto errado → `Error`.
- **Restrição operacional**: tudo in-process (threads), **nenhum daemon standalone spawnado** —
  smoke de daemon só em VM/qemu (incidente WSL2; regra de sessão).
- **Disciplina Kahneman**: #13 — este teste valida protocolo+política; o modo de falha real
  (swap ativo, broker morto) é o ITEM-11.

### ITEM-11 — Drill de D-state em qemu

**`scripts/kernel/qemu-broker-drill.sh`**

- **Propósito**: worst-case obrigatório do PRD §14: dentro da VM qemu — `ramsharedd --backend
  ram --slices 2 --slice-mb 64 --listen-nbd tcp://127.0.0.1:10809 --arbiter-listen
  127.0.0.1:7777` + `ramshared-agent --broker 127.0.0.1:7777 --tenant vm`; espera slice ativa
  em `/proc/swaps`; `kill -9` no daemon; mede até o agente completar swapoff; imprime
  `PASS: watchdog swapoff em <N>s` se N<5 e a VM segue responsiva, senão `FAIL`.
- **Requisitos cobertos**: RNF-1, R2/R7, gate P1.
- **Padrão de referência**: `scripts/kernel/qemu-ublk-daemon.sh` (harness F2: mesma VM, mesmo
  mecanismo de injeção/coleta).
- **Testes requeridos**: o script **é** o teste; log de execução vai para o IMPL.
- **Disciplina Kahneman**: #5 — ver mapa (proibido rodar fora do qemu).

### ITEM-12 — Runbook civm

**`docs/runbooks/CIVM-TENANT.md`**

- **Propósito**: RF-L4 — provisionamento copiável do tenant civm: instalar `nbd-client`,
  copiar binário `ramshared-agent`, unit systemd (template literal no doc), conectividade
  (resultado do `measure-net.sh` decide Tailscale vs port-forward), verificação
  (`/proc/swaps`, `--status`), **runbook de remoção** (RNF-1: swapoff → nbd-client -d →
  disable unit) e seção "Validado em / Não validado em" (#1 WYSIATI).
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
- **Impacto**: quebra a assinatura → callers: `spawn_reader` (`conn.rs`, ITEM-7) e os testes
  do próprio arquivo. Não há outros usuários (Confirmado: grep `server_handshake`). Wire
  format para cliente sem `-N` permanece **byte-idêntico** (RNF-4).
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
  projeção fina por cima, sem tocar CUDA), DT-4.
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
- **Impacto**: sem quebra; `ublk_server.rs` ajusta o import de `RamBackend` (re-export
  `pub use` em `lib.rs` mantém `ublk_server::RamBackend` válido **não** — sem alias de
  compat: atualizar os usos para `crate::backend::RamBackend`, Day-0).
- **Testes requeridos**: `SliceView` sobre `RamBackend`: leitura/escrita em slices vizinhas
  não vazam (offsets disjuntos); offset além do len da slice → `serve` devolve EINVAL (reuso
  do teste `out_of_range_is_einval` de `request.rs` como referência); `base+len > inner` panica
  em debug.

### ITEM-7 — `crates/ramshared-wsl2d/src/conn.rs`

- **O que muda**: (a) reader/writer/acceptor ficam **genéricos sobre o stream** (Unix e TCP);
  (b) handshake passa a negociar export e o `Job` carrega o índice; (c) anti-DoS de WRITE usa
  o tamanho do export negociado.
- **Requisitos cobertos**: RF-L1, RF-L2.
- **Função/bloco afetado**: `Job`, `spawn_reader`, `spawn_writer`, `spawn_acceptor`.
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
  `exports[idx].size`; cada `Job` sai com `export: idx`. `WMsg`/`Reply`/`LiveCount`/`CHAN_CAP`
  **não mudam** (o término determinístico DT-15 e o backpressure DT-7 da multiconn ficam
  intactos — os dois acceptors compartilham o mesmo canal, e `Opened`/`Closed` continuam
  balanceados por conexão).
- **Por quê**: RF-L2 (TCP) e RF-L1 (slice por conexão) sem duplicar o pipeline H1.
- **Impacto**: quebra assinaturas internas → callers: `run_nbd` (ITEM-8) e testes do módulo.
  uAPI/ABI: nenhuma. Wire NBD: idêntico para cliente sem `-N`.
- **Testes requeridos**: existentes preservados; novo: dois acceptors (Unix+TCP em loopback)
  alimentando um worker — `live_count` balanceado e jobs de ambos servidos (in-process).
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
     `BackendKind::Ram` (novo aqui; `RamBackend` de ITEM-6; pula CUDA, canário §9.4 fica
     `None`) — necessário para o drill ITEM-11 e e2e sem GPU; (b) tamanho = `--slices ×
     --slice-mb` MiB (uma alocação única `ctx.alloc`, como hoje) ou `--size` no modo single;
     (c) tabela `Vec<Export>` vinda de `SliceMap::exports()` (modo single: 1 export `"default"`,
     resolvido também por nome vazio); (d) sobe `spawn_acceptor` (Unix) e, se `--listen-nbd`,
     `spawn_acceptor_tcp` no mesmo canal; (e) **worker**: para cada `Job`, resolve
     `exports[job.export]` → `SliceView::new(&mut backend, base, len)` → `serve(&job.req,
     &job.payload, &mut view)` — canário §9 (latência serve-only) e sonda §9.4 continuam
     globais, sem mudança de semântica (eviction WDDM é GPU-wide); (f) **roteamento do
     DEMOTE**: com broker ativo, `Verdict::Demote` envia `DemoteReason` pelo canal ao broker
     (vira `DemoteAll` nos agentes) em vez de `spawn_swapoff` local; sem broker, caminho
     atual intacto (RNF-4); (g) registra `handle_shutdown` p/ SIGINT/SIGTERM também no caminho
     NBD quando broker ativo (shutdown ordenado = fluxo 4 do PRD); (h) se `--arbiter-listen`,
     chama `spawn_broker` (ITEM-8/CRIAR) com `NbdEndpoint` derivado (`--listen-nbd` → Tcp;
     senão Unix `--sock`).
  4. **Prefixo de log**: `[wsl2d]` → `[ramsharedd]` em todas as `eprintln!` do binário (DT-5).
  5. **`lib.rs`**: exporta `broker_srv` e os novos tipos públicos.
- **Requisitos cobertos**: RF-B1..B4 (fiação), RF-L1, RF-L2, RF-P2 (parcial), RNF-2, RNF-4, DT-2..DT-5.
- **Antes/Depois (resumo do shape)**: `run_nbd(size, sock, force, nbd_dev)` →
  `run_nbd(cfg: NbdRunConfig)` com `struct NbdRunConfig { slices: u16, slice_mb: u64, size: u64,
  sock: String, listen_nbd: Option<std::net::SocketAddr>, arbiter: Option<std::net::SocketAddr>,
  force: bool, nbd_dev: String, backend: BackendKind }` (mantém `run()` <80 linhas por função —
  regra de coding).
- **Por quê**: PRD §8 — superfície CLI definitiva do P1.
- **Impacto**: binário muda de nome → `scripts/kernel/qemu-ublk-daemon.sh` ajusta (abaixo);
  docs (`README.md`/`ARCHITECTURE.md`) no mesmo commit. uAPI kernel: nenhuma.
- **Testes requeridos**: parsing/validação das flags (recusa 0.0.0.0; ublk+slices; slice-mb
  ausente) extraído para função pura testável; `run_nbd` com `--backend ram --slices 2` coberto
  pelo e2e ITEM-10 (transitivamente) e drill ITEM-11.
- **Disciplina Kahneman**: #5 (bind) — ver mapa.

### `crates/ramshared-wsl2d/src/ublk_server.rs`

- **O que muda**: `RamBackend` sai daqui (movido para `backend.rs`, ITEM-6); imports/usos
  atualizados (`spawn_server_dt3`, testes, `UblkHandle::Ram` no main). Nenhuma mudança
  funcional no caminho ublk (RNF-4; DT-3).
- **Requisitos cobertos**: DT-3, RNF-4.
- **Testes requeridos**: suíte ublk existente verde sem edição de asserts.

### `scripts/kernel/qemu-ublk-daemon.sh`

- **O que muda**: nome do binário `ramshared-wsl2d` → `ramsharedd` (DT-5). Só isso.
- **Testes requeridos**: rodar o harness F2 uma vez pós-rename (PASS igual ao baseline).

### `Cargo.toml` (raiz do workspace)

- **O que muda**: `members` ganha `"crates/ramshared-broker"` e `"crates/ramshared-agent"`.
- **Impacto**: CI (`cargo test --workspace`) cobre os crates novos sem mudança no workflow.

### `docs/LIBRARIES.md`

- **O que muda**: entradas `serde`/`serde_json` (versão, motivo, link ADR-0005) — mesmo commit
  da dep (disciplina #11).

### `docs/reliability/DEGRADATION-MATRIX.md`

- **O que muda**: novos modos de falha: broker morto com swap remoto ativo (detecção:
  watchdog 3s; unwind: swapoff best-effort + EIO bounded via `-timeout 30`; validação: drill
  ITEM-11) · `wsl --shutdown` mata broker (R7; = broker morto) · agente morto com slice ativa
  (detecção: EOF no broker; unwind: slice fica no estado, runbook de remoção) · `SwapOn` falha
  no destino (unwind: slice `Free`, retry no tick, log).
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
| Lease grant/revoke/release | Info | `lease`, `holder`, `bytes`, `slices`, `revoked_from` |
| DemoteAll (residência ou shutdown) | Info | `reason` (`Latency\|Corruption\|FreeFloor\|shutdown`), `slices_active` |
| Tenant registrado/perdido | Info | `tenant`, `transport`, `slices_reconciliadas` |
| Watchdog expirado (agente) | Error | `broker`, `idle_s`, `slices_limpas`, `swapoff_ok` |
| SwapOn/SwapOff executado (agente) | Info | `slice`, `dev`, `prio`, `ok`, `detail` |
| Bind recusado (RNF-2) | Error | `addr`, `motivo=rnf2_bind_privado` |

`Status` (RF-B4): `StatusReply` com slices/tenant, PSI por tenant e `last_rebalance_secs` —
acessível via `ramshared-agent --status` ("cada um sabe quem está precisando mais").

## Contratos e documentação viva

| Documento | Atualização necessária | Motivo |
| --- | --- | --- |
| `Documentation/` (uAPI/ABI) | N/A | Nenhuma uAPI de kernel nova (PRD §8); ublk/NBD existentes |
| `Kconfig` (help) | N/A | Sem novo CONFIG_/module param |
| `CLAUDE.md` | N/A | Nenhum padrão estrutural de trabalho muda |
| `.claude/rules/*.md` | N/A | Nenhuma convenção nova |
| `docs/decisions/ADR-0005-broker-protocol-jsonl.md` | Criar | DT-1 + dep serde (disciplina #11) |
| `docs/methodology/KAHNEMAN-DISCIPLINES.md` | N/A | Disciplinas existentes cobrem (nenhuma âncora nova) |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alterar | 4 modos de falha novos (ver MODIFICAR) |
| `docs/LIBRARIES.md` | Alterar | serde/serde_json |
| `docs/runbooks/CIVM-TENANT.md` | Criar | RF-L4 |
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
7. **ITEM-7** — `ramshared-wsl2d/src/conn.rs`: streams genéricos, export no `Job`, acceptor TCP.
8. **ITEM-8** — `ramshared-wsl2d`: `broker_srv.rs`, `main.rs` (flags, `run_nbd`, demote routing, rename `ramsharedd`), `lib.rs`, `scripts/kernel/qemu-ublk-daemon.sh`, `README.md`/`ARCHITECTURE.md`.
9. **ITEM-9** — `ramshared-agent`: `psi.rs`, `swap.rs`, `watchdog.rs`, `main.rs`.
10. **ITEM-10** — `crates/ramshared-wsl2d/tests/broker_e2e.rs`.
11. **ITEM-11** — `scripts/kernel/qemu-broker-drill.sh` + execução do drill (PASS commitado no IMPL). **[GATE P1: drill <5s]**
12. **ITEM-12** — `docs/runbooks/CIVM-TENANT.md` + `DEGRADATION-MATRIX.md` + e2e real WSL2↔civm (cenário 1 do PRD: action na civm + build no WSL2, logs de rebalanço → IMPL). **[GATE P1: cenário 1 demonstrado]**

Checkpoints de commit: 1 commit por item (atômico, revisável), rastreando `RF-*`/`DT-*` no body;
itens 5–8 carregam `Rollback trigger:` (mudança de contrato/estrutura — disciplina #2).

## Plano de testes

**Backend (crates Rust)**

- Unitários: `protocol.rs` (roundtrip por variante, teto de linha, shape inválido);
  `slices.rs` (transições, offsets disjuntos); `arbiter.rs` (histerese, cooldown, nunca-zero,
  counterfactual com clock falso, lease > nunca-zero, round-robin); `psi.rs` (fixtures reais
  WSL2/civm, entrada corrompida); `watchdog.rs` (clock falso); `swap.rs` (argv puro: `-timeout`
  sempre, `-persist` nunca, `-p` condicional); `SliceView` (isolamento entre slices, EINVAL
  fora da janela); parsing de flags do daemon (0.0.0.0, ublk+slices).
- Integração: `broker_e2e.rs` (ITEM-10, in-process); dois acceptors no mesmo worker (ITEM-7);
  suíte existente intacta (`handshake`, `conn`, `ublk_*`, `residency`, `state`) — RNF-4.
- Isolamento de ring: N/A kernel (Ring 0 não muda); análogo coberto por: bounds por slice,
  euid-gate do agente, bind privado.
- Concorrência / atomicidade: `slow_writer_does_not_deadlock` preservado; `assign` em slice
  não-Free falha (fronteira de atomicidade); ordem SwapOff→Done→SwapOn observada no e2e.

**Drivers (drm/amd/nouveau)**: N/A — nenhum código de kernel neste escopo.

**Manuais**

- Smoke GPU (host EMEDEV, sudo ok): `ramsharedd --slices 2 --slice-mb 128` + `nbd-client -unix
  ... -N s0 /dev/nbd0` e `-N s1 /dev/nbd1` + `mkswap/swapon` nos dois; verificação em
  `/proc/swaps` (prioridades distintas das locais — DT-7).
- Cenários de erro: export inexistente (`-N s9` → conexão recusada); broker com bind 0.0.0.0
  (recusa); agente sem root (erro claro); `SwapOn` com `nbd-client` ausente
  (`SwapOnDone{ok:false}` + retry).
- Evidências objetivas das etapas críticas (mapa Kahneman): `P0-RESULTS.md` preenchido; saída
  `PASS` do drill qemu; logs do árbitro com PSI dos dois lados no e2e real WSL2↔civm (gates
  P1, critérios de aceitação 1 e 4 do PRD §13).

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
- [ ] Nenhum ponto crítico com linguagem vaga: os triggers são numéricos (<5s watchdog, 2× PSI
      em 60s, 0.0.0.0 recusado, P0 com n≥3 rodadas)
