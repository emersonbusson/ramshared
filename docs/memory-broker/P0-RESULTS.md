# P0-RESULTS — RamShared Memory Broker (gate numérico de P1)

> SSDV3 PASSO 3 / ITEM-1 de [`SPECv2.md`](SPECv2.md). **Este arquivo é o gate anti-halo (#11):
> nenhum item de P1 (ITEM ≥ 3) inicia enquanto qualquer célula obrigatória estiver vazia ou
> "estimada".** Disciplina #3 (número, não adjetivo): cada célula = número + unidade + n de
> rodadas + data + ambiente. Scripts: [`scripts/p0/`](../../scripts/p0/).

## Status do gate

**Gate P1: 🔴 FECHADO — falta só §4 render.** Fechados: **§1 PSI** (idle+carga, WSL2+civm),
**§2 rede R1** (decisão port-forward), **§3 NBD/TCP** (loopback + **cross-host p50 644 µs**),
**§5 calibração** (propõe `delta_psi=10`). Falta só: **§4 render** — precisa de cena real que
falhava por VRAM, do tester (Alex); o script já está pronto/validado no host.

## Ambientes

| Tag | Host/VM | Papel | PSI (`/proc/pressure/memory`) | PAGE_SIZE | Data |
| --- | --- | --- | --- | --- | --- |
| WSL2 | EMEDEV / WSL2 (`6.6.123.2-microsoft-standard-WSL2+`) | tenant dev (cérebro) | **habilitado** (CONFIG_PSI=y, legível) | 4096 B | 2026-06-13 |
| civm | `gha-ubuntu-2404` (Hyper-V, kernel `6.8.0-124-generic`) | tenant CI | **habilitado** (`some`/`full` legíveis; some.avg10 0.5–7.8 conforme carga de CI) | 4096 B | 2026-06-13 |
| host | EMEDEV (Windows + RTX 2060) | render (tester Alex) | N/A (Windows) | N/A | — |

## 1. PSI por ambiente (idle / carga) — `measure-psi.sh`

Métrica de arbitragem = linha `some`, `avg10` (DT-15). Gate exige **≥3 rodadas** por célula
(idle: 300 s; carga: durante `cargo build -j4` no WSL2 / action real na civm).

| Ambiente | Cenário | some.avg10 média | some.avg10 máx | full.avg10 máx | n rodadas | Data |
| --- | --- | --- | --- | --- | --- | --- |
| WSL2 | idle | 0.011 (3 rod., 831 am.) | 0.55 | 0.00 | 3 × ~300 s | 2026-06-13 |
| WSL2 | carga (mem., cgroup hog) | **14.25** | 22.54 | 18.26 | 40 (confinado) | 2026-06-13 |
| civm | idle / CI (natural) | 1.237 (3 rod., 806 am.) | **19.44** (burst CI) | 7.75 | 3 × ~300 s | 2026-06-13 |
| civm | carga | bursts CI até ~19 (linha acima); hog confinado **não** rodado na VM de CI (restrição) | — | — | obs. | 2026-06-13 |

> **Carga = `scripts/p0/measure-psi-load.sh`** (hog anônimo confinado em cgroup v2, `memory.max`
> teto + `swap.max=0`; 0 OOM, cgroup limpo pós-teste). O SPEC dizia "cargo build -j4", mas o P0
> achou que build é **CPU-bound** e não gera PSI de memória → substituído pelo hog confinado
> (correção de metodologia do P0). **Caveat (#1 WYSIATI):** carga confinada é **lower bound** do
> PSI real (só o cgroup estagna) → pressão real do host dá some.avg10 **≥ 14**. CSVs em `/tmp`.

## 2. Rede VM↔WSL2 (alcançabilidade / RTT) — `measure-net.sh`

| Sentido | Transporte | RTT p50 (ms) | RTT p99 (ms) | Porta (TCP:22 teste) | n (ping) | Data |
| --- | --- | --- | --- | --- | --- | --- |
| WSL2 → civm | LAN (192.168.0.50) | 0.375 | 0.849 | aberta | 50 | 2026-06-13 |
| WSL2 → civm | Tailscale (100.123.103.106) | 1.02 | **430** | aberta | 50 | 2026-06-13 |
| civm → WSL2 | direto (NAT 172.31.230.209) | — | — | **100% perda (NAT)** | 5 | 2026-06-13 |
| civm → WSL2 | Tailscale | N/A | N/A | **WSL2 não é nó Tailscale** (sem IP TS) | — | 2026-06-13 |

**Decisão de transporte (R1): port-forward no host Windows.** O sentido crítico (agente civm →
broker WSL2) está **bloqueado por NAT** — WSL2 em 172.31.x, `ping` da civm = **100% perda**
(`ip route get` na civm manda 172.31.x pro gateway LAN, que não conhece a sub-rede) — e o **WSL2
não é nó Tailscale** (nenhum IP TS por nenhum método). Tailscale-no-host tem **cauda ruim
(p99 430 ms vs LAN 0.85 ms)**, inviável p/ o data-plane de swap (Fase B: 241–326 µs). → usar
`netsh portproxy` no host EMEDEV (LAN:porta → 172.31.230.209:porta) para `--arbiter-listen` e
`--listen-nbd`. ITEM-12 (runbook) e DT-25 (endpoints) seguem isso. WSL2 gw/host vNIC = 172.31.224.1.

## 3. NBD/TCP cru no virt-switch — `measure-nbd-tcp.sh`

Baseline honesto (sem código nosso). Comparar com Fase B: **p50 241 µs (ublk) / 326 µs (NBD-Unix)**.

| Caminho | Modo | p50 (µs) | p99 (µs) | IOPS | stddev | n rodadas | Data |
| --- | --- | --- | --- | --- | --- | --- | --- |
| NBD/TCP loopback | randread 4k | 174 | 285–578 | ~5200 | p50 spread 169–188 | 3 | 2026-06-13 |
| NBD/TCP loopback | randwrite 4k | 202 | 351–3228 | ~4200 | p50 spread 182–225 | 3 | 2026-06-13 |
| NBD/TCP WSL2↔civm | randread 4k | 644 | ~1250 | ~1450 | p50 611/676/644 | 3 | 2026-06-13 |
| NBD/TCP WSL2↔civm | randwrite 4k | 644 | ~1100 (r1 2278) | ~1400 | p50 742/644/644 | 3 | 2026-06-13 |

> **Loopback ≠ virt-switch.** Loopback p50 ~174 µs = piso (sem rede). **Cross-host MEDIDO**
> (civm → `netsh portproxy` no host → WSL2 `nbdkit`, 3 rodadas): **p50 644 µs**, p99 ~1.0–1.5 ms.
> = piso + ~470 µs do virt-switch/portproxy (≈ 1 RTT LAN por op NBD; RTT p50 0.375 ms). **R4
> confirmado:** 644 µs por 4k é muito abaixo de swap em disco saturado (ms+) → a civm lucra usando
> a VRAM remota como swap (a Inferência do PRD agora é número). Setup/teardown: nbdkit no WSL2
> (userspace), portproxy+firewall no host (removidos após), nbd-client+fio na civm (`-timeout 30`).

> Host de medição atual: tem `nbd-client` + `fio`, **não** tem `nbdkit`/`nbd-server`
> (`sudo apt install nbdkit` antes de rodar — preflight no script, F17).

## 4. Render VRAM/RAM (out-of-core nativo) — `measure-render-vram.ps1` (alimenta gate de P2)

| Cena | GPU/VRAM | VRAM usada máx (MiB) | RAM disp. mín (MiB) | Resultado (coube? spill?) | Data |
| --- | --- | --- | --- | --- | --- |
| (Alex #1, falhava) | PENDENTE | PENDENTE | PENDENTE | PENDENTE | — |
| (Alex #2) | PENDENTE | PENDENTE | PENDENTE | PENDENTE | — |

> **Script validado no host EMEDEV** (RTX 2060): captura VRAM (nvidia-smi) + RAM livre OK
> (ex.: VRAM 2015→1828 MiB / 6144, RAM livre ~3670 MiB). **Bug pego na validação** (regra "rodar
> no host primeiro", #13): `Get-Counter '\Memory\Available MBytes'` é **localizado** e quebra em
> Windows pt-BR → trocado por CIM `Win32_OperatingSystem.FreePhysicalMemory` (neutro de locale).
> Falta só a coleta **real**: a cena do Alex que falhava por VRAM (Anexo B do PRD).

## 5. Calibração dos defaults do árbitro (ITEM-4)

Defaults provisórios viram finais quando estas células fecharem (recalibração = update do
SPECv2 + commit citando este arquivo).

| Parâmetro | Default provisório | Valor calibrado | Base (célula) |
| --- | --- | --- | --- |
| `delta_psi` | 15.0 | **propor 10.0** (validar no e2e P1) | idle Δ ~0–1 (WSL2 0.011, civm 1.237 → não disparar); WSL2 carga **14.25** vs civm idle ~1.2 ⇒ Δ≈13 → com 15 **não move** sob pressão clara. delta_psi=10 + streak=5 pega pressão sustentada (≥10) e ignora ruído idle + bursts transientes de CI |
| `streak` | 5 ticks (10 s) | **manter 5** | filtra os bursts transientes da civm (picos a 19.4 que não duram 10 s) sem perder carga sustentada |
| `cooldown` | 60 s | 60 s (fixo, PRD §14) | — |
| `psi_floor` | 5.0 | **OK** | idle WSL2 ~0.01 e civm ~1.2 (ambos <5); carga ≥14 (>5) → separa idle de pressão real |
| `cf_window`/`cf_factor`/`cf_cooldown` | 60 s / 2.0 / 300 s | fixos (trigger PRD §14) | — |

## 6. Telemetria & reconciliação (feature `broker-telemetry-reconciliation`)

Números da sessão 2026-06-16 (`docs/broker-telemetry-reconciliation/`). Disciplina #3 (número) + #1
(estado).

| Item | Valor | Unidade | Ambiente | Data |
| --- | --- | --- | --- | --- |
| VRAM `total` / `free` / `used` | 6143 / 5040 / 1103 | MiB | RTX 2060, WSL2, desktop em uso | 2026-06-16 |
| `vram_alloc_daemon` (teste) | 64 | MiB | idem (alloc do teste) | 2026-06-16 |
| **`vram_outros`** (gráficos por subtração) | **1039** | MiB | idem | 2026-06-16 |
| `reconcile_delta` sob swap normal | **≈ -1.0** (ocupado≈0 ≤ emprestado) | frac | drill qemu broker RAM | 2026-06-16 |

- **Gauge real (RF-3):** `vram_gauge_outros_captures_real_graphics_usage` (`backend.rs`, `--ignored`) —
  `mem_info` real → `vram_outros=1039 MiB` capta o uso de gráficos do desktop (sinal de consumidor externo).
- **Calibração `tol_frac`/`streak` (DT-7):** `tol_frac=0.10`, `streak=3` **provisórios e seguros** —
  `Unaccounted` só dispara se `ocupado > emprestado·(1+tol)`; sob operação normal `ocupado ≤ emprestado`
  (`delta ≤ 0`; ~-1.0 no drill), então **sem falso-positivo**. Fronteira unit-testada. Distribuição
  exata sob carga real = refinamento no e2e civm (não bloqueia).
- **JSONL e2e:** drill qemu broker com `--telemetry-jsonl` → `KTEST-TELEMETRY=ok` (daemon vivo escreve
  a linha na VM isolada).
- **Pendente (civm/GPU):** flag `eviction` sob carga WDDM real (canário disparando) — env-bound (GPU+daemon).

## Checklist de fechamento do gate

- [x] §1 PSI WSL2: idle (0.011) + **carga (14.25)** ✓
- [x] §1 PSI civm: idle/CI (1.237, burst 19.4) + PSI habilitado + PAGE_SIZE 4096 ✓
- [x] §2 RTT/portas nos dois sentidos + **decisão de transporte (port-forward)**
- [x] §3 NBD/TCP: loopback (p50 174 µs) + **cross-host (p50 644 µs, 3 rodadas)** ✓
- [ ] §4 render VRAM/RAM (≥1 cena real do Alex — tester; script validado no host)
- [x] §5 calibração: **`delta_psi=10` proposto** (validar no e2e P1), `streak`=5, `psi_floor`=5 OK
- [ ] **Gate P1 → ABERTO** (falta só §4 render) → libera ITEM-3+ (código P1)
