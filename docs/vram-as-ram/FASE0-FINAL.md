---
slug: vram-wsl2-cuda-swap
title: Fase 0 — Veredito Consolidado (A baseline + B eviction + C tiering)
spec: SPECv2-WSL2.md
date: 2026-06-04
status: go-com-pivo-arquitetural
---

# Fase 0 — Veredito Consolidado

Três experimentos rodados nesta máquina (kernel 6.6.114, RTX 2060 6 GiB,
16 GiB RAM, swap VHDX 8 GiB). Ferramenta: `c0deJedi/nbd-vram` (CUDA+NBD) + tooling
próprio (`vramhog`, `memhog`). RAWs: `FASE0-RAW.txt`, `FASE0B-RAW.txt`,
`FASE0C-RAW.txt`.

## TL;DR

**VRAM-as-swap no WSL2/GPU-PV é viável, mas NÃO como swap quente único.** É
**data-safe** porém **latency-unsafe** sob pressão de GPU do host. O uso correto,
**provado empiricamente**, é como **tier de páginas frias atrás de zram**:
`RAM → zram (prio alta) → VRAM (prio média) → VHDX (prio baixa)`.

Veredito: **`go` com pivô arquitetural** — do "VRAM como swap cru de alta
prioridade" (MVP do SPEC-WSL2/SPECv2) para "VRAM como tier frio numa cascata
com zram à frente".

## A) Baseline justo — INCONCLUSIVO (viés de cache inevitável)

3 rodadas, arquivo pré-escrito não-sparse + `drop_caches` (guest).

| 4K QD32 | VRAM | VHDX (host-cached) |
|---|--:|--:|
| randwrite IOPS | ~5.7–6.2k | ~17–19k |
| randwrite p99 | ~17 ms | ~5 ms |
| randread IOPS | ~8.7–9.6k | ~20–22k |
| randread p99 | ~6–12 ms | ~2–8.6 ms |

**Por que inconcluso:** `drop_caches` limpa só o cache do **guest**; o cache do
**host Windows** não é acessível de dentro do WSL2, e o arquivo de 2 GiB coube
nele → baseline VHDX **otimista** (16–22k IOPS 4K = RAM, não disco). O 1º run
(`FASE0-RAW.txt`) teve o viés oposto (VHDX sparse-frio → p99 183 ms, VRAM parecia
12× melhor). A verdade está no meio; **Part C (swap real) é o árbitro.**

## B) Eviction WDDM (§9) — DATA-SAFE, LATENCY-UNSAFE ⚠️

1 GiB pela `nbd-vram` + canário 256 MiB + `vramhog` forçando VRAM a **0 MiB livre**
(alocou +4096 MiB; `cuMemAlloc` **teve sucesso** → WSL2 permitiu oversubscription).

- **Integridade final: hash IDÊNTICO** — sem corrupção. O host paginou a alocação
  para sysmem e trouxe de volta intacta.
- **Latência: 1 sample 4K saltou para 1 183 094 µs (~1,18 s)** vs. ~3–4 ms normal
  (≈330×), recuperando em seguida.

**Conclusão:** o risco no WSL2 **não é perda de dados, é latência**. Uma página de
swap na VRAM pode custar **>1 s** se o host estiver sob pressão de VRAM
(jogo/compositor) → **congela** o processo. Portanto VRAM não pode ser o swap
quente; o canário (§9) deve abortar por **latência**, não só por corrupção.
(Observação: pode ser eviction-repaging e/ou contenção do copy-engine do hog;
operacionalmente dá no mesmo — stall fatal pra swap quente.)

## C) zram-tiering — CASCATA PROVADA ✅

zram 1 GiB (prio 200) > VRAM 1 GiB (prio 100) > VHDX (prio -2). Hog
**incompressível** de 2400 MiB confinado em cgroup (`MemoryMax=400M`). Snapshot
durante a pressão:

```text
/dev/zram0  1024M / 1024M  prio 200   <- zram CHEIO (DATA 1G, COMPR 1023.9M)
/dev/nbd0   1024M /  983M  prio 100   <- VRAM absorveu 983 MiB de spill
/dev/sdc       8G /  1.2G  prio -2    <- VHDX INTOCADO
```

~2 GiB swapou (`pswpout` +513k páginas); zram encheu primeiro, **overflow caiu na
VRAM**, VHDX nunca foi tocado. `swapoff` de ambos sem panic no teardown. **A
cascata funciona exatamente como projetado.**

## Gates do SPECv2

- **GATE-PERF:** passou no 1º run (VRAM > VHDX em escrita/seq); 2º run confundido
  por host-cache. Líquido: VRAM é **competitiva**, com vantagem clara em
  **escrita/seq** e fraqueza em **latência de leitura quente**.
- **GATE-RESIDENCIA:** **condicional** — data-safe, mas latência sob pressão exige
  que o canário aborte por latência (gatilho (b) da §9.3 confirmado). VRAM como
  swap **quente único** = reprovado. VRAM como tier **frio** = aprovado.

## Recomendação (pivô arquitetural)

1. **Mudar o MVP** de "VRAM = swap cru prio 32767" para a **cascata**
   `zram (hot) → VRAM (cold, prio média) → VHDX (prio baixa)`. Atualizar §1/§6 do
   SPECv2 e promover a §16 a arquitetura principal. Isso joga a favor das forças
   medidas da VRAM (bandwidth/escrita/capacidade) e esconde a fraqueza (latência
   de leitura sob pressão).
2. **§9 obrigatória:** canário com **abort por latência** (p99 da leitura de swap
   > limiar por N amostras → degradar/remover a VRAM do pool sem matar processos).
3. **Pendência menor:** baseline justo real (impossível defeitar o host-cache de
   dentro; medir via swap real com `pswpin/out` por tier já dá o número honesto —
   feito na Part C).
4. **`CONFIG_ZRAM_WRITEBACK` não setado** neste kernel → a integração mais
   elegante (zram escreve frio direto na VRAM) exigiria kernel custom. A cascata
   por **prioridade** (validada na Part C) não exige nada disso — é o caminho Day-0.

## Estado da máquina

Limpo. Todo teardown OK; `swapon --show` final = só `/dev/sdc` (8 GiB, prio -2).
Artefatos do experimento em `/home/emdev/fase0/` (fora do repo).
