# PRD — RamShared Memory Broker (unificado final)

> SSDV3 PASSO 1 **consolidado**. Slug: `memory-broker`. Este é o **PRD único** do qual a SPEC será
> gerada. Absorve e substitui como fonte: `docs/vram-arbiter/PRD.md`, `docs/dcc-out-of-core/PRD.md`
> e `docs/memory-broker/VISION.md`. A avaliação crítica dos documentos de origem está no Anexo A.
> Disciplinas: SSDV3 (fato vs inferência por item) + Kahneman (counterfactuals e rollback triggers
> em §14; gates anti-halo entre fases em §10).

## 1. Resumo

**Uma plataforma de tiering de memória para um host físico com GPU**: um **broker** (árbitro) +
**agentes** por ambiente + o primitivo de **lease de VRAM revogável**, servindo N consumidores com
o mecanismo nativo de cada um — kernel Linux consome VRAM como **swap** (ublk/NBD, Fase B, pronto);
app DCC no Windows consome **RAM como backing da VRAM** (out-of-core); a arbitragem move capacidade
para **quem está precisando mais** (PSI/pressão), em **qualquer GPU com VRAM** (trait de backend:
CUDA pronto, Vulkan a seguir). Produto **instalável**: serviço Windows (`.exe`/winget) + binário
Linux (`.deb`/systemd) + addon Blender.

## 2. Contexto técnico

- **Confirmado no codebase (Fase B validada em hardware):** VRAM servida como block device por
  **ublk** (p50 241µs, ~26% mais rápido que NBD 326µs); **NBD** multi-conexão (hoje Unix socket);
  máquina de **DEMOTE** pronta (canário §9 latência, sonda §9.4 conteúdo/free, `ResidencySampler`
  com histerese, `spawn_swapoff`); CUDA por **dlopen** (binário sem dependência de libcuda);
  `BlockBackend` é a costura — o tier de swap não tem nada CUDA-específico.
- **Confirmado em docs (civm):** VM Hyper-V `gha-ubuntu-2404` (runner GitHub Actions, label `civm`)
  no **mesmo host físico** (`EMEDEV`) que o WSL2 + RTX 2060; alcançável por SSH/Tailscale; sem GPU.
- **Confirmado em docs (CUDA/Blender):** oversubscription UVM por page-fault é **Linux-only**
  (WDDM não tem demand paging); Cycles/CUDA tem fallback de memória do host para cena que não cabe
  (cobertura exata por backend/versão — **a medir na P0**).
- **Confirmado (relato do tester):** Alex (artista 3D, Windows, sem WSL2) perde **dias** otimizando
  cena à mão para caber na VRAM, com RAM ociosa; disponível para testar com cenas reais.
- **Duas personas** *(Confirmado nas conversas)*:
  - **Dev/CI (Emerson, host EMEDEV):** WSL2 (compila) + civm (Actions) disputam memória; a VRAM é o
    tier extra. Cérebro no WSL2 (a stack vive lá).
  - **Artista (Alex, desktop Windows):** Blender disputa VRAM com o resto; a RAM é o tier extra.
    **Não há WSL2** — o cérebro precisa rodar como serviço Windows nativo.
- **Inferência (validar na P0):** conectividade VM→WSL2 (NAT do WSL2 pode exigir Tailscale no WSL2
  ou port-forward); latência NBD/TCP no virt-switch; cobertura out-of-core do OptiX/HIP.

## 3. Opção recomendada

**Plataforma protocol-first; um cérebro por host; mecanismos nativos por consumidor; GPU por trait.**

- **Um protocolo** (agente ↔ broker): registrar tenant, reportar pressão, receber comandos
  (swapon/swapoff de slice, release/grant de lease, demote-all). O cérebro vira detalhe de
  deployment: WSL2 no host dev; serviço Windows no host do artista.
- **Lease de VRAM revogável** como primitivo universal: todo uso de VRAM pelo tier de swap é
  empréstimo; a revogação é o DEMOTE já construído. É o que une os dois mundos: o addon do Blender
  **pede** a VRAM que o tier de swap **devolve**.
- **Mecanismos por consumidor (irredutível, é SO/física):** Linux = block device (ublk local, NBD
  remoto — prontos); DCC = configuração do out-of-core nativo (MVP) e, gated, interposer (v2);
  Windows-como-consumidor-de-swap = fora de escopo (exigiria driver de disco Windows).
- **GPU por trait** (`VramProvider`: alloc/free/read_at/write_at/budget): CUDA (pronto) → Vulkan
  (qualquer placa em Windows/Linux nativos; o tier não usa shader, só alloc+cópia) → D3D12/dxg
  (pesquisa; único caminho plausível p/ não-NVIDIA dentro do WSL2).

**Rejeitadas:** binário único idêntico em todo lugar (mecanismos divergem por SO); UVM-only (não
cobre Windows); GPU-P/passthrough na civm (suporte consumer ruim); broker-no-host-Windows para a
persona dev (stack é Linux/Day-0; no host dev o WSL2 é o lugar); partição estática (não atende
"quem precisa mais").

## 4. Requisitos funcionais

**Broker core**
- **RF-B1** Protocolo agente↔broker: `Register(tenant, transport)`, `PsiReport`, `SwapOn/Off(slice)`,
  `LeaseRequest/Release`, `DemoteAll`, `Status`. Formato na SPEC (JSON-lines vs length-prefixed).
- **RF-B2** Árbitro: compara pressão entre tenants; rebalanceia com **histerese + cooldown** (padrão
  `ResidencySampler` — reuso); nunca deixa tenant sob pressão com zero slices.
- **RF-B3** **Lease revogável**: revogação = demote per-slice (reuso `spawn_swapoff` + canário);
  prioridade: pedido explícito de VRAM (DCC) > swap tier.
- **RF-B4** Observabilidade: cada decisão logada com as pressões dos dois lados; `Status` mostra
  slices/tenant, PSI, último rebalanço ("cada um sabe quem está precisando mais").

**Tenants Linux (WSL2 + civm)**
- **RF-L1** Slices: `--slices K --slice-mb N`; K devices independentes (offsets disjuntos no mesmo
  `DeviceMem`); mapa slice→tenant dinâmico. *(Checar na SPEC se `VramBackend` comporta view
  offset/len — Regra dura #1.)*
- **RF-L2** Listener **NBD/TCP** (`--listen-nbd tcp://IP:PORT`), coexistindo com Unix socket; bind
  só em interface privada/Tailscale.
- **RF-L3** Agente Linux: lê `/proc/pressure/memory` + `/proc/swaps`, executa swapon/swapoff.
- **RF-L4** Runbook copiável de provisionamento civm (nbd-client + agente + systemd), respeitando a
  política do civm (peer copia template; zero automação de host).

**Windows / DCC (Fase C)**
- **RF-W1** Agente Windows (Rust nativo): pressão de memória do SO + NVML/budget da GPU.
- **RF-W2** Addon Blender MVP: predição cabe/não-cabe (footprint vs VRAM livre), configuração
  automática do out-of-core nativo (backend/flags), proxies/mipmaps **não-destrutivos** opcionais,
  monitor de spill/tempo durante o render.
- **RF-W3** Ponte addon↔broker: "vou renderizar → libera VRAM" (LeaseRequest) e devolução pós-render.
- **RF-W4** *(v2, gated — ver §10)* Interposer de residência (hook da Driver API; prefetch/pinning;
  compressão opcional na RAM).

**Cross-vendor**
- **RF-G1** Trait `VramProvider` extraído da camada atual (CUDA vira um backend).
- **RF-G2** Backend **Vulkan** (`DEVICE_LOCAL` + `VK_EXT_memory_budget` + transfer queue) —
  destrava "qualquer placa" em Windows/Linux nativos.
- **RF-G3** *(pesquisa)* D3D12/`/dev/dxg` para não-NVIDIA dentro do WSL2.

**Produto**
- **RF-P1** Instaláveis: `ramshared-setup.exe`/winget (serviço Windows + CLI) e `.deb` + systemd
  (Linux/WSL2/civm). Binários Rust nativos; GPU APIs via dlopen/driver (zero dependência extra).
- **RF-P2** Transporte com fallback: ublk onde o kernel tem (`CONFIG_BLK_DEV_UBLK`); **NBD como
  fallback universal** (medido: ~26% mais lento — aceitável).
- **RF-P3** Configuração única (TOML) por host: tenants, slices, binds, política do árbitro.

## 5. Requisitos não-funcionais

- **RNF-1 Anti-D-state (o risco nº1, já nos mordeu):** tenant remoto com swap em device do broker
  morto = D-state. Mitigações obrigatórias: slices remotas com **prioridade de swap menor** que o
  swap local; **watchdog no agente** (broker sumiu → swapoff best-effort imediato); runbook de
  remoção; teardown ordenado validado (harness qemu — já PASS na Fase B/F2).
- **RNF-2 Segurança:** NBD/TCP e protocolo do broker **sem auth nativa** → bind apenas em rede
  privada/Tailscale, nunca `0.0.0.0`; addon local-only; **zero telemetria externa**.
- **RNF-3 Anti-flapping:** histerese + cooldown; rebalanço raro e barato (slice pequena, swapoff
  bounded, fora do hot path).
- **RNF-4 Zero regressão:** Fase B (ublk single-tenant, NBD Unix) continua passando os smokes.
- **RNF-5** `unsafe` confinado aos crates FFI (`ramshared-uring`, `ramshared-cuda`, futuro
  `ramshared-vulkan`); daemon-lib `#![forbid(unsafe_code)]`.
- **RNF-6 Day-0:** sem shims; cada fase entrega a forma definitiva da sua superfície.

## 6. Fluxos

1. **CI vs build (persona dev):** Actions na civm (PSI sobe) + `cargo build` no WSL2 → árbitro vê
   `psi_civm ≫ psi_wsl2` por N amostras → swapoff slice no WSL2 → swapon na civm via NBD → inverte
   quando a pressão inverte.
2. **Render do artista:** addon detecta cena > VRAM livre → `LeaseRequest` ao broker → broker
   demove slices de swap (revoga lease) → render com VRAM inteira + out-of-core p/ RAM → fim do
   render → `LeaseRelease` → broker re-arrenda ao swap tier.
3. **Broker morre:** agentes detectam (watchdog) → swapoff best-effort das slices remotas → tenants
   seguem com swap local (RNF-1).
4. **Shutdown ordenado:** demote-all → agentes confirmam swapoff → STOP/DEL (ublk) + fecha NBD →
   zera VRAM (reuso do teardown validado).

## 7. Modelo de dados

`Tenant{id, transport: Ublk|NbdTcp|DccAgent, psi, slices}` · `Slice{id, offset, len, tenant?,
state: Active|Draining|Free}` · `Lease{holder, bytes, revocable}` · `PsiSample{avg10, avg60,
stall_us}`. Protocolo: formato na SPEC.

## 8. API / Interfaces

- `ramsharedd`: `--slices K --slice-mb N --listen-nbd tcp://IP:PORT --arbiter-listen IP:PORT
  --transport {ublk,nbd} --backend {cuda,vulkan}` (+ os flags atuais).
- `ramshared-agent`: `--broker IP:PORT --tenant NAME [--swap-prio P]`.
- Addon Blender (Python): fala com o agente/broker local.
- Trait `VramProvider { alloc, free, read_at, write_at, budget }` — `BlockBackend` permanece a
  interface do tier de swap (Confirmado: já é agnóstica).
- **Nenhuma uAPI de kernel nova** (ublk/NBD existentes).

## 9. Dependências e riscos

| # | Risco | Mitigação |
|---|---|---|
| R1 | Conectividade VM↔WSL2 (NAT) *(Inferência)* | P0 mede; Tailscale no WSL2 ou port-forward |
| R2 | **D-state em tenant remoto** (broker morto) | RNF-1 (prioridade, watchdog, runbook) |
| R3 | Flapping do árbitro | RNF-3 (histerese+cooldown) + counterfactual §14 |
| R4 | Latência NBD/TCP no virt-switch | P0 mede; vs swap em VHDX saturado a civm ainda lucra *(Inferência)* |
| R5 | Vulkan dentro do WSL2 imaturo (não-NVIDIA) | matriz honesta: CUDA cobre WSL2/NVIDIA; Vulkan cobre nativos; D3D12 = pesquisa RF-G3 |
| R6 | Hooking `nvcuda.dll` frágil (v2) | v2 gated; degrada p/ caminho nativo (RNF-4 da Fase C) |
| R7 | `wsl --shutdown` mata o broker do host dev | = R2 (watchdog); documentar |
| R8 | Out-of-core nativo insuficiente (OptiX/geometria) | P0 com cenas reais decide MVP vs v2 |
| R9 | Disponibilidade do tester | Confirmada; Anexo B coleta o contexto |

## 10. Estratégia de implementação (fases com gates anti-halo)

- **P0 — Medição (sem código de produto):** PSI idle/carga nos 3 ambientes; alcançabilidade e RTT
  VM↔WSL2; p50/p99 NBD/TCP cru; cenas do Alex (Anexo B) + comportamento do out-of-core nativo.
  **Gate:** números documentados; sem eles nenhuma fase seguinte inicia.
- **P1 — Broker core, Linux↔Linux:** RF-B1..B4, RF-L1..L4 (slices, NBD/TCP, agente, árbitro).
  E2e real: action na civm + build no WSL2 com rebalanço observado. **Gate:** cenário 1 demonstrado
  com logs de PSI; D-state drill (matar broker → watchdog limpa <5s).
- **P2 — Ponte Windows + MVP DCC:** RF-W1..W3 (agente Windows, addon MVP, lease bridge).
  **Gate:** ≥1 cena real do Alex que falhava renderiza sem edição manual; lease revoga/devolve.
- **P3 — Qualquer GPU:** RF-G1..G2 (trait + Vulkan). **Gate:** smoke do tier de swap passando em
  GPU não-NVIDIA (nativa).
- **P4 — Gated (só com números):** interposer v2 (RF-W4; gate: MVP não destrava cena do Alex ou
  custo >2× vs working-set-na-VRAM), driver de swap Windows, D3D12-WSL2 (RF-G3).

## 11. Documentos a atualizar

`docs/memory-broker/SPEC.md` (**próximo passo**, deste PRD); IMPL por fase; runbook civm copiável;
`README`/`ARCHITECTURE` (plataforma); `MEMORY.md`. PRDs de origem ficam como histórico (marcados).

## 12. Fora de escopo

Modelo de negócio do addon (preço/licença); auth/criptografia própria (rede privada só); >2 tenants
Linux na validação; reescrever Cycles; Windows-como-consumidor-de-swap (driver de disco); competir
com RAM em latência (PCIe manda); render distribuído.

## 13. Critérios de aceitação (plataforma)

1. P1: rebalanço automático WSL2↔civm sob carga real, com logs "quem precisa mais" e D-state drill
   limpo.
2. P2: cena do Alex que falhava renderiza sem edição manual; lease revoga o swap tier e devolve.
3. P3: tier de swap funcional em GPU não-NVIDIA nativa (Vulkan).
4. RNF-4: smokes da Fase B continuam verdes em todas as fases.
5. Produto instalável: setup Windows + `.deb` Linux com config TOML única.

## 14. Validação (Kahneman)

- **Counterfactual do árbitro (#2):** decisão errada = drenar quem precisava. **Rollback trigger:**
  PSI do tenant drenado piora >2× em 60s pós-rebalanço ⇒ devolve a slice + cooldown longo (logado).
- **Counterfactual do lease (#2):** revogar swap p/ render que não usa a VRAM ganha. **Trigger:**
  uso de VRAM do solicitante <50% do lease em 5min ⇒ devolve ao swap tier.
- **Anti-halo (#11):** cada fase só inicia com o gate numérico da anterior; o sucesso da Fase B não
  "aprova" o broker — P0/P1 têm que provar com números próprios.
- **Worst-case (#5):** D-state drill obrigatório no P1 (matar o broker com swap remoto ativo num
  ambiente descartável — harness qemu estendido, nunca no host).

---

## Anexo A — Avaliação dos documentos de origem (o que mudou na consolidação)

1. **`vram-arbiter/PRD.md`** — sólido em topologia e reuso do demote. **Lacunas corrigidas aqui:**
   (a) não tratava o ciclo de vida do WSL2 (`wsl --shutdown` mata o cérebro → R7/RNF-1 watchdog);
   (b) assumia o cérebro fixo no WSL2 — o protocolo-first torna o cérebro detalhe de deployment
   (persona do artista não tem WSL2); (c) D-state era risco listado, aqui virou **RNF + drill
   obrigatório** (P1), porque já nos mordeu uma vez.
2. **`dcc-out-of-core/PRD.md`** — certo em measurement-first e MVP barato. **Lacunas corrigidas:**
   (a) o MVP isolado não conversava com o broker — RF-W3 (lease bridge) é o que une as frentes e
   resolve a disputa real de VRAM no host do artista; (b) "qualquer GPU" também vale pro DCC
   (Cycles HIP/oneAPI p/ AMD/Intel — vai pro P0 medir); (c) v2 ganhou gate numérico explícito.
3. **`VISION.md`** — direção certa (plataforma, lease, cross-vendor, packaging), sem rigor SSDV3.
   Este PRD a operacionaliza: RF/RNF numerados, riscos com mitigação, fases com gates, validação
   Kahneman. A VISION fica como leitura de contexto.
4. **Conflito resolvido:** a Fase B serve VRAM→SO e a Fase C pede VRAM pro app — no mesmo host
  disputariam a GPU às cegas. O **lease revogável** (RF-B3) é a resolução: um único dono da verdade
  sobre a VRAM.

## Anexo B — Perguntas para o tester (P0, contexto que o Alex pediu)

1. SO e versão (Windows 10/11), GPU (modelo/VRAM), RAM total.
2. Versão do Blender e backend (OptiX ou CUDA — se souber; se não, mandar screenshot das
   Preferences → System).
3. 1-2 .blend (ou descrição: nº objetos, texturas e resoluções) que **falharam** por VRAM + a
   mensagem de erro exata.
4. O que tentou à mão (reduzir texturas, decimate, tiles, passes) e quanto tempo perdeu.
5. Topa rodar um script de medição nosso (lê VRAM/RAM durante o render; **não altera a cena**)?
