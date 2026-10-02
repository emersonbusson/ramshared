# IMPL — Public repository candidate integrity

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/public-repository-hygiene/SPEC.md`

## Status

PARTIAL · owned implementation, repository candidate, contract, local validation, and cover ✓ · hosted same-revision required CI remains unobserved · BINARY_MATCH N/A

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `tools/ci/check-public-hygiene.mjs` | checker / RF-1–RF-10 | Existing candidate/text/image controls plus one immutable HEAD/index/blob snapshot, a strict ancillary PNG allowlist with raw iTXt and indexed-background validation, immutable committed JPEG authority, UTF-16 metadata scanning, and one exact C2PA JUMBF profile. |
| `tools/ci/check-public-hygiene.test.mjs` | tests / RF-1–RF-10 | Hermetic symbolic-HEAD/index movement; actual two-zlib-stream PNG, indexed background, raw iTXt, metadata bomb/privacy; same-candidate JPEG+manifest, UTF-16 metadata, malformed C2PA; canonical C2PA/JPEG assets; existing topology, text, and decoder cases. |
| `docs/governance/public-binary-digests.json`, `docs/governance/public-binary-digests.schema.json` | JPEG policy / RF-9 | Strict sorted registry of the two reviewed tracked JPEG assets by canonical path, byte length, and SHA-256 plus its exact dependency-free schema contract. |
| `tools/ci/plan-rust-slice-coverage.mjs` | planner / RF-11–RF-12 | Global pure-line and relocation ownership; bounded confined non-symlink map/SPEC/source/test/wrapper reads; raw BOM-preserving changed-path validation. |
| `tools/ci/plan-rust-slice-coverage.test.mjs` | tests / RF-11–RF-12 | Hermetic duplicate pure owners, external symlink trust inputs, map/list symlinks, and BOM/Cc/Cf path records, plus existing relocation cases. |
| `docs/governance/public-hygiene-allowlist.json` | policy / RF-4 | Scoped, owned, expiring public-contact exception schema. |
| `scripts/docs-check.sh`, `.github/workflows/ci.yml`, `docs/governance/ci-contract.json` | gate / RF-6, RF-8 | Candidate scan, direct clean-checkout committed-candidate step, named tests, reachable command contract, and per-file Node coverage. |
| `.github/workflows/release-integrity.yml`, `tools/ci/check-ci-contract.mjs` | recovery / DT-12 | Preserve parser-compatible current recovery helpers while retaining exact historical tag/SHA, read-only permissions, and no publication path. |
| `validation.md`, `docs/governance/redaction-ledger.json` | evidence governance | Exact historical prefix plus schema-valid append-only facts; digest-bound sanitized correction without restating a private value. |
| portable Windows and Linux script defaults named in the SPEC | portability / RF-5 | Removed developer-specific roots without executing operator scripts. |

## Validation (numbers)

### 2026-08-24 R5 immutable Git and binary-parser remediation

The five new adversarial findings are closed without broadening the runtime
surface. The Git-authority finding has two independent fixtures, so the focused
RED/GREEN set contains six tests.

- TDD RED: the exact six-test focused command returned exit 1, 0 passed and 6
  failed before production changes. Symbolic-HEAD movement was ignored; index
  movement did not refuse; the real two-stream fixture exposed missing SPEC and
  evidence mapping; indexed `bKGD` and high-bit iTXt language passed; and
  UTF-16/malformed-C2PA JPEG metadata passed.
- focused GREEN: the same exact command returned exit 0, 6 passed and 0 failed:
  `candidate_binds_one_head_oid_when_symbolic_head_moves_mid_run`,
  `staged_index_snapshot_refuses_mid_run_index_move`,
  `png_two_zlib_stream_fixture_is_explicit_and_refused`,
  `png_indexed_background_requires_existing_palette_entry`,
  `png_itxt_language_requires_exact_raw_ascii_bytes`, and
  `jpeg_metadata_profiles_refuse_utf16_and_malformed_c2pa_app11`.
- full public-hygiene suite plus the per-file 80% cover gate: exit 0, 45 passed,
  0 failed; 93.55% lines, 81.74% branches, and 99.14% functions for
  `check-public-hygiene.mjs`.
- canonical candidate: `node tools/ci/check-public-hygiene.mjs --candidate` →
  exit 0, 909 files, 0 findings.
- The task did not authorize planner or full-Node reruns. No Cargo, PowerShell,
  WSL, host, device, swap, GPU, service, publication, external-system, or
  network-write action was used.

### 2026-08-24 R5 ancillary, authority, and planner-trust remediation

The five adversarial findings are closed in owned code and fixtures. The slice
remains `PARTIAL` because the newly strict planner correctly exposes five
duplicate owners in the shared coverage map and one separately owned Rust
named-test drift. Neither residual is relaxed or rewritten in this dispatch.

- TDD RED: `png_ancillary_metadata_is_bounded_parsed_and_privacy_scanned`,
  `jpeg_candidate_refuses_changed_bytes_mutable_manifest_and_private_metadata`,
  `global_line_coverage_ownership_refuses_duplicate_pure_owners_including_cli_all`,
  `planner_trust_inputs_require_confined_non_symlink_regular_files`, and
  `planner_cli_rejects_bom_or_control_changed_paths_without_trimming` all
  reproduced their prior false green before implementation.
- focused GREEN: all five named blocker regressions passed, 5/5.
- public-hygiene suite and coverage: 39/39 passed; 92.89% lines, 80.97%
  branches, and 99.01% functions for `check-public-hygiene.mjs`.
- planner suite: 43/44 passed. The sole failure is the unchanged external Rust
  assertion for `daemon_worker_shutdown_drains_queued_io_before_stop`.
  Production coverage is 89.95% lines, 82.42% branches, and 98.63% functions.
- canonical planner `--all`: `BLOCKED`, exit 1, with five exact
  `line-coverage-production-owner-duplicate` findings for
  `crates/ramshared-cli/src/cascade/lifecycle.rs`,
  `crates/ramshared-cli/src/main.rs`,
  `crates/ramshared-winsvc/src/config.rs`,
  `crates/ramshared-winsvc/src/evidence.rs`, and
  `crates/ramshared-winsvc/src/runtime.rs`. The shared map is intentionally not
  edited under this dispatch.
- no Cargo, PowerShell, WSL, host, device, swap, GPU, service, publication,
  external-system, or network-write action was used.

### 2026-08-22 historical documentation-audit receipt (superseded)

`validation.md` record `EVD-0029` supersedes these checker-specific current
numbers without rewriting the historical implementation receipt below. Every
identity match in changed public Markdown, JSON, or a candidate filename is
evaluated independently: a historical/no-execution marker never suppresses a
concrete lab-VM, path, timestamped-run, UUID, device, principal, or private-IP
finding. `SANITIZED_*` placeholders are allowed. Candidate filesystem reads use
canonical realpath containment; a symlink escape is diagnosed before content is
read. Git paths reject Cc/Cf/bidi controls, DEL, and literal backslashes before
filesystem access or diagnostic rendering. Changed public Markdown/JSON decode
strictly and reject every non-normalized Cc or Cf/bidi character before identity
or activation matching; the finding carries an ASCII `u+XXXX` code point, never
the control itself. Staged files and the staged allowlist are read from Git
index blobs. Activation instructions in prose, inline code, bullets, emphasis,
and fences require an adjacent warning **before** the instruction.

- tests: `node --test tools/ci/check-public-hygiene.test.mjs` → exit 0; 21
  passed, 0 failed, including symlink escape, C0/C1/bidi/backslash paths,
  Cc/Cf public Markdown/JSON controls, ASCII-safe control diagnostics,
  all-match, JSON/filename identities, warning-after, inline/prose/bullet, and
  staged-allowlist regressions.
- cover: the CI-equivalent Node command → 97.65% lines, 89.01% branches, and
  100.00% functions for `check-public-hygiene.mjs`.
- candidate E2E: `node tools/ci/check-public-hygiene.mjs --candidate` → exit 0;
  877 files, 0 findings. This is read-only repository validation, not an
  operational activation.
- refusals: staged-index/worktree divergence, private-path/credential/key/address
  fixtures, raw identities beside warnings, Cc/Cf public content, symlink
  escape, unsafe Git paths, invalid mode, and Git failure returned the required
  non-zero class without echoing a sensitive match or rendering a control.
- script validation: 24 Windows static harnesses passed; Windows PowerShell 5.1 parsed 80 scripts; no operator script was executed.
- docs E2E: `./scripts/docs-check.sh` twice → exit 0 twice; 25 lines each; output SHA-256 `775d0f6b5c60b7d396d70cf713cd110b4f7dfa961318513e2dd4b9e3b166e6c4` both times.
- BINARY_MATCH: N/A — repository-only read-only tooling.

## Gaps

The five implementation/fixture gaps are closed. Current local checks show no
duplicate pure owners, generated documentation catalogs are in sync, and the
memory-broker shutdown source matches the current DT-50 contract. The historical
name `daemon_worker_shutdown_drains_queued_io_before_stop` is not a current
SPEC requirement: queued I/O is preempted at the terminal iteration boundary.
Keep status `PARTIAL` until hosted required CI reports success for the same
source revision. This slice does not qualify runtime, driver, signing, VM,
WSL2, or physical-host behavior.

## 2026-09-27 local reconciliation

- Planner `--all`: READY, exit 0; the shared source-owner map has no duplicate
  line owners.
- Public hygiene candidate: PASS, 1,156 selected files, 0 findings.
- Public hygiene tests: 45/45; per-file Node cover: 93.58% lines, 81.58%
  branches, 99.14% functions.
- Planner tests: 44/44; per-file Node cover: 89.95% lines, 82.40% branches,
  98.63% functions.
- Documentation inventory, capability observations, campaign lifecycle, docs
  index, and SPEC evidence checks pass in check mode.
- The old queued-drain test name was superseded by DT-50's
  `daemon_worker_shutdown_preempts_queued_io_at_iteration_boundary`; the
  continuous-refill test also exists under
  `daemon_worker_terminal_flag_wins_over_512_continuous_queue_refills`.

Final local reconciliation completed after the release identity and evidence
edits stabilized:

- `RUSTC_WRAPPER= cargo test -p ramshared-cli`: 355 unit and 10 integration
  tests passed; no failures.
- `cargo test -p ramshared-wsl2d`: exit 0 after rerunning outside the restricted
  test sandbox; all active test targets passed. Nineteen hardware, root, or
  device-specific tests remained intentionally ignored.
- `./scripts/docs-check.sh`: exit 0; every local documentation, release,
  evidence, public-hygiene, and regression gate passed. Localization reports
  `PARTIAL` only because the informational Portuguese translation has no
  current human review receipt; its source and translation hashes match.
- Campaign evidence catalog check: 176 observations, pass.
- Bash syntax checks for `scripts/install.sh`,
  `scripts/safety/install-cascade-boot.sh`, and
  `scripts/safety/nbd-product-preflight.sh`: pass.
- The prior direct preflight suite remains 47/47, and Clippy and rustfmt checks
  passed before the final test-fixture-only literal correction.

Hosted same-revision required CI remains unobserved, so this slice remains
`PARTIAL`. No release build, host installation, or hosted workflow was run.
Detailed command and evidence records are in the current
`evidence/validation-summary.json` and `evidence-manifest.json`.

## Rollback trigger

One staged blob read from the worktree, one nonignored or newly committed
candidate skipped, one symbolic HEAD reuse or moved index accepted, one Git
topology failure returned as zero files, one BOM or
invalid UTF-8 sequence skipped, one final symlink target followed, one public
binary accepted without its exact extension/signature/structure/size contract,
one JPEG accepted without its immutable committed path/size/SHA or standalone
baseline entropy/metadata-profile proof, one PNG zlib or ancillary payload bypasses parsing/privacy
limits, one duplicate line or relocation owner reaches READY, one planner
input escapes through a symlink, one changed path is trimmed before control
validation, one sensitive match echoed, or candidate mode exceeding 10 seconds in three
consecutive no-load runs.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1–RF-12 | checker, public binary contract, global ownership, planner trust inputs, clean-checkout gate, recovery compatibility, evidence governance | No RF implementation changed in this validation/evidence reconciliation; see the recorded evidence and branch history. |
