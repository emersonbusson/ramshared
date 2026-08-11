# IMPL — Public repository candidate integrity

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/public-repository-hygiene/SPEC.md`

## Status

implemented · cover ✓ · E2E ✓ · BINARY_MATCH N/A

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `tools/ci/check-public-hygiene.mjs` | checker / RF-1–RF-7 | Bounded candidate, staged, and tracked scans with sanitized findings. |
| `tools/ci/check-public-hygiene.test.mjs` | tests / RF-1–RF-7 | Git-index, candidate, binary, sensitive-data, determinism, and legitimate repository fixtures. |
| `docs/governance/public-hygiene-allowlist.json` | policy / RF-4 | Scoped, owned, expiring public-contact exception schema. |
| `scripts/docs-check.sh`, `.github/workflows/ci.yml` | gate / RF-6 | Candidate scan, named tests, and per-file Node coverage. |
| portable Windows and Linux script defaults named in the SPEC | portability / RF-5 | Removed developer-specific roots without executing operator scripts. |

## Validation (numbers)

- tests: `node --test tools/ci/check-public-hygiene.test.mjs` → exit 0; 12 passed, 0 failed.
- cover: `node --experimental-test-coverage --test tools/ci/check-public-hygiene.test.mjs` → lines 94.38%, branches 85.29%, functions 100.00%.
- legitimate E2E: `node tools/ci/check-public-hygiene.mjs --candidate` → exit 0; 672 files, 0 findings.
- refusals: staged-index/worktree divergence, private-path/credential/key/address fixtures, invalid mode, and Git failure all returned their required non-zero class without echoing the match.
- script validation: 24 Windows static harnesses passed; Windows PowerShell 5.1 parsed 80 scripts; no operator script was executed.
- docs E2E: `./scripts/docs-check.sh` twice → exit 0 twice; 25 lines each; output SHA-256 `775d0f6b5c60b7d396d70cf713cd110b4f7dfa961318513e2dd4b9e3b166e6c4` both times.
- BINARY_MATCH: N/A — repository-only read-only tooling.

## Gaps

closed for the public-candidate hygiene slice. This does not qualify runtime,
driver, signing, VM, WSL2, or physical-host behavior.

## Rollback trigger

One staged blob read from the worktree, one nonignored candidate skipped, one
sensitive match echoed, or candidate mode exceeding 10 seconds in three
consecutive no-load runs.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1–RF-7 | checker, portable defaults, gates | pending — no automatic commit |
