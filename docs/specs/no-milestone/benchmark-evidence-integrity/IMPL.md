# IMPL — Benchmark and validation evidence integrity

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/benchmark-evidence-integrity/SPEC.md`

## Status

partial · Node validators complete · runtime monitor fix source-tested · deployed E2E pending

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `docs/benchmarks/evidence.schema.json` | ITEM-1 / RF-1–RF-4 | Versioned public evidence envelope. |
| `tools/ci/check-benchmark-evidence.mjs` | ITEM-2/3 / RF-1–RF-6, RF-9 | Bounds, sanitization, fingerprints, statistics, artifacts and parity. |
| `docs/benchmarks/{legacy-unqualified,benchmark-map}.json` | ITEM-3 / RF-5/6 | Honest one-to-one mapping without historical fabrication. |
| `tools/ci/check-spec-evidence.mjs` | ITEM-4 / RF-7–RF-9 | Explicit fail-closed claim manifest validation. |
| `docs/specs/evidence-manifest.schema.json` | ITEM-4 / RF-7–RF-9 | Claim manifest contract. |
| `scripts/docs-check.sh` | ITEM-5 / RF-10 | Runs tests and both live repository validators. |
| `crates/ramshared-cli/src/monitor.rs` | ITEM-6 / RF-11 | Runtime panel refuses legacy/unqualified reports and recomputes displayed metrics from samples in promotable v1 evidence. |

## Validation (numbers)

- benchmark tests: `node --test tools/ci/check-benchmark-evidence.test.mjs` → exit 0; 11 passed, 0 failed.
- claim tests: `node --test tools/ci/check-spec-evidence.test.mjs` → exit 0; 7 passed, 0 failed.
- benchmark live gate: `node tools/ci/check-benchmark-evidence.mjs --check` → exit 0; 5 sections, 3 records, 5 legacy markers.
- SPEC manifest live gate: `node tools/ci/check-spec-evidence.mjs --check` → exit 0.
- docs E2E: `./scripts/docs-check.sh` twice → exit 0; byte-identical output.
- cover: N/A — pure Node business logic is exercised through named built-in
  test-runner fixtures; Rust slice coverage does not apply.
- E2E: before 5 prose / 3 unmapped pre-schema rows → action validators → after
  5/5 sections and 3/3 rows mapped, all historical results non-promotable.
- Runtime evidence consumer: the legacy Build #5 `latest.json` now returns
  `AWAITING_QUALIFICATION`; only a clean, promotable v1 PASS with a qualified
  comparison, binary match, completed cleanup, no residue, and consistent
  samples may populate the panel.
- Runtime tests: `cargo test -p ramshared-cli -j 1 monitor_benchmark_` → 4
  passed; full CLI suite → 345 unit + 10 dispatch passed; strict Clippy passed;
  `monitor.rs` line coverage → 88.7% (2,033/2,292); formatting and whitespace
  checks passed.

## Gaps

The Node benchmark validators and claim-manifest slice are complete. ITEM-6
adds the runtime monitor consumer, and its source tests and line-coverage gate
pass. The changed parser has not passed a deployed `BINARY_MATCH` check because
the corrected source has not been built and installed in the deployment
environment. No promotable three-tier stress record exists yet, so the panel
must remain `AWAITING_QUALIFICATION`.

## Rollback trigger

One invalid record accepted, one sensitive diagnostic, one forged statistic,
one false DONE, or nondeterministic output for identical input.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1–RF-10 | ITEM-1–ITEM-5 | earlier implementation commits in branch history |
| RF-11 | ITEM-6 | `5e4d8289`, `4ebc75fa` |
