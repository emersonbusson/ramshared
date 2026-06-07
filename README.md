# RamShared

VRAM de GPU como tier de **swap** no Linux/WSL2. Em cargas não-gráficas a VRAM passa
~90% ociosa enquanto a RAM estoura e o sistema faz swap para SSD — dezenas de vezes
mais lento que a VRAM. O RamShared põe a VRAM no meio desse caminho.

## Abordagem

A VRAM **não** é swap quente. Sob pressão, a eviction do WDDM (WSL2/GPU-PV) preserva
os dados mas injeta latência: uma leitura 4K mediu **1,18 s** com a VRAM cheia —
*data-safe, latency-unsafe*. Por isso ela entra como tier **frio** numa cascata por
prioridade de `swapon`:

```text
pressão de memória ─► zram  (RAM comprimida, lzo-rle)  prio 200  HOT
                   ─► VRAM  (CUDA + NBD)               prio 100  COLD
                   ─► VHDX  (swap do WSL2)             prio  -2  LAST
```

O zram absorve o working set quente; a VRAM pega só o spill frio (esconde a fraqueza
de latência, usa a força de capacidade/banda). Um **canário** demove a VRAM
(`swapoff`) sob spike de latência, sem derrubar processos.

## Status

Validado end-to-end no WSL2 (RTX 2060), pressão confinada por cgroup v2:

- **spill:** 511 MiB caíram na VRAM, **332.800 páginas íntegras**;
- **DEMOTE:** 481 MiB vivos migraram VRAM→VHDX via `swapoff`, **0 corrupção**.

Evidência: [`docs/vram-as-ram/VALIDATION-CASCADE.md`](docs/vram-as-ram/VALIDATION-CASCADE.md).

## Uso

> **WSL2:** builds Rust pesados podem travar o ambiente — mantenha o `cargo` escopado
> por crate, sem `--release`.

```bash
cargo build -p ramshared-cli -p ramshared-wsl2d

sudo ./target/debug/ramshared check        # preflight: WSL2/CUDA/kernel/tiers
sudo ./target/debug/ramshared up --vram 1024 --zram 1024
swapon --show                              # zram(200) > nbd0(100) > vhdx(-2)
sudo ./target/debug/ramshared down         # swapoff antes do disconnect (anti-panic)
```

## Estrutura (7 crates; exceção userspace gated)

| Crate | Papel |
|---|---|
| `ramshared-tier` | prioridades da cascata + rede de segurança A1 do DEMOTE |
| `ramshared-cuda` | CUDA Driver API via `dlopen` (**único `unsafe` do projeto**) |
| `ramshared-block` | protocolo NBD fixed-newstyle + I/O (lib pura) |
| `ramshared-integrity` | checksum + padrões de teste |
| `ramshared-uring` | wrapper seguro sobre `io-uring` para a Fase B |
| `ramshared-wsl2d` | daemon: máquina de estados, `VramBackend`, canário/DEMOTE |
| `ramshared-cli` | `check`/`doctor`/`up`/`down`/`status` |

Nota Fase B: o backend `ublk` aprovou uma exceção userspace gated para a crate
`io-uring` (ADR-0004). Ela entrou via `ramshared-uring` apenas para o smoke mínimo do ring
e só permanece se o bench ublk vs NBD provar ganho.

## Documentação

- [ARCHITECTURE.md](ARCHITECTURE.md) — arquitetura, componentes e fluxo
- [ROADMAP.md](ROADMAP.md) — onde esteve e para onde vai
- [MANIFESTO.md](MANIFESTO.md) — princípios (bare-metal first)
- [`docs/vram-as-ram/SPECv3-WSL2.md`](docs/vram-as-ram/SPECv3-WSL2.md) — spec ativo
- [`docs/methodology/`](docs/methodology/) — SSDV3 + disciplinas Kahneman
- [CLAUDE.md](CLAUDE.md) / [AGENTS.md](AGENTS.md) / [`.claude/rules/`](.claude/rules/) — regras

## Requisitos

Rust (edition 2024). WSL2 com NVIDIA via GPU-PV (`/dev/dxg` + `libcuda`),
`CONFIG_BLK_DEV_NBD`, `CONFIG_ZRAM`, `nbd-client`, `zramctl`.

## Aviso

Projeto de P&D. Mexe em **swap real** e na **GPU** — rode confinado e com cuidado.
Política **Day-0**: sem shims nem workarounds; cada mudança é a versão definitiva.
