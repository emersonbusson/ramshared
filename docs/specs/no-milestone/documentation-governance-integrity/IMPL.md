# IMPL — Documentation governance integrity

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/documentation-governance-integrity/SPEC.md`

## Status

implemented · cover ✓ · E2E ✓ · BINARY_MATCH N/A

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `docs/DOCUMENTATION-PARITY.md`, `docs/reference/REFERENCE-INDEX.md` | ITEM-1/2 / RF-1–RF-3 | Canonical ownership matrix and question router. |
| `docs/governance/` contracts | ITEM-1/3/4/6 / RF-3–RF-6 | Claim, provenance, allowlist, baseline, and deterministic journey records. |
| `tools/ci/check-documentation-governance.mjs` | ITEM-2–ITEM-7 / RF-1–RF-8 | Fail-closed parity, claims, provenance, redundancy, journey, and postmortem checks. |
| `tools/generate-docs-index.mjs` | ITEM-3 / RF-3 | Index status derives from evidence-qualified claims instead of file presence. |
| `tools/ci/check-validation-schema.mjs` | ITEM-7 / RF-6–RF-8 | Strict schema-1 closure records plus controlled public-provenance redactions. |
| `scripts/docs-check.sh`, `.github/workflows/ci.yml` | ITEM-8/9 / NFR-1–NFR-5 | Named tests, per-file coverage, legitimate repository gate, and deterministic output. |

## Validation (numbers)

- governance tests: `node --test tools/ci/check-documentation-governance.test.mjs` → exit 0; 34 passed, 0 failed.
- governance cover: per-file Node coverage → lines 100.00%, branches 82.87%, functions 100.00%.
- validation tests: `node --test tools/ci/check-validation-schema.test.mjs` → exit 0; 14 passed, 0 failed.
- validation cover: per-file Node coverage → lines 86.67%, branches 88.24%, functions 96.67%.
- index tests: `node --test tools/generate-docs-index.test.mjs` → exit 0; 7 passed, 0 failed; coverage 96.72% lines, 80.26% branches, 100.00% functions.
- legitimate E2E: `node tools/ci/check-documentation-governance.mjs --all` → exit 0; 300 structural files, 0 findings.
- deterministic E2E: two runs produced SHA-256 `e34f89b1a478e9e401a3a5fe84070246630b4b833ff479c1be8a4208f98b2cc7`.
- refusals: false DONE, stale artifact, missing BINARY_MATCH, private provenance, sensitive diagnostics, broad allowlist, copied normative blocks, unbounded journey, and prose-only postmortem all returned non-zero in named fixtures.
- docs E2E: `./scripts/docs-check.sh` twice → exit 0 with byte-identical 25-line output.
- BINARY_MATCH: N/A — repository-only documentation tooling.

## Gaps

closed for documentation ownership, claim qualification, provenance,
redundancy, journey, and validation-schema governance. Individual product
surfaces remain in their own evidence state and are not promoted by this slice.

## Rollback trigger

One false DONE promotion, one sensitive matched value in diagnostics, one
automatic source rewrite, or nondeterministic output for identical input.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1–RF-8, NFR-1–NFR-5 | ITEM-1–ITEM-9 | pending — no automatic commit |
