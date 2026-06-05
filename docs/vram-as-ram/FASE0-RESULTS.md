---
slug: vram-wsl2-cuda-swap
title: Fase 0 — Resultados do GATE-PERF (SPECv2-WSL2)
spec: SPECv2-WSL2.md
date: 2026-06-04
status: go-condicional
---

# Fase 0 — Resultados (GATE-PERF)

## Ambiente

```text
kernel:   6.6.114.1-microsoft-standard-WSL2
GPU:      NVIDIA GeForce RTX 2060, 6144 MiB (~4.2 GiB livres no teste)
RAM/swap: 16 GiB / 8 GiB VHDX (/dev/sdc, prio -2, ~1.2 GiB em uso)
root FS:  /dev/sdd ext4 (VHDX no NVMe do host)
ferramenta: c0deJedi/nbd-vram (CUDA Driver API + NBD), daemon 1 GiB
método:   ioping (4K QD1), fio (4K QD32 libaio O_DIRECT), dd (seq O_DIRECT)
rodadas:  1 (idle). FALTA: 3 rodadas + baseline cold-cache (ver Caveats).
```

A `nvidia-smi` não estava no PATH do root (rodou como root sem `/usr/lib/wsl/lib`),
mas o daemon fez `cuInit`+`cuMemAlloc(1 GiB)`+NBD com sucesso — GPU OK.

## Números (medidos)

| Métrica | VRAM (NBD/CUDA) | Baseline VHDX (`/dev/sdd`) | Vencedor |
|---|---:|---:|---|
| 4K read latência QD1 (ioping avg) | **448 µs** | 13 µs ⚠️cache | VHDX (cache) |
| 4K randread IOPS QD32 | 9 491 | 190 000 ⚠️cache | VHDX (cache) |
| 4K randread p99 QD32 | **5.93 ms** | 1.19 ms ⚠️cache | VHDX (cache) |
| 4K randwrite IOPS QD32 | **6 324** | 2 392 | **VRAM 2.6×** |
| 4K randwrite p50 QD32 | **3.95 ms** | 7.18 ms | **VRAM 1.8×** |
| 4K randwrite **p99** QD32 | **14.7 ms** | 183 ms | **VRAM 12.5×** |
| seq write (dd O_DIRECT) | **489 MB/s** | 77.6 MB/s | **VRAM 6.3×** |
| seq read (dd O_DIRECT) | **702 MB/s** | 201 MB/s | **VRAM 3.5×** |

⚠️ = baseline servido pela **page cache do host Windows** (190k IOPS 4K só é
possível em RAM), logo **otimista** para leitura. As escritas O_DIRECT do baseline
podem estar **pessimistas** (expansão de VHDX sparse / writeback stalls → p99 183 ms).

## Leitura dos dados (anti-System-1)

A expectativa inicial ("VRAM ~450 µs parece lento → provável abort") estava
**errada**. O que decide swap não é a latência QD1 isolada, é o **caminho de
swap-out (escrita) sob pressão** e sua **cauda**:

- **Escrita (swap-out):** VRAM ganha de forma decisiva — 2.6× IOPS, p50 1.8×
  melhor e, crucial, **p99 12.5× melhor (14.7 ms vs 183 ms)**. Cauda de escrita
  é o que congela a aplicação no swap-out; VRAM é muito mais consistente.
- **Sequencial:** VRAM ganha 3.5–6.3× (read/write).
- **Leitura (swap-in):** o baseline "ganha", mas é **cache de RAM do host**, não
  disco. Contra NVMe frio o resultado seria outro; mesmo assim o p99 de leitura
  da VRAM (5.93 ms QD32) é o ponto mais fraco e tem headroom (daemon de referência
  é **síncrono single-thread**; o modelo async/multi-stream da §8 do SPEC deve
  reduzir esse p99).

A claim bare-metal da referência ("27× melhor latência que NVMe") **não
transfere** para WSL2 — o round-trip GPU-PV custa ~450 µs por op (confirma a
§3.2.1). O valor no WSL2 está em **throughput de escrita e cauda**, não latência.

## GATE-PERF

Definição do SPECv2 §3.2: `W99_vram > W99_vhdx → ABORT`, conforto se
`W99_vram ≤ 0.8·W99_vhdx`.

```text
W99_vram (write p99) = 14.7 ms
W99_vhdx (write p99) = 183 ms
14.7 ≤ 0.8 × 183 (146)  →  GO confortável no caminho de escrita.
```

### Veredito: `go` condicional

VRAM-as-swap via NBD/CUDA no WSL2/GPU-PV é **viável e superior ao swap VHDX no
caminho que mais importa para swap** (swap-out/escrita). Não é abort.

**Condições para `go` duro (antes de construir o daemon de produção):**
1. **Re-rodar com baseline justo:** `echo 3 > /proc/sys/vm/drop_caches` antes das
   leituras e arquivo **pré-alocado e pré-escrito** (não-sparse) para escrita —
   remove os dois vieses (cache/sparse). 3 rodadas, reportar stddev.
2. **GATE-RESIDENCIA (§9) — agora é o gatekeeper real:** rodar o probe de eviction
   WDDM (induzir pressão de VRAM no host Windows com a alocação viva) e verificar
   se canário detecta antes de corrupção. Risco de **dado**, supera o de perf.
3. Confirmar p99 de **leitura** aceitável com o modelo async da §8 (ou aceitar a
   cascata zram-tiering da §16, que esconde leitura quente em RAM comprimida).

## Próximos passos recomendados

- Curto: re-rodar Fase 0 com baseline justo (item 1) — barato.
- Médio: implementar o probe de residência (§9) — decide segurança de dados.
- Forte candidato de arquitetura (§16): **zram (hot, RAM) + VRAM (cold/write, prio
  alta) + VHDX (frio, prio baixa)** — joga a favor das forças medidas da VRAM
  (escrita/seq/cauda) e esconde a fraqueza (latência de leitura quente).
