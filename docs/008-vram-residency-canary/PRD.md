---
slug: 008-vram-residency-canary
title: Canário de residência dedicado (§9.4) — gatilhos de conteúdo e free-floor
milestone: —
issues: [8]
---
# PRD — Issue #8 — Canário de residência dedicado (§9.4)

## 1. Resumo

O daemon do tier VRAM (`ramshared-wsl2d`) detecta eviction WDDM por um **canário de
residência** (SPECv3 §9). Hoje a fiação inline alimenta a decisão só com a **latência
do `serve()`** — `sample(lat, content_ok=true, free=u64::MAX)` — deixando os gatilhos
de **conteúdo** (`Demote(Corruption)`) e **free-floor** (`Demote(FreeFloor)`) como
**código morto**. _(Confirmado no codebase: `crates/ramshared-wsl2d/src/main.rs`.)_

Esta feature implementa o **canário dedicado** previsto na SPECv3 §9.4: uma
região-canário separada na VRAM + uma sonda periódica que produz os três sinais reais
(latência de round-trip fixo, integridade de conteúdo, free do `cuMemGetInfo`). Valor:
a segurança do tier frio deixa de repousar num único proxy de latência e ganha uma
**guarda de integridade** explícita antes do kernel ler swap potencialmente corrompido.

## 2. Contexto técnico

- **Módulo alvo:** `crates/ramshared-wsl2d` (daemon). Reusa `crates/ramshared-cuda`
  (I/O VRAM) e a decisão pura de `residency.rs`. _(Confirmado no codebase.)_
- **Estado atual a reutilizar:**
  - `residency.rs`: `Canary::new(cfg, baseline_us)`, `sample(latency_us, content_ok, free_bytes) -> Verdict`, `ResidencyConfig { latency_mult: 8, consecutive: 3, free_floor_bytes: 0 }`, `DemoteReason { Latency, Corruption, FreeFloor }`. **A decisão já trata os 3 gatilhos e tem 5 testes.** _(Confirmado no codebase.)_
  - `driver.rs`: `Context::alloc(bytes) -> DeviceMem`, `DeviceMem::write_at/read_at/zero`, `Context::mem_info() -> (free, total)`. _(Confirmado no codebase.)_
  - `main.rs`: serve loop com `canary: Option<Canary>`, `baseline`, `demoted`, `demote_rx` (DEMOTE confirmado por canal, re-arma se falhar). _(Confirmado no codebase.)_
- **Escopo de memória:** VRAM via CUDA Driver API (`cuMemAlloc`/`cuMemcpy*`), userspace; sem kernel-space. CUDA é **thread-local** — tudo roda na thread do serve loop. _(Confirmado no codebase: doc do `Context`.)_
- **Evidência de plataforma:** eviction WDDM é **data-safe / latency-unsafe** (4K → 1,18 s). _(Confirmado em docs: `FASE0-FINAL.md`, SPECv3 §9.5.)_
- **Confirmado em docs:** SPECv3 §9.3 (3 gatilhos), §9.4 (canário dedicado — previsto, não implementado).
- **Proposto:** região-canário como `DeviceMem` separado; sonda em cadência por contagem de requests; `free_floor_bytes` default não-zero.

## 3. Opção recomendada

**Sonda inline com região-canário dedicada**, amostrando a cada `CANARY_EVERY` requests
dentro do serve loop. Cada amostra: escreve um padrão-sentinela (derivado de um
contador) na região-canário, relê, compara (`content_ok`); cronometra esse round-trip
de tamanho fixo (`latency_us`); chama `mem_info()` (`free_bytes`). Alimenta o `Canary`
existente; qualquer `Verdict::Demote(_)` aciona o caminho de DEMOTE já existente.

- **Motivo:** reusa 100% da decisão (`residency.rs`) e do I/O (`ramshared-cuda`);
  zero `unsafe` novo; latência de tamanho fixo é sinal mais limpo que a do `serve()`
  (que varia com o tamanho do request); ativa os 3 gatilhos.
- **Alternativas descartadas:**
  - *Thread amostradora dedicada* — CUDA é thread-local; exigiria `cuCtxSetCurrent`
    (acopla a H1, multi-thread). Fica fora (ver §12).
  - *Canário no fim da região de swap* — obrigaria reduzir o tamanho anunciado e
    arriscaria sobrepor dado de swap. Região separada é mais limpa (Day-0).
  - *Manter só latência do serve* — é o estado atual; não dá guarda de integridade.
- **Trade-offs aceitos:** sem tráfego de swap (idle) a sonda não roda — aceitável,
  pois sem swap ativo não há página em risco (detecção em idle = H1, futuro).

## 4. Requisitos funcionais

- **RF-1 — Região-canário isolada.** Aloca um `DeviceMem` separado de `CANARY_BYTES`,
  independente da região de swap. **Aceite:** o tamanho NBD anunciado continua = região
  de swap; a região-canário não é endereçável por requests. **Isolamento:** região
  distinta na VRAM; sem exposição a userspace além do já existente.
- **RF-2 — Gatilho de conteúdo.** A sonda escreve sentinela, relê e compara;
  divergência → `content_ok=false` → `Demote(Corruption)`. **Aceite:** teste com leitura
  divergente injetada produz `Verdict::Demote(DemoteReason::Corruption)`.
- **RF-3 — Gatilho de latência dedicado.** A latência alimentada ao `Canary` passa a
  ser o round-trip de tamanho fixo da sonda (não a do `serve()`). **Aceite:** o baseline
  e as amostras vêm da sonda; sob spike, `Demote(Latency)` após `consecutive` amostras.
- **RF-4 — Gatilho de free-floor.** A sonda lê `mem_info().free` e alimenta o `Canary`;
  `free_floor_bytes` passa a ter default > 0. **Aceite:** `free < free_floor_bytes` →
  `Demote(FreeFloor)`.
- **RF-5 — Cadência amortizada.** A sonda roda a cada `CANARY_EVERY` requests de I/O.
  **Aceite:** overhead por request amortizado ≤ ~1 round-trip 4K a cada `CANARY_EVERY`.
- **RF-6 — DEMOTE unificado.** Qualquer `Verdict::Demote(_)` (Latency/Corruption/
  FreeFloor) aciona o `swapoff` confirmado por canal já existente. **Aceite:** os 3
  motivos chegam ao mesmo caminho de DEMOTE e logam a razão.

## 5. Requisitos não-funcionais

- **Performance:** overhead amortizado pequeno (1 round-trip de `CANARY_BYTES` a cada
  `CANARY_EVERY` requests). Sem cópia extra no caminho de dados de swap.
- **Segurança:** sem `unsafe` novo (usa `ramshared-cuda`); região-canário sem dado de
  usuário; nenhum endereço de VRAM logado.
- **Observabilidade:** o log de DEMOTE inclui a razão e os valores (latência/free) que
  dispararam — _(estende o log já existente)_.
- **Resiliência:** erro CUDA na sonda (write/read/mem_info) é sinal de perda de
  residência → tratado como DEMOTE conservador (decisão a fechar no SPEC).
- **Escalabilidade:** N/A (1 daemon por device, 1 conexão = vida do swap).
- **LGPD:** N/A (sem dado pessoal; sentinela é padrão sintético).

## 6. Fluxos

**Sonda por ciclo (a cada `CANARY_EVERY` requests, durante swap ativo):**

1. `seq += 1`; escreve `seq`-derived pattern na região-canário (`write_at`).
2. relê a região (`read_at`) e compara → `content_ok`.
3. cronometra (1)+(2) → `latency_us`.
4. `free = mem_info().free`.
5. `canary.sample(latency_us, content_ok, free)`:
   - `Verdict::Ok` → segue;
   - `Verdict::Demote(reason)` → dispara `swapoff <nbd>` (thread, confirmado por canal;
     re-arma se falhar) e loga `reason`.

Baseline: as primeiras `N` latências da sonda formam a mediana (como hoje, mas da
sonda).

## 7. Modelo de dados

- **Sentinela:** `CANARY_BYTES` bytes derivados de `seq` (ex.: FNV/contador por palavra)
  — reusa o padrão de `ramshared-integrity` se couber, ou um preenchimento simples.
  _(Inferência: detalhe fechado no SPEC.)_
- **Estado da sonda** (no daemon): `region: DeviceMem`, `seq: u64`, `counter: u32`,
  `every: u32`. Sem persistência. _(Proposto.)_

## 8. API / Interfaces

Nenhuma uAPI/ABI nova. Mudança interna ao daemon. Reusa `residency.rs::Canary` e
`ramshared-cuda` (sem novas funções públicas exigidas; possivelmente um helper interno
no daemon). _(Confirmado no codebase: APIs já existem.)_

## 9. Dependências e riscos

- **Dep:** `ramshared-cuda` (`alloc`/`write_at`/`read_at`/`mem_info`) e
  `residency.rs` — ambos prontos. _(Confirmado no codebase.)_
- **Risco R1:** `cuMemGetInfo` no WSL2/GPU-PV pode não refletir host eviction → free-
  floor ruidoso. **Mitigação:** `free_floor_bytes` conservador/tunável; latência segue
  primário. _(Inferência.)_
- **Risco R2:** cadência mal calibrada (alta = overhead; baixa = detecção lenta).
  **Mitigação:** `CANARY_EVERY` constante tunável; default medido. _(Inferência.)_
- **Risco R3:** sonda não roda em idle (serve loop bloqueado no `read`). **Mitigação:**
  declarado fora de escopo (H1). _(Confirmado no codebase: serve serial.)_
- **Risco R4:** conteúdo nunca diverge se WDDM é data-safe → `Demote(Corruption)` raro.
  **Aceito:** é guarda de defesa-em-profundidade (dispara só se a premissa falhar).

## 10. Estratégia de implementação

1. Alocar a região-canário (`DeviceMem`) no daemon, após a região de swap.
2. Helper de sonda no daemon: write sentinela → read → compare + `mem_info`.
3. Substituir o feed `sample(lat_serve, true, u64::MAX)` pela sonda em cadência.
4. Default `ResidencyConfig.free_floor_bytes > 0`.
5. Tratar erro CUDA da sonda como DEMOTE conservador.
6. Testes: `residency.rs` já cobre a decisão; adicionar teste unit da **cadência**
   (sem GPU) e um teste `--ignored` da sonda real (com GPU).

## 11. Documentos a atualizar (mesmo commit do IMPL)

- `docs/008-vram-residency-canary/SPEC.md` (Passo 2) e `IMPL.md` (Passo 3).
- `docs/vram-as-ram/SPECv3-WSL2.md` §9.4 → marcar implementado.
- `docs/vram-as-ram/IMPL.md`, `ARCHITECTURE.md` (limitação C1 resolvida), `MEMORY.md`.

## 12. Fora de escopo

- Detecção em **idle** (thread amostradora + `cuCtxSetCurrent`) — depende de H1.
- Daemon multi-conexão / multi-thread (H1).
- Writeback do zram na VRAM (Fase B).

## 13. Critérios de aceitação

- `Demote(Corruption)` e `Demote(FreeFloor)` deixam de ser código morto: alcançáveis a
  partir da sonda do daemon (não mais `true`/`u64::MAX` fixos).
- Os 3 gatilhos chegam ao mesmo caminho de DEMOTE confirmado por canal.
- Sem regressão na aceitação §14 (spill + DEMOTE por latência continuam OK).
- `clippy --workspace -D warnings` + testes verdes; daemon sem `unsafe` novo.

## 14. Validação

- **Unit (sem GPU/root):** `cargo test -p ramshared-wsl2d` — decisão (`residency.rs`,
  já existe) + novo teste de cadência.
- **GPU (`--ignored`):** sonda real escreve/relê a região-canário (round-trip íntegro).
- **Ao vivo (opcional, root+GPU):** `vramhog` reduz `free` abaixo do `free_floor` →
  `Demote(FreeFloor)` observável no log; re-rodar `cascade-validate.sh`/`cascade-demote.sh`
  (sem regressão).
- **checkpatch.pl/make modules:** N/A (Rust userspace).
