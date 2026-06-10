# PRD — Tier VRAM multi-tenant: arbitragem WSL2 ↔ civm por pressão de memória

> SSDV3 PASSO 1. Slug: `vram-arbiter`. Pedido do usuário (2026-06-09): "que o ramshared funcionasse
> na vm como vai funcionar no wsl2 e cada um saber exatamente quem tá precisando mais — por exemplo
> quando as actions estiverem rodando na vm e tiver compilando no wsl2".

## 1. Resumo

Hoje o tier VRAM (RTX 2060, 6 GB) serve **um** consumidor: o WSL2. Este PRD estende o RamShared
para **dois tenants no mesmo host físico** — o WSL2 (dev/compilação) e a VM de CI `civm`
(`gha-ubuntu-2404`, GitHub Actions) — com **arbitragem dinâmica**: um árbitro compara a pressão de
memória (PSI) dos dois e move capacidade de swap-VRAM para quem está precisando mais, com histerese.

## 2. Contexto técnico

- **Confirmado em docs (civm):** VM Hyper-V `gha-ubuntu-2404` (Ubuntu 24.04, 4+ cores, runner label
  `civm`) roda **no mesmo host Windows `EMEDEV`** que o WSL2; alcançável por SSH (Tailscale ou rede
  local); `civmctl` (Go) roda só no guest; **zero automação no host Windows**.
- **Confirmado no codebase (ramshared):** o daemon serve VRAM por **dois transportes**: NBD
  fixed-newstyle (multi-conexão, hoje **só Unix socket**) e **ublk** (local, validado, p50 241µs vs
  326µs do NBD). CUDA via `dlopen` (binário roda sem libcuda). Máquina de DEMOTE pronta: canário §9
  (latência), sonda §9.4 (conteúdo/free), `ResidencySampler` (histerese por streak), `spawn_swapoff`.
- **Confirmado no codebase:** o kernel WSL2 tem PSI (`/proc/pressure/memory`) — sinal moderno de
  "quem está sofrendo por memória" (avg10/avg60 + stall total).
- **Inferência (validar):** a civm **não tem GPU/CUDA** (sem GPU-PV configurado; GPU-P em GPUs
  consumer é limitado) → não pode rodar um daemon VRAM próprio → o tier chega a ela **pela rede**.
- **Inferência (validar):** conectividade VM→WSL2 existe via rede local/Tailscale; WSL2 em NAT pode
  exigir port-forward no host ou Tailscale no próprio WSL2.

## 3. Opção recomendada

**Opção A — um único daemon (WSL2, dono da GPU) + slices exportadas por transporte por tenant:**
- WSL2 consome via **ublk** (local, validado, mais rápido).
- civm consome via **NBD sobre TCP** (reuso do servidor NBD já existente; a VM só precisa de
  `nbd-client`, sem GPU). Falta apenas o **listener TCP** (hoje Unix socket).
- A VRAM é dividida em **K slices** de tamanho fixo (ex.: 8×512 MiB); cada slice é um device
  independente (ublk ou NBD). O **mapa slice→tenant é dinâmico**: rebalancear = `swapoff` da slice
  num tenant + `swapon` no outro (o mecanismo de swapoff/DEMOTE já existe e fica por-slice).
- **Árbitro** no daemon: recebe PSI de um **agente leve por tenant**, decide o mapa com histerese.

**Rejeitadas:**
- **B — GPU-PV/GPU-P na VM + segundo daemon:** GPU-P em RTX consumer é mal suportado; dois contextos
  CUDA disputando a mesma GPU adicionam o risco de eviction mútuo; complexidade de provisionamento
  no host (civm tem política de zero automação de host).
- **C — daemon no host Windows:** fora da stack (projeto é kernel Linux / Rust em Linux); Day-0.
- **D — partição estática (sem árbitro):** não atende "quem precisa mais ganha mais"; desperdiça o
  tier quando um lado está ocioso.

## 4. Requisitos funcionais (RF)

- **RF-1** Listener **NBD/TCP** no daemon (`--listen tcp://ADDR:PORT`), coexistindo com Unix socket.
  Bind restrito (default: só interface privada/Tailscale). *(Reuso: `conn.rs`/`spawn_acceptor` são
  agnósticos de stream — confirmar na SPEC.)*
- **RF-2** **Slices**: `--slices K --slice-mb N` — K devices independentes servidos do mesmo
  `DeviceMem` (offsets disjuntos), cada um exportável por ublk (local) ou NBD (remoto).
- **RF-3** **Agente de pressão** (binário mínimo, mesmo repo): lê `/proc/pressure/memory`
  (avg10/avg60) + `/proc/swaps` do tenant e reporta ao árbitro (TCP, mesmo canal/porta de controle);
  executa os comandos `swapon`/`swapoff` de slices que o árbitro mandar. Roda no WSL2 **e** na civm.
- **RF-4** **Árbitro** no daemon: compara PSI dos tenants; rebalanceia o mapa slice→tenant com
  **histerese** (streak de N amostras, padrão `ResidencySampler`) e cooldown; nunca deixa um tenant
  sob pressão com zero slices.
- **RF-5** **DEMOTE global**: o canário §9/§9.4 (eviction WDDM é da GPU inteira) demove **todas** as
  slices nos **dois** tenants (comando de swapoff via agentes) antes de desligar o tier.
- **RF-6** **Observabilidade**: o árbitro loga cada decisão com os PSI dos dois lados ("quem está
  precisando mais"), e um `status` consultável (slices por tenant, pressões, último rebalanço).
- **RF-7** Provisionamento civm documentado como **runbook copiável** (instalar `nbd-client` +
  agente + systemd unit), respeitando a política do civm (peer copia template; sem automação de host).

## 5. Requisitos não-funcionais (RNF)

- **RNF-1** Segurança: NBD/TCP **sem auth nativo** → bind apenas em rede privada/Tailscale; nunca
  `0.0.0.0` público. Documentar o modelo de ameaça.
- **RNF-2** **Falha do daemon não pode travar a civm**: slices remotas entram com **prioridade de
  swap menor** que o swap local da VM; runbook de remoção; o risco D-state (NBD órfão) documentado —
  é o mesmo hazard do incidente WSL2 (2026-06-09).
- **RNF-3** Rebalanço é **barato e raro**: histerese/cooldown evitam flapping; `swapoff` de slice é
  bounded (slice pequena) e fora do hot path (thread própria, padrão `spawn_swapoff`).
- **RNF-4** Sem regressão nos caminhos atuais (NBD Unix socket, ublk single-tenant).
- **RNF-5** `unsafe` segue confinado a `ramshared-uring`/`ramshared-cuda`.

## 6. Fluxos

1. **Cenário canônico (do usuário):** Actions rodando na civm (PSI civm sobe) + `cargo build` no
   WSL2 (PSI wsl2 sobe menos) → árbitro detecta `psi_civm >> psi_wsl2` por N amostras → manda o
   agente WSL2 `swapoff slice_i` → confirma → manda o agente civm `swapon slice_i` (NBD) → civm tem
   mais swap-VRAM enquanto a action roda; ao terminar, a pressão cai e o fluxo inverte.
2. **Boot:** daemon aloca VRAM, divide em K slices, exporta; agentes conectam, registram tenant,
   recebem mapa inicial (ex.: K/2 cada) e fazem swapon.
3. **DEMOTE global:** canário dispara → árbitro manda swapoff de tudo nos dois tenants → tier off.
4. **Shutdown ordenado:** agentes fazem swapoff → daemon STOP/DEL (ublk) e fecha NBD → zera VRAM.

## 7. Modelo de dados

- `Slice { id, offset, len, tenant: Option<TenantId>, state: Active|Draining|Free }`.
- `Tenant { id, transport: Ublk|NbdTcp, psi: PsiSample{avg10, avg60, stall_us}, slices: Vec<id> }`.
- Protocolo agente↔árbitro (TCP, mesmo formato length-prefixed do NBD ou JSON-lines — decisão de
  SPEC): `Register`, `PsiReport`, `SwapOn(slice)`, `SwapOff(slice)`, `Ack/Nack`, `DemoteAll`.

## 8. API / Interfaces

- CLI do daemon: `--slices K --slice-mb N --listen-nbd tcp://IP:PORT --arbiter-listen IP:PORT`.
- Binário novo `ramshared-agent`: `--daemon IP:PORT --tenant {wsl2|civm} [--prio P]`.
- Nenhuma mudança de uAPI de kernel. O `BlockBackend` ganha uma **view com offset/len**
  (slice-do-`DeviceMem`) — checar na SPEC se `VramBackend` já comporta (Regra dura #1).

## 9. Dependências e riscos

- **Risco A — conectividade WSL2↔VM** *(Inferência a validar)*: WSL2 NAT pode bloquear inbound da
  VM. Mitigações: Tailscale no WSL2 (civm já usa Tailscale) ou port-forward no host. **Fato a
  confirmar antes da SPEC** (teste de alcançabilidade real).
- **Risco B — D-state na civm** se o daemon morrer com swap NBD ativo (mesmo mecanismo do freeze de
  2026-06-09). Mitigações: RNF-2 (prioridade menor, swap local primário), watchdog no agente
  (daemon sumiu → swapoff imediato best-effort), runbook.
- **Risco C — flapping** do árbitro (PSI oscila). Mitigação: RF-4 (histerese + cooldown).
- **Risco D — latência NBD/TCP** sobre virt-switch (vs 326µs do Unix socket). Medir na Fase 0 do
  IMPL; se inviável (>1 ms p50), reavaliar (a civm ainda lucra vs swap em VHDX? Provável que sim —
  o disco dela é VHDX dinâmico em volume V: já saturado — Confirmado em docs civm).
- **Risco E — segurança do NBD/TCP** (RNF-1).

## 10. Estratégia de implementação

- **F0** Medir: PSI nos dois ambientes + alcançabilidade de rede + latência NBD/TCP crua (Fase 0,
  sem código novo de produto).
- **F1** NBD/TCP listener (reuso do acceptor) + smoke local.
- **F2** Slices (view com offset no backend + N devices) + smoke multi-device local.
- **F3** Agente PSI + protocolo + árbitro com histerese; e2e local (2 tenants fake no WSL2).
- **F4** Provisionamento civm (runbook + systemd) e e2e real: action na civm + build no WSL2,
  observar o rebalanço. Gate anti-halo: números de PSI/latência antes-depois.

## 11. Documentos a atualizar

`docs/vram-arbiter/{SPEC,IMPL}.md`; `README`/`ARCHITECTURE` (tier multi-tenant); runbook copiável
para o repo civm (`docs/CIVM.md` template do peer); `MEMORY.md`.

## 12. Fora de escopo

- GPU-PV/CUDA na civm; daemon no host Windows; >2 tenants (modelo é genérico, validação com 2);
  auth/criptografia no NBD (rede privada só — RNF-1); QoS por job de CI (granularidade é tenant).

## 13. Critérios de aceitação

- civm usa swap-VRAM via NBD/TCP servido pelo daemon no WSL2 (slice visível em `/proc/swaps` da VM).
- Cenário canônico demonstrado: sob action na civm + build no WSL2, o árbitro move ≥1 slice para a
  civm e devolve depois (logs com PSI dos dois lados).
- DEMOTE global desativa o tier nos dois tenants.
- Sem regressão: ublk single-tenant e NBD Unix socket continuam passando os smokes.
- Falha forçada do daemon não trava a VM (swap local segue; agente faz swapoff best-effort).

## 14. Validação

- Fase 0 (números antes de codar): PSI idle/sob-carga nos dois ambientes; RTT WSL2↔VM; p50/p99 NBD
  TCP cru. Smokes por fase (F1-F3 locais; F4 e2e real). Kahneman: counterfactual do árbitro
  (decisão errada = swapoff de quem precisava → registrar trigger de reversão: PSI do tenant
  drenado piora >2× em 60s após rebalanço ⇒ devolve a slice e entra em cooldown longo).
