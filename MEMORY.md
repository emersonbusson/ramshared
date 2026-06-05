# MEMORY — RamShared

Memória compartilhada da sessão (regra: [`.claude/rules/coding.md`](.claude/rules/coding.md) §Session Memory).
Ler de **baixo para cima** (entrada mais recente no fim). **Append-only**: nunca
apagar/reescrever entradas antigas. Nunca gravar secrets nem endereços que vazem KASLR.

---

## 2026-06-05 — vram-as-ram: SPECv3 + scaffold Rust + import de método

- **`docs/vram-as-ram/`** convergiu para **SPECv3-WSL2** (cascata
  `zram(200) → VRAM(100) → VHDX`; VRAM é tier **frio**, não swap quente; DEMOTE
  por latência §9). v1/v2 preservados como superseded. Esteira aplicada:
  SPEC → Passo 2.5 → SPECv2 → Passo 2.5 → SPECv3.
- **Fase 0** medida em GPU real (RTX 2060, WSL2/GPU-PV): eviction WDDM é
  **data-safe mas latency-unsafe** (4K → 1,18 s sob pressão); tiering **provado**
  (zram 1 GiB cheio + VRAM absorveu 983 MiB de spill, VHDX intocado).
  Ver [`docs/vram-as-ram/FASE0-FINAL.md`](docs/vram-as-ram/FASE0-FINAL.md).
- **Passo 3 (Rust):** `crates/ramshared-tier` (cascata + invariante A1) e
  `crates/ramshared-cuda` (wrapper `libcuda`, **roundtrip validado em GPU real**)
  — `fmt`/`clippy -D warnings`/testes verdes. Próximo: `ramshared-block` (NBD §8),
  depois daemon `ramshared-wsl2d`.
- **Método importado do advoq** (fechou cargo-cult de disciplinas que citavam
  arquivos inexistentes): `docs/postmortems/TEMPLATE.md` (#7),
  `docs/reliability/DEGRADATION-MATRIX.md` (#5), `docs/methodology/SUPERPROMPT.md`
  (#14). `SSDV3-PROMPTS.md` em de-web (era port superficial; **regra: o que não
  existe em kernel — SQL/DDL/migrations/endpoints/SDK — sai, não se traduz**).
- **Pendente:** terminar de-web do `SSDV3-PROMPTS.md`; #9 (gatilhos web→kernel no
  `ssdv3.md`, exige sync `CLAUDE.md`/`AGENTS.md`); `docs/decisions/` + ADRs
  (ainda não existem, citados pelas disciplinas); `docs/LIBRARIES.md`.
