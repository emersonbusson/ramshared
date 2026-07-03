# Black-Box Forense & Segurança — RamShared no WSL2

> Medidas de segurança que (a) fazem o daemon **falhar-seguro** em vez de travar o host e
> (b) **capturam automaticamente** toda evidência de travamento pra debug. Motivado por dois
> travamentos reais em 2026-07-03 (ver `MEMORY.md`): o #1 deu `kernel BUG` (mlockall×dxgkrnl),
> o #2 quase não deixou rastro e exigiu escavação manual no Event Log do Windows. Complementa
> o [`DEGRADATION-MATRIX.md`](DEGRADATION-MATRIX.md).

## Honestidade de escopo

Não dá pra garantir **zero** travamento de causas fora do nosso código (host Windows, driver
NVIDIA, Hyper-V). O que este sistema garante: (1) o **nosso** daemon não seja a causa
(preflight gate + fix do mlockall), e (2) **qualquer** travamento deixe evidência durável e
automática. Prevenção + caixa-preta, não escudo mágico.

## Componentes (`scripts/safety/`)

| Arquivo | Papel |
| --- | --- |
| `preflight.sh` | **Portão falha-seguro** antes de subir o daemon: RECUSA (exit≠0, o daemon nem inicia) se o binário não tiver o fix do mlockall, se a GPU não responder, se faltar VRAM, ou se houver `/dev/ublkb*` órfão. Roda o snapshot no sucesso. |
| `preflight-snapshot.sh` | Baseline "estado bom" antes de um start arriscado (git commit, `nvidia-smi`, mem/swap, `.wslconfig`, cmdline) + **arma** o coletor (`.armed`). |
| `postmortem.sh` | **Coletor forense**: `postmortem.sh --auto` (no boot) coleta se o boot anterior teve `kernel BUG`/Oops/OOM OU marcador `.armed`. Junta journal, sinais de crash, **Windows Event Log** (Kernel-Power 41=host travou, TDR 4101=GPU crashou, Hyper-V-VmSwitch=restart), console durável do kernel, estado GPU/mem/swap. Idempotente. |
| `kmsg-recorder.sh` | Espelha `/dev/kmsg` (`dmesg --follow`) pra `C:\wsl-forensics\kernel-console.log` em tempo real — caixa-preta host-side que pega o **call trace completo** do BUG que o journald perde ao congelar. |
| `install.sh` | Instala tudo (unidades systemd + drop-in journald) de forma idempotente. NÃO habilita o `ramsharedd` (rollout supervisionado); habilita só os serviços de segurança. |

Unidades systemd em `scripts/safety/systemd/` (versionadas no repo):
`ramshared-kmsg-recorder.service` (recorder, ativo no boot), `ramshared-postmortem.service`
(coletor oneshot no boot), `ramsharedd.service` (com `ExecStartPre=preflight.sh`),
`10-ramshared-persistent.conf` (journald `Storage=persistent`).

## Onde a evidência mora

**`/mnt/c/wsl-forensics/`** — NTFS do host, **sobrevive à morte da VM** (ao contrário do
`/var` do guest). Contém: `postmortem-<ts>-boot<N>.md` (relatórios), `snapshot-<ts>.md`
(baselines), `kernel-console.log` (console vivo), `kernel-console.prev.log` (console do boot
que travou).

## Como usar

```bash
# Instalar (uma vez):
sudo bash scripts/safety/install.sh

# Coletar forense à mão de um boot específico (ex.: o boot anterior):
bash scripts/safety/postmortem.sh -1        # ou -2, -3...

# Antes de um start arriscado manual, o gate roda sozinho via ExecStartPre; à mão:
bash scripts/safety/preflight.sh            # exit 0 = seguro; exit 1 = RECUSADO
```

Após qualquer travamento: reinicie o WSL2 e o `ramshared-postmortem.service` gera o relatório
automaticamente no boot seguinte, em `/mnt/c/wsl-forensics/`.

## Validação (feita em 2026-07-03)

- Coletor provado contra os **dois travamentos reais**: boot `-2` (kernel BUG) → veredito
  "CRASH detectado" + linha exata `mm/memory.c:2345`; boot `-1` (morte abrupta sem BUG) →
  "sem assinatura de crash" + explicação WSL2 correta. Bate com a investigação manual.
- Preflight gate: passa com binário com-fix; **RECUSA** binário sem o fix (o que travaria o
  host). `--auto` idempotente + consumo do marcador `.armed` testados.
- Recorder ativo escrevendo o console do kernel em `/mnt/c` em tempo real; journald
  `Storage=persistent` confirmado.

## Pendente (fora do MVP)

- **Auto-reboot em vez de hang**: hoje `panic=-1` + `BUG_ON` com lock preso = hang eterno.
  Avaliar `oops=panic panic=10` no `.wslconfig` pra VM reiniciar sozinha + logs capturados —
  muda comportamento de panic do kernel todo, **testar em qemu antes**.
