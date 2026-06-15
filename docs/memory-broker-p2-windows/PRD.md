# PRD — RamShared P2: Ponte Windows + MVP DCC (Blender)

> **Escopo:** este PRD detalha a fase **P2** do PRD unificado (`docs/memory-broker/PRD.md` §10):
> a ponte entre o host **Windows do artista** (sem WSL2) e o broker, mais o **addon Blender MVP**.
> Cobre **RF-W1, RF-W2, RF-W3** e as fatias de **RF-P1/RF-P3** que a P2 exige. O doc-pai agrupa
> esses RFs sob o título "Windows / DCC (Fase C)" no §4 e os chama de "P2" no §10 — é o **mesmo
> corpo de trabalho**; aqui padronizamos **P2**.
>
> **SSDV3:** P2 toca **uAPI/produto novo** (agente Windows nativo, protocolo de lease consumido por
> um cliente externo, packaging) → PRD é obrigatório (passo 1). Próximo passo após aprovação: SPEC.
> **Gate de IMPL:** inputs do Alex (Anexo B) — sem ≥1 cena real que falhava + erro exato, a IMPL
> não inicia.

## 1. Resumo

No host do artista (Windows, **sem WSL2**) o "cérebro" do RamShared roda como **serviço Windows
nativo** (agente) e o **addon Blender** fala com ele. O agente mede pressão de memória do SO + o
**orçamento de VRAM** (NVML); o addon prevê se a cena cabe na VRAM livre e, quando não cabe,
**configura o out-of-core nativo** do Cycles automaticamente (a RAM vira backing da VRAM) — o que
hoje o Alex faz à mão, perdendo dias. Quando há um **tier de swap** co-localizado consumindo VRAM
(host dev com WSL2), o addon pede um **lease revogável** ao broker antes do render (`LeaseRequest`)
e devolve no fim (`LeaseRelease`), reusando o **DEMOTE já construído** (P1). Entrega o produto
**instalável** do lado Windows (`.exe`/winget) + config **TOML única**.

Valor direto pro Alex = **RF-W2** (cena que falhava renderiza sem edição manual). Valor
arquitetural = **RF-W3** (o lease é o que une o tier-de-swap e o app DCC num único dono da VRAM,
Anexo A.4 do doc-pai).

## 2. Contexto técnico

- **Confirmado em docs (doc-pai §2):** oversubscription UVM por page-fault é **Linux-only**
  (`cudaMallocManaged`); no Windows o WDDM não faz demand paging → no Windows o caminho é o
  **out-of-core nativo do Cycles** (host memory fallback), não UVM.
- **Confirmado em docs (relato do tester, §2):** Alex (artista 3D, **Windows, sem WSL2**) perde
  **dias** ajustando cenas que estouram a VRAM. O cérebro precisa ser **serviço Windows nativo**.
- **Confirmado no codebase — protocolo de lease PRONTO** (`crates/ramshared-broker/src/protocol.rs`):
  `enum Msg` com `LeaseRequest{bytes:u64}` (L40), `LeaseRelease{lease:u32}` (L43),
  `LeaseGranted{lease:u32,bytes:u64}` (L62), `LeaseDenied{reason:String}` (L66). Wire = **JSON-lines
  UTF-8** (`#[serde(tag="type", rename_all="snake_case")]`) sobre **TCP**; `write_msg`/`read_msg`
  (L107/L119, teto 64 KiB anti-DoS). Servidor: `broker_srv.rs:331-365`, **1 lease ativo por vez**
  (P1).
- **Confirmado no codebase — NVML AUSENTE.** Não há crate/módulo NVML (única ocorrência de "NVML" é
  string de erro em `crates/ramshared-cli/src/main.rs:516`). O orçamento de VRAM hoje é via **CUDA
  Driver API** (`Context::mem_info()` → `cuMemGetInfo`, `crates/ramshared-cuda/src/driver.rs`),
  acessível só de dentro do Linux/WSL2. Um agente **Windows nativo** não pode usar isso → **precisa
  de NVML** (`nvml.dll`). → novo crate (§7/§8).
- **Confirmado no codebase — flags do daemon** (`crates/ramshared-wsl2d/src/main.rs:220-326`): 12
  flags (`--size --sock --force --nbd --transport --queue-depth --backend --slices --slice-mb
  --listen-nbd --arbiter-listen --advertise-nbd`) com validações. Mapeiam direto pra TOML (RF-P3).
- **Confirmado em docs — addon = `dcc-out-of-core/PRD.md`** (absorvido no doc-pai, 2026-06-09): o
  MVP do addon Blender (predição cabe/não-cabe via footprint vs VRAM, config automática, monitor de
  spill) é a fonte do RF-W2. A lacuna corrigida (Anexo A.2): o MVP isolado não falava com o broker →
  RF-W3 (lease bridge) une as frentes.
- **Confirmado no codebase — agente de referência:** `crates/ramshared-agent` (Linux) já é cliente
  do broker (watchdog + reconexão com backoff). O agente Windows reusa o **papel de cliente**;
  troca a telemetria (PSI→APIs de memória do Windows) e adiciona NVML.

## 3. Opção recomendada

**Protocol-first, cérebro como detalhe de deployment.** Reusar o protocolo de lease já implementado
e o DEMOTE da P1; o que a P2 acrescenta é **um cliente Windows** (agente nativo) + **orquestração
no Blender** (addon) + **um provedor de orçamento de VRAM portável** (NVML).

- **Agente Windows** (Rust, target `x86_64-pc-windows-msvc`): serviço do SO; telemetria de memória
  do Windows + NVML; cliente do broker (reuso do papel de `ramshared-agent`).
- **Addon Blender** (Python): lógica pura de Blender (predição + config do out-of-core nativo +
  monitor) + ponte de lease via o agente/broker local.
- **`ramshared-nvml`** (Rust FFI): orçamento de VRAM `(free,used,total)` portável (Win/Linux),
  reusado por P2 e P3.

**Rejeitado (fora da P2):** Windows-como-consumidor-de-swap (exigiria driver de disco Windows →
P4); interposer `nvcuda.dll` v2 (RF-W4, gated → P4); Vulkan/qualquer-GPU (RF-G2 → P3).

## 4. Requisitos funcionais (RF)

Rastreiam o doc-pai §4 (mesmos IDs).

- **RF-W1 — Agente Windows nativo.** Serviço Windows (Rust) que mede (a) **pressão de memória do
  SO** (working set / commit / disponível — API a fixar na SPEC: `GlobalMemoryStatusEx` /
  PDH/perfmon) e (b) **orçamento de VRAM** via NVML (`free/used/total`). Publica telemetria ao
  broker (reuso do papel de cliente do `ramshared-agent`). *(SO-pressure + NVML = **Inferência/a
  construir**; papel de cliente = Confirmado.)*
- **RF-W2 — Addon Blender MVP** *(fonte: `dcc-out-of-core/PRD.md` RF-3)*. Em Python: (a)
  **predição cabe/não-cabe** (footprint estimado da cena vs VRAM livre do NVML); (b) **ativar o
  out-of-core nativo** do Cycles automaticamente quando não cabe (host memory fallback); (c)
  proxies/mipmaps **não-destrutivos** opcionais; (d) **monitor de spill** (VRAM/RAM durante o
  render). Meta: a cena que falhava **renderiza sem edição manual**. *(Inferência/a construir;
  orquestra recurso nativo do Blender — sem reescrever Cycles.)*
- **RF-W3 — Ponte addon↔broker (lease).** No "vou renderizar": `LeaseRequest{bytes}` → broker
  **revoga** slices do tier de swap (DEMOTE da P1) → render com VRAM livre → `LeaseRelease{lease}`
  no fim → broker re-arrenda ao swap tier. Reusa `ramshared-broker::Msg` (Confirmado). Onde **não
  há** tier co-localizado, o grant é no-op (nada a demover), mas a ponte/protocolo são exercidos.
- **RF-P1 — Instalável (lado Windows).** `ramshared-setup.exe`/winget: instala o **serviço Windows**
  (agente) + o **addon Blender** + CLI. *(A `.deb`/systemd Linux é da plataforma; aqui só a fatia
  Windows.)* *(Inferência/a construir.)*
- **RF-P3 — Config TOML única.** Um TOML por host mapeia as 12 flags atuais do `ramsharedd`
  (Confirmado) + os campos do agente Windows (broker endpoint, tenant, política). Substitui flags
  espalhadas; um loader único. *(Mapa Confirmado; loader a construir.)*

**Fora desta fase:** RF-W4 (interposer v2 → P4), RF-G1/G2/G3 (trait/Vulkan/D3D12 → P3),
RF-P2 (fallback de transporte — já há NBD; refino é da plataforma).

## 5. Requisitos não-funcionais (RNF)

- **RNF-1 (hot path do render):** o lease **não** pode adicionar latência perceptível ao início do
  render. `LeaseRequest`→`LeaseGranted` deve resolver em **< 1 s** no caminho local (medir; doc-pai
  RNF). Falha/timeout do broker ⇒ o addon **degrada pro out-of-core nativo** (render prossegue).
- **RNF-2 (rede privada só):** binds do broker/agente só em loopback/rede privada (igual P1); sem
  auth/cripto própria (fora de escopo). NVML é leitura local (sem rede).
- **RNF-3 (anti-flap do lease):** histerese — não pedir/devolver lease em rajada; respeitar o
  counterfactual §14 (uso <50% ⇒ devolve).
- **RNF-4 (zero regressão):** os smokes da Fase B (ublk single-tenant, NBD Unix) continuam verdes;
  a P2 **não** toca o data-plane do tier de swap.
- **RNF-5 (Day-0):** sem shims; cada superfície (agente, addon, TOML) entregue na forma definitiva.
- **RNF-6 (portabilidade do NVML):** `ramshared-nvml` carrega `nvml.dll` (Win) / `libnvidia-ml.so.1`
  (Linux) por dlopen, **falha-graciosa** se ausente (degrada: sem orçamento NVML ⇒ heurística
  conservadora no addon).

## 6. Fluxos

1. **Render do artista (host Windows puro, sem tier de swap):** addon estima footprint → NVML diz
   VRAM livre → **não cabe** → addon ativa out-of-core nativo + (opcional) proxies → render conclui
   usando RAM como backing → monitor registra pico VRAM/RAM. *(LeaseRequest é no-op: nada a demover.)*
2. **Render com tier de swap co-localizado (host dev):** addon → `LeaseRequest{bytes}` → broker
   `DemoteAll`/revoga slices (libera VRAM) → render com VRAM inteira → `LeaseRelease` → broker
   re-arrenda. *(Caminho que prova RF-W3 ponta-a-ponta.)*
3. **Broker indisponível no render:** `LeaseRequest` timeout (RNF-1) → addon **degrada** pro
   out-of-core nativo → render prossegue (sem o ganho do lease, mas sem travar).
4. **Counterfactual do lease (§14):** agente vê (NVML) uso de VRAM do solicitante **<50%** do lease
   por 5 min → sinaliza devolução ao swap tier (o render não precisava de tudo).

## 7. Modelo de dados

Estende o doc-pai §7 (`Tenant{transport: …|DccAgent}`, `Lease{holder,bytes,revocable}`):

- **`DccAgent`** = novo `transport` de tenant (o host Windows do artista). O broker já modela
  `Lease{holder,bytes,revocable}` — o holder passa a poder ser um `DccAgent`.
- **`VramBudget{free,used,total}`** (do `ramshared-nvml`): amostra de orçamento de VRAM; entra na
  telemetria do agente (RF-W1) e na predição do addon (RF-W2).
- **`WinMemPressure{avail,commit,working_set}`** (a fixar na SPEC): equivalente Windows do PSI;
  alimenta o agente. *(Inferência: campos exatos dependem da API escolhida.)*
- **Config TOML** (RF-P3): `[host]`, `[broker] listen/advertise`, `[[tenant]]`, `[[slice]]`,
  `[arbiter]`, `[agent] broker, tenant, swap_prio` — superset tipado das 12 flags atuais.

## 8. API / Interfaces

- **`ramshared-nvml`** (crate novo): `fn init() -> Result<Nvml>`, `Nvml::device(ordinal)`,
  `Device::mem_info() -> VramBudget{free,used,total}`. FFI dlopen p/ `nvml.dll`/`libnvidia-ml.so.1`;
  `#![forbid(unsafe_code)]` na superfície segura, `unsafe` isolado no binding (com `// SAFETY:`).
- **Agente Windows** (`ramshared-agent` ou `ramshared-agent-win`): CLI/serviço `--broker IP:PORT
  --tenant NAME` (igual doc-pai §8) + leitura TOML (RF-P3). Reusa `ramshared-broker::{Msg, write_msg,
  read_msg}`.
- **Addon Blender** (Python): fala com o **agente/broker local** por TCP JSON-lines (mesmo wire do
  `ramshared-broker`) ou via um shim do agente; envia `LeaseRequest`/`LeaseRelease`; lê `VramBudget`
  do agente p/ a predição.
- **TOML loader** (RF-P3): no `ramsharedd` e no agente; **substitui** o parsing de flags atual
  (flags continuam como override). **Nenhuma uAPI de kernel nova** (ublk/NBD existentes).

## 9. Dependências e riscos

Rastreiam o doc-pai §9 onde aplicável.

| # | Risco | Mitigação |
|---|---|---|
| P2-R1 | **NVML ausente** — a base de orçamento não existe (Confirmado) | crate `ramshared-nvml` é o **1º item da IMPL**; dlopen com falha-graciosa (RNF-6) |
| P2-R2 | Out-of-core nativo do Cycles **insuficiente** p/ a cena do Alex (geometria/OptiX) | **gate P0 com cenas reais** (Anexo B) decide MVP-basta vs v2; honesto se não destravar |
| P2-R3 | Pressão de memória do Windows sem equivalente direto do PSI | SPEC fixa a API (`GlobalMemoryStatusEx`/PDH); heurística conservadora se ruidosa |
| P2-R4 | Lease no **hot path** do render adiciona latência (Kahneman) | RNF-1 (<1 s + degrade); medir; counterfactual §14 |
| P2-R5 | Empacotar serviço Windows + addon (assinatura, UAC, paths do Blender) | RF-P1 com winget; testar em host limpo; documentar |
| P2-R6 | Disponibilidade/representatividade da cena do Alex | doc-pai R9 (confirmada); Anexo B coleta; gate exige ≥1 cena |
| P2-R7 | Predição de footprint imprecisa (falso "cabe") | medir vs NVML real no monitor; margem de segurança; degrade seguro |

## 10. Estratégia de implementação

**Pré-requisito (gate):** inputs do Alex (Anexo B) — ≥1 `.blend` que falhava + erro exato + medição
P0 (VRAM/RAM no render). Sem isso, a IMPL **não inicia** (anti-halo, §14).

Ordem (cada passo testável de forma independente):
1. **`ramshared-nvml`** (crate FFI) + teste de orçamento num host com GPU. Destrava RF-W1/RF-W2.
2. **RF-P3 TOML** (loader no `ramsharedd` + agente; flags viram override) — refactor verde,
   validável já (sem deps externas).
3. **RF-W1 agente Windows** (telemetria SO + NVML + cliente do broker). Valida contra o broker P1.
4. **RF-W2 addon Blender MVP** (predição + out-of-core + monitor) — contra cenas do Alex.
5. **RF-W3 lease bridge** (addon → agente → broker) — ponta-a-ponta num host com tier de swap.
6. **RF-P1 instalável Windows** (serviço + addon + CLI) por último.

Disciplina: cada passo cita seu RF-ID nos commits; SSDV3 IMPL.md por fase.

## 11. Documentos a atualizar

`docs/memory-broker-p2-windows/SPEC.md` (**próximo passo** deste PRD); `IMPL.md` por passo;
`docs/memory-broker/PRD.md` (marcar P2 detalhado aqui); `README`/`ARCHITECTURE` (lado Windows +
TOML); `docs/LIBRARIES.md` (nova dep NVML); `MEMORY.md`.

## 12. Fora de escopo

Windows-como-consumidor-de-swap (driver de disco → P4); interposer `nvcuda.dll` v2 (RF-W4 → P4);
Vulkan/D3D12/qualquer-GPU (RF-G* → P3); reescrever Cycles; modelo de negócio/licença do addon;
auth/cripto própria (rede privada só); render distribuído; competir com RAM em latência (PCIe manda).

## 13. Critérios de aceitação (P2)

1. **RF-W2/Gate do doc-pai §13.2:** ≥1 cena real do Alex que **falhava** por VRAM **renderiza sem
   edição manual** (números antes/depois do monitor — falhou → concluiu).
2. **RF-W3:** num host com tier de swap, o `LeaseRequest` **revoga** o swap (DEMOTE observado) e o
   `LeaseRelease` **devolve** (re-arrendado) — com logs.
3. **RF-W1:** o agente Windows reporta `VramBudget` (NVML) + pressão do SO ao broker, estável.
4. **RNF-1:** `LeaseRequest`→`LeaseGranted` < 1 s local; timeout do broker ⇒ degrade pro out-of-core
   (render prossegue).
5. **RNF-4:** smokes da Fase B continuam verdes.
6. **RF-P1/P3:** instalável Windows sobe o serviço + addon; um TOML único configura o host.

## 14. Validação (Kahneman)

- **Número, não adjetivo (#3):** "renderiza" = medição antes/depois (cena falhava → conclui), com
  VRAM/RAM do monitor; "rápido" = `LeaseRequest`→grant em ms medidos.
- **Counterfactual do lease (#2):** revogar swap p/ um render que **não usa** a VRAM é perda.
  **Trigger:** uso de VRAM do solicitante (NVML) **<50%** do lease por 5 min ⇒ devolve ao swap tier
  (logado). Liga RF-W1 (telemetria) a RF-W3 (decisão).
- **Anti-halo (#11):** P2 só inicia com o gate de P0/Alex; o sucesso da Fase B **não** "aprova" a
  P2 — ela prova com números próprios (cena real).
- **Pior caso / disponibilidade (#5):** broker indisponível no render é o caso de falha esperado →
  degrade pro out-of-core nativo é **requisito** (RNF-1), não opcional.
- **Reuso antes de criação (regra dura #1):** lease/DEMOTE/`ramshared-broker` reusados (Confirmado);
  o novo é só NVML + agente Windows + addon. Sem reescrever Cycles nem o data-plane.

---

## Anexo B — Inputs do tester (Alex) = gate de IMPL

Herdado do doc-pai (Anexo B). Sem estes, a IMPL da P2 não inicia:

1. SO/versão (Win 10/11), GPU (modelo/VRAM), RAM total.
2. Versão do Blender + backend (OptiX/CUDA — screenshot de Preferences → System se incerto).
3. **1-2 `.blend`** (ou descrição: nº objetos, texturas+resoluções) que **falharam** por VRAM + a
   **mensagem de erro exata**.
4. O que tentou à mão (reduzir texturas, decimate, tiles, passes) e quanto tempo perdeu.
5. Topa rodar o script de medição (lê VRAM/RAM no render; **não altera a cena**)?
