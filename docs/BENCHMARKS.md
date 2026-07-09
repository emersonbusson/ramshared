# RamShared — Registro de Benchmarks

> **Log único de TODOS os benchmarks**, com contexto completo (tipo, branch/commit, horário, carga da
> máquina e o que estava aberto). Número sem contexto engana — a mesma medição muda conforme a máquina
> está ociosa ou em uso (Kahneman #3 número-não-adjetivo + #1 WYSIATI registrar o estado).
>
> **Append-only:** cada run é uma entrada nova ao fim; não reescrever entradas antigas. Decisões
> consolidadas (go/no-go) vão para [`memory-broker/P0-RESULTS.md`](memory-broker/P0-RESULTS.md).

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
