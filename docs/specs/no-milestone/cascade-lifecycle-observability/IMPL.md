# IMPL — Cascade lifecycle observability

> SSDV3 Step 3 · SPEC: [`SPEC.md`](SPEC.md) · PRD: [`PRD.md`](PRD.md)

## Status

**implemented** · gates: unit ✓ · clippy -D warnings ✓ · cover lifecycle **94.65% lines / 87.95% regions** ✓ · E2E live ✓

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-cli/src/cascade/lifecycle.rs` | ITEM-1 | Pure phase + JSON + 15 tests |
| `crates/ramshared-cli/src/cascade/mod.rs` | ITEM-2 | `status(bool)`, snapshot, daemon pid |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | ITEM-2 | `status(false)` after up/down |
| `crates/ramshared-cli/src/main.rs` | ITEM-2 | `status --json` usage |
| `scripts/safety/cascade-health.sh` | ITEM-4 | `phase` / `demote` / `thresholds_kib` from CLI |
| `docs/FAQ.md`, `README.md`, `ARCHITECTURE.md` | ITEM-5 | human docs |
| `validation.md` | ITEM-6 | live sample |

## ITEM-3

Demote counters: **skipped** — `demote.total` / `last_reason` null; `in_progress` only via injectable snapshot in unit tests.

## Validation (numbers)

- tests: `cargo test -p ramshared-cli` → **63 passed** (15 lifecycle)
- clippy: `cargo clippy -p ramshared-cli --all-targets -- -D warnings` → exit 0
- cover: `cargo llvm-cov -p ramshared-cli --summary-only`  
  - `lifecycle.rs` **94.65%** lines (SPEC ≥80%)  
  - package total ~63% (CLI main not in cover gate)
- E2E live 2026-07-14:
  - `ramshared status` → `phase: UsingZram` (zram used_kib≈42080 ≥ 1024; vram 176)
  - `status --json` parses; `ok:true`, `order_ok:true`, daemon pid set
  - `cascade-health.sh` → `phase=UsingZram` `phase_reason=zram_used_ge_threshold`

## Gaps

- Daemon demote export still future (SPEC optional ITEM-3).
- Health parses status JSON four times with python (acceptable at 30s loop).

## Rollback trigger

- Healthy cascade (ok, order 200>100>-2, vram used &lt;1 MiB, daemon alive) reporting phase ≠ Armed|UsingZram for ≥3 samples → `RAMSHARED_STATUS_LEGACY=1` or revert.
- status p99 &gt; 5s idle → revert.
