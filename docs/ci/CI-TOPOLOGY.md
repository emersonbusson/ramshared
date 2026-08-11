# CI topology

> Checked-in source contract. This document is a design companion to
> [`ci-trust-and-release-integrity`](../specs/no-milestone/ci-trust-and-release-integrity/SPEC.md).
> Local policy validation proves the source topology only. It does not assert
> a hosted run, remote setting, protected environment, release publication, or
> product/platform proof.

## Purpose

RamShared CI must distinguish code that is safe to execute for an untrusted
pull request from work that can affect an isolated lab or a release. A green
hosted build is valuable, but it is not proof of a loaded driver, a WSL2
reclaim path, a kernel drill, a signed package, or a physical-host campaign.

The planned contract has four trust zones.

| Zone | Trigger | Runner authority | Permitted work | Explicitly forbidden |
| --- | --- | --- | --- | --- |
| Pull request | `pull_request` | hosted, read-only | Rust/docs/security checks, Windows Rust build, static PowerShell tests | secrets, write token, driver/service/VM actions, hardware, reboot, daily or physical host |
| Main push | protected branch push | hosted, least privilege | repeat pull-request gates and produce bounded diagnostics | protected-lab mutation, public driver promotion |
| Isolated lab | manual protected dispatch | dedicated isolated lab only | initially plan-only; later only a separately specified lab drill | daily host, physical host, untrusted revision, automatic host reboot |
| Release | protected tag/release path | protected release environment | bundle, manifest, SBOM, eligibility validation | untrusted source, test-signed public driver, secret disclosure |

## P0 gate map

| Gate | Checked-in owner | Trust zone | Terminal rule |
| --- | --- | --- | --- |
| CI contract | `tools/ci/check-ci-contract.mjs` | pull request, main push | Contract/workflow mismatch is `FAIL` |
| Rust quality | existing Rust workflow path | pull request, main push | Format, Clippy, or selected test failure is `FAIL` |
| Documentation and evidence | existing documentation gate | pull request, main push | Documentation, provenance, benchmark, or SPEC-evidence failure is `FAIL` |
| Rust supply chain | security workflow | pull request, main push | `cargo audit` or `cargo deny check` failure is `FAIL` |
| Public hygiene coverage | Node coverage command | pull request, main push | Any metric below 80% is `FAIL` |
| Windows static | hosted Windows workflow and static suite | pull request, main push | Compile/static failure or mutation attempt is `FAIL` |
| Artifact hygiene | artifact manifest checker | every artifact-producing job | Missing hash/class/retention or prohibited content is `FAIL` |
| Aggregate | `ci-contract.yml` local reusable callers | pull request, main push | Called failure/cancellation/unsafe skip is `FAIL` |
| Closed PR cancellation | `cancel-closed-pr.yml` | trusted merged same-repository PR only | Exact queued/running PR-head runs are cancelled; fork/unmerged closure is a no-op |
| Isolated-lab plan | manual Windows/WSL2 skeleton | isolated lab | Unsafe target/ref/approval request is `FAIL` before action |
| Release integrity | release manifest/SBOM checker | release | Unbound source, missing hash/SBOM, or ineligible driver is `FAIL` |

No gate is considered successful merely because its workflow file exists. The
aggregate calls the same-revision local reusable workflows inside one entrypoint
run, and consumes only those caller conclusions. It never races separately
scheduled workflow runs or infers a status from artifacts.

`CI Contract` is the sole automatic pull-request/main entrypoint. Its reusable
canonical workflows expose `workflow_call` only, so a source revision does not
run a direct duplicate beside the aggregate. The security workflow retains an
explicit manual `workflow_dispatch` path for an operator-initiated read-only
scan; it is not a second automatic admission path.

## Selection and aggregate semantics

Every pull-request/main caller runs instead of relying on an outer path filter.
The Rust slice subplan alone uses the complete pull-request diff rather than
only the latest push: it selects an exact canonical command from
[`rust-slice-coverage.json`](../governance/rust-slice-coverage.json), and an
unmapped changed `crates/*/src/**/*.rs` path fails before coverage runs. A
no-Rust-change result is explicit inside the successful coverage job, never a
skipped job.

The only acceptable terminal values are:

| Value | Meaning | Aggregate treatment |
| --- | --- | --- |
| `PASS` | The selected gate completed successfully. | eligible |
| `NO_CHANGE` | The always-run Rust coverage subplan found no business Rust source change. | eligible sub-result only |
| `FAIL` | A command, policy, artifact, or assertion failed. | fail closed |
| `BLOCKED` | Required environment/control/evidence is unavailable. | non-promotable |
| cancelled, missing, unexpected skip | The result is incomplete. | fail closed |

`continue-on-error` is prohibited for canonical quality, security, coverage,
artifact, aggregate, lab-selection, and release-eligibility gates. Uploading a
SARIF result may be best effort only after the corresponding scan has already
failed or passed correctly.

## Permissions and input trust

Every future workflow declares its permissions explicitly. The default posture
is `contents: read` only.

| Capability | Permitted location | Rule |
| --- | --- | --- |
| repository read | all workflow classes | minimum permission |
| pull-request metadata read | diff/PR validation job only | read only |
| security result upload | SARIF job only | `security-events: write` only |
| action cancellation | merged same-repository closed-PR cleanup only | `actions: write` only; no checkout, no fork/unmerged path |
| release publishing | protected release job only | protected short-lived release identity |
| signing material | no pull-request/main job | never printed, uploaded, or exposed to untrusted code |

Pull-request workflows must not use `pull_request_target`. A fork may execute
only the hosted read-only surface. Manual dispatch inputs are closed enums and
an exact trusted revision; they are not arbitrary shell fragments or runner
labels.

Every action reference is an immutable full commit SHA with a human version
comment. The CI contract checks this rule and the reviewed action policy.

## Boundedness and retry policy

| Job class | Maximum planned timeout | Retry policy |
| --- | ---: | --- |
| contract | 10 minutes | none |
| documentation | 20 minutes | none |
| Rust | 30 minutes | none |
| security | 30 minutes | none |
| Windows static | 45 minutes | none |
| plan-only lab | 15 minutes | none |
| release integrity | 45 minutes | none |

Only an explicitly classified dependency-fetch transport failure may retry. It
has at most two total attempts, a fixed 15-second delay, and a sanitized
classifier record. Test, lint, coverage, security, identity, artifact,
signing, target-selection, and timeout failures are never retried.

## Windows, WSL2, and kernel boundary

The hosted Windows lane is intentionally narrow:

- compile and test the named Windows Rust packages;
- run a fixed PowerShell static-test allowlist; and
- reject all arguments or code paths that can install a driver, alter SCM,
  manipulate a VM, use hardware, create pressure, change BCD, shut down, or
  reboot.

WDK, InfVerif, Driver Verifier, VM lifecycle, custom-kernel, QEMU, kselftest,
WSL2, BINARY_MATCH, and hardware evidence remain platform-specific gates. A
future isolated-lab drill must be specified separately and use the required
before → action → after evidence protocol. It must not substitute a hosted
compile for live proof.

The manual lab skeleton accepts only `plan` mode at P0. It must reject a
daily-host or physical-host target before any runner action. It cannot run a
physical campaign, invoke a shared-host pressure harness, or reboot a host.

## Artifact policy

Artifacts are evidence, not a raw-log archive. Every upload is preceded by a
manifest that records source revision, class, byte size, SHA-256, retention,
sanitization result, and terminal job status.

| Class | Contents | Retention | Public rule |
| --- | --- | ---: | --- |
| `pr-diagnostic` | bounded failed test/log summary | 7 days | sanitized only |
| `coverage-static` | coverage JSON/report and static-test summary | 14 days | sanitized only |
| `lab-plan` | plan-only dispatch result | 14 days | no private runner or host data |
| `release-evidence` | manifest, checksums, SBOM, eligibility record | immutable release material | public only after verification |

Raw dumps, private paths, usernames, secrets, signing material, raw kernel
addresses, real user data, and unredacted lab logs are never uploaded. A
sanitization failure blocks the job and reports a stable rule code without
echoing the prohibited value.

## Remote controls: manual promotion boundary

The following controls are external to source files and therefore remain
environment-bound until an administrator records a dated, sanitized
observation:

- required branch-protection contexts and review policy;
- default GitHub token permission and workflow approval capability;
- allowed-action policy;
- protected-environment reviewer rules;
- default artifact retention; and
- release/signing authority.

The repository checker may validate that evidence is present and current. It
must never mutate these controls. Absence of this observation is `BLOCKED`, not
an implied pass.

## Evidence required before implementation closure

The future IMPL must include all of the following on the actual CI/operator
surfaces:

1. a real internal pull-request before/action/after record covering selection,
   aggregate, timeout, permissions, and artifact-policy results;
2. a legitimate hosted Windows static run and a refusal for a mutation request;
3. a legitimate protected isolated-lab `plan` dispatch plus daily/physical and
   untrusted-revision refusals, with no host mutation;
4. a legitimate release-manifest validation plus dirty, missing-SBOM,
   hash-mismatch, and test-signed-driver refusals; and
5. a manual remote-control observation, or an explicit `PARTIAL` state with
   the missing control and next proof.

Until then, the checked-in topology remains `PARTIAL`: source admission is
locally validated, while hosted, lab, release, and administrator-control proof
remain unqualified.
