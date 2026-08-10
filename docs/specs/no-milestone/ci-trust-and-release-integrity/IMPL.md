# IMPL — CI trust and release integrity

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/ci-trust-and-release-integrity/SPEC.md`

## Status

partial · cover ✓ · E2E hosted-pending · BINARY_MATCH N/A

The local CI topology, exact coverage planning, artifact/release integrity,
Windows static boundary, and GitHub remote controls are implemented and
validated. The strict local contract is PASS. The hosted same-run aggregate is
not claimed until the consolidation pull request publishes and passes the
`required-checks` context.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `.github/workflows/ci-contract.yml` and reusable workflows | ITEM-1–ITEM-4 / RF-1–RF-6 | Add the same-run fail-closed aggregate, explicit permissions/timeouts, pinned Actions, Rust supply-chain checks, and hosted Windows static checks. |
| `.github/workflows/{windows-lab,wsl2-lab}.yml` | ITEM-5 / RF-6 | Add protected plan-only isolated-lab entrypoints with no host execution. |
| `.github/workflows/release-integrity.yml` | ITEM-6 / RF-7 | Add protected nonpublishing bundle/SBOM validation. |
| `tools/ci/check-ci-contract.mjs` | ITEM-1–ITEM-8 / RF-1–RF-11 | Validate source and observed remote controls, aggregate same-run results, and fail closed on missing, unsafe, or stale evidence. |
| `tools/ci/plan-rust-slice-coverage.mjs` | ITEM-6.6 / RF-10 | Map every changed Rust production file to its owning SPEC gate and execute exact coverage commands. |
| `tools/ci/check-rust-slice-coverage.mjs` | ITEM-6.6 / RF-10 | Isolate coverage target state and enforce a terminal 15-minute child deadline. |
| `docs/governance/{ci-contract,remote-controls-observation,rust-slice-coverage}*` | ITEM-1–ITEM-8 / RF-1–RF-11 | Record the executable contract, sanitized GitHub REST observation, and exact SPEC ownership map. |

## Validation (numbers)

- contract tests: `node --test tools/ci/check-ci-contract.test.mjs tools/ci/check-ci-aggregate.test.mjs` → 50 passed, 0 failed.
- contract cover: 90.36% lines, 82.64% branches, 99.08% functions.
- strict source/remote gate: `node tools/ci/check-ci-contract.mjs --check` → exit 0, PASS.
- Node repository suite: `node --test tools/**/*.test.mjs` → 242 passed, 0 failed.
- actionlint: pinned 1.7.7 over every workflow → exit 0.
- Rust planner: `plan-rust-slice-coverage.mjs --all --base-revision <origin-main-sha> --run` → exit 0; every mapped production file at least 80%, minimum 81.5%.
- remote observation: read-only workflow tokens, PR approval disabled, selected Actions with SHA pinning, 30-day retention, strict/admin/conversation branch protection, `required-checks`, and two protected environments.
- E2E: hosted PR action is pending; lab workflows remained plan-only and no host, VM, driver, GPU, swap, shutdown, or reboot action ran.

## Gaps

- open: the consolidation PR must publish and pass the hosted
  `required-checks` aggregate before this slice becomes `implemented`.
- env-bound: release signing/publishing and any future live isolated-lab action
  remain outside this SPEC revision.

## Rollback trigger

One selected failure/cancellation/skip reaches aggregate green; a pull-request
job gains undeclared write authority; a mutable Action reference executes; a
coverage child exceeds 15 minutes without terminal failure; or a plan-only lab
path reaches a host action.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1–RF-11 | ITEM-1–ITEM-8 | pending consolidation commit and hosted PR evidence |
