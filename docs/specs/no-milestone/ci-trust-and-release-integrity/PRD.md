---
slug: ci-trust-and-release-integrity
title: CI trust and release integrity
milestone: —
issues: []
---

# PRD — CI trust and release integrity

> SSDV3 Step 1 only. This PRD defines a RamShared-native CI and release
> integrity plan. It creates no workflow, remote setting, release asset,
> package, driver, daemon, kernel, VM, or host mutation.

## 1. Summary

RamShared already has Rust, documentation, secret-scanning, dependency-audit,
SPEC-evidence, benchmark-evidence, and public-hygiene controls. The remaining
risk is that the trust contract between a pull request, a protected lab, a
release artifact, and a remote repository setting is not represented in one
fail-closed, testable model.

The outcome is a small CI contract that distinguishes four surfaces:

1. fork-safe pull-request validation;
2. protected manual isolated-lab dispatches;
3. protected release integrity validation; and
4. remote repository controls that require an administrator and therefore
   remain explicitly environment-bound until recorded.

The plan freezes a P0 implementation matrix for workflow trust, dependency
policy, coverage, Windows static validation, lab isolation, artifact hygiene,
release provenance, and aggregate status. It does not promote Windows support,
driver signing, WSL2 support, or any product capability.

## 2. Technical context

### Confirmed in codebase

- [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml) runs the
  Rust format, Clippy, workspace-test, and documentation paths.
- [`.github/workflows/security-scans.yml`](../../../../.github/workflows/security-scans.yml),
  [`.github/workflows/gitleaks.yml`](../../../../.github/workflows/gitleaks.yml),
  and [`.github/dependabot.yml`](../../../../.github/dependabot.yml) provide
  current repository security and dependency-update foundations.
- [`deny.toml`](../../../../deny.toml) already declares a strict Rust
  dependency, license, source, advisory, and wildcard policy, but its command
  is not yet a canonical workflow gate.
- [`tools/ci/check-rust-slice-coverage.mjs`](../../../../tools/ci/check-rust-slice-coverage.mjs)
  enforces the SSDV3 per-production-file Rust line threshold rather than a
  workspace average.
- [`tools/ci/check-public-hygiene.mjs`](../../../../tools/ci/check-public-hygiene.mjs),
  [`tools/ci/check-documentation-governance.mjs`](../../../../tools/ci/check-documentation-governance.mjs),
  [`tools/ci/check-benchmark-evidence.mjs`](../../../../tools/ci/check-benchmark-evidence.mjs),
  and [`tools/ci/check-spec-evidence.mjs`](../../../../tools/ci/check-spec-evidence.mjs)
  establish public-data and evidence integrity boundaries.
- [`scripts/windows/`](../../../../scripts/windows/) contains static PowerShell
  harnesses and isolated-VM drills. The repository rules prohibit unsupervised
  pressure on the daily WSL2 host and require explicit approval for a physical
  Windows campaign.
- [`scripts/package/build-linux-bundle.sh`](../../../../scripts/package/build-linux-bundle.sh)
  produces a Linux bundle and checksum file. The existing release workflow is
  version/changelog automation; it is not yet a source-bound bundle,
  provenance, or SBOM gate.
- [`docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md`](../../../packaging/WINDOWS-DRIVER-DISTRIBUTION.md)
  keeps public Windows driver distribution conditional on trusted signing and
  package verification.

### Confirmed in documentation

- [`.claude/rules/ssdv3.md`](../../../../.claude/rules/ssdv3.md) requires
  platform-correct proof, per-file coverage for Rust business logic, and an
  honest `PARTIAL` state for an environment-bound gate.
- [`.claude/rules/governance.md`](../../../../.claude/rules/governance.md)
  applies Kahneman #15–#18 to CI, guards, watchdogs, and host-safety scripts.
- [`.claude/rules/benchmarks.md`](../../../../.claude/rules/benchmarks.md)
  requires fixed parameters, three or more observations, context capture, and
  verifiable sanitized evidence for benchmark claims.
- [`docs/reliability/GAP-REGISTER.md`](../../../reliability/GAP-REGISTER.md)
  is the current public location for environment-bound product gates.

### Inference / proposal

- Repository workflow source alone cannot prove branch protection, default
  token permissions, allowed-action policy, protected-environment reviewers,
  artifact-retention defaults, or signing authority. Those remote controls
  require a separately recorded administrator observation.
- A pull-request workflow must not be treated as a substitute for Windows WDK,
  Driver Verifier, isolated WSL2, custom-kernel, signing, or physical-host
  proof. It can compile and run static tests safely.
- A machine-readable CI contract can prevent a removed, skipped, silently
  tolerated, or differently scoped canonical gate from producing a false
  green result.

### Current gaps to close

| Gap | Risk | Required outcome |
| --- | --- | --- |
| No checked-in canonical CI contract | Required gates can drift from workflow files or protection settings | One fail-closed contract, checker, fixtures, and aggregate result |
| `deny.toml` is not a required command | Source/license/wildcard policy can be documented but unexecuted | `cargo deny check` is a required security job |
| Per-file slice cover is feature-local only | A changed Rust business file can rely on a workspace-average false green | Contract requires the SPEC-selected per-file command for affected slice logic |
| Workflow jobs have inconsistent boundedness and privilege declaration | A hung job or inherited write token can obscure a failure or enlarge blast radius | Explicit timeout and least-permission policy for every job |
| No fork-safe Windows CI lane | Windows-only Rust and static harness regressions depend on local discovery | Hosted Windows compilation and static PowerShell suite with no installation or reboot |
| No protected-lab workflow contract | A future lab job could accidentally target a daily or physical host | Dispatch-only skeletons fixed to isolated-lab intent and refusal-tested |
| No artifact retention/sanitization policy | Public CI can retain private paths, secrets, raw addresses, or unverifiable diagnostics | Bounded sanitized artifact manifest and retention classes |
| Release record is not an integrity gate | A release can lack a bundle hash, SBOM, source binding, or signing classification | Release manifest/SBOM validation and test-signed-driver refusal |
| Remote repository controls are external | CI source can look secure while remote enforcement differs | Explicit environment-bound manual promotion evidence |

## 3. Recommended option

Implement one primary CI trust path around a checked-in contract and a
fail-closed aggregate. Keep platform actions separate by trust level and never
route a pull request to a host-affecting runner.

The P0 matrix is deliberately frozen before implementation:

| P0 | Capability | Primary future path | Safe trigger | Required refusal / evidence |
| --- | --- | --- | --- | --- |
| P0-1 | CI contract | `docs/governance/ci-contract.json` and `tools/ci/check-ci-contract.mjs` | pull request and main push | Disabled, skipped, or tolerated canonical gate fails |
| P0-2 | Rust supply chain | security workflow running `cargo audit` and `cargo deny check` | pull request and main push | Unknown source, license, advisory, or wildcard fails |
| P0-3 | Bounded least privilege | canonical workflows with explicit `permissions`, timeout, and concurrency | pull request and main push | Missing timeout, broad token, or unsafe trigger fails |
| P0-4 | Public hygiene confidence | built-in Node coverage for the public-hygiene checker | pull request and main push | Coverage below 80% or a leaking fixture fails |
| P0-5 | Fork-safe Windows static lane | `windows-static.yml` and a static-suite wrapper | pull request and main push | Driver install, SCM mutation, reboot, VM control, or physical-host flag fails |
| P0-6 | Protected lab boundary | manual Windows and WSL2 lab skeletons | protected manual dispatch only | Daily-host, physical-host, untrusted-ref, or missing approval fails before action |
| P0-7 | Artifact hygiene | CI artifact manifest/sanitizer and retention policy | any artifact-producing job | Secret, private path, raw kernel address, missing hash, or invalid class fails |
| P0-8 | Release integrity | release manifest validator plus CycloneDX SBOM | protected tag/release path | Dirty/unbound source, absent hash/SBOM, or test-signed driver promotion fails |
| P0-9 | Result integrity | final aggregate job over whole-pull-request selection | pull request and main push | Selected failure, cancellation, or unexpected skip fails closed |
| P0-10 | Action provenance | full-SHA action references and a checked policy | pull request and main push | Mutable action reference or unapproved action fails |
| P0-11 | Remote promotion | administrator checklist and sanitized observation record | manual only | Missing remote proof leaves the slice `PARTIAL` |

This option uses the existing evidence and public-hygiene tooling as the
sanitization authority where applicable. It adds no generic orchestration
engine, no production compatibility path, and no implicit promotion from a
workflow file's presence.

### Alternatives rejected

| Alternative | Reason |
| --- | --- |
| Run WDK, VM, WSL2 pressure, signing, or physical-host drills on every pull request | Unsafe for untrusted code and incompatible with the daily-host policy |
| Treat a passing hosted Windows compile as live driver validation | Compile proof cannot replace WDK, Driver Verifier, BINARY_MATCH, or a supervised lab drill |
| Use one broad workflow token for all jobs | It expands the impact of a compromised or malformed job |
| Treat skipped jobs as automatically successful | A selected security, coverage, or platform gate can disappear behind a green aggregate |
| Retry every failure until green | This hides deterministic test, identity, signing, and host-safety regressions |
| Publish every lab artifact unchanged | Lab output can expose private paths, identities, secrets, or kernel-sensitive data |
| Publish a test-signed Windows driver as a normal release asset | Test signing is not a public trust chain |
| Change remote settings from repository code | Remote policy is an administrator-controlled promotion boundary, not a source-file side effect |

### Trade-offs

The contract adds workflow and tool maintenance, and an initial protected-lab
skeleton may remain `PARTIAL` until a suitable isolated runner and reviewer
environment exist. This is preferable to treating unavailable hardware proof
as green. Full-SHA action pinning increases update work; Dependabot remains the
reviewed update mechanism. A release SBOM adds a pinned tool dependency and
must be versioned as part of the release toolchain.

## 4. Functional requirements

| ID | Requirement | Verifiable acceptance |
| --- | --- | --- |
| RF-1 | Define one versioned CI contract. | The checker validates canonical jobs, expected context names, triggers, whole-PR selection, permissions, timeouts, artifact class, retry policy, and aggregate membership. |
| RF-2 | Make result selection fail closed. | A selected job that fails, is cancelled, is missing, is unexpectedly skipped, or is marked `continue-on-error` makes the final aggregate fail. A documented no-change path is the only allowed skip. |
| RF-3 | Enforce Rust dependency policy. | CI runs both `cargo audit` and `cargo deny check`; failure is blocking and no step suppresses it. |
| RF-4 | Enforce workflow least privilege and provenance. | Every job declares the minimum GitHub permission set, no pull-request job receives secrets or write authority, and every action reference is a full commit SHA with a human version comment. |
| RF-5 | Preserve public-hygiene test strength. | The checker has positive and refusal fixtures and at least 80% line, branch, and function coverage from the Node built-in runner. |
| RF-6 | Add a fork-safe Windows static surface. | A hosted Windows job compiles the Windows Rust slices and runs only static PowerShell tests; source and tests prove it cannot install a driver, mutate SCM, start a VM, allocate GPU memory, or reboot. |
| RF-7 | Define protected isolated-lab dispatches. | Manual workflows require a protected environment and fixed isolated-lab semantics; physical/daily-host targeting and untrusted references are rejected before runner action. |
| RF-8 | Sanitize and retain artifacts deliberately. | Each uploaded artifact has a class, bounded retention, source SHA, SHA-256 inventory, and sanitization result; prohibited content is refused without echoing it. |
| RF-9 | Bind releases to reproducible inputs. | A release manifest records exact tag, commit, clean-tree state, lockfile hash, tool versions, bundle hashes, SBOM hash, and signing classification. A public Windows driver entry rejects test signing. |
| RF-10 | Preserve SSDV3 coverage semantics. | A Rust business-logic change selected by a feature matrix runs `check-rust-slice-coverage.mjs` with named packages/files and an 80% per-file minimum; a workspace average is rejected. |
| RF-11 | Keep remote controls honest. | Branch protection, default token, allowed-action policy, review policy, protected environments, and retention defaults are recorded as manual, date-bound evidence. Missing proof prevents `DONE`. |

## 5. Non-functional requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| NFR-1 | Host safety | No pull-request workflow can invoke driver install, SCM change, Hyper-V action, reboot, WSL shutdown, GPU pressure, swap operation, or physical host selection. |
| NFR-2 | Boundedness | Canonical hosted jobs have explicit finite timeouts; retries are typed, capped, and recorded. |
| NFR-3 | Observability | Every aggregate and protected dispatch emits a sanitized result explaining selected gates, skipped reason, source revision, artifact inventory, and terminal verdict. |
| NFR-4 | Determinism | Same workflow contract and source tree produce the same selected-gate decision; non-deterministic timestamps are excluded from contract comparison. |
| NFR-5 | Public safety | Diagnostics never print secret values, private paths, raw kernel addresses, signing material, or user data. |
| NFR-6 | No false promotion | A static CI pass does not claim lab E2E, driver signing, Windows public distribution, WSL2 stability, or a release promotion. |

## 6. Flows

### Pull-request flow

1. The contract checker reads the committed CI contract and workflow source
   without executing workflow text.
2. A whole-pull-request change selector determines which canonical gates are
   applicable; it does not use only the last pushed commit.
3. Hosted jobs run Rust, documentation, supply-chain, public-hygiene, and
   fork-safe Windows static validation under explicit read-only permissions.
4. Each job writes a bounded result record. Failure artifacts are sanitized
   before upload.
5. The aggregate checks every selected result. It reports green only when all
   selected gates succeeded or an explicitly permitted no-change condition was
   proved.

### Protected isolated-lab dispatch flow

1. A maintainer manually dispatches a workflow from a trusted revision.
2. The workflow checks its protected environment and fixed isolated-lab mode.
3. Any daily-host, physical-host, untrusted-reference, or missing-approval
   request is refused before a runner action.
4. The initial P0 skeleton emits a sanitized plan/result artifact only. A
   later SPEC revision is required before it may invoke WDK, VM, kernel, or
   WSL2 commands.
5. A future live lab drill must record before → action → after, one legitimate
   case, named refusals, cleanup, and the platform's required identity gates.

### Release flow

1. A protected release path checks the tag/source/lockfile binding and builds
   the Linux bundle from a clean revision.
2. It verifies package hashes, writes a release manifest, and generates a
   CycloneDX SBOM from pinned tooling.
3. The release validator rejects absent or mismatched data and rejects a
   test-signed Windows driver from public promotion.
4. The release asset set is published only after the aggregate and release
   manifest pass. A separately unavailable signing authority remains
   environment-bound rather than simulated.

### Error flow

| Trigger | Result | Safe state |
| --- | --- | --- |
| Missing, disabled, cancelled, or tolerated selected gate | aggregate exits non-zero | no promotion or release action |
| Deterministic test/security/coverage/identity failure | no retry | failed result with sanitized evidence |
| Classified transient dependency fetch failure | at most two attempts with fixed delay | failed result after budget exhaustion |
| Physical/daily target or untrusted ref in lab dispatch | refusal before runner action | no VM, driver, service, or host mutation |
| Artifact sanitization failure | no upload of offending content | failed job with rule code only |
| Test-signed driver in public release set | release validator exits non-zero | no public Windows driver asset |

## 7. Data and state model

The future `docs/governance/ci-contract.json` has a schema version and a
bounded set of canonical gates. Each gate records:

- stable context name and workflow/job source;
- trust level (`pull-request`, `isolated-lab`, or `release`);
- trigger and whole-PR applicability;
- explicit timeout, concurrency group, permissions, and retry class;
- input trust rules and forbidden host-affecting capabilities;
- test/coverage command ownership;
- artifact class, retention, sanitizer, and SHA-256 requirement; and
- aggregate membership and allowed no-change condition.

The future release manifest is separate from benchmark evidence. It records
source/tag/lock/toolchain identity, bundle and SBOM hashes, signing
classification, public-asset eligibility, and a numeric/observable rollback
trigger. It never contains a private key, secret, or signing password.

CI terminal states are `PASS`, `FAIL`, `NO_CHANGE`, and `BLOCKED`. Only
`PASS`, plus a contractually valid `NO_CHANGE`, can satisfy the aggregate.
`BLOCKED` is visible and non-promotable.

## 8. Interfaces

The future implementation has these repository-owned interfaces:

| Interface | Contract |
| --- | --- |
| `node tools/ci/check-ci-contract.mjs --check` | Validates the checked-in contract against workflow source without network mutation. |
| `node tools/ci/check-release-integrity.mjs --check <manifest>` | Validates release identity, artifact hashes, SBOM, and signing classification. |
| `node tools/ci/check-ci-artifacts.mjs --check <manifest>` | Validates artifact inventory, retention class, hashes, and sanitization. |
| `scripts/windows/Test-WindowsCiStatic.ps1` | Runs only named static PowerShell tests; it refuses all mutating flags and host actions. |
| protected manual workflow inputs | Permit only fixed isolated-lab planning modes; they contain no physical or daily-host selector. |

No runtime protocol, CLI, driver uAPI, service API, or kernel interface changes
are authorized by this PRD.

## 9. Dependencies and risks

| Dependency or risk | Mitigation |
| --- | --- |
| Hosted Windows image differs from lab toolchains | Restrict hosted Windows to Rust compilation and static harnesses; WDK/Verifier remain isolated-lab evidence. |
| Full-SHA action pins require maintenance | Keep human version comments and use reviewed dependency updates. |
| Dependency scan tools evolve | Pin tool versions in a checked-in tool-version record and validate their reported versions. |
| Artifact logs can contain sensitive data | Sanitize before upload; store only bounded summaries and hashes in public evidence. |
| Remote controls cannot be changed from a PR | Treat them as manual promotion gates with dated, sanitized evidence. |
| Self-hosted lab runners are more privileged | Never accept fork code, never use pull-request-target, require a protected environment, and initially dispatch a plan only. |
| Release signing is externally unavailable | Keep Windows driver promotion `PARTIAL`; do not invent a local substitute. |

Rollback trigger: halt CI/release promotion and revert the P0 implementation if
one selected gate is accepted while failed/cancelled/skipped, one pull-request
job receives undeclared write authority, one protected-lab workflow targets a
daily or physical host, one sanitizer leaks prohibited content, or one
test-signed driver becomes eligible for public release.

## 10. Implementation strategy

1. Define the contract schema, RED fixtures, checker, and 80% Node coverage.
2. Harden existing hosted workflows with explicit permissions, timeouts,
   full-SHA action references, `cargo deny check`, and public-hygiene coverage.
3. Add the fork-safe Windows Rust and PowerShell static lane.
4. Add artifact sanitizer/retention validation and a fail-closed aggregate.
5. Add protected manual Windows/WSL2 lab skeletons that only plan and refuse
   unsafe targeting.
6. Add release manifest/SBOM validation and test-signed-driver refusal.
7. Run real hosted workflow evidence, then obtain separate administrator
   observations for remote controls. Write `IMPL.md` only after the required
   live CI/lab evidence is available; otherwise record `PARTIAL`.

## 11. Documents to update

- `docs/ci/CI-TOPOLOGY.md`
- `docs/ci/RELEASE-INTEGRITY.md`
- `docs/governance/README.md`
- `docs/governance/ci-contract.json`
- `docs/specs/no-milestone/ci-trust-and-release-integrity/IMPL.md` only after
  Step 3 validation
- `validation.md` only after a real CI/operator-surface run
- `docs/INDEX.md` when a caller explicitly authorizes index regeneration

## 12. Out of scope

- Changing remote GitHub settings, branch protection, reviewer policy,
  environment reviewers, action allowlists, or artifact defaults.
- Deploying, loading, signing, uninstalling, or publishing a Windows driver.
- WDK, InfVerif, Driver Verifier, Hyper-V, physical-host, daily WSL2, swap,
  GPU-pressure, CUDA, kernel, or reboot action.
- Adding a new product dependency, protocol, driver ABI, daemon behavior, or
  public Windows support claim.
- Executing an upstream contribution workflow or representing an external
  maintainer's approval policy.

## 13. Acceptance criteria

- [ ] Every P0 row has one implementation owner, a named test, a refusal case,
      and a terminal aggregate rule.
- [ ] CI contract fixtures prove missing, skipped, cancelled, tolerated, broad,
      mutable, and unsafe configurations fail.
- [ ] New Node CI/security checker business logic reaches at least 80% line,
      branch, and function coverage.
- [ ] Every Rust business-logic coverage row uses the canonical per-file 80%
      command or is explicitly `N/A` with a platform-specific reason.
- [ ] Windows hosted validation is compile/static only and refuses mutation.
- [ ] Protected lab skeletons are manual, isolated, non-rebooting, and reject
      daily/physical host targeting before action.
- [ ] Every CI artifact is sanitized, hashed, classified, and retained by
      policy; raw private lab output is never uploaded.
- [ ] Release validation rejects a dirty/mismatched source, missing SBOM,
      mismatched hash, and test-signed Windows driver.
- [ ] Remote configuration remains visibly environment-bound until an
      administrator records the required observation.

## 14. Validation plan

Step 3 must use TDD for every checker and workflow contract. It must run
formatter/linter/tests for touched code, Node coverage at or above 80% for new
Node business logic, and the canonical Rust slice gate for any touched Rust
business logic. The live surface is a real internal pull request plus a
protected manual plan-only lab dispatch:

1. record the selected-gate and remote-control baseline;
2. run a legitimate fork-safe hosted path and a protected isolated-lab plan;
3. exercise explicit refusals for a disabled gate, unsafe lab target,
   prohibited artifact, and test-signed release entry; and
4. record after-state, result contexts, artifact hashes, retention, and no host
   mutation in sanitized evidence.

Windows WDK/Verifier, real WSL2/kernel drills, and external signing remain
environment-bound until their dedicated isolated-lab or release evidence is
available. They cannot be replaced by a local static test.
