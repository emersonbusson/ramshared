# PRD — Coletor de Telemetria & Reconciliação do Memory Broker

> **Feature-slug:** `broker-telemetry-reconciliation`. SSDV3 PASSO 1.
> **Camada:** Userspace (daemon Rust `ramsharedd` + agente `ramshared-agent`) + protocolo do broker.
> Não toca kernel-core, uAPI de kernel, IRQ nem DMA (a VRAM já é servida pelo data-plane existente).
> **Liga-se a** [`.claude/rules/benchmarks.md`](../../.claude/rules/benchmarks.md) (saída em
> `docs/benchmarks/results.jsonl`) e ao gate P0 do SSDV3.

## Resumo

**O que é.** Um coletor leve que amostra **3 fontes** e as **reconcilia** numa linha unificada por
amostra, para (a) dar observabilidade real à arbitragem de VRAM cross-tenant — o **único fosso**
defensável do RamShared (os fornecedores não arbitram VRAM ociosa entre inquilinos heterogêneos) — e
(b) **detectar quando um consumidor externo (o WDDM/gráficos do Windows) está espremendo a VRAM
compartilhada** ou quando a contabilidade do broker diverge (slice presa, consumidor não-contabilizado).

**Problema que resolve.** Hoje o broker sabe quais *slices* atribuiu, mas **não conta bytes/IO
servidos** e só emite `eprintln` solto — não há como provar que `Σ slices (broker) ≈ Σ SwapUsed
(tenants) ≈ Δ VRAM atribuível ao daemon`. Sem essa reconciliação, não dá pra (1) confiar nos números
do gate P0/benchmark, (2) distinguir "o Windows retomou a VRAM" de "uma slice ficou presa", nem (3)
afirmar que a arbitragem funciona.

**Valor (RamShared).** Telemetria é o que torna o fosso (arbitragem revogável de VRAM ociosa)
**verificável**: o invariante de reconciliação é, ele próprio, um *counterfactual* contínuo (divergência
= algo errado), e o canário de residência já é o detector de "alguém externo espremendo a VRAM".

## Contexto técnico

**Módulos/papéis:**
- `ramshared-broker` (`crates/ramshared-broker/`): protocolo + ledger de slices/árbitro. **Fonte da
  verdade** (ele aloca e serve → conta exato).
- `ramshared-wsl2d` (`crates/ramshared-wsl2d/src/broker_srv.rs`, `main.rs`): roda o broker + o
  worker CUDA (canário) + o contexto de VRAM. É **onde o coletor mora** (vê as 3 fontes).
- `ramshared-agent` (`crates/ramshared-agent/`): tenant remoto; reporta pressão/swap ao broker.
- `ramshared-cuda` (`crates/ramshared-cuda/src/driver.rs`): `mem_info()` (cuMemGetInfo).

**Estado atual a reutilizar/estender:**
- **Confirmado no codebase** — protocolo já tem `Msg::Status` / `Msg::StatusReply { tenants:
  Vec<TenantStatus>, slices: Vec<Slice>, last_rebalance_secs }` (`ramshared-broker/src/protocol.rs:46,69`),
  handler em `broker_srv.rs:214` (`status_reply()` :473–490). **Reusar como o RPC de leitura do ledger.**
- **Confirmado no codebase** — ledger: `Slice { id, offset, len, tenant, state }` + `enum SliceState
  { Free, Active, Draining, Leased }` (`ramshared-broker/src/slices.rs:29`); `TenantState { name,
  transport, present, sid, psi, reconciled }` (`broker_srv.rs:57`).
- **Confirmado no codebase** — agente já lê `/proc/pressure/memory` (`agent/src/psi.rs:15`,
  `read_psi`) e `/proc/swaps` (`psi.rs:44`, `read_swaps` → `SwapEntry { dev, prio, size_kb, used_kb }`)
  e envia `Msg::Psi { sample, swaps }` a cada **1 s** (`agent/src/main.rs:27` `PSI_PERIOD`, loop :273).
- **Confirmado no codebase** — VRAM: `Context::mem_info() -> (free, total)` bytes (cuMemGetInfo,
  `cuda/src/driver.rs:189`).
- **Confirmado no codebase** — canário: `Canary::sample(latency_us, content_ok, free_bytes) -> Verdict`
  + `enum DemoteReason { Latency, Corruption, FreeFloor }` (`wsl2d/src/residency.rs:33`); contador
  observável `ServerHandleDt3VramResidency::demote_count()` (atômico, `ublk_server.rs:446`); veredito
  via canal `demote_tx` → `CoreEvent::Demote` (`broker_srv.rs:640,656`).
- **Confirmado no codebase** — logging é `eprintln!` textual via `Outbound::Log(s)` (`broker_srv.rs:808`);
  **sem** CSV/JSON/`tracing`/métricas. A infra `Outbound::Log` (vetor de ações) é **reaproveitável**
  para emitir uma linha estruturada.

**Confirmado na documentação oficial:**
- VRAM per-PID confiável e o lado **DXGI** exigem rodar **no host Windows**; dentro do WSL2 o GPU-PV
  não expõe isso (ver [`docs/BENCHMARKS.md`](../BENCHMARKS.md) e a análise de fornecedores: DXGI
  `QueryVideoMemoryInfo` LOCAL/NON_LOCAL é a API nativa de orçamento no host).
- `cuMemGetInfo` dá apenas free/total do *device* (não atribui por processo).

**Sendo proposto (Inferência):**
- Contadores de **bytes/IO servidos por slice** no broker (não existem hoje).
- Leitura de `memory.swap.current` (cgroup v2) e `/proc/diskstats` (page-io) no agente.
- Emissão de **linha unificada reconciliada** (JSONL) + flag de divergência.
- Crate `ramshared-nvml` (host) para atribuição per-PID — **fora do MVP** (ver §Fora de escopo).

## Opção recomendada

**O broker é o coletor.** Ele já recebe a telemetria dos tenants (via `Msg::Psi`), é dono do ledger e
do contexto de VRAM, e hospeda o canário — então é o ponto natural onde as 3 fontes se encontram, com
**escritor único** (DT-27, sem corrida). O MVP atribui "VRAM de outros" por **subtração**
(`vram_outros = vram_total_used − vram_alloc_daemon`), sem NVML per-PID — coarse, mas suficiente para
detectar espremedura externa (corroborada pelo canário).

Concretamente:
1. Estender o ledger com contadores de IO/bytes por slice (RF-1).
2. Estender o telemetry do tenant (`Msg::Psi`) com cgroup-swap + diskstats (RF-2).
3. O broker amostra `mem_info()` e calcula a atribuição por subtração (RF-3).
4. O broker reconcilia o invariante e levanta flag de divergência (RF-4).
5. O broker emite 1 linha JSONL por amostra em `docs/benchmarks/results.jsonl` (RF-5), reusando a
   infra `Outbound::Log`.

**Alternativas descartadas:**
- **Coletor standalone** que re-poll tudo por fora: duplica ledger + contexto VRAM + canário que o
  broker já tem → viola Day-0 (caminho paralelo). Descartado.
- **NVML/DXGI per-PID dentro do WSL2:** não-confiável por GPU-PV. Descartado p/ o MVP (host = futuro).
- **Endpoint de métricas Prometheus/exporter:** overkill p/ o MVP; o `StatusReply` (pull) + a linha
  JSONL (append) bastam. Pode virar futuro, sem bloquear.

**Trade-offs aceitos:** atribuição de "outros" por subtração (não per-PID) no MVP; coletor roda no
lado WSL2 (host DXGI/NVML adiado); 1 Hz de cadência (igual ao heartbeat PSI atual).

## Requisitos funcionais

- **RF-1 — Contabilidade de IO/bytes por slice (fonte da verdade).** Adicionar `bytes_served: u64`
  e `io_count: u64` por slice (e agregação por tenant), incrementados no caminho de serve do worker
  (atômico, sem lock no hot path), expostos no `StatusReply`.
  - **Critério de aceite:** após injetar carga conhecida (fio N×4 KiB), `Σ bytes_served` do tenant
    bate com os bytes do fio dentro de ±2%; `io_count` bate com o nº de ops ±1%.
  - **Isolamento:** contadores pertencem ao broker (escritor único, DT-27); leitura via `Status` (RPC),
    sem expor a outros tenants além do próprio agregado.
- **RF-2 — Telemetria estendida do tenant.** O agente passa a ler, além de `/proc/swaps`,
  **`memory.swap.current`** (cgroup v2 do escopo de swap) e **`/proc/diskstats`** (derivar `page_io/s`
  do device de swap), e carrega no `Msg::Psi` (campo novo `mem` opcional; degrade-graceful se ausente).
  - **Critério de aceite:** o broker recebe `swap_used` (cgroup) e `page_io/s` por tenant; quando o
    cgroup/diskstats não existe, o campo vem `None` e o coletor usa `/proc/swaps` (sem quebrar).
  - **Isolamento:** leitura read-only de `/proc` + cgroup do próprio tenant; nenhum acesso cross-tenant.
- **RF-3 — Atribuição de VRAM por subtração (host).** O broker amostra `mem_info()` →
  `vram_total_used = total − free`; calcula `vram_alloc_daemon = Σ slice.len (Active|Draining|Leased) +
  região-canário`; deriva `vram_outros = max(0, vram_total_used − vram_alloc_daemon)`.
  - **Critério de aceite:** com o daemon servindo K MiB, `vram_alloc_daemon ≈ K` (±1 página);
    `vram_outros` sobe de forma observável quando um app gráfico do Windows é aberto.
  - **Isolamento:** `mem_info` é do device (não per-PID); a subtração não vaza identidade de processo.
- **RF-4 — Invariante de reconciliação + flag de divergência.** Por amostra, computar
  `Σ slices(broker) ≈ Σ SwapUsed(tenants) ≈ vram_alloc_daemon`. Se `|divergência| > tol` por `streak`
  amostras, levantar `flag ∈ { eviction, stuck_slice, unaccounted }`, desambiguada pelo canário:
  `demotes↑` ⇒ `eviction`; slice `Draining` há muito ⇒ `stuck_slice`; `vram_outros` cresce sem
  `Status` correspondente ⇒ `unaccounted`.
  - **Critério de aceite:** uma slice presa sintética (não zera) → `stuck_slice`; pressão gráfica
    sintética no host → `eviction` (com `demotes ≥ 1`); convergência normal → `flag = none` (sem
    falso-positivo na janela idle medida no P0).
  - **Isolamento:** decisão puramente de leitura; **não** altera o estado do árbitro (observador, não ator).
- **RF-5 — Linha unificada reconciliada (saída íntegra).** Emitir 1 registro JSON por amostra em
  `docs/benchmarks/results.jsonl` (e resumo humano em `docs/BENCHMARKS.md` quando for benchmark),
  conforme `.claude/rules/benchmarks.md`. Campos: `t, tenant, slice, swap_used, page_io_s,
  vram_alloc_daemon, vram_total_used, vram_outros, canario_demotes, demote_reason, reconcile_delta,
  flag, branch, commit`.
  - **Critério de aceite:** cada linha é JSON válido, parseável, e `reconcile_delta`/`flag` batem com o
    `StatusReply` do mesmo instante; append-only (nunca reescreve).
  - **Isolamento:** arquivo local; sem PII (ver RNF-LGPD).

## Requisitos não-funcionais

- **Performance:** amostragem a **1 Hz** (reusa o `PSI_PERIOD` de 1 s, `agent/main.rs:27`). O hot path
  de serve só ganha **2 incrementos atômicos relaxed** por op (RF-1) → custo desprezível vs os 241 µs
  de serve medidos (P0 §3); meta: overhead < 1% na latência p50 de serve (validar com o canário).
- **Segurança:** sem nova superfície de ataque exposta; o `Status` já existe no protocolo (rede
  privada só, RNF-2 do PRD-pai). Leituras de `/proc`/cgroup são read-only do próprio tenant. Sem
  segredos na linha de telemetria.
- **Observabilidade:** **este é o feature de observabilidade.** Saída dupla (JSONL máquina + MD humano)
  + o flag de divergência. Sem dependência de Prometheus no MVP (pull via `Status` + append JSONL).
- **Escalabilidade:** linear em nº de tenants (o broker já itera tenants no `StatusReply`); a linha
  JSONL é O(slices+tenants) por amostra. Cadência fixa 1 Hz → volume previsível (~1 linha/tenant/s).
- **LGPD:** **sem dados pessoais.** A telemetria é de infraestrutura (bytes, IO, VRAM, PSI). Sem
  per-PID no MVP (a atribuição é por subtração, anônima). Retenção = o arquivo de benchmark (rotação
  manual). Se RF futuro adicionar per-PID/usuário no host, reavaliar.
- **Resiliência:** se uma fonte falha, **degrade-graceful** (campo `None`, flag `partial`), nunca
  aborta o broker. `Status` indisponível (agente caiu) → tenant marcado ausente (reusa `present`).
  cuMemGetInfo falhando → `vram_*` = `None` (sem mascarar como 0). **Nunca** introduzir thrash/pressão
  no host vivo (regra de benchmarks + risco de freeze do WSL2).

## Fluxos

**Happy path**
1. Agente (tenant) lê PSI + `/proc/swaps` + `memory.swap.current` + `/proc/diskstats` (RF-2) e envia
   `Msg::Psi { sample, swaps, mem }` ao broker (1 Hz) — `agent/src/main.rs` loop.
2. Broker atualiza `TenantState`/ledger; o worker incrementa `bytes_served/io_count` por slice no serve
   (RF-1).
3. Tick do coletor (1 Hz) no broker: lê o próprio ledger, soma `Σ slices` e `Σ SwapUsed`, amostra
   `mem_info()` (RF-3), lê `demote_count()`.
4. Reconcilia o invariante (RF-4) → `reconcile_delta` + `flag`.
5. Emite a linha JSONL (RF-5) via `Outbound::Log` (dispatcher estruturado).

**Fluxos alternativos**
- **Sem tier co-localizado / 0 tenants ativos:** o coletor ainda registra `vram_total_used`/`vram_outros`
  (útil para o ângulo "quanta VRAM ociosa existe", BENCHMARKS Q1a).
- **`Status` sob demanda:** uma ferramenta externa manda `Msg::Status` e recebe `StatusReply` (snapshot
  pontual) sem depender do append contínuo.

**Fluxos de erro**
| Condição (trigger) | Resultado ao cliente | Log/level + campos | Impacto na consistência |
|---|---|---|---|
| Agente sem cgroup/diskstats | campo `mem=None` no `Psi` | `warn` `tenant`, usa `/proc/swaps` | nenhum (degrade) |
| `cuMemGetInfo` falha | `vram_*=None`, `flag=partial` | `warn` `op=cuMemGetInfo` | reconciliação parcial |
| Divergência > tol por streak | `flag ∈ {eviction,stuck_slice,unaccounted}` | `error` `delta`, `reason` | sinal (não corrompe estado) |
| `Status` enquanto agente caiu | tenant `present=false` | `info` | reconciliação ignora o tenant |

## Modelo de dados

Estruturas em memória (sem banco). **Estende** o existente:
- `Slice` (**alterada**, `ramshared-broker/src/slices.rs`): `+ bytes_served: u64, + io_count: u64`
  (incrementados no serve; zerados no `Free`). Sem mudança de alinhamento de ABI de kernel (é struct
  Rust interna, serializada via serde no `StatusReply`).
- `TenantStatus` (**alterada**, no `StatusReply`): `+ swap_used_cgroup: Option<u64>, + page_io_s:
  Option<u64>, + bytes_served: u64`.
- `Msg::Psi` (**alterada**): `+ mem: Option<TenantMem>` onde `TenantMem { swap_current: u64,
  diskstats_io: u64 }` (campo novo, retrocompat por `Option` — JSON-lines tolera ausência).
- `TelemetrySample` (**nova**, serde→JSONL): os campos do RF-5. Ciclo de vida: criada por tick,
  serializada, descartada (sem estado retido além do arquivo).
- **Regiões de memória:** nenhuma nova alocação de VRAM/DMA; os contadores são `u64` no heap do broker.
  `vram_alloc_daemon` deriva do ledger (Σ `slice.len`) + a região-canário (`CANARY_BYTES`).

## API / Interfaces

**Não há uAPI de kernel nova** (ioctl/sysfs/debugfs/IRQ/DMA) — é daemon userspace. A "API" é o
**protocolo TCP JSON-lines do `ramshared-broker`** (já existente) + a **saída JSONL**.

| Campo | Valor |
|---|---|
| Operação | RPC pull `Msg::Status` → `Msg::StatusReply` (**existente, estendido**) + stream append JSONL |
| Caminho | TCP (rede privada, `--arbiter-listen`); arquivo `docs/benchmarks/results.jsonl` |
| Permissões | rede privada só (RNF-2 do PRD-pai); arquivo local 0644 |
| Rate limit | cadência fixa 1 Hz (sem amplificação) |
| Idempotência | `Status` é idempotente (read-only); a linha JSONL é append (cada amostra única por `t`) |

**`StatusReply` (estendido) — exemplo:**
```json
{ "type": "status_reply",
  "tenants": [ { "tenant_id": 1, "name": "civm", "present": true,
                 "swap_used_cgroup": 4194304, "page_io_s": 512, "bytes_served": 268435456 } ],
  "slices": [ { "id": 0, "offset": 0, "len": 134217728, "tenant": 1, "state": "active",
                "bytes_served": 134217728, "io_count": 32768 } ],
  "last_rebalance_secs": 12 }
```

**Linha de telemetria (JSONL, RF-5) — exemplo:**
```json
{ "t": 1718500000, "tenant": "civm", "slice": 0, "swap_used": 4194304, "page_io_s": 512,
  "vram_alloc_daemon": 134217728, "vram_total_used": 1517445120, "vram_outros": 1383227392,
  "canario_demotes": 0, "demote_reason": null, "reconcile_delta": 0.004, "flag": "none",
  "branch": "feat/p1-hardening", "commit": "1fba443" }
```

**Erros (protocolo):** reusa `Msg::Error { reason }` (existente). Sem novos códigos de erro de kernel.

**Impacto em ABI:** **nenhum** layout de uAPI de kernel. Mudanças são em structs Rust serializadas por
serde (JSON) — retrocompatíveis por `#[serde(default)]`/`Option` (agente velho ↔ broker novo tolera
campos ausentes; **Day-0:** mas sem produção viva, atualizamos os dois no mesmo release).

**Interrupções/Workqueues:** N/A (userspace).

## Dependências e riscos

**Pré-requisitos:** broker P1 (pronto), canário de residência (pronto), `mem_info` (pronto).

| Risco | Mitigação |
|---|---|
| **Calibração da tolerância** de reconciliação (ruído vs sinal) *(Inferência)* | medir a divergência natural no P0 (janela idle + carga); definir `tol` + `streak` (como o árbitro: histerese). Kahneman #3 (número) no SPEC. |
| Contadores no **hot path** de serve | `AtomicU64` relaxed, sem lock; validar overhead < 1% via canário (RNF perf) |
| `memory.swap.current`/cgroup path **varia** por distro/tenant | detectar o path do cgroup v2; `Option` + degrade p/ `/proc/swaps` |
| **NVML/DXGI ausente** (per-PID) | MVP por subtração (RF-3); per-PID = crate `ramshared-nvml` no host (fora de escopo) |
| **GPU-PV** impede telemetria de host dentro do WSL2 | documentar; o coletor roda no broker (WSL2) com cuMemGetInfo; DXGI/per-PID = host (futuro) |
| Volume de JSONL cresce | append-only + rotação manual (RNF escalabilidade); 1 linha/tenant/s |

**Breaking changes:** nenhum p/ kernel/uAPI. Os campos novos de protocolo são aditivos (`Option`).
**Rollout:** atrás do P1 já mergeado; ativável por flag (ex.: `--telemetry-jsonl <path>`).
**Rollback:** o coletor é *overlay* observador (não muda o árbitro) → desligar a flag reverte sem efeito
no data-plane. **Rollback trigger:** se os contadores de RF-1 degradarem a latência de serve > 2× o
baseline (P0) → reverter RF-1 (`git revert`).
**Disciplinas Kahneman prováveis no SPEC:** #3 (números da tolerância), #1 (registrar estado/carga nas
amostras — WYSIATI), #5 (o flag `eviction` é a detecção de pior-caso externo).

## Estratégia de implementação

Fatias (cada uma compila + testável isolada):
1. **RF-1** contadores por slice + expor no `StatusReply` (+ teste: fio injeta carga, `Status` bate).
2. **RF-5** emissor JSONL (reusa `Outbound::Log`) com os campos já disponíveis (sem cgroup/diskstats
   ainda) — valida o pipeline de saída cedo.
3. **RF-3** subtração de VRAM no broker (cuMemGetInfo + Σ ledger) — valida com smoke VRAM no host.
4. **RF-2** telemetria estendida do tenant (cgroup + diskstats) no agente + campo `Msg::Psi`.
5. **RF-4** invariante + flag de divergência — por último (depende de 1–4); calibrar `tol`/`streak`
   com números do P0.

**Validável cedo:** RF-1 + RF-5 (no host vivo, sem pressão). **Exige ambiente:** o flag `eviction`
ponta-a-ponta (RF-4) precisa de carga gráfica real no host (host vivo, bounded) ou da sessão civm.
**Sem migração/backfill** (Day-0; sem produção viva).

## Fora de escopo

- **`ramshared-nvml` + DXGI per-PID** (atribuição de "outros" por processo, no host Windows): o MVP usa
  subtração (RF-3). Motivo: GPU-PV não expõe per-PID confiável no WSL2; é um subsistema próprio (PRD à
  parte, alinhado à P2/P3).
- **Exporter Prometheus / dashboard:** o MVP entrega JSONL + `Status`. Motivo: evitar dependência;
  pode vir depois sem bloquear.
- **Atuar sobre a divergência** (auto-corrigir slice presa, etc.): o coletor é **observador**. A ação
  (revogar/re-arrendar) já é do árbitro/canário. Motivo: separar observação de controle (isolamento RF-4).
- **Persistência/séries temporais em DB:** arquivo JSONL basta no MVP. Motivo: Day-0 simples.
- **Telemetria de render/DCC (P2):** depende do Alex + do addon; outro PRD.
