# SPEC — RamShared P2: Ponte Windows + MVP DCC (Blender)

> **SSDV3 PASSO 2**, a partir de `docs/memory-broker-p2-windows/PRD.md`. Fecha as decisões que o PRD
> deixou "a fixar na SPEC" e traduz RF-W1/W2/W3 + RF-P1/P3 em mudanças exatas no repo. **Userspace**
> (Rust + Python), sem uAPI de kernel nova. Benchmarks que embasem gate seguem
> [`.claude/rules/benchmarks.md`](../../.claude/rules/benchmarks.md).
>
> **Gate de IMPL (anti-halo, #11):** este SPEC pode ser escrito e revisado agora, mas a **IMPL não
> inicia** sem os inputs do Alex (PRD Anexo B: ≥1 `.blend` que falhava + erro exato + medição P0).
> O SPEC define o *como*; o gate define o *quando*.

## Escopo fechado desta implementação

**Entra agora (SPEC):**
- `ramshared-nvml` — crate FFI (dlopen) de orçamento de VRAM portável (Win/Linux) — RF-W1/RF-W2.
- `ramshared-config` — crate de config TOML única (superset tipado das flags atuais) — RF-P3.
- Agente **DccAgent** Windows (`ramshared-agent-win`) reusando o núcleo de cliente do broker — RF-W1.
- Ponte de **lease** addon↔agente↔broker (protocolo local + bridge p/ o `Msg` existente) — RF-W3.
- Addon Blender MVP (Python): predição cabe/não-cabe + out-of-core nativo + monitor — RF-W2.
- Instalável Windows (serviço + addon + CLI) — RF-P1.

**Fica fora agora (explícito):** RF-W4 (interposer `nvcuda.dll` → P4); RF-G1/G2/G3 (trait/Vulkan/D3D12
→ P3, sendo que RF-G1/G2 já estão no branch de P1); Windows-como-consumidor-de-swap (driver de disco →
P4); reescrever Cycles; auth/cripto própria (rede privada só, RNF-2); **multi-lease** (o broker é
1-lease-por-vez, `broker_srv.rs:403` — DT-8).

**Dependências assumidas prontas (Confirmado no codebase):**
- Protocolo do broker (JSON-lines/TCP): `enum Msg` (`crates/ramshared-broker/src/protocol.rs:19`),
  `write_msg`/`read_msg` (`protocol.rs:132`/`:144`), `PROTO_VERSION=1` (`:12`), `MAX_LINE_BYTES=64KiB`
  (`:14`), `TransportKind` no `Register` (`:21`).
- **Lease end-to-end**: `LeaseRequest`/`LeaseRelease`/`LeaseGranted`/`LeaseDenied`
  (`protocol.rs:42,45,64,68`); árbitro `Action::RevokeForLease`/`GrantLease` (`arbiter.rs:74,80`,
  tick `:164-217`); servidor `on_lease_request`/`on_lease_release` (`broker_srv.rs:391,427`),
  aplicação `:628-664`, auto-release no disconnect (`:456-464`); estado `SliceMap::lease/unlease`
  (`slices.rs:89,98`). **A P2 só cria o consumidor externo.**
- Papel de cliente do broker: `crates/ramshared-agent/src/main.rs` (Register `:264-271`, reconexão
  backoff `:33-38`, watchdog `:274,340`, writer único `:4-9`).
- Orçamento de VRAM (referência de padrão): CUDA `Context::mem_info()` → `cuMemGetInfo_v2`
  (`crates/ramshared-cuda/src/driver.rs:189`); padrão de FFI dlopen em `ramshared-cuda/src/{driver,ffi}.rs`.

## Matriz de rastreabilidade PRD → SPEC

| PRD | Implementação no SPEC |
| --- | --- |
| RF-W1 (agente Windows: pressão SO + NVML + cliente broker) | ITEM-1 (nvml), ITEM-3 (win_mem), ITEM-4 (client core), ITEM-5 (bin `ramshared-agent-win`) — DT-1, DT-2, DT-3 |
| RF-W2 (addon Blender: predição + out-of-core + monitor) | ITEM-8 (addon) — DT-7, DT-10 |
| RF-W3 (ponte lease addon↔broker) | ITEM-6 (`LocalMsg` + listener), ITEM-7 (bridge→`Msg`), ITEM-9 (DccAgent no árbitro) — DT-4, DT-5, DT-10, DT-11 |
| RF-P1 (instalável Windows) | ITEM-10 (packaging + serviço) — DT-9 |
| RF-P3 (config TOML única) | ITEM-2 (`ramshared-config`), ITEM-2b (loaders no daemon/agente) — DT-6 |

## Decisões técnicas

Decisões fechadas aqui (não explícitas ou marcadas "a fixar" no PRD):

| # | Decisão | Justificativa |
| --- | --- | --- |
| DT-1 | **Pressão de memória do Windows = `GlobalMemoryStatusEx`** (`MEMORYSTATUSEX`: `ullAvailPhys`, `ullTotalPhys`, `dwMemoryLoad`, `ullAvailPageFile`/`ullTotalPageFile` p/ commit). **Rejeita PDH/perfmon.** | 1 syscall, sem counter-path frágil nem thread de coleta; cobre o equivalente do PSI (disponível/carga/commit). PDH agrega histórico que o MVP não usa (PRD P2-R3 pede heurística conservadora, não série temporal). |
| DT-2 | **NVML via dlopen hand-rolled no crate `ramshared-nvml`**; **rejeita `nvml-wrapper`** e qualquer crate NVML. | Day-0/zero-dep (LIBRARIES.md #11 — mesmo critério que barrou o `clap`); reusa o **padrão** já provado do `ramshared-cuda` (dlopen de `libcuda`, `driver.rs`/`ffi.rs`), `unsafe` isolado + `// SAFETY:`, superfície `#![forbid(unsafe_code)]`. Símbolos mínimos: `nvmlInit_v2`, `nvmlDeviceGetHandleByIndex_v2`, `nvmlDeviceGetMemoryInfo`, `nvmlShutdown`. |
| DT-3 | **Núcleo de cliente do broker extraído** p/ `ramshared-agent/src/client.rs` (conexão + Register + reconexão/backoff + watchdog + writer único), genérico sobre um trait `AgentRole`. Dois bins finos: o **tenant de swap Linux** (atual) e o novo **`ramshared-agent-win`** (DccAgent). **Rejeita copy-paste** do loop e **rejeita cfg-soup** num bin único. | DRY/Day-0: o loop de reconexão/watchdog é hardened (validado nos drills). RNF-4: o agente Linux existente continua verde (drills) — a extração é coberta pelos testes/drills atuais. Kahneman #14 (abort se drill Linux regredir). |
| DT-4 | **Protocolo local addon↔agente = `LocalMsg` próprio** (localhost JSON-lines), **separado** do `Msg` do broker. Agente traduz `LocalMsg`→`Msg`. | Mantém o `protocol.rs` do broker **intocado** (RNF-4: P2 não toca protocolo de swap). O addon (Python) não é um *tenant* do broker (o broker é 1-lease-por-tenant e a identidade registrada vive no agente, DT-10). `LocalMsg` carrega o que o broker `Msg` não tem (query de `VramBudget`). |
| DT-5 | **`TransportKind::DccAgent`** novo (aditivo em `protocol.rs`); o **árbitro exclui DccAgent da atribuição de slices de swap** (round-robin/rebalance) — ele é lease-holder/observador, não consumidor de swap. | Um DccAgent registra p/ **segurar lease** e reportar VRAM, não p/ receber swap. Sem a exclusão, o árbitro desperdiçaria slices num tenant que ignora `SwapOn`. Mudança é **control-plane puro** (arbiter.rs, testável), data-plane intocado (RNF-4). |
| DT-6 | **Config TOML = novo crate `ramshared-config`** (dep `toml`); precedência **CLI > TOML > default**. Loader chamado no `ramsharedd` e no agente; **flags viram override**, não somem. | Um schema tipado (superset das 12 flags do daemon `wsl2d/main.rs:220-326` + 7 do agente `agent/main.rs:89`); `toml` (MIT/Apache-2.0) é dep madura — hand-roll de parser TOML seria pior que a regra #1. Flags-como-override preserva os drills/CLI atuais (RNF-4). |
| DT-7 | **NVML degrade-graceful (RNF-6):** dlopen falha / sem GPU / símbolo ausente → `VramBudget = None`; o agente segue reportando `WinMemPressure`; o addon cai numa **heurística conservadora** (assume "não cabe" acima de X% do `ullTotalPhys`). | Pior caso #5: um host sem NVIDIA/driver não pode travar o addon; o valor do RF-W2 (out-of-core) não depende de NVML — só a precisão da predição. |
| DT-8 | **Lease continua 1-ativo-por-vez** (P1, `broker_srv.rs:403` `LeaseDenied{"lease_em_andamento"}`). Multi-lease **fora de escopo**. | MVP = 1 artista/1 render por host. Multi-lease exigiria fila/prioridade no árbitro — feature própria, não bloqueia o valor da P2. |
| DT-9 | **Serviço Windows via crate `windows-service`** (SCM) + `windows`/`windows-sys` p/ `GlobalMemoryStatusEx`. **Rejeita** hand-roll de FFI da SCM. | SCM correto (control handler, status reporting) é fácil de errar em FFI cru; `windows-service` (MIT/Apache-2.0) é o padrão. Isola o RF-P1 do resto. Registrar em LIBRARIES.md + deny.toml (licenças). |
| DT-10 | **O addon fala SÓ com o agente local** (nunca direto com o broker). | A identidade de tenant registrada (e o 1-lease-por-tenant) vive no agente; um socket separado do addon seria outro tenant. O agente é o **dono único** da sessão do broker + do NVML (counterfactual DT-11 precisa das duas coisas juntas). |
| DT-11 | **Auto-release por counterfactual (PRD §14) vive no agente:** amostra NVML do uso de VRAM do render; **< 50% do `lease.bytes` por 5 min** ⇒ o agente envia `LeaseRelease` sozinho (logado). | O agente tem NVML **e** a sessão do broker (DT-10); é o único ponto que fecha o loop telemetria→decisão (liga RF-W1 a RF-W3). Kahneman #2 (revogar swap p/ render que não usa a VRAM é perda). |

## Fronteira de atomicidade e política de rollback

**Fronteira de atomicidade desta implementação:**
- **Atômico (já garantido pelo broker, reusado):** a concessão/revogação de lease é serializada no
  árbitro single-thread (`arbiter.rs`) e no servidor (`broker_srv.rs`): 1 lease pendente/ativo por vez
  (DT-8). O `LeaseGranted` só é emitido depois das slices estarem `Leased`/drenadas (`:638-664`).
- **Fora da atomicidade (eventual):** a predição do addon (footprint estimado vs `VramBudget` NVML) é
  um snapshot — a VRAM pode mudar entre a amostra e o render. Mitigação: margem de segurança (DT-7) +
  monitor de spill (RF-W2) + degrade (RNF-1). O counterfactual (DT-11) é uma decisão *eventual* (janela
  de 5 min), não transacional.
- **Estados parciais aceitos:** NVML ausente → `VramBudget=None` (DT-7); broker indisponível →
  `LeaseRequest` timeout → degrade pro out-of-core (RNF-1). Nenhum trava o render.

**Política de rollback:**
- **Rollback de aplicação:** cada ITEM compila isolado; `git revert` do ITEM reverte a superfície
  (ex.: reverter ITEM-9 tira a exclusão DccAgent do árbitro sem afetar swap). O agente Windows e o
  addon são **produtos separados** (não linkados ao data-plane) — removê-los não afeta o tier P1.
- **Rollback de migration:** N/A — sem DB/esquema. O TOML (ITEM-2) é aditivo (flags continuam válidas).
- **Rollback de dados:** N/A — sem dado persistente novo (o TOML é config declarativa do operador).
- **Proibido / forward-only:** N/A (sem produção viva; Day-0).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina Kahneman | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (NVML dlopen) | #1 WYSIATI + #3 Número | [`#1-wysiati--what-you-see-is-all-there-is`](../methodology/KAHNEMAN-DISCIPLINES.md#1-wysiati--what-you-see-is-all-there-is) | O `VramBudget` do NVML bate com o `cuMemGetInfo` (CUDA) no MESMO host/instante? | Teste `#[ignore]` na RTX 2060 (Linux `libnvidia-ml.so.1`): `nvml.free` ≈ `cuda.mem_info().0` ± 1 página | divergência sistemática NVML×CUDA → binding errado, não avançar |
| ITEM-5/ITEM-7 (lease no hot path do render) | #5 Availability (worst-case) + #3 Número | [`#5-availability-heuristic`](../methodology/KAHNEMAN-DISCIPLINES.md#5-availability-heuristic) | `LeaseRequest`→`LeaseGranted` resolve < 1 s local? Broker morto ⇒ degrade? | Medição local (≥3 rodadas): grant em ms; teste de timeout → addon segue no out-of-core | grant > 1 s (RNF-1) ou timeout que **trava** o render → bloquear |
| ITEM-9 (DccAgent exclui swap) | #13 Ilusão de validade | [`#13-ilusão-de-validade`](../methodology/KAHNEMAN-DISCIPLINES.md#13-ilusão-de-validade) | O árbitro realmente não atribui slice a DccAgent, e ainda atribui aos tenants de swap? | Teste puro `arbiter.rs`: com 1 DccAgent + 1 tenant swap, round-robin só serve o swap; lease do DccAgent revoga o swap | DccAgent recebe SwapOn, ou tenant de swap deixa de receber → regressão do árbitro |
| ITEM-3/ITEM-8 (predição cabe/não-cabe) | #2 Counterfactual | [`#2-counterfactual-obrigatório`](../methodology/KAHNEMAN-DISCIPLINES.md#2-counterfactual-obrigatório) | A predição erra p/ "cabe" quando não cabe (falso-negativo perigoso)? | Cena real do Alex (gate): footprint previsto vs pico real do monitor; margem cobre o erro | falso "cabe" que estoura a VRAM no render → aumentar margem / degrade |
| DT-11 (auto-release counterfactual) | #2 Counterfactual | [`#2-counterfactual-obrigatório`](../methodology/KAHNEMAN-DISCIPLINES.md#2-counterfactual-obrigatório) | Revogar o swap p/ um render que usa < 50% da VRAM é perda líquida? | Log do agente: uso NVML < 50% por 5 min ⇒ `LeaseRelease` emitido | swap revogado + VRAM ociosa sem release → counterfactual não fechou |
| DT-3 (extração do client core) | #14 Mass-Refactoring Fallacy | [`#14-falácia-do-refatoramento-em-massa-mass-refactoring-fallacy`](../methodology/KAHNEMAN-DISCIPLINES.md#14-falácia-do-refatoramento-em-massa-mass-refactoring-fallacy) | A extração muda o comportamento do agente Linux? | `cargo test -p ramshared-agent` + drill `qemu-broker-drill.sh` verdes após a extração | qualquer regressão no drill/teste do agente Linux → reverter a extração |

## Checklist de segurança (pré-implementação)

- [ ] **Isolamento (RNF-2):** listener local do agente (ITEM-6) faz bind **só em 127.0.0.1**; o cliente
  do broker só conecta em loopback/rede privada (reusa a validação `parse_private_listen` do daemon).
- [ ] **Buffer overflow / OOB:** `LocalMsg` e `Msg` usam `read_msg` com teto `MAX_LINE_BYTES` (64 KiB,
  `protocol.rs:14`) — anti-DoS já existente, reusado; o FFI NVML só lê struct `nvmlMemory_t` de tamanho fixo.
- [ ] **Permissões:** o agente Windows roda como serviço; **sem** elevação além da necessária p/ ler
  memória/NVML (leitura local). O addon roda no espaço do Blender (sem privilégio). Sem `CAP_SYS_ADMIN`.
- [ ] **Input validation:** `bytes` do `LeaseRequest` já é validado no broker (`> total` → `LeaseDenied`,
  `broker_srv.rs:412`); o `LocalMsg::LeaseAcquire{bytes}` do addon é revalidado no agente antes de encaminhar.
- [ ] **`unsafe`/FFI:** todo `unsafe` do NVML (ITEM-1) e do `GlobalMemoryStatusEx` (ITEM-3) isolado com
  `// SAFETY:` por bloco; superfície dos crates `#![forbid(unsafe_code)]` onde possível (segue `ramshared-vram`).
- [ ] **Segredos/ponteiros:** nenhuma credencial; a telemetria (VRAM/pressão) não carrega endereço nem PII.

## Arquivos a CRIAR

### `crates/ramshared-nvml/src/lib.rs` (+ `Cargo.toml`, `src/ffi.rs`)
- **Propósito:** orçamento de VRAM portável via dlopen do NVML (Win/Linux); superfície segura sobre FFI isolado.
- **Requisitos cobertos:** RF-W1, RF-W2, DT-2, DT-7.
- **Structs/Types:**
  ```rust
  /// Orçamento de VRAM de um device (bytes). Espelha o par do CUDA mem_info (free,total).
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct VramBudget { pub free: u64, pub used: u64, pub total: u64 }

  /// Erro do NVML (mapeado do nvmlReturn_t + falha de dlopen). Sem panic em produção.
  #[derive(Debug)]
  pub enum NvmlError { Load(String), Symbol(String), Api { code: i32, op: &'static str } }

  pub struct Nvml { /* handle da lib + ponteiros de função (privados) */ }
  pub struct NvmlDevice<'a> { /* handle nvmlDevice_t + &Nvml */ }
  ```
- **Funções (assinaturas exatas):**
  - `pub fn init() -> Result<Nvml, NvmlError>` — dlopen (`nvml.dll` / `libnvidia-ml.so.1`), resolve símbolos, `nvmlInit_v2`.
  - `pub fn device(&self, ordinal: u32) -> Result<NvmlDevice<'_>, NvmlError>` — `nvmlDeviceGetHandleByIndex_v2`.
  - `pub fn mem_info(&self) -> Result<VramBudget, NvmlError>` (em `NvmlDevice`) — `nvmlDeviceGetMemoryInfo` (`nvmlMemory_t{total,free,used}`).
  - `impl Drop for Nvml` → `nvmlShutdown` (RAII, ordem inversa — idiom `goto out_err`).
- **Dependências externas:** libc p/ `dlopen`/`dlsym` no Linux; `windows-sys` (`LoadLibraryW`/`GetProcAddress`) no Windows (cfg-gated). Nenhum crate NVML (DT-2).
- **Padrão de referência:** `crates/ramshared-cuda/src/driver.rs` (dlopen + símbolos + `// SAFETY:`) e `ffi.rs`.
- **Testes requeridos:** `#[ignore]` `nvml_budget_matches_cuda` (RTX 2060, Linux): `mem_info().free` ≈ `cuMemGetInfo` ± 1 página; `init_fails_graceful_without_lib` (VramBudget=None quando dlopen falha — DT-7).
- **Disciplina Kahneman:** ITEM-1 → ver mapa (#1 WYSIATI).

### `crates/ramshared-config/src/lib.rs` (+ `Cargo.toml`)
- **Propósito:** schema TOML tipado + loader; superset das flags do `ramsharedd` e do agente (RF-P3).
- **Requisitos cobertos:** RF-P3, DT-6.
- **Structs/Types (serde):**
  ```rust
  #[derive(Debug, Default, serde::Deserialize)]
  pub struct HostConfig {
      #[serde(default)] pub daemon: DaemonSection,   // superset das 12 flags de wsl2d/main.rs
      #[serde(default)] pub agent: AgentSection,      // broker, tenant, swap_prio, transport, watchdog
      #[serde(default)] pub arbiter: ArbiterSection,  // tick, tol_frac, streak (espelha ArbiterConfig)
  }
  // DaemonSection { size_mb, sock, nbd, transport, queue_depth, backend, slices, slice_mb,
  //                 listen_nbd, arbiter_listen, advertise_nbd, telemetry_jsonl } — todos Option.
  ```
- **Funções:** `pub fn load(path: &Path) -> Result<HostConfig, ConfigError>` (lê+`toml::from_str`);
  `pub fn merge_over_cli(...)` **não** — a precedência é CLI>TOML>default aplicada no chamador (DT-6).
- **Dependências externas:** `toml` (MIT/Apache-2.0 — registrar em LIBRARIES.md + deny.toml), `serde`.
- **Padrão de referência:** structs `BrokerConfig`/`ArbiterConfig`/`ResidencyConfig` existentes (mesmos campos).
- **Testes requeridos:** `parse_full_toml`, `absent_sections_default`, `cli_overrides_toml` (unit, sem GPU/rede).

### `crates/ramshared-agent/src/client.rs`  *(ITEM-4 — extração DT-3)*
- **Propósito:** núcleo de cliente do broker reutilizável: connect + `Register` + reconexão/backoff +
  watchdog + writer único, genérico sobre `trait AgentRole` (telemetria + handler de `Msg`).
- **Requisitos cobertos:** RF-W1, DT-3.
- **Types:** `pub trait AgentRole { fn transport(&self) -> TransportKind; fn heartbeat(&mut self) -> Msg; fn on_msg(&mut self, m: Msg) -> Vec<Outbound>; }` + `pub fn run_session<R: AgentRole>(cfg, role) -> ...` (move o loop de `main.rs:256-340` p/ cá, inalterado).
- **Padrão de referência:** o próprio `crates/ramshared-agent/src/main.rs:241-340` (código a mover, não reescrever).
- **Testes requeridos:** manter os testes atuais do agente verdes; `role_dispatch` unit (fake role).
- **Disciplina Kahneman:** DT-3 → ver mapa (#14 — abort se drill Linux regride).

### `crates/ramshared-agent/src/win_mem.rs`  *(ITEM-3 — cfg(windows))*
- **Propósito:** `WinMemPressure` via `GlobalMemoryStatusEx` (DT-1).
- **Types:** `pub struct WinMemPressure { pub avail: u64, pub total: u64, pub load_pct: u32, pub commit_avail: u64, pub commit_total: u64 }` + `pub fn sample() -> WinMemPressure`.
- **Dependências:** `windows-sys` (`GlobalMemoryStatusEx`, `MEMORYSTATUSEX`), cfg-gated. `unsafe` isolado.
- **Testes requeridos:** só compila/roda em Windows (test plan §Drivers). No Linux: `#[cfg(windows)]` exclui.

### `crates/ramshared-agent/src/local.rs`  *(ITEM-6 — RF-W3, DT-4)*
- **Propósito:** protocolo local addon↔agente + listener loopback.
- **Types (serde JSON-lines, tag="type"):**
  ```rust
  #[derive(serde::Deserialize)] #[serde(tag="type", rename_all="snake_case")]
  pub enum LocalMsg { LeaseAcquire { bytes: u64 }, LeaseDrop, VramQuery }
  #[derive(serde::Serialize)] #[serde(tag="type", rename_all="snake_case")]
  pub enum LocalReply { LeaseResult { granted: bool, lease: Option<u32>, bytes: u64 },
                        Vram { free: u64, used: u64, total: u64 }, Unavailable { reason: String } }
  ```
- **Funções:** `pub fn serve_local(addr: SocketAddr /*127.0.0.1*/, bridge) -> ...` — bind loopback (RNF-2), 1 linha/req.
- **Padrão de referência:** codec `write_msg`/`read_msg` (`protocol.rs:132/144`) reusado (mesmo teto 64 KiB).
- **Testes requeridos:** `local_roundtrip` (serde), `bind_rejects_non_loopback`.

### `crates/ramshared-agent/src/bin/ramshared-agent-win.rs`  *(ITEM-5 — RF-W1)*
- **Propósito:** binário do agente DccAgent Windows: `AgentRole` com `transport()=DccAgent`, heartbeat =
  `Msg::Psi{ sample: <neutro>, swaps: vec![], mem: None }` + telemetria de VRAM/pressão via canal próprio;
  hospeda o listener local (ITEM-6) e a bridge de lease (ITEM-7) + counterfactual (DT-11).
- **Requisitos cobertos:** RF-W1, RF-W3, DT-3, DT-11.
- **Dependências internas:** `ramshared-nvml`, `ramshared-config`, `ramshared-broker`, `client.rs`, `local.rs`.
- **Testes requeridos:** bridge `LocalMsg::LeaseAcquire`→`Msg::LeaseRequest` (unit, fake broker); counterfactual `<50% por 5 min ⇒ LeaseRelease` (unit com relógio injetado).

### `addons/ramshared_blender/__init__.py` (+ `predict.py`, `ooc.py`, `monitor.py`, `bridge.py`)
- **Propósito:** addon Blender MVP (RF-W2): predição cabe/não-cabe (footprint vs `VramBudget`), ativação
  do out-of-core nativo do Cycles, monitor de spill, ponte de lease via o agente local (`LocalMsg`).
- **Requisitos cobertos:** RF-W2, RF-W3, DT-7, DT-10.
- **Funções-chave:** `predict.fits(scene) -> (bool, footprint_bytes)`; `ooc.enable_host_fallback()`;
  `monitor.sample() -> (vram_used, ram_used)`; `bridge.lease_acquire(bytes)`/`lease_drop()` (fala com 127.0.0.1 do agente).
- **Padrão de referência:** `docs/dcc-out-of-core/PRD.md` (RF-3, fonte do MVP).
- **Testes requeridos:** teste de predição com fixtures (footprint conhecido); manual no Blender do Alex (gate).

## Arquivos a MODIFICAR

### `crates/ramshared-broker/src/protocol.rs`  *(ITEM-9a — RF-W3, DT-5)*
- **O que muda:** adicionar `TransportKind::DccAgent` (aditivo). **Antes:** enum `TransportKind` sem DccAgent.
  **Depois:** `+ DccAgent`. **Por quê:** o agente Windows registra como DccAgent (holder de lease, não swap).
  **Impacto:** aditivo, retrocompat serde; roundtrip tests do enum ganham 1 caso. Nenhuma mudança de wire nas variantes existentes.

### `crates/ramshared-broker/src/arbiter.rs`  *(ITEM-9b — RF-W3, DT-5)*
- **O que muda:** no `tick` (`:164-217`), **excluir tenants `DccAgent` da atribuição round-robin/rebalance**
  de slices de swap; **manter** a lógica de lease (RevokeForLease/GrantLease) que já responde a qualquer holder.
  **Por quê:** DccAgent não consome swap (DT-5). **Impacto:** control-plane puro; novo teste
  `dccagent_nao_recebe_swap_mas_pode_lease`. Data-plane intocado (RNF-4).
- **Disciplina Kahneman:** ITEM-9 → ver mapa (#13 Ilusão de validade).

### `crates/ramshared-agent/src/main.rs`  *(ITEM-2b, ITEM-4 — RF-P3, DT-3, DT-6)*
- **O que muda:** (a) refatorar p/ usar `client::run_session` (DT-3, código movido, comportamento idêntico);
  (b) carregar `ramshared-config` se `--config PATH` dado, aplicando CLI>TOML>default. **Impacto:** os testes
  atuais + drill `qemu-broker-drill.sh` devem passar inalterados (é o gate anti-regressão do DT-3).

### `crates/ramshared-wsl2d/src/main.rs`  *(ITEM-2b — RF-P3, DT-6)*
- **O que muda:** aceitar `--config PATH`; as 12 flags (`:220-326`) viram override do TOML (precedência DT-6).
  **Antes:** parsing só de flags. **Depois:** default → TOML (se `--config`) → flags. **Impacto:** nenhuma
  flag removida (RNF-4); novo teste de precedência.

### `crates/ramshared-agent/Cargo.toml` / `crates/ramshared-wsl2d/Cargo.toml` / `Cargo.toml` (workspace)
- **O que muda:** workspace `members += ["crates/ramshared-nvml", "crates/ramshared-config"]`; agente ganha
  `[[bin]] ramshared-agent-win`, deps `ramshared-nvml`/`ramshared-config`/`windows-service` (cfg windows)/
  `windows-sys` (cfg windows); daemon ganha dep `ramshared-config`. **Impacto:** novas crates herdam
  `publish = false` (padrão do workspace, deny.toml).

## Arquivos a DELETAR

Nenhum — Day-0 aditivo (o P2 acrescenta consumidor + config; não remove caminhos do P1).

## Observabilidade

- **Agente Windows:** log estruturado de `VramBudget`/`WinMemPressure` (1 amostra/período) + eventos de
  lease (`acquire`/`granted`/`denied`/`release`/`auto-release`). Reusa o padrão de telemetria JSONL do broker.
- **Addon:** monitor de spill (pico VRAM/RAM no render) — número antes/depois (critério de aceitação §13.1 do PRD).
- **Broker:** o `StatusReply` já expõe leases via estado de slice (`Leased`); sem métrica nova no broker.

## Contratos e documentação viva

| Documento | Atualização | Motivo |
| --- | --- | --- |
| `docs/memory-broker-p2-windows/IMPL.md` | criar por passo | rastreabilidade SSDV3 (após gate do Alex) |
| `docs/memory-broker/PRD.md` | marcar P2 detalhada aqui | doc-pai aponta p/ este SPEC |
| `docs/LIBRARIES.md` | registrar `toml`, `windows`/`windows-sys`/`windows-service`, decisão NVML-dlopen | regra dura #1 + #11 (nova dep com critério/alternativa/quando-revisitar) |
| `deny.toml` | conferir allow-list de licenças (windows-*/toml = MIT/Apache-2.0) | gate de supply-chain (P1) cobre P2 |
| `README.md`/`ARCHITECTURE.md` | seção lado-Windows + TOML | superfície nova de produto |
| `MEMORY.md` | entrada por fase | memória de sessão |

## Ordem de implementação

Cada passo compila/valida isolado (PRD §10). **Só inicia após o gate do Alex (Anexo B).**

1. **ITEM-1** `ramshared-nvml` (dlopen) + teste `#[ignore]` na GPU. Destrava predição.
2. **ITEM-2** `ramshared-config` (schema+loader) — puro, validável já. **ITEM-2b** loaders no daemon+agente (flags override).
3. **ITEM-9** `TransportKind::DccAgent` + exclusão no árbitro (control-plane puro, testável já).
4. **ITEM-4** extração do `client.rs` (DT-3) — gate: drill Linux verde.
5. **ITEM-6** `local.rs` (LocalMsg + listener loopback) + **ITEM-7** bridge lease→`Msg`.
6. **ITEM-3/ITEM-5** `win_mem.rs` + bin `ramshared-agent-win` (valida em host Windows).
7. **ITEM-8** addon Blender (contra cenas reais do Alex).
8. **ITEM-10** instalável Windows (serviço + addon + CLI) por último.

## Plano de testes

- **Puro/Backend (roda aqui, Linux/WSL2, sem GPU):** `ramshared-config` (parse/precedência); `arbiter.rs`
  DccAgent (não recebe swap, pode lease); `local.rs` roundtrip + bind-loopback; bridge LocalMsg→Msg;
  counterfactual DT-11 com relógio injetado. **Sem regressão:** `cargo test --workspace`, drills qemu
  broker+ublk (RNF-4).
- **Drivers-GPU (`#[ignore]`, RTX 2060 Linux):** `ramshared-nvml` `mem_info` ≈ CUDA `cuMemGetInfo`.
- **Windows (host do Alex, gate):** `ramshared-agent-win` sobe como serviço, reporta VRAM/pressão;
  `win_mem.rs` real; addon prediz+ativa out-of-core; **cena que falhava renderiza** (critério §13.1);
  lease revoga/devolve swap num host dev com tier co-localizado (§13.2); grant < 1 s (RNF-1); broker
  morto ⇒ degrade (§13.4).

## Checklist de validação

- [ ] `cargo fmt --all -- --check` limpo
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` limpo (novas crates incluídas)
- [ ] `cargo test --workspace` verde (novos testes puros + sem regressão nos 205 atuais)
- [ ] `cargo audit` + `cargo deny check` verdes com as novas deps (`toml`, `windows-*`) — gate P1
- [ ] Drills `qemu-broker-drill.sh` + `qemu-ublk-daemon.sh` PASS (RNF-4)
- [ ] Teste `#[ignore]` NVML×CUDA na RTX 2060 (ITEM-1)
- [ ] **Gate cognitivo:** cada ITEM cita RF/DT no commit; etapas críticas com bloco Kahneman respondido
- [ ] **Gate de IMPL (Anexo B):** inputs do Alex presentes antes de iniciar a IMPL (anti-halo #11)
