# Runbook — Tenant civm (ITEM-12, e2e cross-host)

Valida o Memory Broker P1 **entre máquinas**: o broker roda no WSL2 (EMEDEV, com a RTX 2060) e
um tenant remoto (a VM de CI `gha-ubuntu-2404`) consome VRAM como swap por NBD/TCP, atravessando
um port-forward no host Windows.

O caminho **Linux↔Linux mesma-máquina** já está validado pelo drill qemu (`scripts/kernel/qemu-broker-drill.sh`,
ITEM-11 = PASS). Este runbook cobre o que o drill **não** cobre: a travessia de rede (NAT + NBD/TCP)
e o transporte TCP do agente (DT-25).

> **Por que runbook e não script automático:** o daemon servindo swap real a um tenant **remoto**
> é justamente o cenário que pode **congelar o WSL2** num teardown malsucedido (swapoff sobre NBD
> morto → I/O em D-state). É uma atividade de operador, executada com a ordem de teardown abaixo e
> supervisão. NÃO rode isto sem entender a §5.

## Topologia (medida no P0, R1)

```
  civm gha-ubuntu-2404                EMEDEV (Windows host)              WSL2 (em EMEDEV)
  LAN 192.168.0.50          LAN 192.168.0.250  ── netsh ──►  vEthernet 172.31.224.1
  (agente / tenant)                portproxy            NAT  ►  WSL2 172.31.230.209
        │                                                            (daemon / broker)
        └──── TCP 192.168.0.250:{7000,10809} ──► forward ──► 172.31.230.209:{7000,10809}
```

- **WSL2 não é nó Tailscale** e está atrás de NAT (civm→WSL2 direto = 100% perda). Tailscale-no-host
  tem cauda ruim (p99 430 ms) — inviável p/ swap. → **port-forward `netsh` no host** (decisão R1).
- Baseline cross-host NBD/TCP medido: **p50 644 µs** (vs Fase B local 241–326 µs). Aceitável p/ swap.

Portas: **7000** = árbitro (control-plane); **10809** = NBD/TCP (data-plane, default NBD).

## Segurança: por que o WSL2 não trava aqui (servidor-only, DT-29)

O freeze que travou o WSL2 (2026-06-09) foi **WSL2-como-consumidor**: um `swapon` num device cujo
backend morreu → I/O do **kernel** em **D-state** ininterruptível. Esse vetor mora no **consumidor
do swap**, não no servidor.

Nesta topologia o WSL2 roda **só o broker** (`run_broker`/`run_broker_ram` **nunca** fazem `swapon`);
quem consome o swap é o **civm**. Então:

- Se algo morrer, o D-state cai no **civm** (VM Hyper-V isolada, recuperável por reboot) — não no host.
- A exposição do WSL2 é só **userspace**: processo matável + fechamento de socket + free da VRAM no
  exit. Não é o D-state de kernel que queimou antes.
- **DT-14** (`-timeout 30`, sem `-persist`) faz o nbd do civm **errar** em vez de pendurar pra sempre.

**Invariante (não viole):** nada de **agente local no WSL2** apontando pro broker local — isso
reintroduz WSL2-como-consumidor e o vetor de freeze. Esse caso fica **qemu-only** (já coberto pelo
drill ITEM-11). Comece pela **Fase A (`--backend ram`)** para validar conectividade sem nem tocar GPU.

## 0. Pré-requisitos

- **WSL2:** binários `target/debug/{ramshared-wsl2d,ramshared-agent}`; `nbd-client`; módulo `nbd`.
- **civm:** binário `ramshared-agent` (copiar via `scp`); `nbd-client`; `nbd.ko` carregável; root.
- **host:** PowerShell admin (UAC desativado nesta máquina); IP LAN = `192.168.0.250`.
- Confirme o IP do WSL2: `ip -4 addr show eth0` (esperado `172.31.230.209`; se mudou, ajuste).

## 1. WSL2 — sobe o broker

Comece pela **Fase A (RAM)** para validar conectividade/control-plane sem risco de GPU; só depois
a **Fase B (VRAM)**.

**Fase A — backend RAM (de-risk, sem GPU):**
```bash
sudo ./target/debug/ramshared-wsl2d --transport nbd --backend ram \
  --slices 2 --slice-mb 64 \
  --sock /run/ramshared/broker.sock \
  --listen-nbd tcp://172.31.230.209:10809 \
  --advertise-nbd 192.168.0.250:10809 \
  --arbiter-listen 172.31.230.209:7000
```

**Fase B — backend VRAM (o produto real):** troque `--backend ram` por `--backend vram` (exige a
RTX 2060; o worker roda o canário §9/§9.4 de residência). Tudo o mais é igual.

Notas:
- `--listen-nbd` faz **bind** no IP do WSL2; `--advertise-nbd` é o que o broker **anuncia** ao agente
  (o endereço do host, que o `netsh` encaminha). DT-25.
- RNF-2: nada de `0.0.0.0`. Use o IP privado do WSL2.
- Espere os logs: `broker (árbitro) em 172.31.230.209:7000` e `em transmissão`.

## 2. Host Windows — port-forward (PowerShell admin)

Encaminha LAN→WSL2 para as duas portas e libera no firewall. Rode no host (use o prefixo `!` nesta
sessão, ou um PowerShell admin):
```powershell
netsh interface portproxy add v4tov4 listenaddress=192.168.0.250 listenport=7000  connectaddress=172.31.230.209 connectport=7000
netsh interface portproxy add v4tov4 listenaddress=192.168.0.250 listenport=10809 connectaddress=172.31.230.209 connectport=10809
New-NetFirewallRule -DisplayName "ramshared-broker" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 7000,10809
```
Confira: `netsh interface portproxy show v4tov4`.

## 3. civm — sobe o agente

Na civm (transporte **tcp** → o broker devolve o endpoint `--advertise-nbd`):
```bash
sudo modprobe nbd nbds_max=8
sudo ./ramshared-agent --broker 192.168.0.250:7000 --tenant civm --transport tcp \
  --nbd-base /dev/nbd --swap-prio -3 --watchdog-secs 120
```
Espere `[agent] registrado: tenant_id=...`.

## 4. Validação

- **No agente (civm):** `[agent] registrado`, depois swap ativo:
  ```bash
  cat /proc/swaps        # deve listar /dev/nbd0 (e nbd1 no próximo tick do árbitro)
  swapon --show
  ```
- **No broker (de qualquer host com rota ao árbitro):** `ramshared-agent --broker 192.168.0.250:7000 --status`
  → o tenant `civm` aparece `present` com as slices atribuídas.
- **Data-plane real (Fase B):** gere pressão na civm e observe page-out indo para a VRAM; o canário
  §9.4 no WSL2 não deve demover sob carga normal (DT-16: latência serve-only).

## 5. Teardown (ORDEM IMPORTA — evita congelar o WSL2)

Sempre **soltar o swap no tenant ANTES de derrubar o broker** (swapoff sobre NBD vivo):
```bash
# 1) civm: solta o swap enquanto o broker ainda serve
sudo swapoff /dev/nbd0 /dev/nbd1 2>/dev/null
sudo nbd-client -d /dev/nbd0; sudo nbd-client -d /dev/nbd1
sudo pkill -INT ramshared-agent          # 2) encerra o agente

# 3) WSL2: agora sim, SIGTERM no broker (DemoteAll + saída limpa, DT-28)
sudo pkill -TERM ramshared-wsl2d

# 4) host: remove o forward
```
```powershell
netsh interface portproxy delete v4tov4 listenaddress=192.168.0.250 listenport=7000
netsh interface portproxy delete v4tov4 listenaddress=192.168.0.250 listenport=10809
Remove-NetFirewallRule -DisplayName "ramshared-broker"
```

> Se inverter (derrubar o broker antes do swapoff), o `/dev/nbdX` fica sem servidor; o kernel da
> civm entra em I/O com timeout de 30 s (DT-14, sem `-persist`) — na civm é recuperável, mas é a
> classe de falha que no WSL2 trava o host. Mantenha a ordem.

## 6. Troubleshooting

- **`Network is unreachable` no agente:** loopback/rota. No drill qemu isto foi loopback DOWN; aqui,
  cheque o `netsh portproxy show` e se o firewall liberou 7000/10809.
- **Registra mas sem swap:** o broker não tem `--listen-nbd`/`--advertise-nbd`, ou o agente está em
  `--transport tcp` sem o broker anunciar TCP. Confira o `SwapOn`/endpoint nos logs.
- **`nbd-client` falha na civm:** módulo `nbd` ausente (`modprobe nbd`) ou porta 10809 não chega
  (teste `nc -vz 192.168.0.250 10809` da civm).
- **Latência alta / demote indevido (Fase B):** veja o canário §9.4 nos logs do WSL2; baseline e
  streak. A calibração `delta_psi` veio do P0 (=10).

## Critério de PASS (ITEM-12)

Tenant civm registra → árbitro assina slice → swap ativo em `/dev/nbdX` na civm via VRAM do WSL2 →
teardown na ordem acima sem deixar swap órfão e sem travar o host. Registrar números (RTT, p50 de
page-out) e anexar ao P0-RESULTS/SPECv2.
