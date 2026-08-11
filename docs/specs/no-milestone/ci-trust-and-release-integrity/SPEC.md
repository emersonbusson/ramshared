# SPEC — CI trust and release integrity

> SSDV3 Step 2 · implements [`PRD.md`](PRD.md).
>
> This SPEC freezes the repository-local P0 CI implementation and its remaining
> live/remote closure gates. Source workflows and read-only policy tools do not
> constitute a hosted run, remote configuration, release publication, package
> promotion, driver/VM/WSL2 action, kernel action, or host mutation.

## Closed scope

### In now

- A versioned checked-in CI contract and zero-side-effect validator/test
  implementation.
- P0 workflow hardening requirements: explicit timeouts, least permissions,
  full-SHA action references, Rust dependency policy, public-hygiene coverage,
  a Windows static lane, artifact policy, and a fail-closed aggregate.
- Manual protected-lab skeletons that can only plan for an
  isolated lab and must refuse daily/physical host targeting before action.
- Release-manifest, CycloneDX SBOM, and test-signed-driver rejection tools and
  a nonpublishing protected tag-validation workflow.
- Deterministic local Rust slice-coverage isolation: each checker invocation
  owns its temporary target/profile/report state and refuses ambiguous shared
  ownership rather than letting concurrent `cargo llvm-cov` runs collide.
- Two canonical CI documents that describe planned topology and release
  integrity without claiming they are active.

### Out now

- Hosted execution or promotion of the checked-in workflow changes. Their
  source-level admission can be verified locally, but a real hosted pull
  request, protected lab dispatch, release run, and remote-control observation
  remain separate Step 3 evidence.
- Editing the root validation log, capability registry, or remote repository
  setting as a substitute for live evidence.
- Changing branch protection, default token permissions, review policy,
  allowed-action policy, environment reviewers, artifact default retention, or
  signing authority.
- Running a pull-request workflow, a release workflow, a Windows/WSL2 lab,
  WDK, InfVerif, Driver Verifier, QEMU, kernel selftest, driver install, SCM
  command, VM command, GPU allocation, swap command, physical-host campaign,
  shutdown, or reboot.
- Changing product code, driver ABI, daemon behavior, public release policy,
  or upstream contribution scope.

### Assumed-ready dependencies

- Node 22 and the built-in `node:test` runner for new CI policy tools.
- Rust stable, `cargo audit`, `cargo deny`, `cargo llvm-cov`, and an exact
  pinned CycloneDX generator only after the future tool-version record is
  created.
- Existing repository checks, including `scripts/docs-check.sh`, public
  hygiene, benchmark/spec evidence, and Rust slice coverage.
- GitHub-hosted Linux and Windows runners for fork-safe jobs only. A protected
  isolated-lab runner is optional and is not assumed to exist.
- Existing Windows static PowerShell harnesses remain the source of test
  semantics. The future wrapper invokes only their static modes.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-1, ITEM-2, ITEM-8 |
| RF-2 | DT-2, ITEM-2, ITEM-8 |
| RF-3 | DT-6, ITEM-3 |
| RF-4 | DT-3, DT-4, ITEM-3 |
| RF-5 | DT-7, ITEM-3 |
| RF-6 | DT-8, ITEM-4 |
| RF-7 | DT-9, ITEM-5 |
| RF-8 | DT-10, ITEM-6 |
| RF-9 | DT-11, ITEM-6, ITEM-8 |
| RF-10 | DT-12, DT-25, ITEM-2, ITEM-4, ITEM-6.6 |
| RF-11 | DT-13, ITEM-7 |
| NFR-1 | DT-8, DT-9, ITEM-4, ITEM-5 |
| NFR-2 | DT-3, DT-14, ITEM-3, ITEM-8 |
| NFR-3 | DT-2, DT-10, ITEM-5, ITEM-8 |
| NFR-4 | DT-1, DT-2, ITEM-1, ITEM-8 |
| NFR-5 | DT-10, ITEM-5, ITEM-6 |
| NFR-6 | DT-9, DT-11, DT-13, ITEM-5, ITEM-6, ITEM-7 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | `docs/governance/ci-contract.json` is the single checked-in authority for canonical gate identity, trust level, expected context, workflow source, timeout, permissions, retry policy, artifact class, aggregate membership, and allowed no-change condition. | A workflow filename or status context alone is not a trustworthy policy model. |
| DT-2 | A whole-pull-request selector and final aggregate are mandatory. The aggregate fails if any selected gate fails, is cancelled, is missing, is unexpectedly skipped, or is tolerated with `continue-on-error`. Only a contractually declared `NO_CHANGE` may bypass execution. | The last pushed commit must not hide a prior changed file, and a skipped critical job must not look green. |
| DT-3 | Every canonical job declares explicit permissions and a finite timeout. Initial P0 upper bounds are: contract 10 minutes, documentation 20 minutes, Rust 30 minutes, security 30 minutes, Windows static 45 minutes, plan-only lab 15 minutes, and release integrity 45 minutes. | A workflow must fail visibly instead of waiting indefinitely or inheriting broad authority. |
| DT-4 | Pull-request jobs use read-only permissions except where a documented read scope is required. `security-events: write` is isolated to SARIF upload, `actions: write` is isolated to closed-pull-request cancellation, and release write authority is isolated to a protected release job. No pull-request job uses `pull_request_target`, write token, release secret, signing secret, or privileged runner. | Untrusted source must not receive authority over repository, release, or host state. |
| DT-5 | Every `uses:` reference is a full immutable commit SHA followed by a human version comment. The contract checker rejects a mutable tag, branch, shortened SHA, or an action outside the reviewed action policy. | Action code is part of the CI supply chain. |
| DT-6 | The canonical Rust supply-chain job runs `cargo audit` and `cargo deny check` with locked/pinned tool versions. `cargo audit` uses only the contract-recorded immutable RustSec advisory-db commit, commit UTC, and maximum snapshot age; it fetches that exact commit and runs `--no-fetch`, never falling back to upstream HEAD. Both gates are blocking and neither may be wrapped in a broad retry or `continue-on-error`. | `deny.toml` must be executed, not merely documented, and a malformed upstream advisory-db HEAD must not become an unreviewed CI input. |
| DT-7 | New Node CI/security checker logic uses the built-in runner with at least 80% line, branch, and function coverage. The future public-hygiene coverage command targets `tools/ci/check-public-hygiene.mjs`; a leaking fixture must prove that sensitive bytes are not echoed. | CI policy logic is business logic for repository trust and needs measured regression protection. |
| DT-8 | `windows-static.yml` runs only on a hosted Windows runner for pull requests and main pushes. It runs `cargo test -p ramshared-winbroker` and `cargo test -p ramshared-winsvc`, then invokes `scripts/windows/Test-WindowsCiStatic.ps1`. The wrapper invokes exactly `Test-AutonomousBrokerStatic.ps1`, `Test-ProductOnlineStatic.ps1`, `Test-RamSharedInfIsolationStatic.ps1`, `Test-WindowsDiskCounterAuditStatic.ps1`, `Test-WindowsStorageMatrixStatic.ps1`, `Test-Win11LabMediaContractStatic.ps1`, `Test-Win11LabReadyStatic.ps1`, `Test-Win11LabDriverTestFirmwareStatic.ps1`, `Test-WinDriveIoctlValidationStatic.ps1`, `Test-HostAutonomousLifecycleStatic.ps1`, `Test-GuestExhaustiveStatic.ps1`, and `Test-RecoverGuestVerifierExactRunStatic.ps1`; it accepts only a repository root and refuses install, service, VM, hardware, GPU, pressure, shutdown, reboot, and physical-host switches before reading a harness. | Windows source regressions need a native compile surface without turning untrusted CI into a lab operator. |
| DT-9 | `windows-lab.yml` and `wsl2-lab.yml` are manual protected-environment skeletons. They use only `ubuntu-latest`, never a self-hosted runner; their only P0 mode is `plan`; each accepts exact single-choice `plan` and `isolated-lab` inputs, declares `environment: protected-isolated-lab`, derives the revision from `github.sha`, and passes workflow inputs only through environment variables to `tools/ci/plan-isolated-lab.mjs`. The tool accepts exactly `plan`, `isolated-lab`, a full revision SHA, and `protected-isolated-lab`; it emits a bounded plan plus manifest and refuses daily/physical target, non-plan mode, or untrusted revision before any host action. The sanitized plan directory is verified then uploaded with 14-day retention. Any live drill requires a future SPEC revision. | A workflow skeleton must make unsafe future expansion fail closed rather than normalizing host access. |
| DT-10 | Artifact-producing jobs write a bounded manifest with `schema_version`, source SHA, terminal status, and at most 64 artifact records. Each record has a safe relative path, class, attachment type, retention, byte size, SHA-256, and sanitizer result. Transient pull-request diagnostics retain for 7 days, coverage/static artifacts for 14 days, plan-only lab artifacts for 14 days, and qualified release evidence uses `attachment: release` with no workflow retention. The checker verifies path safety, class/retention pairing, actual file size/hash when a root is supplied, and bounded sanitized text without echoing prohibited values. | Evidence must be auditable without accumulating unsafe raw output. |
| DT-11 | `release-integrity.yml` is a protected, read-only validation workflow for `v*` tag pushes; it never publishes or attaches a release asset. It checks out the tag commit, proves a clean source before build, builds the Linux bundle, and emits a deterministic release manifest. It installs exactly `cargo-cyclonedx 0.5.9` with `--locked` and emits CycloneDX JSON 1.5 using `cargo cyclonedx --manifest-path Cargo.toml --format json --spec-version 1.5 --override-filename ramshared-sbom`. `tools/ci/write-release-manifest.mjs` binds the tag, full source SHA, `Cargo.lock` SHA-256, Rust version, fixed SBOM generator/version/spec, bundle/SBOM byte hashes, `release-evidence` records, an explicit Windows-driver status, and a numeric/observable rollback trigger. `tools/ci/check-release-integrity.mjs` validates those bindings and rejects a Windows driver marked test-signed, untrusted, unknown, or unattested from public promotion. A separate future promotion workflow and remote signing authority remain environment-bound. | A version record alone is not artifact provenance or driver trust. |
| DT-12 | The CI contract links each affected Rust business-logic change to the feature SPEC's exact `check-rust-slice-coverage.mjs -p … --files … --min 80` row. Workspace average, package average, or a generic test count is invalid. Non-Rust policy tools are `N/A — Node` only when their own 80% Node coverage command is present. | Coverage must prove the changed production file, not a neighboring slice. |
| DT-13 | Remote controls are an explicit `env-bound` manual promotion: branch protection contexts, default token setting, pull-request approval setting, action allowlist, protected-environment reviewers, and retention defaults require an administrator observation. The repository checker never changes them. | Source control cannot safely or reliably mutate repository administration. |
| DT-14 | Retries are forbidden by default. The only initial retry class is a dependency-fetch transport failure, bounded to two total attempts with a fixed 15-second delay and a sanitized classifier record. Test, lint, security, coverage, artifact, identity, signing, and target-selection failures never retry. | A generic retry turns deterministic regressions into false flakes. |
| DT-15 | The `cargo-audit` contract records the immutable RustSec advisory-db commit `309ad29d8fe448bf986019e05d47b9e0e29a2218`, its commit UTC `2026-08-09T12:34:06Z`, and a maximum age of 7 days. The checker rejects absent, mismatched, malformed, future-dated, or stale snapshot metadata. A later scheduled curator records upstream-HEAD health separately; it cannot waive snapshot age or make a stale snapshot pass. | The direct upstream successor was malformed when validated, while its direct predecessor was the newest successfully parsed snapshot. Pinning the audited input preserves determinism without silently treating an unhealthy HEAD as safe. |
| DT-16 | `tools/ci/plan-isolated-lab.mjs` is the only P0 plan serializer. It has no runner, VM, WSL2, driver, service, GPU, disk, swap, shutdown, or reboot operation; it writes deterministic JSON only after validating closed enum inputs and creates the paired `lab-plan` artifact manifest from the generated bytes. Its CLI accepts only safe relative output names resolved beneath the current workspace. | A source-only workflow must not encode a latent host command behind dispatch input handling or turn a dispatch value into an arbitrary write target. |
| DT-17 | `.github/workflows/ci-contract.yml` is the one pull-request/main entrypoint for the aggregate. It calls each canonical pull-request workflow through a same-revision local `workflow_call` job (`uses: ./.github/workflows/<name>.yml`), while those source workflows retain their direct events for backwards-compatible visible checks. Each called workflow has a terminal `if: always()` summary job which fails unless every owned gate concluded `success`; its concurrency group includes `${{ github.workflow }}` so a direct run cannot cancel its reusable invocation. The entrypoint aggregate has `if: always()` and an exact `needs` set of those callers plus its direct contract, artifact, and coverage jobs. It accepts only `success`; failure, cancellation, omission, or skip is a failure. It does not use `workflow_run`, check-run polling, artifacts from an independently scheduled run, or a status API. | A naïve aggregate cannot safely infer completion of separately scheduled workflows. `workflow_run` executes on the default-branch SHA and introduces races between independently scheduled runs. Local reusable calls make all observed conclusions members of one run and preserve the caller's immutable event context without granting the aggregate write authority. Caller-scoped concurrency avoids self-cancellation between retained direct runs and reusable calls. |
| DT-18 | `docs/governance/rust-slice-coverage.json` is the bounded SPEC-to-command map. A `rust-line-coverage` entry names one existing feature `SPEC.md`, its exact tokenized canonical `check-rust-slice-coverage.mjs` command, package list, production-file list, and 80% threshold. `tools/ci/plan-rust-slice-coverage.mjs` normalizes the referenced SPEC and rejects a map entry whose exact command is absent. On a pull request it reads the complete base-to-head change list; on a main push it runs every mapped line-coverage command. A no-Rust-change result is a successful, explicit `NO_CHANGE` sub-result inside an always-run job, never a skipped workflow job. Every other changed `crates/*/src/**/*.rs` path fails before coverage runs unless it satisfies one closed DT-23, DT-24, or DT-28 ownership contract. | A generic `crates/` coverage command would either miss the feature matrix or substitute a workspace average. The map makes the SPEC decision executable and makes missing coverage ownership fail closed. |
| DT-19 | `check-ci-contract.mjs --check` remains the strict promotion command and exits non-zero for every gap, including an administrator-only remote-control gap. `--check-local` may exit zero only when every checked-in workflow/tool/aggregate/coverage requirement is current and the sole remaining gap is the declared `remote-controls:remote-control-observation-absent`; it prints `PARTIAL` rather than `PASS`. The hosted source gate uses only `--check-local`. | A repository-local source gate must be executable while remote administration is honestly unresolved. Treating that executable local proof as a complete promotion would turn an external gap into a false green. |
| DT-20 | `cancel-closed-pr.yml` is the only workflow with `actions: write`. It is a 10-minute, no-checkout maintenance path that is eligible only for a merged pull request whose head repository is this repository. It lists only queued/in-progress `pull_request` runs for the exact head SHA, excludes its own run ID, and treats only 409/422 cancellation races as already terminal. Unmerged or fork closure is deliberately a no-op; it never receives this write path. | A closed event must not turn untrusted fork or unmerged workflow source into Actions-write authority. The narrow maintenance path conserves runner capacity after a trusted merge without expanding pull-request privilege. |
| DT-21 | `docs/governance/remote-controls-observation.json` is a sanitized schema-1 administrator observation bound to `emersonbusson/ramshared`, default branch `main`, an exact UTC observation time no older than 30 days, and the GitHub REST control values. Compliance requires default workflow token `read`, Actions PR approval disabled, `allowed_actions=selected`, required SHA pinning, artifact/log retention at most 30 days, strict/enforced-admin branch protection with conversation resolution, the same-run aggregate context `required-checks`, and protected `protected-isolated-lab` plus `protected-release` environments with required reviewers, self-review prevention, and protected-branch policy. A missing record remains the declared env-bound gap; a malformed, stale, foreign, or observed-unsafe record is a blocking NO-GO with stable rule codes, never PASS or a stale-gap error. The checker is read-only and never changes GitHub settings. | The first real observation closed uncertainty but found write-default tokens, Actions PR approval, unrestricted actions, no SHA-pinning requirement, 90-day retention, seven legacy branch contexts, no conversation-resolution requirement, and no protected environments. Recording an unsafe observation as “evidence present” must not promote it. |
| DT-22 | The remote observation reader classifies only `ENOENT` as absent; an unreadable file, inaccessible parent, broken read, invalid JSON, or invalid schema is NO-GO. The checked JSON Schema and runtime validator share the same canonical whole-second UTC pattern, bounded public-safe status-context grammar, and exact environment-name vocabulary. The checker validates the schema definition before accepting an observation. | `existsSync` can report `false` for an existing inaccessible target and turn EACCES into the locally permitted missing-evidence gap. An unused or broader schema also lets public evidence carry uncontrolled identifiers or disagree with runtime parsing. |
| DT-23 | DT-18's ownership kinds are mutually exclusive. `rust-line-coverage` keeps the canonical per-production-file ≥80% gate. `windows-platform-e2e` is permitted only for an exact listed Windows integration source path whose feature SPEC contains one byte-for-byte matching `rust-slice-platform-e2e-v1` JSON declaration, pairing that path with one named existing static harness that `Test-WindowsCiStatic.ps1` executes and one named existing live drill; it is not an `N/A` exemption and cannot carry a coverage command. `rust-localization-comment-differential` is permitted only for exact paths in a byte-for-byte matching `rust-slice-localization-comment-differential-v1` feature-SPEC declaration, when a full immutable base SHA is supplied and the planner proves that the base and head have byte-identical non-comment Rust text; a changed string literal, identifier, punctuation/control token, whitespace outside a comment, unreadable base, malformed source, or absent base is blocking. DT-24 and DT-28 are the only additional kinds. No default, glob, package-wide, generated, or documentation-only exemption exists, and an unmapped Rust path remains blocking. | Windows host-only integration is verified through its named native static/live surface rather than a false Linux line percentage; language migration must never relabel a diagnostic, wire token, policy branch, or control-flow change as a comment edit. |
| DT-24 | `rust-test-only-localization-differential` is the sole additional DT-18 ownership kind. It is permitted only for exact source paths in one byte-for-byte matching `rust-slice-test-only-localization-differential-v1` feature-SPEC declaration. Each verification binds one source path, one exact workspace package, tokenized `cargo test -p <package> --lib`, one root-level exact `#[cfg(test)] mod <declared-name>` region, and named GPU functions carrying an adjacent `#[test]` plus `#[ignore]` attribute whose exact historical commands have a terminal `**PASS**` record in `validation.md` at the supplied immutable base SHA. The planner lexes both base and head, rejects malformed or unbalanced source, projects only those declared test-module spans, applies the existing conservative comment projector to the remaining production projection, and fails if any non-comment production token, string, control token, attribute/header, or module boundary differs. A no-base `--all` map inspection may emit only an explicit deferred-base signal and cannot execute the test-only entry; changed selection and `--run` require the full base SHA. `--run` executes only the declared package test without a shell; it never reruns a GPU ignored test. | Test-source localization can be checked without manufacturing a low production-line-coverage pass, while every possible production change and every unproven GPU claim remains fail-closed. |
| DT-25 | Every non-`--report-only` `check-rust-slice-coverage.mjs` invocation creates a private run root under the operating-system temporary directory and passes its own `CARGO_TARGET_DIR` and JSON output path to `cargo llvm-cov`. It installs `SIGINT`/`SIGTERM` cleanup immediately after that root is created, before attempting one deterministic, repository-scoped, atomic-directory ownership lock. The checker waits at most 60 seconds for a live owner and records only schema version, opaque run ID, PID, start time, and a 45-minute lease. A malformed owner record, owner-ID mismatch, or expired lease whose PID is no longer alive is a stable fail-closed diagnostic; the checker never deletes that foreign, stale, or corrupt lock automatically. Success, child failure, lock-wait interruption, and post-acquisition `SIGINT`/`SIGTERM` cleanup remove only the caller's run root and lock after re-reading an exact matching run ID. | `cargo llvm-cov` otherwise defaults to a shared `target/llvm-cov-target` and profile state, so two legitimate local coverage jobs can make one another fail or read the wrong profile. Private state prevents the collision; bounded ownership coordination and exact-owner cleanup prevent a hang or an unsafe deletion from being mistaken for a retry. |
| DT-26 | The coverage checker's `cargo llvm-cov` child has one 15-minute wall-clock deadline, uses `SIGTERM`, and returns the stable `COVERAGE_CHILD_TIMEOUT` tool error on `ETIMEDOUT`. The timeout is terminal: the checker does not retry, consume a partial report, or relabel the run as a coverage failure. Exact-owner isolation cleanup still runs. | A Rust test binary can deadlock even when lock acquisition and target ownership are correct. An unbounded synchronous child stalls the entire CI aggregate and can retain temporary state indefinitely. Fifteen minutes accommodates a clean private-target workspace build while preserving a deterministic terminal bound. |
| DT-27 | The remote-controls gate uses the distinct implementation state `observed` only after the administrator observation exists and passes DT-21/DT-22. An `observed` gate is valid only for required gate ID `remote-controls`, remote trust, no workflow triggers, `selection.mode=never`, the exact observation path, and zero open gaps; it owns no workflow, job, policy, or command. `contract_state=PASS` may include this one observed gate, while any other observed gate or an observed gate with a gap is blocking. | Treating a closed administrator observation as a local workflow invents an execution surface; keeping it `env-bound` after valid proof makes PASS structurally impossible. A separate terminal state preserves the source/remote trust boundary. |
| DT-28 | `rust-structural-contract` is the sole non-executable Rust ownership kind. Its feature SPEC contains one byte-for-byte matching `rust-slice-structural-contract-v1` JSON declaration. Every verification binds one exact production source path to its workspace package and tokenized `cargo test -p <package> --lib` command. The planner strips comments with its conservative Rust lexer and accepts the complete remaining file only when it consists of crate/module attributes, `mod`/`pub mod` declarations, and `use`/`pub use` declarations; any function, impl, type, constant, static, macro invocation, executable statement, malformed source, unbalanced delimiter, foreign package path, shell command, or undeclared field is blocking. `--run` executes each distinct declared package test once without a shell. This kind is not a coverage percentage and may be used only when LLVM has no executable regions for the whole file and the feature SPEC explicitly justifies `Cover: N/A — structural module surface`. | A module surface containing only declarations can legitimately produce LLVM 0/0. Treating it as 0% blocks valid glue forever, while treating every absent profile as 100% could hide uninstrumented business logic. A closed whole-file grammar plus the package suite preserves fail-closed ownership without manufacturing coverage. |
| DT-29 | The Trivy job treats SARIF generation, validation, and trusted publication as separate fail-closed phases. After the blocking CRITICAL/HIGH scan writes `trivy-results.sarif`, an ordinary shell step requires a non-empty file and validates SARIF 2.1.0 with an array of runs. The immutable CodeQL upload action receives the exact `sarif_file: trivy-results.sarif` input and has no `continue-on-error`; publication runs only for main-repository pull requests or non-pull-request events, because fork tokens cannot receive `security-events: write`. A fork still must pass the scan and local SARIF validation. Named test: `ci_contract_requires_fail_closed_trivy_sarif_publication`. | Omitting `sarif_file` made the upload action use its nonexistent `../results` default, while an allowlisted `continue-on-error` turned the red annotation into a green security job. The vulnerability gate remained blocking, but silent loss of the promised code-scanning artifact is an evidence false-green. |
| DT-30 | On Linux/WSL2, the DT-26 deadline supervises `cargo llvm-cov` and every descendant in one process group through GNU coreutils `timeout --signal=TERM --kill-after=5s 15m`. The Node parent retains a later 15-minute-10-second terminal bound. Exit 124 or an outer `ETIMEDOUT` is the stable `COVERAGE_CHILD_TIMEOUT` tool error; no partial report is consumed and no retry occurs. A missing supervisor is `COVERAGE_CHILD_START_FAILED`. Named test: `coverage_child_deadline_terminates_descendant_process_tree`. | The hosted run `31447000916` proved that `spawnSync` timing out only the direct Cargo process can leave instrumented `cargo`/`ramsharedd` descendants alive. Those descendants retained pipes and delayed terminal failure even though every test and measured file passed. The deadline must own the whole process tree, not just its first PID. |

## Atomicity and rollback

- Atomicity frontier: future changes are repository workflow, documentation,
  and read-only policy-tool files only until a later SPEC explicitly authorizes
  a protected live lab action.
- Userspace/daemon: N/A — no service, daemon, package, or binary mutation in
  the P0 plan.
- Kernel/module or Windows driver: N/A — no load, unload, installation,
  verification run, signing, or ABI change.
- Host/persistent: N/A — no physical host, daily WSL2 host, VM, SCM, pagefile,
  swap, disk, GPU, BCD, shutdown, or reboot mutation.
- Remote configuration: forward-only administrator action outside this
  implementation. The checked-in contract may record an observation but cannot
  change it.
- Coverage-tool state: a local temporary run root and repository-scoped lock
  only; no workspace `target/`, host, workflow, or remote mutation is allowed.
- Rollback: disable/remove the new workflow call paths and restore the last
  known-good policy tools if the rollback trigger fires; retain failure evidence
  and never delete historic release manifests.

Rollback trigger: roll back the P0 workflow changes immediately if one
selected gate reaches green after a failure/cancellation/unsafe skip, one
pull-request job has undeclared write authority, one lab skeleton reaches a
daily/physical host action, one artifact sanitizer exposes prohibited data, or
one public release manifest accepts a test-signed Windows driver.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-1 contract parser | #13 | Can disabled, renamed, omitted, or tolerated canonical gates still pass? | `check-ci-contract.test.mjs` refusal fixtures | Any invalid contract/workflow fixture exits 0 |
| ITEM-2 selection/aggregate | #13, #17 | Does a whole-PR change remain selected after an unrelated synchronize push, and does a repeated aggregate preserve the same result? | `whole_pull_request_change_remains_selected`; two identical aggregate runs | A selected failure/cancellation/skip reports PASS |
| ITEM-3 permissions/retry | #15, #16 | Does an ambiguous failure or broad permission fail safe instead of retrying/escalating? | `retry_policy_rejects_unknown_failure_class`; `pull_request_gate_rejects_write_permission` | Retry on a deterministic failure or undeclared authority |
| ITEM-4 Windows static | #13, #16 | Can a static pull-request path reach installation, service, VM, hardware, or reboot code? | `windows_static_suite_refuses_mutating_switches` | Any forbidden capability is reachable |
| ITEM-5 lab skeleton | #16, #17 | Does unsafe target selection stop before a runner action, and does repeated plan dispatch stay side-effect free? | `lab_dispatch_rejects_daily_or_physical_target`; plan-only before/action/after evidence | A daily/physical target is accepted or a plan mutates state |
| ITEM-6 release validator | #9, #13 | Are source/hash/SBOM/signing fields independently bound and is test signing rejected? | `release_manifest_requires_bound_inputs`; `release_manifest_rejects_test_signed_driver` | One unbound or test-signed entry is public-eligible |
| ITEM-7 remote promotion | #16 | Can unavailable administrative evidence become an implied pass? | `remote_controls_missing_evidence_is_blocked` | Missing remote observation prints DONE/PASS |
| ITEM-6.6 coverage isolation | #13, #16, #17 | Can two exact coverage checks collide, wait forever, or delete another caller's state? | overlap, bounded-wait, stale/corrupt-owner, child-failure, and signal-cleanup fixtures | A concurrent checker succeeds/fails nondeterministically, waits past its bound, or removes a foreign owner |
| ITEM-8 live CI evidence | #17 | Do repeated same-source policy checks produce the same terminal verdict and sanitized artifact inventory? | two internal workflow observations | Divergent selection/verdict on identical source |

## Security checklist (pre-impl)

- [x] Privilege: GitHub token and runner authority are explicit contract fields;
  pull-request jobs are read-only and isolated from secrets.
- [x] User/host copy: workflow-dispatch inputs are bounded enums and an exact
  trusted revision; no arbitrary shell expression is interpolated into a lab
  command.
- [x] Flags: policy tools reject unknown CLI flags, unrecognized trust levels,
  unrecognized retention classes, and mutable action references.
- [x] Info-leak: artifact and checker diagnostics return stable rule/path codes
  without echoing secret values, private paths, raw kernel addresses, signing
  material, or user data.
- [x] IRQ/atomic or IRQL: N/A — no kernel or driver action.
- [x] Lifetime: N/A — no device mapping or process ownership; artifacts use
  bounded owned files and hashes.
- [x] Hot-unplug / device-gone: N/A — P0 lab workflows are plan-only.
- [x] Host safety: no pull-request or P0 plan job can install a driver, mutate
  SCM, operate a VM, pressure WSL2/GPU, change swap/pagefile/BCD, or reboot.
- [x] Replayable ops: contract checking and plan-only dispatch are idempotent;
  repeated invocation does not change host or remote settings.

## Files to CREATE

**`docs/governance/ci-contract.json`**
- Purpose: versioned canonical matrix for gate identity, trust, permissions,
  timeout, selection, coverage, retry, artifacts, and aggregate rules.
- RF / DT: RF-1, RF-2, RF-4, RF-10; DT-1–DT-5, DT-12, DT-14.
- Required tests: `tools/ci/check-ci-contract.test.mjs` ::
  `valid_contract_matches_workflows`, `ci_contract_rejects_removed_required_gate`,
  `ci_contract_rejects_continue_on_error_for_gate`,
  `ci_contract_requires_explicit_timeout`,
  `ci_contract_rejects_mutable_action_reference`.
- Cover target: N/A — declarative data validated by the checker.

**`docs/governance/remote-controls-observation.schema.json`** and
**`docs/governance/remote-controls-observation.json`**
- Purpose: define and record a sanitized, dated, repository-bound observation
  of remote GitHub trust controls without credentials or account identities.
- RF / DT: RF-11; DT-13, DT-19, DT-21.
- Required tests: `remote_controls_missing_evidence_is_blocked`,
  `remote_controls_unsafe_observation_is_no_go`,
  `remote_controls_compliant_observation_is_accepted`, and
  `remote_controls_foreign_or_stale_observation_is_no_go`,
  `remote_controls_unreadable_evidence_is_no_go`,
  `remote_controls_broken_symlink_is_no_go`,
  `remote_controls_schema_matches_runtime_contract`, and
  `remote_controls_sensitive_control_name_is_refused`, and
  `remote_controls_observed_state_closes_only_the_exact_remote_gate`.
- Cover target: N/A — declarative data validated by the Node checker.

**`docs/governance/rust-slice-coverage.json`**
- Purpose: bounded, exact map from a feature SPEC's canonical Rust line-coverage
  command, independently validated Windows platform ownership contract, or
  independently validated test-only localization contract.
- RF / DT: RF-10; DT-12, DT-18, DT-23, DT-24, DT-28.
- Required tests: `tools/ci/plan-rust-slice-coverage.test.mjs` ::
  `spec_coverage_map_requires_exact_command_in_spec`,
  `changed_business_rust_file_requires_mapped_spec_command`, and
  `no_rust_change_is_explicit_no_change_not_skip`,
  `windows_platform_entry_requires_exact_feature_spec_and_named_checks`,
  `windows_platform_entry_refuses_static_harness_outside_windows_ci`,
  `localization_differential_accepts_comment_only_source_change`, and
  `localization_differential_refuses_semantic_change_or_missing_base`,
  `comment_language_test_only_localization_requires_immutable_base_proof`,
  `test_only_localization_differential_accepts_declared_cfg_test_change`, and
  `test_only_localization_differential_refuses_spoofed_or_production_change`.
- Cover target: N/A — declarative data validated by the planner.

**`docs/ci/CI-TOPOLOGY.md`**
- Purpose: planned, RamShared-only trust map for pull-request, isolated-lab,
  release, and manual remote-promotion surfaces.
- RF / DT: RF-1, RF-2, RF-6, RF-7, RF-11; DT-1–DT-4, DT-8–DT-10, DT-13.
- Required tests: `tools/ci/check-ci-contract.test.mjs` ::
  `topology_has_one_owner_per_canonical_gate`.
- Cover target: N/A — structural documentation validated by the contract.

**`docs/ci/RELEASE-INTEGRITY.md`**
- Purpose: planned source/tag/lock/toolchain/bundle/SBOM/signing contract and
  the non-promotion rule for test-signed drivers.
- RF / DT: RF-8, RF-9, RF-11; DT-10, DT-11, DT-13.
- Required tests: `tools/ci/check-release-integrity.test.mjs` ::
  `release_manifest_rejects_test_signed_driver`.
- Cover target: N/A — structural documentation validated by the release checker.

**`tools/ci/check-ci-contract.mjs`**
- Purpose: parse bounded JSON/workflow text and verify P0 gate policy without
  executing commands or changing remote state.
- RF / DT: RF-1, RF-2, RF-4, RF-10; DT-1–DT-5, DT-12, DT-14.
- Functions: `validateContract`, `validateWorkflowPolicy`,
  `selectWholePullRequestGates`, `validateAggregate`, `main`.
- Required tests: `valid_contract_matches_workflows`,
  `ci_contract_rejects_removed_required_gate`,
  `ci_contract_rejects_continue_on_error_for_gate`,
  `ci_contract_requires_explicit_timeout`,
  `ci_contract_rejects_mutable_action_reference`,
  `whole_pull_request_change_remains_selected`,
  `ci_result_aggregator_fails_on_cancelled_selected_job`,
  `retry_policy_rejects_unknown_failure_class`.
- Cover target: ≥80% line, branch, and function coverage through Node's
  built-in runner.
- Kahneman: #13, #15, #16, #17.

**`tools/ci/check-ci-contract.test.mjs`**
- Purpose: positive and manufactured refusal fixtures in temporary directories.
- RF / DT: all CI contract decisions.
- Cover target: N/A — test file.

**`tools/ci/plan-rust-slice-coverage.mjs`**
- Purpose: validate the SPEC-to-command map, select exact canonical line
  coverage commands or closed platform/localization/test-only/structural ownership from a
  complete changed-file list, and execute only tokenized checked-in commands
  without a shell.
- RF / DT: RF-10; DT-12, DT-18, DT-23, DT-24, DT-28.
- Functions: `validateCoverageMap`, `selectCoverageEntries`,
  `runCoveragePlan`, `main`.
- Required tests: `spec_coverage_map_requires_exact_command_in_spec`,
  `changed_business_rust_file_requires_mapped_spec_command`,
  `no_rust_change_is_explicit_no_change_not_skip`, and
  `coverage_runner_uses_no_shell_and_fails_on_command_error`,
  `windows_platform_entry_requires_exact_feature_spec_and_named_checks`,
  `windows_platform_entry_refuses_static_harness_outside_windows_ci`,
  `localization_differential_accepts_comment_only_source_change`, and
  `localization_differential_refuses_semantic_change_or_missing_base`,
  `comment_language_test_only_localization_requires_immutable_base_proof`,
  `test_only_localization_differential_accepts_declared_cfg_test_change`, and
  `test_only_localization_differential_refuses_spoofed_or_production_change`,
  `structural_contract_accepts_only_module_surface_and_runs_package_tests`, and
  `structural_contract_refuses_executable_or_malformed_rust`.
- Cover target: ≥80% line, branch, and function coverage through Node's
  built-in runner.
- Kahneman: #13, #15, #16, #17.

**`tools/ci/check-ci-artifacts.mjs`**
- Purpose: validate bounded artifact manifests, allowed retention classes,
  SHA-256 inventory, and sanitization outcomes without uploading files.
- RF / DT: RF-7, RF-9; DT-10.
- Functions: `validateArtifactManifest`, `validateRetention`,
  `validateSanitizedText`, `main`.
- Required tests: `artifact_manifest_requires_hash_and_retention`,
  `artifact_sanitizer_rejects_private_or_sensitive_content_without_echo`,
  `artifact_manifest_rejects_unknown_class`.
- Cover target: ≥80% line, branch, and function coverage through Node's
  built-in runner.
- Kahneman: #13, #16.

**`tools/ci/check-ci-artifacts.test.mjs`**
- Purpose: positive and refusal-path artifact fixtures.
- Cover target: N/A — test file.

**`tools/ci/plan-isolated-lab.mjs`**
- Purpose: validate closed manual-plan inputs and write a deterministic,
  host-action-free plan plus paired `lab-plan` artifact manifest.
- RF / DT: RF-7, RF-11; DT-9, DT-10, DT-13, DT-16.
- Functions: `validateLabPlanInput`, `buildLabPlan`, `writeLabPlan`, `main`.
- Required tests: `lab_dispatch_requires_protected_environment`,
  `lab_dispatch_rejects_daily_or_physical_target`,
  `lab_dispatch_rejects_untrusted_revision`,
  `plan_only_dispatch_has_no_host_mutation`.
- Cover target: ≥80% line, branch, and function coverage through Node's
  built-in runner.
- Kahneman: #15, #16, #17.

**`tools/ci/plan-isolated-lab.test.mjs`**
- Purpose: positive and refusal-path fixtures for source-only lab planning.
- Cover target: N/A — test file.

**`tools/ci/check-release-integrity.mjs`**
- Purpose: validate release manifest bindings, bundle/SBOM hashes, and public
  signing classification without publishing an asset.
- RF / DT: RF-8, RF-9; DT-11.
- Functions: `validateReleaseManifest`, `validateSourceBinding`,
  `validatePublicDriverEligibility`, `main`.
- Required tests: `release_manifest_requires_bound_inputs`,
  `release_manifest_rejects_dirty_source`,
  `release_manifest_rejects_missing_sbom`,
  `release_manifest_rejects_test_signed_driver`,
  `release_manifest_rejects_hash_mismatch`.
- Cover target: ≥80% line, branch, and function coverage through Node's
  built-in runner.
- Kahneman: #9, #13, #16.

**`tools/ci/check-release-integrity.test.mjs`**
- Purpose: positive and refusal-path release fixtures.
- Cover target: N/A — test file.

**`tools/ci/write-release-manifest.mjs`**
- Purpose: write a deterministic source/bundle/SBOM release manifest from only
  full tag/revision identities and safe relative artifact paths; it does not
  publish, sign, attach, or mutate a release.
- RF / DT: RF-8, RF-9; DT-10, DT-11.
- Functions: `buildReleaseManifest`, `writeReleaseManifest`, `main`.
- Required tests: `release_manifest_writer_binds_exact_input_hashes`,
  `release_manifest_writer_rejects_unsafe_output_or_revision`.
- Cover target: ≥80% line, branch, and function coverage through Node's
  built-in runner.
- Kahneman: #9, #13, #16.

**`tools/ci/write-release-manifest.test.mjs`**
- Purpose: positive and refusal-path fixtures for deterministic manifest
  generation.
- Cover target: N/A — test file.

**`scripts/windows/Test-WindowsCiStatic.ps1`**
- Purpose: invoke a fixed static-test allowlist and refuse mutation switches.
- RF / DT: RF-6, RF-10; DT-8, DT-12, DT-23.
- Functions: `Invoke-WindowsCiStaticSuite`, `Assert-StaticOnlyInvocation`.
- Fixed harnesses: `Test-AutonomousBrokerStatic.ps1`,
  `Test-ProductOnlineStatic.ps1`, `Test-RamSharedInfIsolationStatic.ps1`,
  `Test-WindowsDiskCounterAuditStatic.ps1`, `Test-WindowsStorageMatrixStatic.ps1`,
  `Test-Win11LabMediaContractStatic.ps1`, `Test-Win11LabReadyStatic.ps1`,
  `Test-Win11LabDriverTestFirmwareStatic.ps1`, `Test-WinDriveIoctlValidationStatic.ps1`,
  `Test-HostAutonomousLifecycleStatic.ps1`, `Test-GuestExhaustiveStatic.ps1`,
  and `Test-RecoverGuestVerifierExactRunStatic.ps1` only.
- Required tests: `windows_static_suite_runs_named_static_harnesses`,
  `windows_static_suite_refuses_mutating_switches`,
  `windows_static_suite_rejects_physical_host_flag`,
  `Test-HostAutonomousLifecycleStatic`, `Test-GuestExhaustiveStatic`,
  `Test-RecoverGuestVerifierExactRunStatic`.
- Cover target: N/A — PowerShell orchestration; live hosted Windows static E2E
  is the primary proof.
- Kahneman: #13, #16.

**`.github/workflows/ci-contract.yml`**
- Purpose: run contract/tool tests, artifact-hygiene coverage, exact
  SPEC-selected Rust coverage, exact platform ownership validation, and a
  same-run fail-closed aggregate over local reusable canonical workflow callers
  on pull requests and main pushes.
- RF / DT: RF-1, RF-2, RF-4, RF-5, RF-10; DT-1–DT-7, DT-12, DT-14,
  DT-17–DT-19, DT-23.
- Required tests: named Node tests above plus
  `aggregate_reusable_workflow_architecture_rejects_missing_summary_or_needs`,
  `aggregate_needs_rejects_cancelled_or_skipped_caller`, and
  `ci_contract_local_gate_preserves_remote_partial`.
- Cover target: N/A — workflow orchestration.

**`.github/workflows/windows-static.yml`**
- Purpose: hosted Windows compilation and static PowerShell validation.
- RF / DT: RF-6, RF-10; DT-3, DT-4, DT-8, DT-12, DT-23.
- Required tests: `windows_static_suite_runs_named_static_harnesses`,
  `windows_static_suite_refuses_mutating_switches`,
  `windows_static_suite_rejects_physical_host_flag`,
  `Test-HostAutonomousLifecycleStatic`, `Test-GuestExhaustiveStatic`.
- Cover target: N/A — hosted Windows E2E/static orchestration; Rust business
  files use the feature SPEC's per-file coverage rows.

**`.github/workflows/windows-lab.yml`** and **`.github/workflows/wsl2-lab.yml`**
- Purpose: protected manual plan-only isolated-lab dispatch skeletons.
- RF / DT: RF-7, RF-11; DT-3, DT-4, DT-9, DT-13.
- Required tests: `lab_dispatch_requires_protected_environment`,
  `lab_dispatch_rejects_daily_or_physical_target`,
  `lab_dispatch_rejects_untrusted_revision`,
  `plan_only_dispatch_has_no_host_mutation`.
- Cover target: N/A — manual dispatch E2E only.
- Kahneman: #15, #16, #17.

**`.github/workflows/release-integrity.yml`**
- Purpose: protected tag-validation for clean source binding, Linux bundle,
  SBOM, manifest, artifact policy, and signing classification; it has no
  release publication or asset-attachment authority.
- RF / DT: RF-8, RF-9, RF-11; DT-3, DT-4, DT-10, DT-11, DT-13.
- Required tests: release-validator named tests and
  `release_integrity_live_manifest_passes`.
- Cover target: N/A — protected release orchestration.

**`.github/workflows/cancel-closed-pr.yml`**
- Purpose: cancel queued/in-progress pull-request runs after a merged,
  same-repository closure using the only workflow granted `actions: write`;
  unmerged and fork closures are no-ops.
- RF / DT: RF-2, RF-4; DT-2–DT-4.
- Required tests: `closed_pull_request_cancellation_has_minimum_permission`
  and `closed_pull_request_cancellation_refuses_unscoped_run_selection`.
- Cover target: N/A — API orchestration with a live protected test path.

## Files to MODIFY

**`tools/ci/check-rust-slice-coverage.mjs`** and
**`tools/ci/check-rust-slice-coverage.test.mjs`**
- Purpose: run the exact Rust per-file coverage gate with private Cargo
  target/profile/report state and a bounded, fail-closed repository-scoped
  ownership lock; it does not change Rust code, a workspace target directory,
  a host, a workflow, or a remote setting.
- RF / DT: RF-10; DT-12, DT-14, DT-25, DT-26, DT-30.
- Functions: `createCoverageRun`, `acquireCoverageLock`, `releaseCoverageLock`,
  `runLlvmCov`, `main`.
- Required tests: `overlapping_checker_invocations_isolate_llvm_cov_target_state`,
  `coverage_lock_wait_is_bounded_and_live_owner_is_preserved`,
  `coverage_lock_detects_stale_and_corrupt_owner_fail_closed`,
  `coverage_child_deadline_is_terminal_and_fail_closed`,
  `coverage_child_deadline_terminates_descendant_process_tree`,
  `coverage_run_cleans_owned_state_after_child_failure`, and
  `coverage_run_signal_cleanup_never_deletes_foreign_owner`, and
  `coverage_run_signal_cleanup_covers_lock_wait`.
- Cover target: >=80% line, branch, and function coverage through Node's
  built-in runner.
- Kahneman: #13, #16, #17.

**`.github/workflows/ci.yml`**
- RF / DT: RF-2, RF-4, RF-5, RF-10; DT-2–DT-7, DT-12, DT-14, DT-17.
- Before → after: independent basic jobs without complete contract mapping and
  explicit bounds → bounded jobs that retain direct events, admit a local
  reusable caller, and expose an `if: always()` fail-closed suite summary.
- Tests: `ci_contract_live_gate_passes`; existing Rust and docs checks.
- Cover: N/A — workflow orchestration.

**`.github/workflows/security-scans.yml`**
- RF / DT: RF-3, RF-4; DT-3–DT-6, DT-14, DT-17, DT-29.
- Before → after: advisory scan only → advisory plus `cargo deny check`, each
  under explicit least permissions, timeout, immutable actions, and no broad
  retry/tolerance. `cargo audit` fetches only the contract-recorded immutable
  advisory-db snapshot, checks its 7-day maximum age, and uses `--no-fetch`.
- Tests: `ci_contract_rejects_continue_on_error_for_gate`,
  `ci_contract_rejects_stale_advisory_snapshot`,
  `ci_contract_requires_fail_closed_trivy_sarif_publication`, and live security
  job.
- Cover: N/A — workflow orchestration.

**`.github/workflows/gitleaks.yml`**, **`.github/workflows/comment-language.yml`**,
**`.github/workflows/pr-body.yml`**, and **`.github/workflows/validation-schema.yml`**
- RF / DT: RF-2, RF-4; DT-2–DT-5, DT-17.
- Before → after: individually configured workflow source → explicit policy
  ownership, timeout, permissions, immutable action references, local reusable
  admission, and fail-closed aggregate membership where canonical.
- Tests: `valid_contract_matches_workflows`.
- Cover: N/A — workflow orchestration.

**`.github/workflows/release.yml`**
- RF / DT: RF-8, RF-9; DT-4, DT-11.
- Before → after: version/changelog event → protected release-integrity handoff
  without granting pull-request authority or publishing an ineligible driver.
- Tests: `release_manifest_rejects_test_signed_driver`.
- Cover: N/A — release orchestration.

**`.github/dependabot.yml`**
- RF / DT: RF-4; DT-5, DT-6, DT-11.
- Before → after: dependency updates only → reviewed update coverage for pinned
  actions and release/security tool versions.
- Tests: `ci_contract_rejects_mutable_action_reference`.
- Cover: N/A — declarative configuration.

**`tools/ci/check-public-hygiene.mjs`** and
**`tools/ci/check-public-hygiene.test.mjs`**
- RF / DT: RF-5, RF-7; DT-7, DT-10.
- Before → after: hygiene logic without a canonical measured coverage gate →
  line/branch/function coverage at or above 80% plus no-echo refusal fixtures.
- Tests: `public_hygiene_rejects_sensitive_content_without_echo`,
  `public_hygiene_coverage_meets_threshold`.
- Cover: ≥80% line, branch, and function coverage through Node's built-in
  runner.

**`docs/governance/README.md`**
- RF / DT: RF-1, RF-11; DT-1, DT-13.
- Before → after: documentation-governance entry point → entry point also
  links the CI contract and explicitly marks remote controls manual.
- Tests: `topology_has_one_owner_per_canonical_gate`.
- Cover: N/A — documentation.

## Files to DELETE

None. The initial implementation adds a Day-0 trust path and does not retain a
second compatibility workflow.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| selected canonical gates and no-change reasons | aggregate result | bounded JSON/text summary |
| policy violation | checker stderr | relative path + stable rule code |
| coverage-run ownership | slice-coverage checker stderr | stable isolation/lock rule code; no absolute temporary path |
| timeout/retry class and attempt count | job result/artifact manifest | bounded enum + integer |
| action/tool/version identity | CI contract/release manifest | immutable SHA or pinned version |
| artifact inventory and sanitizer result | artifact manifest | class, retention, bytes, SHA-256, boolean |
| lab target refusal | protected plan result | stable refusal code, no host data |
| release eligibility | release manifest | `eligible` boolean + signing classification |
| remote control evidence | sanitized manual record | observation date, control names, `PARTIAL`/`PASS` state |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | N/A — no product topology change |
| ADR | N/A — repository CI policy is owned by this SPEC unless a future release architecture decision requires one |
| `docs/reliability/DEGRADATION-MATRIX.md` | N/A — no runtime failure mode added |
| `docs/ci/CI-TOPOLOGY.md` | Create; keep planned/active state explicit |
| `docs/ci/RELEASE-INTEGRITY.md` | Create; keep signing and public-driver eligibility explicit |
| `docs/governance/README.md` | Update when the contract exists |
| `validation.md` | Append only after a real hosted/lab/release validation path |
| `docs/BENCHMARKS.md` + JSONL | N/A — no benchmark claim |
| `.claude/rules/*`, `CLAUDE.md`, `AGENTS.md` | N/A unless the implementation changes a permanent repository convention |

## Implementation order

1. **ITEM-1 — Contract RED.** Add the contract data model and manufactured
   missing/disabled/mutable/broad-permission/no-timeout fixtures before the
   checker implementation.
2. **ITEM-2 — Contract GREEN and aggregate policy.** Implement the checker,
   whole-PR selector, retry classifier, and final aggregate semantics; run the
   Node coverage gate.
3. **ITEM-3 — Harden hosted canonical workflows.** Add explicit timeouts,
   permissions, full-SHA action references, actionlint, `cargo deny check`, and
   public-hygiene coverage. Pin and age-check the RustSec advisory-db snapshot
   before `cargo audit`; retain upstream-HEAD health for a later curator only.
   Do not change remote settings.
4. **ITEM-4 — Add fork-safe Windows static validation.** Add the static-suite
   wrapper and hosted Windows workflow; prove all mutation flags refuse.
5. **ITEM-5 — Add artifact and protected-lab plan boundaries.** Add artifact
   validation and manual plan-only Windows/WSL2 skeletons; prove target and
   revision refusals before action.
6. **ITEM-6 — Add release integrity.** Add source/bundle/SBOM/driver-policy
   validation and the protected release workflow.
7. **ITEM-6.5 — Close local aggregate and coverage admission.** First add
   manufactured RED cases for missing caller summaries/needs, cancelled or
   skipped callers, non-exact SPEC commands, unmapped Rust business files, and
   a remote-only local-check result. Then add local reusable callers,
   always-run summaries, the exact SPEC coverage map/planner, artifact and
   contract jobs, and the aggregate. Do not use independently scheduled run
   polling as an aggregate substitute.
8. **ITEM-6.6 — Isolate local Rust coverage execution.** Add RED overlap,
   bounded-wait, stale/corrupt-owner, child-failure, and signal-cleanup
   fixtures before changing the checker. Give each invocation a private Cargo
   target/profile/report root and require an exact-owner lock cleanup; do not
   retry or delete an ambiguous lock. Run the Node coverage gate and one
   serialized canonical Rust slice smoke.
9. **ITEM-7 — Record remote-control gap.** Obtain no remote change from code;
   record an administrator observation separately or keep `PARTIAL`.
10. **ITEM-8 — Real CI evidence and closure.** Run the named local tests,
   coverage, hosted internal pull-request path, plan-only protected dispatch,
   and release-manifest refusal suite. Append validation and write `IMPL.md`
   only when all required evidence exists.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `tools/ci/check-ci-contract.mjs` | `tools/ci/check-ci-contract.test.mjs` :: `valid_contract_matches_workflows` | unit | #13 | ≥80% Node |
| same | same :: `ci_contract_rejects_removed_required_gate` | refusal | #13/#16 | ≥80% Node |
| same | same :: `ci_contract_rejects_continue_on_error_for_gate` | refusal | #13 | ≥80% Node |
| same | same :: `ci_contract_requires_explicit_timeout` | refusal | #16 | ≥80% Node |
| same | same :: `ci_contract_rejects_stale_advisory_snapshot` | refusal | #13/#16 | ≥80% Node |
| same | same :: `ci_contract_rejects_missing_or_mismatched_advisory_snapshot` | refusal | #13/#16 | ≥80% Node |
| same | same :: `ci_contract_rejects_mutable_action_reference` | refusal | #13 | ≥80% Node |
| same | same :: `whole_pull_request_change_remains_selected` | unit | #13 | ≥80% Node |
| same | same :: `ci_result_aggregator_fails_on_cancelled_selected_job` | refusal | #13/#17 | ≥80% Node |
| same | same :: `retry_policy_rejects_unknown_failure_class` | refusal | #15 | ≥80% Node |
| same | same :: `aggregate_reusable_workflow_architecture_rejects_missing_summary_or_needs` | refusal | #13/#17 | ≥80% Node |
| same | same :: `aggregate_needs_rejects_cancelled_or_skipped_caller` | refusal | #13/#17 | ≥80% Node |
| same | same :: `closed_pull_request_cancellation_refuses_unscoped_run_selection` | refusal | #13/#16 | ≥80% Node |
| same | same :: `remote_controls_missing_evidence_is_blocked` | refusal | #16 | ≥80% Node |
| same | same :: `remote_controls_unsafe_observation_is_no_go` | refusal | #13/#16 | ≥80% Node |
| same | same :: `remote_controls_compliant_observation_is_accepted` | unit | #13 | ≥80% Node |
| same | same :: `remote_controls_foreign_or_stale_observation_is_no_go` | refusal | #3/#13 | ≥80% Node |
| same | same :: `remote_controls_unreadable_evidence_is_no_go` | refusal | #13/#16 | ≥80% Node |
| same | same :: `remote_controls_broken_symlink_is_no_go` | refusal | #13/#16 | ≥80% Node |
| same | same :: `remote_controls_schema_matches_runtime_contract` | unit/refusal | #13 | ≥80% Node |
| same | same :: `remote_controls_sensitive_control_name_is_refused` | refusal | #16 | ≥80% Node |
| `tools/ci/check-rust-slice-coverage.mjs` | `tools/ci/check-rust-slice-coverage.test.mjs` :: `overlapping_checker_invocations_isolate_llvm_cov_target_state` | integration | #13/#17 | ≥80% Node |
| same | same :: `coverage_lock_wait_is_bounded_and_live_owner_is_preserved` | refusal | #16 | ≥80% Node |
| same | same :: `coverage_lock_detects_stale_and_corrupt_owner_fail_closed` | refusal | #13/#16 | ≥80% Node |
| same | same :: `coverage_run_cleans_owned_state_after_child_failure` | refusal | #16 | ≥80% Node |
| same | same :: `coverage_run_signal_cleanup_never_deletes_foreign_owner` | refusal | #16/#17 | ≥80% Node |
| same | same :: `coverage_run_signal_cleanup_covers_lock_wait` | refusal | #16/#17 | ≥80% Node |
| `tools/ci/plan-rust-slice-coverage.mjs` | `tools/ci/plan-rust-slice-coverage.test.mjs` :: `spec_coverage_map_requires_exact_command_in_spec` | unit/refusal | #13 | ≥80% Node |
| same | same :: `changed_business_rust_file_requires_mapped_spec_command` | refusal | #13/#16 | ≥80% Node |
| same | same :: `windows_platform_entry_requires_exact_feature_spec_and_named_checks` | unit/refusal | #13/#16 | ≥80% Node |
| same | same :: `windows_platform_entry_refuses_static_harness_outside_windows_ci` | refusal | #13/#16 | ≥80% Node |
| same | same :: `localization_differential_accepts_comment_only_source_change` | unit | #13 | ≥80% Node |
| same | same :: `localization_differential_refuses_semantic_change_or_missing_base` | refusal | #13/#16 | ≥80% Node |
| same | same :: `test_only_localization_differential_accepts_declared_cfg_test_change` | unit | #13/#16 | ≥80% Node |
| same | same :: `test_only_localization_differential_refuses_spoofed_or_production_change` | refusal | #13/#16 | ≥80% Node |
| same | same :: `structural_contract_accepts_only_module_surface_and_runs_package_tests` | unit | #13/#16 | ≥80% Node |
| same | same :: `structural_contract_refuses_executable_or_malformed_rust` | refusal | #13/#16 | ≥80% Node |
| `tools/ci/check-ci-artifacts.mjs` | `tools/ci/check-ci-artifacts.test.mjs` :: `artifact_manifest_requires_hash_and_retention` | unit/refusal | #13 | ≥80% Node |
| same | same :: `artifact_sanitizer_rejects_private_or_sensitive_content_without_echo` | refusal | #16 | ≥80% Node |
| `tools/ci/check-release-integrity.mjs` | `tools/ci/check-release-integrity.test.mjs` :: `release_manifest_requires_bound_inputs` | unit | #9/#13 | ≥80% Node |
| same | same :: `release_manifest_rejects_test_signed_driver` | refusal | #13/#16 | ≥80% Node |
| `tools/ci/check-public-hygiene.mjs` | `tools/ci/check-public-hygiene.test.mjs` :: `public_hygiene_rejects_sensitive_content_without_echo` | refusal | #13 | ≥80% Node |
| `scripts/windows/Test-WindowsCiStatic.ps1` | same :: `windows_static_suite_refuses_mutating_switches` | static/refusal | #16 | N/A — hosted Windows static E2E |
| protected lab workflow contract | workflow/static fixture :: `lab_dispatch_rejects_daily_or_physical_target` | dispatch refusal | #16 | N/A — manual isolated-lab E2E |
| protected lab workflow contract | workflow/static fixture :: `plan_only_dispatch_has_no_host_mutation` | dispatch E2E | #17 | N/A — manual isolated-lab E2E |
| release workflow contract | workflow/static fixture :: `release_integrity_live_manifest_passes` | protected release E2E | #13 | N/A — release orchestration |
| Rust business logic selected by feature SPEC | feature test matrix exact name | unit/drill | feature-specific | `check-rust-slice-coverage.mjs` per file ≥80% |

## Validation checklist

- [ ] `node --test --experimental-test-coverage` reaches at least 80% lines,
      branches, and functions for each new Node CI/security checker.
- [ ] `node tools/ci/check-ci-contract.mjs --check` accepts the canonical tree
      and refusal fixtures exit non-zero.
- [ ] `node tools/ci/check-ci-contract.mjs --check-local` accepts only a
      source-clean tree whose sole remaining result is the explicit
      administrator-only remote-control `PARTIAL`; it must not print `PASS`.
- [ ] The aggregate entrypoint calls only exact same-revision local reusable
      workflows, each called workflow exposes a fail-closed `if: always()`
      summary, and a cancelled/skipped/missing caller makes the aggregate
      non-zero.
- [ ] The Rust coverage planner rejects a map command absent from its source
      SPEC and every changed `crates/*/src/**/*.rs` file without an exact
      mapped line command or a closed DT-23/DT-24/DT-28 contract; a platform declaration must bind
      existing Windows static/live named checks, and a localization declaration
      must prove byte-identical non-comment Rust text from a full base SHA. A
      DT-24 test-only declaration must prove its declared root `cfg(test)`
      region, byte-identical production projection, package test command, and
      immutable-base ignored-GPU evidence. A DT-28 structural declaration must
      accept only a whole-file module/reexport surface and run every distinct
      tokenized package test once. It invokes canonical commands without a
      shell.
- [ ] Two overlapping `check-rust-slice-coverage.mjs` invocations use distinct
      private Cargo target/profile/report roots, and the lock fixtures prove a
      bounded live-owner wait, stale/corrupt-owner fail-closed result, and
      exact-owner cleanup after child failure and `SIGINT`/`SIGTERM` handling.
- [ ] `node tools/ci/check-ci-artifacts.mjs --check <fixture>` accepts one
      sanitized manifest and rejects each prohibited case without echoing it.
- [ ] `node tools/ci/check-release-integrity.mjs --check <fixture>` rejects
      dirty/unbound/missing-SBOM/hash-mismatched/test-signed cases.
- [ ] `cargo fmt --all -- --check`, Clippy, and targeted Rust tests pass for
      touched crates.
- [ ] For each touched Rust business file, the feature SPEC's canonical
      `check-rust-slice-coverage.mjs -p … --files … --min 80` command passes.
- [ ] `cargo audit` using the exact age-valid advisory-db snapshot and `cargo
      deny check` both exit zero in the security job; upstream-HEAD health is
      recorded separately and cannot waive snapshot age.
- [ ] The hosted Windows job compiles/tests its named Rust slices and runs only
      the static PowerShell suite.
- [ ] The static suite refuses installation, service, VM, GPU, pressure,
      physical-host, shutdown, and reboot switches.
- [ ] A real internal pull request supplies before/action/after evidence for
      selection, aggregate, permissions, artifact retention, and refusal paths.
- [ ] A protected manual lab plan proves one legitimate isolated-lab plan and
      daily/physical/untrusted-ref refusals before runner action; no host
      mutation occurs.
- [ ] A protected release-manifest path proves a legitimate Linux bundle entry
      and refusal of test-signed Windows driver promotion.
- [ ] Remote-control evidence is present and sanitized, or the final status is
      `PARTIAL` with concrete missing control and next proof.
- [ ] `git diff --check` passes. Do not claim `IMPL` or `DONE` before the live
      CI/operator-surface evidence and manual remote promotion are complete.
