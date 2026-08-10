# IMPL — Benchmark and validation evidence integrity

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/benchmark-evidence-integrity/SPEC.md`

## Status

implemented · cover N/A (Node) · E2E ✓ · BINARY_MATCH N/A

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `docs/benchmarks/evidence.schema.json` | ITEM-1 / RF-1–RF-4 | Versioned public evidence envelope. |
| `tools/ci/check-benchmark-evidence.mjs` | ITEM-2/3 / RF-1–RF-6, RF-9 | Bounds, sanitization, fingerprints, statistics, artifacts and parity. |
| `docs/benchmarks/{legacy-unqualified,benchmark-map}.json` | ITEM-3 / RF-5/6 | Honest one-to-one mapping without historical fabrication. |
| `tools/ci/check-spec-evidence.mjs` | ITEM-4 / RF-7–RF-9 | Explicit fail-closed claim manifest validation. |
| `docs/specs/evidence-manifest.schema.json` | ITEM-4 / RF-7–RF-9 | Claim manifest contract. |
| `scripts/docs-check.sh` | ITEM-5 / RF-10 | Runs tests and both live repository validators. |

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

## Gaps

closed for the benchmark evidence and explicit claim-manifest slice. Existing
features without a claim manifest remain unqualified; the separate
documentation-governance slice owns migration and index presentation.

## Rollback trigger

One invalid record accepted, one sensitive diagnostic, one forged statistic,
one false DONE, or nondeterministic output for identical input.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1–RF-10 | ITEM-1–ITEM-5 | pending — no automatic commit |
