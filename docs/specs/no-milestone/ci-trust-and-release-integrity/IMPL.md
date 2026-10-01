# IMPL — CI trust and release integrity

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/ci-trust-and-release-integrity/SPEC.md`

## Status

partial · cover ✓ · E2E hosted revalidation pending · BINARY_MATCH N/A

The local CI topology, exact coverage planning, artifact/release integrity,
Windows static boundary, and GitHub remote controls are implemented and
locally validated. Every gate below carries a named command and a measured
number from this revision, or is left PARTIAL with the exact gate that blocks
it. **Hosted qualification is not claimed**: the most recent `CI Contract`
aggregate, run `36814584553`, is `failure`. CI validates the pull-request
**merge ref**, not the branch head — that run executed
`refs/pull/2088/merge` = `69067544` (merge of `ed6cc9df` into `1bcc9806`),
and that merge tree reproduces two of the six red jobs locally. The earlier
run `31446546130` (20/20 SUCCESS) predates the DT-32 topology change and is
historical evidence only.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `.github/workflows/ci-contract.yml` and reusable workflows | ITEM-1–ITEM-4 / RF-1–RF-6 | Add the same-run fail-closed aggregate, explicit permissions/timeouts, pinned Actions, Rust supply-chain checks, and hosted Windows static checks. |
| `.github/workflows/{windows-lab,wsl2-lab}.yml` | ITEM-5 / RF-6 | Add protected plan-only isolated-lab entrypoints with no host execution. |
| `.github/workflows/release-integrity.yml` | ITEM-6 / RF-7 | Add nonpublishing bundle/SBOM validation plus an exact tag/SHA read-only recovery path for immutable-tag workflow defects. |
| `tools/ci/merge-release-sboms.mjs` | ITEM-6 / RF-7 | Merge exactly the CLI and WSL2 daemon cargo-cyclonedx roots into one deterministic, path-free, tag/SHA-bound public SBOM. |
| `tools/ci/check-ci-contract.mjs` | ITEM-1–ITEM-8 / RF-1–RF-11 | Validate source and observed remote controls, aggregate same-run results, and fail closed on missing, unsafe, or stale evidence. |
| `tools/ci/plan-rust-slice-coverage.mjs` | ITEM-6.6 / RF-10 | Map every changed Rust production file to exact line, platform, localization, test-only, or whole-file structural ownership and execute only tokenized commands. |
| `tools/ci/check-rust-slice-coverage.mjs` | ITEM-6.6 / RF-10 | Isolate coverage target state and enforce a terminal 15-minute process-tree deadline with a five-second TERM-to-KILL grace period. |
| `.github/workflows/{ci,ci-contract,security-scans,gitleaks,comment-language,pr-body,validation-schema,windows-static}.yml`, `tools/ci/check-ci-contract.mjs` | DT-32 | Make CI Contract the sole automatic entrypoint, retain only its local reusable callers, and reject undeclared direct triggers. |
| `docs/governance/{ci-contract,remote-controls-observation,rust-slice-coverage}*` | ITEM-1–ITEM-8 / RF-1–RF-11 | Record the executable contract, sanitized GitHub REST observation, and exact SPEC ownership map. |

## Validation (numbers)

- contract tests: `node --test tools/ci/check-ci-contract.test.mjs tools/ci/check-ci-aggregate.test.mjs` → 71 passed, 0 failed (2026-10-01 re-measure; previously cited 52).
- contract cover: `node --test --test-coverage-include=tools/ci/check-ci-contract.mjs --test-coverage-include=tools/ci/check-ci-aggregate.mjs --experimental-test-coverage tools/ci/check-ci-contract.test.mjs tools/ci/check-ci-aggregate.test.mjs` → 91.77% lines, 85.52% branches, 98.68% functions (2026-10-01 re-measure). The aggregate CLI is exported from `check-ci-contract.mjs`; there is no separate `check-ci-aggregate.mjs` source file, so the report covers `check-ci-contract.mjs` alone.
- strict source/remote gate: `node tools/ci/check-ci-contract.mjs --check` → exit 0, PASS (2026-10-01).
- Node CI suite: `node --test tools/ci/*.test.mjs` → 526 passed, 0 failed (2026-10-01 re-measure; previously cited 243).
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
- Rust planner cover: 88.85% lines, 81.80% branches, 97.70% functions. Coverage
  map: `docs/governance/rust-slice-coverage.json` holds **55 entries** (43
  `rust-line-coverage`, 4 `rust-structural-contract`, 4 `windows-platform-e2e`,
  2 `rust-adapter-test-contract`, 1 `rust-ignored-test-relocation`, 1
  `rust-module-export-glue-differential`) spanning **110 unique production
  files**. `plan-rust-slice-coverage.mjs --all` →
  `RUST_SLICE_COVERAGE_STATUS=READY`, exit 0 (2026-10-01 re-measure; previously
  cited 19 entries).
- structural Rust: two declaration/reexport-only `lib.rs` files → N/A line coverage by DT-28; exact `cargo test -p ramshared-broker --lib` and `cargo test -p ramshared-winsvc --lib` commands pass, while manufactured executable/malformed surfaces are refused.
- actionlint: pinned 1.7.7 over every workflow → exit 0 (2026-10-01 re-measure).
- Rust planner self-tests: `node --test tools/ci/plan-rust-slice-coverage.test.mjs` → **47 passed, 0 failed** (2026-10-01).
- Rust planner execution: `CARGO_BUILD_JOBS=1 node tools/ci/plan-rust-slice-coverage.mjs --all --base-revision 1bcc98068218f48aa357103b21e7ee692df91939 --run`
  → **75 per-file rows, 75 `[ok]`, 0 `[FAIL]`**, minimum **80.1%**
  (`crates/ramshared-vulkan/src/lib.rs`, 444/554). Lowest measured margins:
  `ramshared-vulkan/src/lib.rs` 80.1%, `ramshared-wsl2d/src/ublk_server.rs`
  80.3% (383/477), `ramshared-wsl2d/src/broker_srv.rs` 80.6% (737/914),
  `ramshared-cli/src/bounded_process.rs` 81.0% (425/525),
  `ramshared-block/src/gpu_cache_worker.rs` 81.0% (683/843). Two rows are
  legitimately 0-instrumented (`ramshared-cuda/src/{ffi,lib}.rs`). The harness
  time limit stopped the first full pass at **35 of 43** line gates (all 55
  entries selected, 0 failures). (2026-10-01 re-measure; previously cited 36
  rows and a minimum of 80.8% on `crates/ramshared-cli/src/main.rs`, 893/1,105.)
- supply chain: `cargo deny check` → `advisories ok, bans ok, licenses ok,
  sources ok`, exit 0 (`cargo-deny` 0.19.9, `cargo-audit` 0.22.2).
- DT-6 advisory-db pin, reproduced exactly as `security-scans.yml` does:
  `RUSTSEC_DB_COMMIT=ef03605143a913024f864d2edf476adad5720c93` matches HEAD,
  commit epoch `1790587811` = `2026-09-28T09:30:11Z` matches the declared UTC,
  snapshot age **243948 s = 2.82 days** ≤ 7 → `RUSTSEC_AGE_VALID=yes`. Then
  `cargo audit --no-fetch` → **1273 security advisories loaded, 193 crate
  dependencies scanned, exit 0**. No fallback to upstream HEAD (2026-10-01).
- protected isolated-lab plan: `node tools/ci/plan-isolated-lab.mjs` → 2 valid
  plans (`windows`, `wsl2`) with `host_action: none`, `terminal_status: PASS`,
  14-day retention, sha256-bound manifest; 8 refusal paths return `FAIL`/exit 1
  with stable codes (`lab-target-invalid`, `lab-mode-invalid`,
  `lab-environment-invalid`, `lab-revision-invalid`, `lab-kind-invalid`,
  `lab-output-path-invalid`). After: only the 2 valid artifacts exist — no
  refusal writes an artifact (2026-10-01).
- release integrity surface: `node --test tools/ci/check-release-integrity.test.mjs tools/ci/write-release-manifest.test.mjs tools/ci/merge-release-sboms.test.mjs tools/ci/check-release-automation.test.mjs tools/ci/check-release-publication.test.mjs`
  → **39 tests, 39 pass, 0 fail**. All five named SPEC tests green:
  `release_integrity_recovery_is_exact_tag_sha_read_only`,
  `release_workspace_sbom_is_deterministic_path_free_and_binds_both_binaries`,
  `release_sbom_requires_exact_release_roots_and_path_free_source_binding`,
  `release_manifest_requires_bound_inputs`,
  `release_manifest_rejects_test_signed_driver` (2026-10-01).
- remote observation: `docs/governance/remote-controls-observation.json` is
  **20.29 days** old (≤ 30). Read-only workflow tokens, PR approval disabled,
  `allowed_actions=selected`, SHA pinning required, 30-day artifact/log
  retention, strict/enforced-admin branch protection, `required-checks`
  context, and `protected-isolated-lab` + `protected-release` each with
  required reviewers, prevent-self-review, and protected branches (2026-10-01).
- public hygiene, this tree: `node tools/ci/check-public-hygiene.mjs --candidate`
  → `PUBLIC_HYGIENE_STATUS=PASS` (1196 files, 2026-10-01).
- E2E (historical): hosted run `31446546130` completed 20/20 jobs successfully in one immutable revision. `required-checks` job `93642837435` is SUCCESS; Windows static completed in 98 seconds, exact Rust coverage in 221 seconds, Rust supply-chain policy in 292 seconds, and Trivy generated, validated, and uploaded its exact SARIF in 19 seconds. Lab workflows remained plan-only and no host, VM, driver, GPU, swap, shutdown, or reboot action ran. This is **not** current hosted qualification — see Gaps.
- DT-32 local gate: 71 contract/aggregate tests passed (2026-10-01 re-measure; previously cited 53). Strict contract, actionlint 1.7.7, docs-check, and scoped whitespace checks exited 0. The old PR #191 hosted run remains evidence of the stale duplicate only, not proof of the new topology.

## Gaps

Tick legend: **✓** measured on this revision · **PARTIAL** exact blocking gate named.

| Item | State | Evidence or blocking gate |
| --- | --- | --- |
| Local CI contract + remote-controls gate | ✓ | `check-ci-contract.mjs --check` and `--check-local` → PASS |
| Exact SPEC coverage map integrity | ✓ | `plan-rust-slice-coverage.mjs --all` → READY, 55 entries / 110 files |
| Exact slice coverage ≥80% per mapped file | ✓ 75 rows · PARTIAL 8/43 line gates | min 80.1% measured; the completion pass must finish all 43 line gates |
| Rust supply chain (`cargo audit` / `cargo deny`) | ✓ | age-valid pinned snapshot 2.82 d; 1273 advisories / 193 crates; deny all-ok |
| Protected isolated-lab plan-only | ✓ | 2 PASS plans + 8 stable refusals, `host_action: none` |
| Release integrity / SBOM / manifest | ✓ | 39/39 tests, all 5 named SPEC tests |
| Remote-control observation freshness | ✓ | 20.29 d ≤ 30, DT-21 compliant |
| Public hygiene (`--candidate`, this tree) | ✓ | PASS |
| **Hosted `required-checks` aggregate** | **PARTIAL** | run `36814584553` is `failure`. CI validates the PR **merge ref** — `refs/pull/2088/merge` = `69067544`, merge of `ed6cc9df` into `1bcc9806` — not the branch head. Gate = one green same-revision hosted aggregate on that merge ref |
| **`spec-command-missing` on `wsl2-cascade-native-bootstrap`** | **PARTIAL** | The SPEC cover-gate line lacked `--report-json`, so it did not match `command.join(' ')`. Fixed in `54d56274` — **unpushed**. Gate = push and re-run hosted |
| **Public hygiene on the PR merge ref** | **PARTIAL** | Merge `69067544` → 10 unredacted findings in `validation.md` (RAW_DEVICE ×2, RAW_ARTIFACT_RUN_PATH ×3, RAW_UUID ×5) at lines 8797, 8813, 8847, 8849, 8854, 8868, 8966, 10462, 10574, 11498. Gate = a governed `docs/governance/public-hygiene-redactions.jsonl` entry per line — never a silent rewrite of append-only evidence |
| **`windows-static` compile** | **PARTIAL** | `error[E0432] unresolved import crate::gpu_cache_worker` in `ramshared-block`, from uncommitted in-flight edits to `crates/ramshared-block/{gpu_cache_worker,ipc_cache_client}.rs`. Gate = those edits landing coherent and compiling for the Windows target |
| **`pr-body` Commits table** | **PARTIAL** | PR #2088's table is missing **250** branch commits. Gate = regenerate the PR body with the full 4-column commit table required by `.github/pull_request_template.md` |
| **`ci-core / fmt + clippy + test`** | **PARTIAL** | `cargo fmt --check` reports a diff on a `workspace_bytes` signature. Gate = `cargo fmt --all` and a green clippy/test pass |
| Release signing / publishing | PARTIAL (env-bound) | outside this SPEC revision |
| Live isolated-lab action | PARTIAL (env-bound) | `plan-isolated-lab.mjs` is plan-only by design (`host_action: none`) |

**Unresolved diagnostic discrepancy (not a defect claim).** The same
`check-public-hygiene.mjs` reports PASS on this working tree and NO-GO on the
PR merge ref `69067544`, while `git diff 69067544 HEAD -- validation.md` is a
pure 94-line append and the flagged window (lines 8797–11498) is byte-identical
in both (both blobs are under the 1 MiB `MAX_VALIDATION_LOG_BYTES` cap). Swapping
HEAD's `validation.md` into the merge worktree makes the ten historical findings
disappear and surfaces two different ones on the appended tail. The cause of the
divergence is **not isolated**; it is recorded here rather than fixed as a bug.

**Gate diagnostic improvement (closed this revision).**
`plan-rust-slice-coverage.mjs` discarded `finding.detail`, so a BLOCKED
`RUST_SLICE_COVERAGE_ERROR=<rule>` named no entry. It now emits a companion
`RUST_SLICE_COVERAGE_DETAIL=<entry-id>` line while leaving the `ERROR` token
format unchanged (47/47 planner self-tests green).

- env-bound: release signing/publishing and any future live isolated-lab action
  remain outside this SPEC revision.
- promotion condition: the final PR revision must pass the hosted
  `required-checks` aggregate on its merge ref before merge; a rerun of an old
  revision is not accepted as proof.
- promotion condition (DT-32): the changed topology must complete one fresh
  same-revision hosted `required-checks` aggregate with no duplicate automatic
  CI workflow and no unfinished reusable child check.

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
