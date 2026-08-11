# IMPL — CI trust and release integrity

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/ci-trust-and-release-integrity/SPEC.md`

## Status

implemented · cover ✓ · E2E hosted ✓ · BINARY_MATCH N/A

The local CI topology, exact coverage planning, artifact/release integrity,
Windows static boundary, and GitHub remote controls are implemented and
validated. The strict local contract is PASS. Pull request #189 run
`31446546130` executed the same revision through the hosted same-run aggregate;
all 20 jobs succeeded and the terminal `required-checks` context is SUCCESS.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `.github/workflows/ci-contract.yml` and reusable workflows | ITEM-1–ITEM-4 / RF-1–RF-6 | Add the same-run fail-closed aggregate, explicit permissions/timeouts, pinned Actions, Rust supply-chain checks, and hosted Windows static checks. |
| `.github/workflows/{windows-lab,wsl2-lab}.yml` | ITEM-5 / RF-6 | Add protected plan-only isolated-lab entrypoints with no host execution. |
| `.github/workflows/release-integrity.yml` | ITEM-6 / RF-7 | Add protected nonpublishing bundle/SBOM validation. |
| `tools/ci/check-ci-contract.mjs` | ITEM-1–ITEM-8 / RF-1–RF-11 | Validate source and observed remote controls, aggregate same-run results, and fail closed on missing, unsafe, or stale evidence. |
| `tools/ci/plan-rust-slice-coverage.mjs` | ITEM-6.6 / RF-10 | Map every changed Rust production file to exact line, platform, localization, test-only, or whole-file structural ownership and execute only tokenized commands. |
| `tools/ci/check-rust-slice-coverage.mjs` | ITEM-6.6 / RF-10 | Isolate coverage target state and enforce a terminal 15-minute process-tree deadline with a five-second TERM-to-KILL grace period. |
| `docs/governance/{ci-contract,remote-controls-observation,rust-slice-coverage}*` | ITEM-1–ITEM-8 / RF-1–RF-11 | Record the executable contract, sanitized GitHub REST observation, and exact SPEC ownership map. |

## Validation (numbers)

- contract tests: `node --test tools/ci/check-ci-contract.test.mjs tools/ci/check-ci-aggregate.test.mjs` → 52 passed, 0 failed.
- contract cover: 90.36% lines, 82.64% branches, 99.08% functions.
- strict source/remote gate: `node tools/ci/check-ci-contract.mjs --check` → exit 0, PASS.
- Node CI suite: `node --test tools/ci/*.test.mjs` → 243 passed, 0 failed.
- coverage deadline regression: hosted run `31447000916` attempt 1 reached the
  direct-child deadline after every Rust test and measured file passed, then
  GitHub cleanup found orphaned `cargo` and instrumented `ramsharedd`
  processes. The DT-30 RED classified exit 124 as a generic child failure.
  GREEN is 13/13 checker tests; the manufactured GNU `timeout` process group
  returned 124 and the descendant PID returned `ESRCH`. Checker coverage is
  93.08% lines, 86.18% branches, and 91.30% functions.
- serial Rust admission: the exact bounded local workspace command
  `timeout --signal=TERM --kill-after=5s 300s cargo test --workspace -- --test-threads=1`
  completed with exit 0 in 24.69 seconds. All non-ignored tests passed; GPU,
  root/ublk, and dangerous WSL2 daemon cases remained ignored. The CI contract
  and exact coverage runner both require the one-thread argument.
- Rust planner cover: 88.85% lines, 81.80% branches, 97.70% functions; exact PR merge-ref selection → READY with 19 entries and zero unmapped paths.
- structural Rust: two declaration/reexport-only `lib.rs` files → N/A line coverage by DT-28; exact `cargo test -p ramshared-broker --lib` and `cargo test -p ramshared-winsvc --lib` commands pass, while manufactured executable/malformed surfaces are refused.
- actionlint: pinned 1.7.7 over every workflow → exit 0.
- Rust planner: `plan-rust-slice-coverage.mjs --all --base-revision <origin-main-sha> --run` → exit 0; the hosted exact selection emitted 36 per-file PASS rows and every mapped production file was at least 80%, with a minimum of 80.8% (`crates/ramshared-cli/src/main.rs`, 893/1,105 lines).
- remote observation: read-only workflow tokens, PR approval disabled, selected Actions with SHA pinning, 30-day retention, strict/admin/conversation branch protection, `required-checks`, and two protected environments.
- E2E: hosted run `31446546130` completed 20/20 jobs successfully in one immutable revision. `required-checks` job `93642837435` is SUCCESS; Windows static completed in 98 seconds, exact Rust coverage in 221 seconds, Rust supply-chain policy in 292 seconds, and Trivy generated, validated, and uploaded its exact SARIF in 19 seconds. Lab workflows remained plan-only and no host, VM, driver, GPU, swap, shutdown, or reboot action ran.

## Gaps

- closed: source, remote-control, hosted aggregate, exact coverage, Windows
  static, supply-chain, artifact, and SARIF publication gates.
- env-bound: release signing/publishing and any future live isolated-lab action
  remain outside this SPEC revision.
- promotion condition: the final PR revision containing DT-30 must pass the
  hosted `required-checks` aggregate before merge; a rerun of the old revision
  is not accepted as proof for the fix.

## Rollback trigger

One selected failure/cancellation/skip reaches aggregate green; a pull-request
job gains undeclared write authority; a mutable Action reference executes; a
coverage child exceeds 15 minutes without terminal failure, leaves one Cargo or
test descendant alive, or consumes a partial report; or a plan-only lab path
reaches a host action.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1–RF-11 | ITEM-1–ITEM-8 | `0c903e8`, `965ba57`, `aa2282b`, `bba912f`, `5368771`, `a171678`, `3eab21e`; hosted run `31446546130` |
