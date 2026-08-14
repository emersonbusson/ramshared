# SPEC — Protected beta release promotion and publication

## Closed scope

### In now

- A source-only release contract for exactly `v0.9.0-beta.1`.
- GitHub-App-only Release Please credentials for tag/draft production.
- Read-only tag integrity that emits a four-file candidate artifact.
- A protected, manual, exact-SHA beta publication workflow and pure exported
  Node validation/planner logic; its bounded CLI writes only local candidate
  and plan JSON files for workflow handoff.
- Contract topology checks proving `ci.yml` has no direct trigger alongside
  its canonical reusable caller.

### Out now

- Actual tag creation, draft creation, asset upload, beta publication, or
  remote environment/configuration change.
- Any stable release or `v0.8.0` mutation.
- Windows driver public distribution, host action, lab action, daemon runtime,
  Rust behavior, kernel, swap, or GPU work.

### Assumed-ready dependencies

- The configured GitHub App is installed on this repository, its public bot
  identity is `emersonbusson-ramshared-release[bot]`, and encrypted Actions
  secrets supply its App ID and private key without a source or log copy.
- The existing protected release environment remains a human-controlled remote
  gate. Local source cannot prove reviewer settings.
- The selected Actions policy admits the pinned App-token action used by the
  successful Release Please producer. A later permission or allowlist
  regression must fail before delegation rather than select another token.
- GitHub-hosted `gh`, checkout, upload-artifact, and download-artifact actions
  are available only when the workflows are later run.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | DT-1, ITEM-1, ITEM-5 |
| RF-2 | DT-2, DT-2a, ITEM-2, ITEM-5 |
| RF-3 | DT-3, ITEM-3 |
| RF-4 | DT-4, ITEM-4, ITEM-5 |
| RF-5 | DT-3, DT-4, ITEM-3, ITEM-4 |
| RF-6 | DT-5, ITEM-3, ITEM-4 |
| RF-7 | DT-3, ITEM-3, ITEM-4 |
| RF-8 | DT-3, ITEM-4 |
| NFR-1 | DT-2, DT-4, ITEM-2, ITEM-4 |
| NFR-2 | DT-4, DT-5, ITEM-3, ITEM-4 |
| NFR-3 | DT-3, DT-5, ITEM-3 |
| NFR-4 | DT-3, ITEM-3 |
| NFR-5 | DT-4, ITEM-4 |
| NFR-6 | DT-4, ITEM-4 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | `ci.yml` stays `workflow_call`-only and `ci-contract.yml` is its sole automatic pull-request/main caller. Contract inspection rejects a child workflow that declares any direct `pull_request` or `push` trigger in addition to reusable invocation. | Direct and reusable scheduling produces duplicate test runs and breaks same-run aggregate ownership. |
| DT-2 | `release.yml` requires `RELEASE_APP_ID` and `RELEASE_APP_PRIVATE_KEY`; it creates an installation token unconditionally and passes only `steps.release-app-token.outputs.token` to Release Please. The producer token expression may not reference `GITHUB_TOKEN`, `RELEASE_PLEASE_TOKEN`, `PAT`, `||`, or a conditional credential fallback. | A fallback changes the tag producer identity and can create releases outside the App policy. |
| DT-2a | Release Please uses schema-supported, deprecated `release-as: 0.9.0-beta.1` together with `include-v-in-tag: true`, `versioning: prerelease`, `prerelease-type: beta`, `draft: true`, `prerelease: true`, `force-tag-creation: true`, and `skip-github-release: false`. The contract declares baseline manifest `0.8.0` and admits exactly that baseline or the generated target manifest `0.9.0-beta.1`; every other version, malformed object, or extra package entry is refused. The producer exits `NO_CHANGE` before Release Please when `v0.9.0-beta.1` already exists. | The checked-in baseline and generated release PR are consecutive authorized states of one exact transition. Binding both endpoints avoids rejecting the release PR while preventing an intermediate or future manifest from widening the one-shot target. |
| DT-2b | Recovery from a merged release PR whose body lost Release Please metadata uses top-level `last-release-sha` pinned to exact `v0.8.0` merge `568e7b42b78b3c9edb8ea390cb4297142a37e412`. The configured generated header contains all seven mandatory repository PR headings and an explicit instruction that the machine-readable release notes below Release Please's separator remain intact. The checker rejects any other recovery SHA or incomplete header. Remove the recovery key after the exact beta tag/draft is verified. Named test: `release_producer_preserves_machine_body_and_bounds_recovery`. | The upstream recovery control bounds changelog collection after an accidentally merged bad release PR. Keeping governance metadata inside the configured header lets Release Please parse its own body after merge; replacing the whole body produced a false successful workflow with no tag and an oversized successor PR. |
| DT-3 | `docs/governance/release-promotion.json` is schema version 1 and pins `target_tag` to `v0.9.0-beta.1`, channel `beta`, and the exact ordered public filenames: archive, detached archive SHA-256, SBOM, manifest. `v0.8.0` is not accepted. A future target changes this policy deliberately. | One concrete beta prevents historic or future tags from entering a new unreviewed path. |
| DT-4 | `.github/workflows/release-integrity.yml` normally runs from the exact target tag and also admits a bounded manual recovery from `main` with required exact tag and 40-hex source SHA inputs. Both paths have `contents: read`, declare no deployment environment, check out the tag SHA, prove tag/SHA equality, build the candidate, merge exactly the two packaged cargo-cyclonedx roots into one deterministic path-free SBOM, write/validate the detached checksum and manifest, and upload the same named integrity artifact. The recovery run has an exact display title that binds both inputs; its merger tooling is copied from trusted `main` into `RUNNER_TEMP` before the immutable tag checkout. Integrity has no `workflow_run`, `gh release`, release-creation action, asset-upload action, or write permission. `.github/workflows/release-publication.yml` is the only publication path. A maintainer-authored `workflow_dispatch` request validates the exact tag/SHA/run tuple before using a Contents-write GitHub App token to emit `repository_dispatch` type `release-publication-app` with the same tuple. A dedicated admission job requires that exact event type, payload stage, and actor `emersonbusson-ramshared-release[bot]` before `environment: protected-release`, preserving required human review with `prevent_self_review`. The protected job uses its read-only `GITHUB_TOKEN` for Actions metadata/artifact reads and the contents-write App token for release mutation. Publication accepts a successful normal tag run whose workflow name is `Release Integrity` and whose `head_sha` is exact, or a successful recovery run from `main` whose API `name` and `display_title` both equal the exact tag/SHA-bound recovery title; the downloaded manifest and four assets are validated identically. Named test: `release_publication_request_delegates_only_to_exact_app_actor`. | A deterministic defect in an immutable tag's workflow must be recoverable without deleting or moving the tag. Repository dispatch uses an already proven App permission, separates the run initiator from the human environment reviewer, and does not expose the private key or require Actions-write authority. GitHub exposes a parameterized recovery `run-name` in both the Actions API `name` and `display_title` fields, so both values are bound instead of falsely requiring the static workflow name. |
| DT-5 | Publication inputs are exact `tag`, 40-lowercase-hex `source_sha`, and decimal `integrity_run_id`. It downloads only `release-integrity-<tag>-<sha>`, verifies the Git tag resolves to `source_sha`, and requires the selected successful run to come from `.github/workflows/release-integrity.yml`. A normal push run must bind the static workflow name and `head_sha` directly; a recovery run must be a `workflow_dispatch` from `main` whose API `name` and `display_title` both equal the exact title binding the tag and source SHA. It then checks the manifest and all four local assets before classifying the remote release inventory. An exact published beta returns `NO_CHANGE`; an exact draft can advance; any wrong tag/SHA/channel/draft mode, missing/mismatched/extra asset, or ambiguous inventory is a refusal before mutation. It never deletes or overwrites assets. GitHub's release response `target_commitish` is not used as a SHA because it can be a branch name; a manufactured branch-value fixture proves it does not false-refuse. | Replay safety (#17) requires explicit matching, not an overwrite shortcut or a mutable branch field. |
| DT-6 | The detached checksum file contains exactly one GNU `sha256sum` record for the archive, uses the archive basename, and is independently validated against the archive hash. The bundle’s internal `SHA256SUMS` is neither modeled nor uploaded as a public asset. | The public checksum must be independently consumable and cardinality must stay bounded. |

## Atomicity and rollback

### Atomicity frontier

- **Source-only contract:** atomic at file review level; exported Node
  validators/planners do not call GitHub or mutate a release. Their CLI writes
  only bounded local JSON handoff files.
- **Tag integrity:** produces a bounded candidate artifact only after all four
  local files validate. An invalid candidate does not become an artifact.
- **Publication:** remote state has no safe general rollback after a public
  beta transition. The workflow therefore verifies every local/remote identity
  before its first mutation, uploads only absent matching assets, rechecks the
  exact inventory, and changes draft visibility last.

### Rollback

- **Userspace/daemon:** N/A — no runtime code changes.
- **Kernel/module or Windows driver:** N/A — no driver/kernel action.
- **Host/persistent:** N/A — no host mutation.
- **Remote release:** forward-only after publication. Abort before publish if
  any of four asset names, bytes, hashes, target tag/SHA, or beta draft state
  differs. Rollback trigger: a nonmatching remote asset, an unexpected fifth
  asset, or a tag/manifest SHA mismatch observed before visibility transition.
- **Remote Actions allowlist:** no release attempt is authorized while the
  selected-action configuration excludes `actions/create-github-app-token@*`.
  Remediation is a separately approved repository setting change, not a
  workflow/source fallback.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-1 topology | #13/#16 | Can a direct child trigger coexist with its canonical reusable call? | `ci_topology_rejects_duplicate_direct_and_reusable_invocation` | Any duplicate trigger finding. |
| ITEM-2 producer | #13/#16/#17/#18 | Does missing App material stop before Release Please, are only the exact baseline/target manifest states admitted, and does recovery retain the machine-readable body? | `release_producer_requires_github_app_token_without_fallback`; `release_producer_accepts_only_exact_manifest_transition`; `release_producer_preserves_machine_body_and_bounds_recovery` | Any fallback token reference, conditional App token, foreign manifest version, unbounded recovery SHA, or incomplete generated header. |
| ITEM-3 integrity | #9/#13 | Do tag, SHA, archive, detached checksum, SBOM, and manifest agree exactly? | `release_manifest_requires_exact_four_public_assets` | One missing/mismatched name, bytes, or hash. |
| ITEM-4 publication | #16/#17 | Does a replay preserve a verified completed beta and refuse a mixed inventory? | `publication_plan_is_idempotent_and_refuses_mismatched_remote_assets` | Extra/missing/mismatched asset or nonbeta state. |
| ITEM-5 workflow admission | #15/#18 | Is publication only explicit protected dispatch, not a retry/loop workaround? | static workflow/contract refusal tests and actionlint | `workflow_run`, direct publish in integrity, unbounded retry, or wrong token authority. |

## Security checklist (pre-implementation)

- [x] Privilege: publication write authority comes only from a protected
  GitHub App token; integrity is read-only.
- [x] User/host copy: dispatch input is parsed as bounded tag/SHA/run-ID text;
  no host buffer or driver interface exists.
- [x] Flags/IOCTL codes: N/A — no driver/uAPI surface.
- [x] Info-leak: stable error codes must not echo credentials, tokens, or
  private paths.
- [x] IRQ/atomic or IRQL: N/A — no kernel surface.
- [x] Lifetime: artifact path, tag/SHA binding, and remote inventory are
  validated before mutating the release.
- [x] Hot-unplug / device-gone: N/A — no device surface.
- [x] Host safety: workflows do not invoke VM, driver, service, disk, GPU,
  swap, pressure, shutdown, or reboot action.
- [x] Replayable ops: identical completed inventory is `NO_CHANGE`; differing
  inventory refuses without delete/overwrite (#17).

## Files to CREATE

**`docs/governance/release-promotion.json`**

- Purpose: bounded target/channel/exact asset policy.
- RF / DT: RF-3, RF-7, RF-8; DT-3.
- Fields: schema version, target tag, beta channel, ordered public assets.
- Required tests: `publication_input_rejects_historical_or_non_exact_identity`.
- Cover target: N/A — declarative policy covered by Node checker tests.
- Kahneman: #13/#16.

**`tools/ci/check-release-publication.mjs`**

- Purpose: pure exported validation and idempotency planner for dispatch input,
  local candidate manifest, and remote release inventory. It must not call
  `gh` or spawn a command. Its CLI may serialize a validated candidate/plan to
  a caller-supplied safe relative JSON path.
- RF / DT: RF-3, RF-5, RF-6, RF-7, RF-8; DT-3, DT-5, DT-6.
- Functions: `validateReleasePromotionPolicy`, `validatePublicationInput`,
  `validatePublicationBinding`, `candidateFromReleaseManifest`,
  `planPublication`, `main`.
- Required tests: `publication_input_rejects_historical_or_non_exact_identity`,
  `publication_validate_only_cli_stops_invalid_dispatch_before_credentials`,
  `publication_accepts_branch_target_commitish_but_refuses_wrong_tag_or_dispatch_sha`,
  `publication_plan_accepts_exact_draft_candidate`, and
  `publication_plan_is_idempotent_and_refuses_mismatched_remote_assets`.
- Cover target: ≥80% Node line, branch, and function coverage.
- Kahneman: #13/#16/#17.

**`tools/ci/check-release-publication.test.mjs`**

- Purpose: positive and refusal fixtures for the pure publication planner.
- Cover target: N/A — test file.

**`.github/workflows/release-publication.yml`**

- Purpose: two-stage manual beta publication consumer. Its request stage
  validates the exact tuple and delegates through an App-authored repository
  event using Contents-write authority; an admission job rejects every
  non-App attempt to create the internal stage.
  The App-authored protected job checks out the exact dispatch SHA, downloads
  only the named integrity candidate, reruns the local validators, requires the
  existing Release Please draft, asks the planner for
  advance/no-change/refusal, and only then uses a contents-write App token to
  upload/publish the release. It never creates a release.
- RF / DT: RF-4 through RF-8; DT-2 through DT-6.
- Required tests: `publication_workflow_is_protected_manual_exact_sha_only`
  and `release_publication_request_delegates_only_to_exact_app_actor`.
- Cover target: N/A — protected orchestration; live dispatch remains
  environment-bound.
- Kahneman: #16/#17.

## Files to MODIFY

**`tools/ci/write-release-manifest.mjs`** and
**`tools/ci/check-release-integrity.mjs`**

- Before → after: bind archive/SBOM only → bind/verify an exact ordered
  four-file public set, including detached checksum and target policy.
- RF / DT: RF-3, RF-5, RF-7; DT-3, DT-6.
- Required tests: `release_manifest_requires_exact_four_public_assets` and
  `release_manifest_rejects_invalid_detached_checksum_or_historical_target`.
- Cover target: ≥80% Node line, branch, and function coverage per file.
- Kahneman: #9/#13/#16.

**`tools/ci/check-release-integrity.test.mjs`** and
**`tools/ci/write-release-manifest.test.mjs`**

- Purpose: add RED/Green fixtures for target policy, detached checksum, exact
  asset cardinality, and stable no-echo errors.
- Cover target: N/A — test files.

**`tools/ci/check-ci-contract.mjs`** and
**`tools/ci/check-ci-contract.test.mjs`**

- Before → after: verifies read-only integrity as non-publishing → verifies
  the paired tag integrity/manual protected publication topology, App-only
  producer, no workflow loop, and exact workflow inputs.
- RF / DT: RF-1, RF-2, RF-4; DT-1, DT-2, DT-4, DT-5.
- Required tests: `ci_topology_rejects_duplicate_direct_and_reusable_invocation`,
  `release_producer_requires_github_app_token_without_fallback`,
  `release_producer_accepts_only_exact_manifest_transition`,
  `release_producer_preserves_machine_body_and_bounds_recovery`, and
  `publication_workflow_is_protected_manual_exact_sha_only`.
- Cover target: ≥80% Node line, branch, and function coverage.
- Kahneman: #13/#16/#17.

**`docs/governance/ci-contract.json`**

- Purpose: retain the integrity workflow's `publication: forbidden` boundary,
  add the paired manual-publication policy/gate, and keep that gate outside the
  pull-request aggregate.
- RF / DT: RF-1, RF-4; DT-1, DT-4.
- Required tests: the named CI contract tests above.
- Cover target: N/A — declarative policy.

**`.github/workflows/release.yml`**

- Before → after: conditional App token plus fallback expression → mandatory
  App credentials/token with no fallback, beta/draft-only release metadata.
- RF / DT: RF-2, RF-8; DT-2, DT-3.
- Required tests: `release_producer_requires_github_app_token_without_fallback`.
- Cover target: N/A — workflow orchestration.

**`.github/workflows/release-integrity.yml`**

- Before → after: generic tag-only read-only manifest → exact target beta
  candidate with detached checksum, four-file validator, bounded integrity
  artifact, and no publication command.
- RF / DT: RF-3 through RF-5, RF-7, RF-8; DT-3, DT-4, DT-6.
- Required tests: `item6_release_integrity_workflow_is_current_and_nonpublishing`.
- Cover target: N/A — workflow orchestration.

**`release-please-config.json`**

- Purpose: configure beta/draft metadata so producer output is not an automatic
  public final release; it does not alter historical release records.
- RF / DT: RF-2, RF-8; DT-2, DT-3.
- Required tests: `release_producer_requires_github_app_token_without_fallback`.
- Cover target: N/A — declarative configuration.

## Files to DELETE

None. There is one release path; no compatibility publication workflow is
retained.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| target tag and source SHA | candidate manifest and dispatch validation | bounded public identifiers |
| four asset records | manifest and planner input | filename, bytes, SHA-256 |
| detached checksum validity | integrity checker | stable pass/refusal rule |
| publication decision | planner stdout | `ADVANCE`, `NO_CHANGE`, or refusal code |
| producer credential class | static contract checker | `github-app-required` rule only; no token value |
| remote inventory mismatch | planner stderr | stable rule code; no asset content echo |

## Living docs

| Document | Action |
| --- | --- |
| `docs/INDEX.md` | Documentation owner reconciles it after this pack settles; this slice does not regenerate it. |
| `docs/specs/no-milestone/ci-trust-and-release-integrity/SPEC.md` | Cross-reference this successor where its future promotion wording is superseded. |
| `validation.md` | N/A until a real protected dispatch happens. |
| `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` | N/A — no measurement claim. |
| `.claude/rules/*`, `CLAUDE.md`, `AGENTS.md` | N/A — no lasting repository convention change. |

## Implementation order

1. **ITEM-1 — topology RED.** Add a manufactured child workflow that contains
   both `workflow_call` and direct `pull_request`/`push`, and prove contract
   validation refuses it before changing checker behavior.
2. **ITEM-2 — producer RED/Green.** Add a test that rejects conditional App
   creation or fallback token text; then make release producer credentials
   mandatory and configure beta/draft metadata.
3. **ITEM-3 — candidate RED/Green.** Add failing checksum/four-asset fixtures
   before extending writer/integrity checker and target policy. Generate the
   detached checksum deterministically from the archive basename.
4. **ITEM-4 — publication planner and workflow RED/Green.** Add pure planner
   fixtures for exact draft, complete replay, and mixed inventory refusal;
   then add protected manual workflow source that consumes only an integrity
   artifact and never overwrites assets.
5. **ITEM-5 — contract and static admission.** Extend CI contract policy/tests,
   actionlint all workflows, validate Node coverage, and hand the pack to the
   documentation owner for index/inventory reconciliation. Do not dispatch
   workflows or create `IMPL.md`.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `tools/ci/check-ci-contract.mjs` | `tools/ci/check-ci-contract.test.mjs` :: `ci_topology_rejects_duplicate_direct_and_reusable_invocation` | refusal | #13/#16/#17 | ≥80% Node |
| same | same :: `release_producer_requires_github_app_token_without_fallback` | refusal | #13/#16 | ≥80% Node |
| same | same :: `release_producer_accepts_only_exact_manifest_transition` | unit/refusal | #13/#16/#17 | ≥80% Node |
| same | same :: `release_producer_preserves_machine_body_and_bounds_recovery` | static/refusal | #13/#16/#17/#18 | ≥80% Node |
| same | same :: `release_integrity_refuses_any_deployment_environment` | static/refusal | #13/#16/#18 | ≥80% Node |
| same | same :: `publication_workflow_is_protected_manual_exact_sha_only` | static/refusal | #16/#17 | ≥80% Node |
| `tools/ci/write-release-manifest.mjs` | `tools/ci/write-release-manifest.test.mjs` :: `release_manifest_writer_binds_exact_four_public_assets` | unit | #9/#13 | ≥80% Node |
| `tools/ci/check-release-integrity.mjs` | `tools/ci/check-release-integrity.test.mjs` :: `release_manifest_requires_exact_four_public_assets` | unit | #9/#13 | ≥80% Node |
| same | same :: `release_manifest_rejects_invalid_detached_checksum_or_historical_target` | refusal | #13/#16 | ≥80% Node |
| `tools/ci/check-release-publication.mjs` | `tools/ci/check-release-publication.test.mjs` :: `publication_input_rejects_historical_or_non_exact_identity` | refusal | #13/#16 | ≥80% Node |
| same | same :: `publication_plan_accepts_exact_draft_candidate` | unit | #13 | ≥80% Node |
| same | same :: `publication_plan_is_idempotent_and_refuses_mismatched_remote_assets` | unit/refusal | #16/#17 | ≥80% Node |
| same | same :: `publication_validate_only_cli_stops_invalid_dispatch_before_credentials` | CLI refusal | #13/#16 | ≥80% Node |
| same | same :: `publication_accepts_branch_target_commitish_but_refuses_wrong_tag_or_dispatch_sha` | API-shape/refusal | #13/#16/#17 | ≥80% Node |
| `.github/workflows/release-integrity.yml` | `tools/ci/check-ci-contract.test.mjs` :: `item6_release_integrity_workflow_is_current_and_nonpublishing` | static | #13/#16 | N/A — workflow |
| same | same :: `release_integrity_recovery_is_exact_tag_sha_read_only` | static/refusal | #13/#16/#17 | N/A — workflow |
| `tools/ci/merge-release-sboms.mjs` | `tools/ci/merge-release-sboms.test.mjs` :: `release_workspace_sbom_is_deterministic_path_free_and_binds_both_binaries` | unit/refusal | #9/#13/#17 | ≥80% Node |
| `.github/workflows/release-publication.yml` | `tools/ci/check-ci-contract.test.mjs` :: `publication_workflow_is_protected_manual_exact_sha_only` | static | #16/#17 | N/A — protected E2E |
| same | same :: `release_publication_request_delegates_only_to_exact_app_actor` | static/refusal | #13/#16/#17 | N/A — protected E2E |

## Validation checklist

- [x] New Node logic has ≥80% line, branch, and function coverage through the
  built-in runner and the canonical PR caller wires each new checker.
- [x] RED was observed for every new production behavior before its Green
  implementation.
- [x] `node tools/ci/check-ci-contract.mjs --check-local` admits the exact
  paired topology and rejects manufactured duplicate/fallback paths.
- [x] `node tools/ci/check-release-integrity.mjs --check …` accepts a valid
  four-file beta candidate and rejects each cardinality/checksum/SHA refusal.
- [x] `node tools/ci/check-release-publication.mjs …` accepts only exact draft
  advancement and matching completed replay.
- [x] `go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7 .github/workflows/*.yml`
  exits zero.
- [x] `node tools/generate-docs-index.mjs --check`, `./scripts/docs-check.sh`,
  and `git diff --check` admit the synchronized issue/frontmatter index and
  current contract documentation.
- [x] Request run `31790968940` passed admission and tuple validation, then
  failed before delegation when the App refused ungranted Actions-write
  authority. It produced no repository dispatch, asset upload, or release
  mutation; the replacement repository-dispatch path remains unexecuted until
  its hosted checks and merge pass.
- [x] The pinned App-token action and required encrypted App secrets are
  configured; secret contents were not copied into source, logs, or artifacts.
