# IMPL — Documentation governance integrity

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/documentation-governance-integrity/SPEC.md`

## Status

partial · cover ✓ · E2E ✓ · BINARY_MATCH N/A

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `docs/DOCUMENTATION-PARITY.md`, `docs/reference/REFERENCE-INDEX.md` | ITEM-1/2 / RF-1–RF-3 | Canonical ownership matrix and question router. |
| `docs/governance/` contracts | ITEM-1/3/4/6 / RF-3–RF-6 | Claim, provenance, allowlist, baseline, and deterministic journey records. |
| `tools/ci/check-documentation-governance.mjs` | ITEM-2–ITEM-7 / RF-1–RF-8 | Fail-closed parity, claims, provenance, redundancy, journey, and postmortem checks. |
| `tools/generate-docs-index.mjs` | ITEM-3 / RF-3 | Index status derives from evidence-qualified claims instead of file presence. |
| `tools/ci/check-validation-schema.mjs` | ITEM-7 / RF-6–RF-8 | Strict schema-1 closure records plus controlled public-provenance redactions. |
| `scripts/docs-check.sh`, `.github/workflows/ci.yml` | ITEM-8/9 / NFR-1–NFR-5 | Named tests, per-file coverage, legitimate repository gate, and deterministic output. |
| `tools/ci/generate-capability-observations.{mjs,test.mjs}` | DT-18 / RF-8 | Ignore untracked empty slugs, preserve a tracked README-only surface, and refuse a dangling named document entry. |
| `docs/governance/{CAPABILITY-OBSERVATIONS.md,capability-observations.generated.json,claims.json}` | DT-18 / RF-8 | Document deterministic discovery, regenerate the passive catalog, and demote the affected claim pending same-revision hosted admission. |
| `evidence-manifest.json`, root `validation.md` | DT-18 / RF-8 | Current SPEC/test/coverage integrity and append-only sanitized before/action/after proof; no unmanifested campaign artifact is introduced. |

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
- capability-observation tests: `node --test --experimental-test-coverage --test-coverage-include=tools/ci/generate-capability-observations.mjs --test-coverage-lines=80 --test-coverage-branches=80 --test-coverage-functions=80 tools/ci/generate-capability-observations.test.mjs` → exit 0; 8 passed, 0 failed; lines 97.89%, branches 93.10%, functions 100.00%.
- DT-18 E2E: `node tools/ci/generate-capability-observations.mjs --write` then `--check` → exit 0; 33 observations. `./scripts/docs-check.sh` twice → exit 0; SHA-256 `84ba3f08fd5e6765eadbabb15391e2769ad8ff0bc1298055ab2126a0f6017b6d` both times.
- DT-18 refusals: an empty local slug is excluded and a dangling named `SPEC.md` symlink returns the terminal `document-unsafe` finding in the named fixtures; a README-only historical surface remains present.

## Gaps

open — fresh same-revision GitHub Actions documentation admission and
`required-checks` success for DT-18. The local repository documentation E2E is
green, but the claim remains `PARTIAL` until that clean-checkout proof exists.
Individual product surfaces remain in their own evidence state and are not
promoted by this slice.

## Rollback trigger

One false DONE promotion, one sensitive matched value in diagnostics, one
automatic source rewrite, or nondeterministic output for identical input.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1–RF-8, NFR-1–NFR-5 | ITEM-1–ITEM-9 | pending — no automatic commit |
