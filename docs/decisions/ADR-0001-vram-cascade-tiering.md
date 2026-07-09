# ADR-0001 — Cascata de swap zram → VRAM → VHDX (VRAM como tier frio)

**Status:** Accepted (2026-06-05). Supersede o MVP de "VRAM como swap cru de alta
prioridade" (SPEC-WSL2 v1).

## Context

Objetivo: usar VRAM ociosa como memória do sistema no WSL2/GPU-PV (RTX 2060). A
Fase 0 ([`../reliability/wsl2-fase0-final.md`](../reliability/wsl2-fase0-final.md)) mediu em
GPU real:

- Eviction WDDM: dado **sobrevive** (hash íntegro), mas uma leitura 4K sob VRAM
  cheia custou **1 183 094 µs (~1,18 s)** vs. ~3–4 ms normal.
- Cascata por prioridade de `swapon` funciona: zram 1 GiB encheu, a VRAM absorveu
  **983 MiB** de overflow, VHDX intocado.

Conclusão: VRAM é **data-safe mas latency-unsafe** sob contenção de GPU do host.

## Decision

A VRAM **não** é swap quente. É um **tier frio** numa cascata por prioridade:
`zram (200) → VRAM (100) → VHDX (−2)`. zram (RAM comprimida) absorve o working set
quente; a VRAM pega só o spill frio. Um canário de latência **demove** a VRAM
(swapoff só dela; páginas caem pro VHDX) sob eviction, sem matar processos.

## Consequences

- (+) Usa a força da VRAM (bandwidth/capacidade) e esconde a fraqueza (latência sob pressão).
- (+) Day-0: cascata por prioridade nativa, sem kernel custom.
- (−) Exige zram e a invariante A1 (DEMOTE só é seguro com um tier abaixo da VRAM).

## Alternatives considered

- **VRAM como swap quente (prio máxima):** rejeitado — latency-unsafe (1,18 s congela o processo).
- **zram com writeback para VRAM:** exige `CONFIG_ZRAM_WRITEBACK` (não setado) → kernel custom; fica como Fase B.
- **NUMA hotplug / HMM `DEVICE_PRIVATE`:** impossível no WSL2 GeForce consumer
  (`nvidia_p2p_*` → `EINVAL`; sem controle DRM no guest).

## Kahneman

- #5 worst-case (eviction WDDM medida, não suposta) · #3 número (1,18 s; 983 MiB)
  · #2 counterfactual (rollback abaixo).

## Rollback trigger

Reverter para swap VHDX-only se, num re-teste de 3 rodadas, o p99 de leitura do
tier VRAM sob pressão real exceder o p99 do VHDX **e** o canário (§9) falhar em
detectar a eviction antes de qualquer divergência de hash.

Links: [`../specs/no-milestone/wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §1, §9 ·
[`../reliability/DEGRADATION-MATRIX.md`](../reliability/DEGRADATION-MATRIX.md).
