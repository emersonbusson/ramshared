# Roadmap — RamShared

O caminho executável hoje é o **WSL2**; o destino é o **ring 0 bare-metal**
(ver [MANIFESTO.md](MANIFESTO.md)). Datas são omitidas de propósito — P&D.

## Feito

- **Avaliação dos 6 PRDs** + ambiente real (WSL2/GPU-PV): só o PRD-2 (block device +
  CUDA) é viável no guest; os demais exigem DRM/BAR/DAMON ausentes.
- **Fase 0** (GPU real): eviction WDDM é *data-safe, latency-unsafe* (4K → 1,18 s);
  cascata provada (zram cheio + VRAM absorveu 983 MiB de overflow).
- **SPECv3-WSL2** — convergência via Passo 2.5 (SPEC → SPECv2 → SPECv3): VRAM como
  tier frio + DEMOTE.
- **Port Rust** (6 crates) e **validação de aceitação §14** no sistema vivo (spill
  511 MiB íntegro; DEMOTE 481 MiB migrado, 0 corrupção).
- **Hardening pós-revisão adversarial** (issue #3): C3 (FFI CUDA duplicada removida,
  CLI `forbid(unsafe_code)`), M1/M2/M3/M4/M5 + name-buffer.

## Agora — issue #3 (Fase A, WSL2)

- **C1 — canário dedicado (§9.4):** região-canário com checagem de **conteúdo**
  (sentinela write/read) e **free-floor** (`cuMemGetInfo` periódico), ativando
  `Demote(Corruption)`/`Demote(FreeFloor)` além da latência. _Em esteira SSDV3
  (PRD → SPEC)._
- **H1 — daemon multi-thread / leitor dedicado:** servir o NBD sem
  head-of-line-blocking, encurtando a janela do DEMOTE sob eviction.
- **LOW:** erros tipados (enum) no daemon/cascade; `clap` no parse de args.

## Fase B — kernel custom (WSL2 + kernel próprio)

- `CONFIG_ZRAM_WRITEBACK`: writeback do zram frio direto na VRAM (elimina o salto por
  userspace no caminho frio).
- `ublk` no lugar do NBD (menos cópias, menos context-switch).

## Visão maior — bare-metal (gated em sair do WSL2)

Exploratórios; precisam de DRM/BAR/DAMON/CXL indisponíveis no guest GPU-PV. Cada um
tem PRD:

- **NUMA node** para a VRAM ([`PRD`](docs/vram-as-ram/PRD.md), [`PRD-4`](docs/vram-as-ram/PRD-4.md) com DAMON/tiering proativo).
- **zswap/zpool backend** na VRAM via BAR ([`PRD-3`](docs/vram-as-ram/PRD-3.md)).
- **HMM `DEVICE_PRIVATE` + SDMA + eBPF** ([`PRD-6`](docs/vram-as-ram/PRD-6.md)).
- **CXL / PCIe Gen5** — memória coerente como tier nativo.

## Princípios de avanço

- Cada item estrutural passa pela esteira **SSDV3** (PRD → SPEC → IMPL) e pelas
  **disciplinas Kahneman** (counterfactual + rollback trigger numérico).
- Nada vira swap quente sem evidência de latência; **medir antes de codar** (Fase 0).
- **Day-0:** sem shims; a saída do WSL2 reescreve os caminhos, não os empilha.
