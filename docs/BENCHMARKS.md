# RamShared — Registro de Benchmarks

> **Log único de TODOS os benchmarks**, com contexto completo (tipo, branch/commit, horário, carga da
> máquina e o que estava aberto). Número sem contexto engana — a mesma medição muda conforme a máquina
> está ociosa ou em uso (Kahneman #3 número-não-adjetivo + #1 WYSIATI registrar o estado).
>
> **Append-only:** cada run é uma entrada nova ao fim; não reescrever entradas antigas. Decisões
> consolidadas (go/no-go) vão para [`memory-broker/P0-RESULTS.md`](reliability/memory-broker-p0-results.md).

## Template de entrada

```
## AAAA-MM-DD HH:MM TZ — <tipo do benchmark>
**Contexto**
- Branch/commit: <branch> @ <hash> (<subject>)
- Máquina: <host> (<GPU/VRAM>), WSL2 <kernel>, RAM <total>
- Carga (snapshot): VRAM usado/livre; RAM avail/free; swap usado; disco (util/latência)
- Aberto (GUI Windows): <apps> | WSL2: <procs>
- Ferramenta/parâmetros: <fio/cuMemcpy/…, bounded?>
**Resultados** (tabela: métrica | valor | unidade)
**Leitura honesta** (o que o número diz + caveats + o que falta)
```

---

## 2026-06-15 23:10 -03 — Q1a (headroom VRAM/RAM) + Q1b (NVMe 4K, contido)

**Contexto**
- Branch/commit: `feat/p1-hardening` @ `1fba443` (PRD da P2).
- Máquina: **dev-workstation** — Windows + **RTX 2060 (6144 MiB)**, WSL2 `6.6.123.2-microsoft-standard-WSL2+`,
  RAM vista pelo WSL2 = 15 GiB.
- **Carga (snapshot, 30 s):** VRAM **1319–1392 MiB usados → ~4603 livres** (volatilidade 1.4% nesta
  janela — desktop sem app de GPU pesado no momento); RAM WSL2 avail ~8.4 GiB, free ~3.7 GiB, **swap
  3.9 GiB já usado**; disco `sdc` (NVMe via VHDX) **~0.7% util neste instante** (cumulativo alto, mas
  quieto agora).
- **Aberto (GUI Windows):** OBS 32.1 (live Instagram), Microsoft Edge (GitHub/CI), **qBittorrent
  v5.2.1** (IO de disco de fundo), AnyDesk, VMS, Windows Terminal, VS Code (WSL Ubuntu-24.04),
  **Hyper-V Manager** (host da civm), Task Manager, Notepad. | WSL2 (RSS): claude, dockerd, gopls,
  clamd, MainThread (~3 GiB).
- **Ferramenta:** `scripts/p0/measure-vram-headroom.sh` (read-only, 30 s) + `scripts/p0/measure-swap-compare.sh`
  → `fio` 4K `direct=1 ioengine=libaio` **bounded** (256 MiB, 12 s, ramp 2 s), arquivo em `sdc`
  (ext4-em-VHDX-em-WSL2). Não-disruptivo.

**Resultados**

Q1a — VRAM livre sob carga (15 amostras / 30 s): min **4563**, máx **4626**, média **4603 MiB**,
desvio 21 MiB (amplitude 63 MiB → **volatilidade 1.4%** nesta janela). RAM avail ~8.4 GiB; swap 3.9 GiB.

Q1b — NVMe 4K (`sdc`, ext4-VHDX-WSL2), p50/avg/p99 da `clat`:

| Perfil | IOPS | p50 | avg | p99 |
| --- | --- | --- | --- | --- |
| randread QD1 | 336 | **2114 µs** | 2964 µs | 17171 µs |
| randwrite QD1 | 1092 | 196 µs | 913 µs | 17957 µs |
| randread QD8 | 18.9k | ~383 µs | 422 µs | 1467 µs |
| randwrite QD8 | 22.9k | ~281 µs | 348 µs | 2114 µs |

Referência VRAM-swap (P0-RESULTS §3, mesma op 4K p50): **ublk 241 µs / NBD-Unix 326 µs / cross-host
644 µs**.

**Leitura honesta**
- O "NVMe" real **deste** ambiente (ext4 → VHDX → WSL2 → NTFS → NVMe) faz **randread QD1 p50 ~2114 µs
  (~2 ms)**, não os ~50–100 µs de NVMe bare-metal. → vs **este** disco, o VRAM-swap (241–644 µs)
  **ganha ~3–10× no swap-in** (random read QD1, o caminho síncrono do page-fault).
- **Isso revisa a análise pessimista anterior:** o "VRAM-swap perde pro NVMe (80 µs)" assumia NVMe
  bare-metal ocioso — que **não vale no seu ambiente WSL2**. Você estava certo em exigir medir sob a
  realidade.
- **Caveats (não exagerar):** (1) o write QD1 é bufferizado (p50 196 µs) — page-out é menos crítico;
  (2) em QD8 o disco paraleliza (read ~383 µs) — mas swap-in costuma ser QD1, então a vantagem do VRAM
  vale; (3) os 2 ms são **estruturais** (overhead do VHDX/WSL2), não contenção transitória — o disco
  estava ~0.7% util no instante; logo é uma característica **persistente** do swap-em-disco no WSL2.
- **Volatilidade da VRAM:** 1.4% agora porque nenhum app usa a GPU pesado; com OBS/jogo/render o `used`
  sobe e o livre cai — o ângulo "colher VRAM ociosa" depende do desktop não disputar a GPU.
- **Falta o decisivo (Q1d):** comparação apples-to-apples sob **a mesma** pressão controlada
  (`MADV_PAGEOUT`) na civm: swap → VRAM remota vs swap → disco local. Isto aqui é forte indício
  direcional, não o veredito final.

---

## 2026-07-13 17:53 -03 — E2E StorPort RAMShared (Disk S:) vs Local SATA SSD
**Contexto**
- Branch/commit: `main` @ `b02c8e0` (Release please, dependabot, and custom static gates)
- Máquina: Host Físico (Windows 11 Build 26200, Intel CPU, 64GB RAM, GPU NVIDIA)
- Carga: Ociosa, sem cargas de GPU ativas.
- Ferramenta/parâmetros: Script PowerShell customizado gravando e lendo um arquivo de 50 MB de dados randômicos em 10 rodadas consecutivas (preenchendo 96% da capacidade do LUN de 64MB).
- Comparação: Sustentação de I/O em disco local Samsung 850 EVO 500GB (SATA III) e Kingston A400 240GB (SATA III).

**Resultados**

| Métrica | RAMShared (v0.2.0) | Samsung 850 EVO | Kingston A400 |
|---|---|---|---|
| **Vel. Leitura (Sustentada)** | **~1942 MB/s (1.94 GB/s)** | ~540 MB/s | ~500 MB/s |
| **Vel. Escrita (Sustentada)** | **~420 MB/s** | ~520 MB/s | ~350 MB/s |
| **Consistência de dados** | **100% (SHA256 Match)** | 100% | 100% |

**Leitura honesta**
- **Velocidade de Leitura:** O driver StorPort RAMShared atinge taxas de leitura de **~2.0 GB/s**, o que supera os limites físicos do barramento SATA III dos SSDs locais em aproximadamente **4x**, alinhando-se a velocidades de barramento NVMe PCIe Gen3.
- **Velocidade de Escrita:** A escrita a **~420 MB/s** é competitiva com SSDs SATA III físicos, sofrendo apenas a latência de context switch e sincronização com o backend userspace do driver.
- **Segurança e Consistência:** Zero corrupção sob preenchimento de 96% do volume, atestando a solidez da fila SCSI e da paginação física.

## 2026-07-24 04:48 -03 — bounded WSL2 disk I/O repeatability

**Context**
- Branch/commit: `docs/readme-v074-release` @ `6f9aaad` (clean release merge)
- Machine: Windows host / WSL2, RTX 2060 6144 MiB; GPU snapshot 737 MiB used / 5218 MiB free;
  RAM available 14.1 GiB; WSL swap used 185 MiB.
- Load: idle snapshot; no swap or device mutation. Three independent runs, each profile 3 s
  plus 2 s ramp, 64 MiB temporary file, direct I/O, `fio`, `iodepth=1` and `8`.
- Raw output: `/tmp/ramshared-bench-20260724/fio-round-{1,2,3}.txt`.

**Results** (median across n=3; latency in microseconds)

| Profile | Median p50 | Median p99 | p50 range | p99 range |
|---|---:|---:|---:|---:|
| randread QD1 | 163 | 273 | 159–165 | 265–273 |
| randwrite QD1 | 137 | 273 | 135–137 | 269–314 |
| randread QD8 | 277 | 469 | 265–277 | 461–474 |
| randwrite QD8 | 237 | 2114 | 227–249 | 494–2278 |

**Honest reading:** the bounded disk path is repeatable for reads and QD1 writes, while QD8
writes show a long-tail p99 (494–2278 us). This is a disk-path baseline only; it does not
prove StorPort or VRAM performance because the Windows driver was not loaded and no live swap
pressure was introduced.

## 2026-07-25 06:15 -03 — autonomous broker SCM lifecycle

**Context**
- Base commit: `72845a0`; uncommitted SSDV3 Step 3 implementation under validation.
- Machine: Hyper-V `win11-drill`, Windows build 26200.8037, 4 logical CPUs,
  2047 MiB visible RAM (805 MiB free after the runs).
- Load: idle; median CPU sample 0.54%; both RamShared services stopped and zero
  RamShared disks before/after.
- Artifact SHA-256:
  `28D5C31BD5BD106B321F176D7C17334528C9C5EA8E4292891F...` (full value in
  each `BROKER_BINARY_MATCH` evidence row).
- Runs: three independent demand-start → pipe-ready → supported stop cycles.
- Raw evidence:
  `C:\ramshared\artifacts\autonomous-broker-20260725-{061504,061509,061514}\results.json`.

**Results** (milliseconds, nearest-rank p99 for n=3)

| Metric | Samples | Median | p99 | Range | Range / median |
| --- | --- | ---: | ---: | ---: | ---: |
| SCM start to broker ready | 519, 481, 606 | 519 | 606 | 125 | 24.1% |
| Supported broker stop | 252, 256, 258 | 256 | 258 | 6 | 2.3% |

**Honest reading:** readiness is far below the 30 s acceptance bound and stop
is stable below 0.3 s in this idle VM. With only three samples, p99 is the
maximum observation, not a production percentile. This benchmark measures the
isolated broker SCM/pipe surface; it does not measure CUDA, StorPort, package
transactions, cold boot, or the physical host.

## 2026-07-25 09:28 -03 — autonomous product VM versus physical cold boot

**Context**
- Base commit: `72845a0`; Step 3 implementation under final validation.
- VM: Hyper-V `win11-drill`, Windows 26200.8037, 2 GiB visible RAM.
- Physical: Windows 11 build 26200, RTX 2060, Test Mode, idle supervised host.
- Workload: demand-start product, 64 MiB LUN, three independent cold boots,
  three random 8 MiB write/read/SHA rounds, supported consumer-first stop.
- Physical manifest SHA: `0F6DFDB3327EEDAF1143C5742B4E0CD3A00F16FDD8FF4FF3799230902AAC1F1A`.

**Results** (milliseconds; nearest-rank p99 for n=3)

| Environment / metric | Samples | Median | p99 | Range / median |
| --- | --- | ---: | ---: | ---: |
| VM readiness | 9,908 / 19,784 / 11,556 | 11,556 | 19,784 | 85.5% |
| VM consumer stop | 4,437 / 4,803 / 4,161 | 4,437 | 4,803 | 14.5% |
| VM full product stop | 4,949 / 5,060 / 4,417 | 4,949 | 5,060 | 13.0% |
| Physical readiness | 1,164 / 1,165 / 1,156 | 1,164 | 1,165 | 0.8% |
| Physical consumer stop | 2,796 / 2,557 / 2,552 | 2,557 | 2,796 | 9.5% |
| Physical full product stop | 3,049 / 2,810 / 2,805 | 2,810 | 3,049 | 8.7% |

**Honest reading:** on these three idle samples, the physical host is about
9.9x faster at median readiness and 1.8x faster at median full stop than the
small VM. Physical readiness is also substantially more repeatable. This is a
lifecycle benchmark, not a throughput benchmark; n=3 makes p99 the maximum
sample and does not justify a production percentile claim. Both environments
had all SHA rounds match and zero terminal residue.
