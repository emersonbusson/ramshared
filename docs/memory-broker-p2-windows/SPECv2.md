# SPEC — RamShared P2: Ponte Windows + MVP DCC (Blender)

> Versão melhorada após auditoria do Passo 2.5.
> Baseline preservado: `SPEC.md`.
> Motivo: o `SPEC.md` recebeu **no-go** — 3 findings CRITICAL + 3 HIGH assumiam estruturas de código
> inexistentes. Corrigidos aqui (candidato ativo para nova auditoria):
> - **C1** (DT-5): exclusão de DccAgent estava em `arbiter.rs` (que é transport-agnóstico); movida p/
>   `broker_srv::on_tick` (constrói `present` a partir de `TenantState`, que **tem** `transport`).
> - **C2** (ITEM-9a): `TransportKind` está em `model.rs:48` (não `protocol.rs`) e `endpoint_for`
>   (`broker_srv.rs:183-195`) é `match` exaustivo → adicionar variante **quebra a compilação** até tratar
>   `DccAgent`; agora listado.
> - **C3** (DT-3): "mover o loop inalterado" era falso (`Outbound` é do crate `wsl2d`, indisponível ao
>   agente; o loop é acoplado a swap) → reescrito como **refactor com preservação de comportamento**;
>   trait `AgentRole` devolve `Vec<Msg>` + hooks `poll_outbound`/`on_teardown`.
> - **H1** (DT-11): NVML `GetMemoryInfo` é device-wide → símbolo-set estendido c/
>   `nvmlDeviceGetComputeRunningProcesses` (uso por-PID); fórmula do counterfactual fechada.
> - **H2**: gating de cross-compile fixado (deps Windows sob `[target.'cfg(windows)']`; bin com `main`
>   stub em `not(windows)`) → `cargo test --workspace` continua verde no host Linux.
> - **H3** (DT-4): `write_msg`/`read_msg` são **monomórficos em `Msg`** → `local.rs` implementa codec
>   próprio (reimplementa o teto de 64 KiB), não "reusa".
>
> **SSDV3 PASSO 2**, a partir de `PRD.md`. Userspace (Rust + Python), sem uAPI de kernel nova.
> **Gate de IMPL (anti-halo #11):** a IMPL não inicia sem os inputs do Alex (PRD Anexo B).

## Escopo fechado desta implementação

**Entra agora:** `ramshared-nvml` (FFI dlopen, RF-W1/W2); `ramshared-config` (TOML, RF-P3); agente
**DccAgent** Windows (`ramshared-agent-win`, RF-W1); ponte de lease addon↔agente↔broker (RF-W3); addon
Blender MVP (RF-W2); instalável Windows (RF-P1).

**Fora agora:** RF-W4 (interposer → P4); RF-G3 (D3D12 → P3); Windows-consumidor-de-swap (→ P4);
reescrever Cycles; auth/cripto própria (rede privada só); **multi-lease** (broker é 1-lease-por-vez,
`broker_srv.rs:403` — DT-8).

**Dependências assumidas prontas (Confirmado no codebase, verificado na auditoria):**
- Protocolo JSON-lines/TCP: `enum Msg` (`crates/ramshared-broker/src/protocol.rs:19`), `write_msg`/`read_msg`
  (`protocol.rs:132`/`:144`, **monomórficos em `Msg`** — ver DT-4), `MAX_LINE_BYTES=64KiB` (`:14`),
  `PROTO_VERSION=1` (`:12`).
- **Lease end-to-end**: `LeaseRequest`/`Release`/`Granted`/`Denied` (`protocol.rs:42,45,64,68`); árbitro
  `Action::RevokeForLease`/`GrantLease` (`arbiter.rs:74,80`), bloco de lease no tick (`arbiter.rs:165-217`);
  servidor `on_lease_request`/`on_lease_release` (`broker_srv.rs:391,427`), 1-lease (`:403`), capacidade
  (`:412`), aplicação (`:628-664`), auto-release no disconnect (`:456-464`); `SliceMap::lease/unlease`
  (`slices.rs:89,99`).
- Cliente do broker: `crates/ramshared-agent/src/main.rs` (Register `:264-271`, backoff `:33-38`, watchdog
  `:274,340`, loop `session()` `:278-347` — **acoplado a swap**, ver DT-3).
- Padrão dlopen: `crates/ramshared-cuda/src/driver.rs:110,189` (`cuMemGetInfo_v2`).

## Matriz de rastreabilidade PRD → SPEC

| PRD | Implementação no SPEC |
| --- | --- |
| RF-W1 | ITEM-1 (nvml), ITEM-3 (win_mem), ITEM-4 (client core), ITEM-5 (bin) — DT-1, DT-2, DT-3, DT-12, DT-13 |
| RF-W2 | ITEM-8 (addon) — DT-7, DT-10 |
| RF-W3 | ITEM-6 (`LocalMsg`+listener), ITEM-7 (bridge→`Msg`), ITEM-9 (DccAgent no `on_tick`) — DT-4, DT-5, DT-10, DT-11 |
| RF-P1 | ITEM-10 (packaging+serviço) — DT-9, DT-12 |
| RF-P3 | ITEM-2 (`ramshared-config`), ITEM-2b (loaders) — DT-6 |

## Decisões técnicas

| # | Decisão | Justificativa |
| --- | --- | --- |
| DT-1 | Pressão de memória Windows = `GlobalMemoryStatusEx` (`MEMORYSTATUSEX`). Rejeita PDH/perfmon. | 1 syscall, sem counter-path frágil; cobre disponível/carga/commit. |
| DT-2 | NVML via dlopen hand-rolled (`ramshared-nvml`); rejeita `nvml-wrapper`. Símbolos: `nvmlInit_v2`, `nvmlDeviceGetHandleByIndex_v2`, `nvmlDeviceGetMemoryInfo`, **`nvmlDeviceGetComputeRunningProcesses_v3`** (uso por-PID, p/ DT-11), `nvmlShutdown`. | Day-0/zero-dep (LIBRARIES.md #11, mesmo critério do `clap`); reusa o padrão `ramshared-cuda` (dlopen, `// SAFETY:`). O símbolo de RunningProcesses foi adicionado p/ o counterfactual (H1). |
| DT-3 | **Refactor com preservação de comportamento** (NÃO "mover verbatim"): extrair p/ `ramshared-agent/src/client.rs` só o que é genérico — socket + `Register` + backoff + watchdog + writer. O trait `AgentRole` devolve **`Vec<Msg>`** (tipo do `ramshared-broker`, disponível ao agente — **nunca** `Outbound`, que é do crate `wsl2d`, `broker_srv.rs:58`). Exec/cleanup ficam **dentro do role**, expostos ao core por `poll_outbound()` + `on_teardown()`. | O loop `session()` (`main.rs:278-347`) é acoplado a swap (`res_rx` `:305-322`, `active` `:273`, `ExecCmd` `:331`, `detach_swap` `:350-354`, `exec_loop` `:430-458`). Não é movível inalterado (C3). O role Linux mantém suas threads/canais; o role Windows (DccAgent) não tem exec. RNF-4 + #14: drills Linux verdes = gate anti-regressão. |
| DT-4 | `LocalMsg`/`LocalReply` próprios (localhost JSON-lines) + **codec próprio** em `local.rs` (reimplementa o teto 64 KiB). **Não** reusa `write_msg`/`read_msg` (são **monomórficos em `Msg`**, `protocol.rs:132,144`, H3). | O `Msg` do broker não tem query de `VramBudget` (justifica protocolo local, confirmado na auditoria); e o codec do broker não é genérico. Broker `protocol.rs` **intocado** (RNF-4). |
| DT-5 | Exclusão de DccAgent do swap acontece em **`broker_srv::on_tick`** (`:573-584`): ao construir `present: Vec<TenantView>` a partir de `self.tenants`, **filtrar** tenants com `transport == DccAgent`. `arbiter.rs` fica **inalterado** (é transport-agnóstico: `TenantView` só tem `id/psi/slices`, `arbiter.rs:49-54`). O lease do DccAgent segue funcionando: o holder chega por `self.pending_lease` (de `self.sessions`, `:421`), **independente** de `present`. | C1: o árbitro não conhece transport; `TenantState` (`broker_srv.rs:72`) tem `transport`. Filtrar em `on_tick` exclui o DccAgent do round-robin (`arbiter.rs:281-287`) e do rebalance (`:249-278`) sem tocar o árbitro nem o bloco de lease (`:165-217`). Teste = **`BrokerCore`** (não "arbiter puro"). |
| DT-6 | Config TOML = `ramshared-config` (dep `toml`); precedência **CLI > TOML > default**; flags viram override. | Superset das flags (daemon `wsl2d/main.rs:220-326` + agente `agent/main.rs:89`); `toml` (MIT/Apache-2.0) maduro. RNF-4: flags-override preserva drills. |
| DT-7 | NVML degrade-graceful (RNF-6): dlopen/símbolo/GPU ausente → `VramBudget=None`; agente segue com `WinMemPressure`; addon usa heurística conservadora (assume "não cabe" acima de X% de `ullTotalPhys`). | Pior caso #5: host sem NVIDIA não trava o addon; RF-W2 (out-of-core) não depende de NVML. |
| DT-8 | Lease 1-ativo-por-vez (P1, `broker_srv.rs:403`). Multi-lease fora de escopo. | MVP 1 artista/host; multi-lease exigiria fila/prioridade no árbitro. |
| DT-9 | Serviço Windows via crate `windows-service` (SCM) + `windows-sys` (`GlobalMemoryStatusEx`). Rejeita hand-roll de SCM. | SCM correto é fácil de errar em FFI cru; crate padrão MIT/Apache-2.0. |
| DT-10 | Addon fala **só** com o agente local (nunca direto com o broker). | Identidade de tenant + 1-lease-por-tenant vivem no agente; o agente é dono único da sessão do broker + NVML (DT-11 precisa das duas). |
| DT-11 | Auto-release por counterfactual **no agente**: usa `nvmlDeviceGetComputeRunningProcesses` (DT-2) p/ somar `usedGpuMemory` **do(s) PID(s) do render**; se `render_used < 0.5 * lease.bytes` por **5 min** ⇒ agente envia `LeaseRelease`. | H1: `GetMemoryInfo` é device-wide (ambíguo com swap co-localizado); por-PID mede o uso real do render. #2 counterfactual (revogar swap p/ render que não usa VRAM é perda). |
| DT-12 | **Cross-compile:** deps Windows (`windows-sys`, `windows-service`) sob `[target.'cfg(windows)'.dependencies]`; `win_mem.rs` é `#[cfg(windows)]`; o bin `ramshared-agent-win` tem `#[cfg(windows)] fn main` **e** `#[cfg(not(windows))] fn main` stub (`eprintln!` + `exit(2)`). | H2: mantém `cargo test --workspace` verde no host Linux (o bin compila como stub; deps Windows não são puxadas fora do Windows). |
| DT-13 | Heartbeat do DccAgent ao broker = keepalive `Msg::Psi{ sample: PsiSample::default(), swaps: vec![], mem: None }`. O `VramBudget` é servido **localmente** ao addon (`LocalMsg::VramQuery`), **não** empurrado ao broker (o broker não modela VRAM por-agente; o gauge dele vem do worker do daemon). | M1: fecha o "`<neutro>`" (é `PsiSample::default()`); a arbitragem ignora esse PSI porque o DccAgent é excluído do swap (DT-5). Evita inventar campo de VRAM no `Msg` (RNF-4). |

## Fronteira de atomicidade e política de rollback

**Atômico (reusado do broker):** concessão/revogação de lease serializada no árbitro single-thread e no
servidor (1 lease por vez, DT-8); `LeaseGranted` só após slices `Leased`/drenadas (`broker_srv.rs:638-664`).
**Eventual:** predição do addon (footprint vs `VramBudget`) é snapshot → margem (DT-7) + monitor +
degrade (RNF-1); counterfactual (DT-11) é decisão de janela (5 min), não transacional.
**Estados parciais aceitos:** NVML ausente → `VramBudget=None` (DT-7); broker morto → timeout → degrade.

**Rollback:** *app* — cada ITEM compila isolado; `git revert` do ITEM reverte a superfície (reverter
ITEM-9 tira a exclusão de DccAgent do `on_tick` sem afetar swap; reverter ITEM-4 volta o agente Linux
monolítico). Agente Windows e addon são produtos separados (não linkam o data-plane P1). *migration* —
N/A (TOML aditivo). *dados* — N/A. *forward-only* — N/A (Day-0, sem produção viva).

## Mapa Kahneman por etapa crítica

| Etapa / ITEM | Disciplina | Link | Pergunta obrigatória | Evidência mínima | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (NVML) | #1 WYSIATI + #3 Número | [`#1-wysiati--what-you-see-is-all-there-is`](../methodology/KAHNEMAN-DISCIPLINES.md#1-wysiati--what-you-see-is-all-there-is) | O `VramBudget` NVML bate com `cuMemGetInfo` no mesmo host? | Teste `#[ignore]` RTX 2060 (Linux `libnvidia-ml.so.1`): `nvml.free ≈ cuda.mem_info().0 ± 1 página` | divergência sistemática → binding errado |
| ITEM-9 (DccAgent no `on_tick`) | #13 Ilusão de validade | [`#13-ilusão-de-validade`](../methodology/KAHNEMAN-DISCIPLINES.md#13-ilusão-de-validade) | O filtro em `on_tick` exclui DccAgent do swap **mas** preserva o lease e o swap dos outros? | Teste **`BrokerCore`**: 1 DccAgent + 1 tenant swap → só o swap recebe `SwapOn`; lease do DccAgent revoga o swap; arbiter.rs sem diff | DccAgent recebe `SwapOn`, ou tenant swap deixa de receber, ou `arbiter.rs` mudou |
| ITEM-4 (extração client) | #14 Mass-Refactoring | [`#14-falácia-do-refatoramento-em-massa-mass-refactoring-fallacy`](../methodology/KAHNEMAN-DISCIPLINES.md#14-falácia-do-refatoramento-em-massa-mass-refactoring-fallacy) | A extração muda o comportamento do agente Linux? | `cargo test -p ramshared-agent` + drill `qemu-broker-drill.sh` verdes | qualquer regressão no teste/drill do agente Linux → reverter extração |
| ITEM-5/ITEM-7 (lease no hot path) | #5 Availability | [`#5-availability-heuristic`](../methodology/KAHNEMAN-DISCIPLINES.md#5-availability-heuristic) | `LeaseRequest`→`Granted` < 1 s? broker morto ⇒ degrade? | Medição local ≥3 rodadas (ms); teste de timeout → addon segue out-of-core | grant > 1 s ou timeout que trava o render |
| DT-11 (counterfactual) | #2 Counterfactual | [`#2-counterfactual-obrigatório`](../methodology/KAHNEMAN-DISCIPLINES.md#2-counterfactual-obrigatório) | `render_used` (por-PID) < 50% do lease por 5 min ⇒ release? | Log do agente com `nvmlDeviceGetComputeRunningProcesses` (usedGpuMemory do PID do render) | swap revogado + VRAM do render ociosa sem release |
| ITEM-3/ITEM-8 (predição) | #2 Counterfactual | [`#2-counterfactual-obrigatório`](../methodology/KAHNEMAN-DISCIPLINES.md#2-counterfactual-obrigatório) | Predição erra "cabe" quando não cabe? | Cena real (gate): footprint previsto vs pico do monitor; margem cobre o erro | falso "cabe" que estoura a VRAM |

## Checklist de segurança (pré-implementação)

- [ ] **Isolamento (RNF-2):** listener local (ITEM-6) bind **só 127.0.0.1**; cliente do broker só loopback/rede privada.
- [ ] **OOB/DoS:** `local.rs` reimplementa o teto de 64 KiB por linha (não reusa `read_msg`, DT-4/H3); FFI NVML lê structs de tamanho fixo.
- [ ] **Permissões:** agente como serviço sem elevação além de ler memória/NVML; addon no espaço do Blender; sem `CAP_SYS_ADMIN`.
- [ ] **Input validation:** `bytes` do lease revalidado no agente antes de encaminhar; broker já recusa `> total` (`:412`).
- [ ] **`unsafe`/FFI:** NVML (ITEM-1) e `GlobalMemoryStatusEx` (ITEM-3) isolados com `// SAFETY:`; superfície `#![forbid(unsafe_code)]`.
- [ ] **Segredos/ponteiros:** sem credencial; telemetria sem endereço/PII.

## Arquivos a CRIAR

### `crates/ramshared-nvml/src/lib.rs` (+ `Cargo.toml`, `src/ffi.rs`)  *(ITEM-1 — RF-W1/W2, DT-2, DT-7, DT-11)*
- **Structs:** `VramBudget { free:u64, used:u64, total:u64 }`; `RenderVram { pid:u32, used:u64 }`; `NvmlError { Load(String), Symbol(String), Api{code:i32, op:&'static str} }`; `Nvml`; `NvmlDevice<'a>`.
- **Funções:** `init() -> Result<Nvml,NvmlError>` (dlopen `nvml.dll`/`libnvidia-ml.so.1` + `nvmlInit_v2`); `device(u32)`; `NvmlDevice::mem_info() -> Result<VramBudget,_>` (`nvmlDeviceGetMemoryInfo`); `NvmlDevice::running_procs() -> Result<Vec<RenderVram>,_>` (`nvmlDeviceGetComputeRunningProcesses_v3`, DT-11); `impl Drop` → `nvmlShutdown`.
- **Deps:** `libc` (dlopen, Linux) / `windows-sys` (`LoadLibraryW`/`GetProcAddress`, sob `[target.'cfg(windows)']`). Nenhum crate NVML (DT-2).
- **Padrão:** `ramshared-cuda/src/driver.rs` (`// SAFETY:` por bloco). **Testes:** `#[ignore]` `nvml_budget_matches_cuda` (RTX 2060); `init_fails_graceful_without_lib` (DT-7).

### `crates/ramshared-config/src/lib.rs` (+ `Cargo.toml`)  *(ITEM-2 — RF-P3, DT-6)*
- **Structs (serde):** `HostConfig { daemon: DaemonSection, agent: AgentSection, arbiter: ArbiterSection }` (todos `#[serde(default)]`; campos `Option`, superset das flags).
- **Funções:** `load(&Path) -> Result<HostConfig, ConfigError>` (`toml::from_str`). Precedência CLI>TOML>default aplicada no chamador (DT-6).
- **Deps:** `toml` (registrar LIBRARIES.md + deny.toml), `serde`. **Testes:** `parse_full_toml`, `absent_sections_default`, `cli_overrides_toml`.

### `crates/ramshared-agent/src/client.rs`  *(ITEM-4 — RF-W1, DT-3)*
- **Trait:** `pub trait AgentRole { fn transport(&self) -> TransportKind; fn heartbeat(&mut self) -> Option<Msg>; fn on_msg(&mut self, m: &Msg) -> Vec<Msg>; fn poll_outbound(&mut self) -> Vec<Msg>; fn on_teardown(&mut self); }` — devolve `Vec<Msg>` (do `ramshared-broker`), **nunca `Outbound`**.
- **Função:** `pub fn run_session<R: AgentRole>(cfg: &ClientConfig, role: &mut R) -> io::Result<()>` — connect + `Register{proto,tenant,role.transport()}` + loop{ escreve `role.heartbeat()` se devido; escreve `role.poll_outbound()`; lê `Msg` (timeout) → escreve `role.on_msg(&m)`; watchdog } + `role.on_teardown()` no fim. Reconexão/backoff idênticos ao atual (`main.rs:33-38,241`).
- **Padrão:** o `session()` atual (`main.rs:278-347`) é **refatorado** (não copiado): o exec de swap sai p/ o role (`LinuxSwapRole` mantém `res_rx`/`exec_loop`/`detach_swap`). **Testes:** `role_dispatch` (fake role) + suite atual do agente verde.

### `crates/ramshared-agent/src/win_mem.rs`  *(ITEM-3 — RF-W1, DT-1; `#[cfg(windows)]`)*
- `WinMemPressure { avail, total, load_pct, commit_avail, commit_total }` + `sample() -> WinMemPressure` (`GlobalMemoryStatusEx`). `unsafe` isolado. **Só compila/roda no Windows** (DT-12).

### `crates/ramshared-agent/src/local.rs`  *(ITEM-6 — RF-W3, DT-4)*
- **Types:** `enum LocalMsg { LeaseAcquire{bytes:u64}, LeaseDrop, VramQuery }` (Deserialize); `enum LocalReply { LeaseResult{granted:bool, lease:Option<u32>, bytes:u64}, Vram{free:u64,used:u64,total:u64}, Unavailable{reason:String} }` (Serialize); ambos `#[serde(tag="type", rename_all="snake_case")]`.
- **Codec próprio:** `fn read_local<R:BufRead>(r) -> io::Result<Option<LocalMsg>>` + `fn write_local<W:Write>(w,&LocalReply)` — reimplementa o teto 64 KiB (H3; **não** reusa `write_msg`/`read_msg`, monomórficos em `Msg`).
- **Função:** `serve_local(addr /*127.0.0.1*/, bridge)` — bind loopback (RNF-2). **Testes:** `local_roundtrip`, `bind_rejects_non_loopback`, `read_local_caps_at_64k`.

### `crates/ramshared-agent/src/bin/ramshared-agent-win.rs`  *(ITEM-5 — RF-W1/W3, DT-3, DT-12, DT-13)*
- `#[cfg(windows)] fn main()`: role `WinDccRole` (`transport()=DccAgent`, `heartbeat()=Some(Msg::Psi{PsiSample::default(), vec![], None})` DT-13, `poll_outbound()`=leases pendentes do listener + auto-release DT-11, `on_msg` trata `LeaseGranted/Denied`); hospeda `local::serve_local` (ITEM-6) + bridge (ITEM-7) + NVML (ITEM-1).
- `#[cfg(not(windows))] fn main()`: stub `eprintln!("ramshared-agent-win: Windows-only"); exit(2)` (DT-12 — workspace verde no Linux).
- **Testes:** bridge `LocalMsg::LeaseAcquire`→`Msg::LeaseRequest` (unit, fake broker); counterfactual `<50% por 5 min ⇒ LeaseRelease` (relógio injetado).

### `addons/ramshared_blender/` (`__init__.py`, `predict.py`, `ooc.py`, `monitor.py`, `bridge.py`)  *(ITEM-8 — RF-W2, DT-7, DT-10)*
- `predict.fits(scene)->(bool, footprint_bytes)`; `ooc.enable_host_fallback()`; `monitor.sample()->(vram_used,ram_used)`; `bridge` fala `LocalMsg` com 127.0.0.1 do agente (DT-10). Fonte: `docs/dcc-out-of-core/PRD.md`. **Testes:** predição com fixtures; manual no Blender do Alex (gate).

## Arquivos a MODIFICAR

### `crates/ramshared-broker/src/model.rs`  *(ITEM-9a — RF-W3, DT-5) — C2*
- **O que muda:** `enum TransportKind` (`:47-51`) ganha `DccAgent` (aditivo). **Impacto:** aditivo no wire (serde), mas **quebra `match` exaustivo** em `endpoint_for` (ver abaixo) → tem de vir junto.

### `crates/ramshared-broker/src/arbiter.rs`
- **O que muda:** **NADA** (C1). Fica transport-agnóstico; a exclusão é no `broker_srv::on_tick`. Registrado aqui para deixar explícito que o abort trigger do ITEM-9 inclui "arbiter.rs sem diff".

### `crates/ramshared-wsl2d/src/broker_srv.rs`  *(ITEM-9 — RF-W3, DT-5) — C1, C2*
- **`endpoint_for` (`:183-195`):** adicionar braço `TransportKind::DccAgent => None` (DccAgent não tem endpoint NBD). **Antes:** `match` só `NbdUnix`/`NbdTcp`. **Depois:** `+ DccAgent => None`. **Por quê:** manter o `match` exaustivo compilando (C2).
- **`on_tick` (`:573-584`):** ao construir `present: Vec<TenantView>` (`:575-579`), **pular** tenants com `t.transport == TransportKind::DccAgent`. **Por quê:** exclui DccAgent do round-robin/rebalance sem tocar o árbitro (C1/DT-5). O lease segue via `pending_lease`.
- **Testes:** `BrokerCore`: `dccagent_nao_recebe_swap` (1 DccAgent + 1 swap → só o swap recebe `SwapOn`); `dccagent_pode_lease` (lease do DccAgent revoga o swap). **Kahneman:** ITEM-9 (#13).

### `crates/ramshared-agent/src/main.rs`  *(ITEM-2b, ITEM-4 — RF-P3, DT-3, DT-6)*
- Refatorar p/ `client::run_session` com `LinuxSwapRole` (exec de swap sai do loop p/ o role — DT-3, comportamento preservado, gate = drills). Carregar `ramshared-config` se `--config PATH` (CLI>TOML>default). **Impacto:** testes + `qemu-broker-drill.sh` inalterados (gate anti-regressão).

### `crates/ramshared-wsl2d/src/main.rs`  *(ITEM-2b — RF-P3, DT-6)*
- Aceitar `--config PATH`; 12 flags (`:220-326`) viram override do TOML. Nenhuma flag removida (RNF-4).

### `Cargo.toml` (workspace) / `crates/ramshared-agent/Cargo.toml` / `crates/ramshared-wsl2d/Cargo.toml`
- Workspace `members += ramshared-nvml, ramshared-config`. Agente: `[[bin]] ramshared-agent-win`, deps `ramshared-nvml`/`ramshared-config`; **`[target.'cfg(windows)'.dependencies]` `windows-sys`+`windows-service`** (DT-12). Daemon: dep `ramshared-config`. Novas crates herdam `publish=false`.

## Arquivos a DELETAR

Nenhum — Day-0 aditivo.

## Observabilidade

Agente: log de `VramBudget`/`WinMemPressure` + eventos de lease (acquire/granted/denied/release/auto-release).
Addon: monitor de spill (pico VRAM/RAM — critério §13.1 do PRD). Broker: leases via estado `Leased` (sem métrica nova).

## Contratos e documentação viva

| Documento | Atualização | Motivo |
| --- | --- | --- |
| `docs/memory-broker-p2-windows/IMPL.md` | criar por passo | rastreabilidade SSDV3 (após gate do Alex) |
| `docs/memory-broker/PRD.md` | marcar P2 detalhada aqui | doc-pai aponta p/ este SPEC |
| `docs/LIBRARIES.md` | `toml`, `windows-sys`/`windows-service`, NVML-dlopen | regra #1 + #11 |
| `deny.toml` | conferir licenças (windows-*/toml = MIT/Apache-2.0; passam na allow-list atual) | gate supply-chain P1 |
| `README.md`/`ARCHITECTURE.md` | lado Windows + TOML | superfície nova |
| `MEMORY.md` | entrada por fase | memória |

## Ordem de implementação

**Só após o gate do Alex (Anexo B).** 1) `ramshared-nvml` + teste `#[ignore]`. 2) `ramshared-config` + loaders (2b).
3) ITEM-9 `DccAgent` (`model.rs` + `endpoint_for` + `on_tick`, control-plane, testável já). 4) ITEM-4 extração
`client.rs` (gate: drill Linux verde). 5) ITEM-6 `local.rs` + ITEM-7 bridge. 6) ITEM-3/ITEM-5 `win_mem`+bin
(host Windows). 7) ITEM-8 addon (cenas do Alex). 8) ITEM-10 instalável.

## Plano de testes

- **Puro/Backend (roda aqui, Linux):** `ramshared-config` (parse/precedência); `BrokerCore` DccAgent
  (não recebe swap, pode lease; `arbiter.rs` sem diff); `local.rs` roundtrip + bind-loopback + cap 64 KiB;
  bridge LocalMsg→Msg; counterfactual DT-11 (relógio injetado). Sem regressão: `cargo test --workspace`
  (o bin Windows compila como stub, DT-12), drills qemu broker+ublk (RNF-4).
- **Drivers-GPU (`#[ignore]`, RTX 2060 Linux):** `ramshared-nvml` `mem_info` ≈ CUDA; `running_procs` lista o PID.
- **Windows (host do Alex, gate):** serviço reporta VRAM/pressão; `win_mem` real; addon prediz+ativa
  out-of-core; cena que falhava **renderiza** (§13.1); lease revoga/devolve swap (§13.2); grant < 1 s
  (RNF-1); broker morto ⇒ degrade (§13.4).

## Checklist de validação

- [ ] `cargo fmt --all -- --check` limpo
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` limpo (novas crates + bin stub)
- [ ] `cargo test --workspace` verde (novos testes puros + 205 atuais sem regressão; bin Windows = stub no Linux, DT-12)
- [ ] `cargo audit` + `cargo deny check` verdes com `toml`/`windows-*` (gate P1)
- [ ] Drills `qemu-broker-drill.sh` + `qemu-ublk-daemon.sh` PASS (RNF-4)
- [ ] Teste `#[ignore]` NVML×CUDA na RTX 2060 (ITEM-1)
- [ ] **`arbiter.rs` sem diff** (abort trigger do ITEM-9)
- [ ] Gate cognitivo: cada ITEM cita RF/DT; etapas críticas com bloco Kahneman respondido
- [ ] **Gate de IMPL (Anexo B):** inputs do Alex antes de iniciar a IMPL (#11)
