---
slug: release-promotion-publication
title: Protected beta release promotion and publication
milestone: v0.9.0-beta.1 — WSL2 NBD
issues:
  - 195
  - 219
  - 221
  - 223
  - 225
  - 227
  - 229
---

# PRD — Protected beta release promotion and publication

> SSDV3 governance and release-orchestration slice. It changes neither the
> RamShared cascade, daemon, kernel module, driver, lab, VM, swap, disk, nor
> host state. It does not create a tag, draft, release, asset, or remote
> repository setting while this change is developed.

## 1. Summary

RamShared needs a release path in which tag production, read-only artifact
integrity, and public beta publication are separate, observable boundaries.
The current source already has a tag-triggered integrity workflow and a
Release Please producer, but the producer can fall back to repository or
personal tokens and integrity does not hand an exact public asset set to an
explicit protected publication action.

The outcome is a fixed, fail-closed contract for the next intended beta target
`v0.9.0-beta.1`. Its public asset set contains exactly four files:

1. `ramshared-linux-v0.9.0-beta.1.tar.gz`;
2. `ramshared-linux-v0.9.0-beta.1.tar.gz.sha256`;
3. `ramshared-sbom.cdx.json`; and
4. `release-manifest.json`.

The bundle may carry an internal `SHA256SUMS` file, but that internal file is
not a fifth public release asset. The historical `v0.8.0` tag and release
remain unchanged. This PRD authorizes no publication while it is implemented.

## 2. Technical context

### Confirmed in codebase

- [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml) is a
  `workflow_call`-only reusable workflow.
- [`.github/workflows/ci-contract.yml`](../../../../.github/workflows/ci-contract.yml)
  is the canonical pull-request/main caller and owns the same-run aggregate.
- [`.github/workflows/release.yml`](../../../../.github/workflows/release.yml)
  runs Release Please on `main`, but its token expression currently permits
  GitHub-token and personal-token fallbacks.
- [`.github/workflows/release-integrity.yml`](../../../../.github/workflows/release-integrity.yml)
  checks a tag source, builds a Linux bundle, creates a CycloneDX SBOM, and
  validates a release manifest with read-only job permissions.
- [`scripts/package/build-linux-bundle.sh`](../../../../scripts/package/build-linux-bundle.sh)
  creates the Linux archive and writes an internal `SHA256SUMS` inventory.
- [`tools/ci/write-release-manifest.mjs`](../../../../tools/ci/write-release-manifest.mjs)
  and [`tools/ci/check-release-integrity.mjs`](../../../../tools/ci/check-release-integrity.mjs)
  already bind a source revision, bundle, SBOM, and evidence hashes.
- [`.release-please-manifest.json`](../../../../.release-please-manifest.json)
  records `0.8.0`, and `v0.8.0` is a historical repository tag.

### Confirmed in documentation

- [`ci-trust-and-release-integrity`](../ci-trust-and-release-integrity/SPEC.md)
  defines the existing tag integrity path as read-only and requires future
  promotion to be explicit rather than inferred from a CI pass.
- [`.claude/rules/governance.md`](../../../../.claude/rules/governance.md)
  applies Kahneman #15 through #18 to CI guards, retries, and replayable
  actions.
- [`.claude/rules/ssdv3.md`](../../../../.claude/rules/ssdv3.md) classifies CI
  as normally outside SSDV3 but permits this explicit specification because it
  establishes a release trust contract.

### Inference / proposal

- A maintainer request can receive a tag, full source SHA, and integrity-run
  identifier, validate them, and delegate the same immutable tuple to the
  GitHub App. Only the App-authored run can enter protected publication, so
  `prevent_self_review` retains a distinct human approval boundary.
- A pinned GitHub App installation token is the appropriate authority for
  Release Please tag production and for the explicitly protected publication
  job. A missing App credential must stop before either producer executes.
- A checked-in target policy for only `v0.9.0-beta.1` prevents an accidental
  publication of `v0.8.0` or a later unreviewed version. A later target needs
  an explicit policy/SPEC revision.

## 3. Recommended option

Use one Day-0 release path with three stages and no `workflow_run` handoff.

1. **Producer:** the `main` Release Please job obtains a GitHub App token
   unconditionally. It has no `GITHUB_TOKEN`, personal-token, or empty-token
   fallback. It opens/updates the one-shot release PR and, after that PR is
   merged, creates the beta tag/draft release only under that identity.
2. **Integrity:** a tag-triggered, read-only workflow accepts only
   `v0.9.0-beta.1`, resolves the tag to its full commit SHA, builds and verifies
   the four-file candidate set, and uploads a bounded integrity artifact. It
   cannot publish a release asset.
3. **Publication:** a maintainer explicitly dispatches a bounded request with
   the exact target tag, exact 40-hex source SHA, and integrity run ID. That
   stage validates the tuple before minting a Contents-write App token and
   emits an exact `repository_dispatch` payload to the same workflow. An
   admission job rejects a direct internal event from any non-App actor; only
   the exact App bot actor reaches the unchanged
   `protected-release` environment, where the human reviewer approves the
   deployment. The protected job downloads only the named integrity artifact,
   validates every binding, requires the existing Release Please draft,
   uploads only missing matching files, and publishes the beta only after the
   exact set is complete.

The manual action is idempotent: a completed matching beta returns `NO_CHANGE`
or `PASS`; a wrong tag, SHA, asset name, byte count, hash, release mode, or
extra public asset is a terminal refusal and performs no overwrite. It never
uses `--clobber`, broad asset globs, a mutable tag target, a workflow-run
trigger, or independently scheduled run polling.

### Alternatives rejected

| Alternative | Reason |
| --- | --- |
| Let Release Please publish assets directly | It combines tag production and public publication without the protected, exact-SHA integrity boundary. |
| Let integrity publish immediately after tag push | A tag push is not explicit publication approval and cannot establish a reviewer-controlled promotion boundary. |
| Rebuild in the publication workflow | A second build can differ from the read-only candidate; publication must consume and revalidate the integrity output. |
| Upload a glob or use `gh release upload --clobber` | A stale, extra, or mismatched file could be added or overwritten silently. |
| Use `workflow_run` to trigger publication | It introduces a separate-run race and a default-branch workflow context; the manual dispatch keeps authority explicit. |
| Reuse `GITHUB_TOKEN` or a PAT if the App fails | It changes the producer identity and can bypass intended App policy. |
| Treat `v0.8.0` as a candidate | It is historical and outside this beta contract. |

### Release Please reachability

`release-please-config.json` uses the upstream
[manifest configuration schema](https://raw.githubusercontent.com/googleapis/release-please/main/schemas/config.json).
The one-shot `release-as` and `last-release-sha` recovery controls were used to
create the exact beta tag/draft and were removed after protected publication.
The current configuration retains the App-only prerelease shape but cannot
replay either recovery control.

The exact lifecycle is:

1. While the target tag was absent, the App-only producer invoked Release
   Please with the reviewed one-shot target and recovery boundary.
2. Release Please opened or updated its release PR. When that PR was merged,
   it created the `v0.9.0-beta.1` tag and a draft prerelease. Forced tag
   creation was necessary because a draft otherwise permits lazy tag creation.
3. The tag starts read-only integrity. Publication does not create a release:
   it requires the exact existing draft and only changes visibility after the
   verified quartet is present.
4. Now that the exact target tag exists, `release.yml` reports `NO_CHANGE` before
   calling Release Please, so this fixed configuration cannot produce a later
   target accidentally.

The tag-triggered integrity workflow is read-only and does not declare a
deployment environment. `protected-release` belongs only to the separate
manual publication job; attaching it to integrity makes an exact tag fail
before checkout when the environment admits protected branches only.

The one-shot recovery from the merged-but-unrecognized release PR was bounded
to the exact `v0.8.0` merge
`568e7b42b78b3c9edb8ea390cb4297142a37e412`. The generated header still contains
the seven repository PR sections before Release Please's separator; its
machine-readable release notes below that separator are never replaced. Both
`last-release-sha` and `release-as` are absent after publication.

The local source test parses every listed configuration value, requires the
published manifest state `0.9.0-beta.1`, and rejects the two retired recovery
keys. Historical, later, malformed, or multi-package manifests are refused.
The same gate rejects fallback credentials.

GitHub's [release response example](https://docs.github.com/en/enterprise-cloud%40latest/rest/releases/releases)
shows `target_commitish` may be a branch name. Publication therefore does not
treat that response field as an immutable SHA. It verifies the Git tag resolves
to the dispatch SHA, the selected integrity run has that `head_sha`, and the
candidate manifest has the same SHA; a branch-valued `target_commitish` is
accepted only after those stronger bindings pass.

## 4. Functional requirements

| ID | Requirement | Verifiable acceptance |
| --- | --- | --- |
| RF-1 | Keep one canonical PR entrypoint. | `ci.yml` remains `workflow_call`-only, `ci-contract.yml` remains the only pull-request caller, and a manufactured direct-plus-reusable topology fails. |
| RF-2 | Require a GitHub App for tag production. | The release producer fails before Release Please when App credentials/token output are absent; source contains no PAT or `GITHUB_TOKEN` fallback for that token. |
| RF-3 | Define the beta candidate exactly. | The policy accepts only `v0.9.0-beta.1`, a full 40-hex source SHA, and exactly the four listed public filenames. |
| RF-4 | Separate integrity from publication. | Tag integrity has read-only permissions and no publish command; publication is `workflow_dispatch`, the request delegates only after tuple validation, only the exact App actor reaches the protected job, and there is no `workflow_run` trigger. |
| RF-5 | Bind artifact bytes to the exact tag SHA. | The manifest, detached checksum, SBOM, archive, dispatch SHA, checked-out tag, and integrity artifact agree or the workflow exits before release mutation. |
| RF-6 | Publish idempotently. | Empty/matching draft state can advance; an already complete matching beta does not overwrite; mismatched, missing, or extra assets fail closed. |
| RF-7 | Keep public asset scope exact. | A public beta contains only the four contract names; an internal bundle `SHA256SUMS` never becomes a separate public upload. |
| RF-8 | Preserve historical release state. | No workflow target or dispatch policy accepts `v0.8.0`; no tag/release is created while validating this change. |

## 5. Non-functional requirements

| ID | Requirement | Acceptance |
| --- | --- |
| NFR-1 | Least authority | Read-only integrity uses `contents: read`; the request uses App `contents: write` only for exact repository dispatch; protected publication reads Actions with its read-only job token and receives release write authority only through the App token. |
| NFR-2 | Boundedness | Every job has a finite timeout; deterministic identity, asset, test, or command errors never retry. |
| NFR-3 | Observability | Stable rule codes identify refusal without echoing secrets, private paths, or token values. |
| NFR-4 | Determinism | The public asset ordering and manifest serialization are stable; a target policy binds one tag and SHA-shaped input. |
| NFR-5 | Safety | No workflow touches a host, VM, driver, disk, swap, GPU, or live RamShared daemon. |
| NFR-6 | No false promotion | A passing tag integrity run is not publication; the protected manual action is still required. |

## 6. Flows

### Legitimate beta promotion

1. Release Please runs on `main`, refuses absent App credentials, and uses only
   the App token to create the target beta tag/draft metadata.
2. The tag event invokes integrity for `v0.9.0-beta.1`; it resolves the tag to
   one full commit SHA, checks out that revision, builds the archive/SBOM,
   writes the detached checksum and manifest, validates all four files, and
   stores the candidate as an integrity artifact.
3. A maintainer explicitly dispatches publication with the target tag, source
   SHA, and integrity run ID. The request validates that tuple, then delegates
   it through an exact `repository_dispatch` created with a Contents-write App
   token. Only the App-authored run reaches
   `protected-release`; the maintainer reviews that distinct run and approves
   it there. The protected job fetches the exact tag revision and named
   artifact, validates it, then requires and resumes the matching beta draft
   created by Release Please.
4. It uploads only absent verified assets, verifies the remote inventory equals
   the four-file contract, and changes the draft to the public beta state.
5. A repeat sees the same full inventory and exits without replacement.

### Refusal flow

| Trigger | Result | Safe state |
| --- | --- | --- |
| App credentials/token absent | Producer fails before Release Please | no tag/draft from fallback authority |
| Direct `pull_request` on `ci.yml` plus reusable caller | contract test fails | CI topology is not admitted |
| Tag is `v0.8.0`, another version, or malformed | integrity/publication refuse | no artifact upload or release mutation |
| Dispatch SHA differs from tag or manifest | publication refuses | no remote mutation |
| Candidate lacks/changes one of the four files | integrity/publication refuse | no public asset upload |
| Existing release has extra/mismatched assets or non-beta state | publication refuses | no overwrite or publish |
| Integrity run is missing or artifact name does not bind tag/SHA | publication refuses | no release mutation |

## 7. Data and state model

The target policy is repository-owned and contains:

```text
schema_version: 1
target_tag: v0.9.0-beta.1
release_channel: beta
public_assets: [archive, detached checksum, SBOM, manifest]
```

The release manifest records source tag/SHA, clean tree, lockfile hash, Rust
version, archive record, detached-checksum record, SBOM record, and an exact
ordered public-asset record. The integrity artifact is named from target tag
and source SHA and contains exactly those four files. Publication state is one
of `draft-empty`, `draft-partial`, `published-complete`, or terminal `refused`.
Only matching `draft-empty`/`draft-partial` can advance; a matching
`published-complete` is idempotent; every other state is refused.

## 8. Interfaces

| Interface | Contract |
| --- | --- |
| `node tools/ci/check-release-integrity.mjs --check …` | Validates tag/SHA, detached checksum, four-file contract, hashes, SBOM, and manifest without publication. |
| `node tools/ci/check-release-publication.mjs …` | Classifies local candidate and remote release inventory as advance, no-change, or refusal without network mutation. |
| `.github/workflows/release-integrity.yml` | Tag-triggered read-only candidate producer; uploads only the bounded integrity artifact. |
| `.github/workflows/release-publication.yml` | Two-stage consumer; human `workflow_dispatch` validates the tuple, an App-authored `repository_dispatch` carries it internally, and only that exact App event can enter protected publication. |
| `.github/workflows/release.yml` | GitHub-App-only Release Please tag/draft producer. |

## 9. Dependencies and risks

| Dependency or risk | Mitigation |
| --- | --- |
| App credentials are absent, rotated, or lack requested permissions | Fail before delegation or producer side effects and record no fallback. |
| Tag can move after integrity | Dispatch supplies full SHA; both tag resolution and manifest binding must match it. |
| Artifact transfer can select another run | Require an explicit numeric run ID and deterministic artifact name containing tag/SHA. |
| Remote release already contains unknown data | Refuse rather than overwrite or delete it. |
| Bundle build has an internal checksum | Generate and validate a detached archive checksum separately; upload only the contract quartet. |
| Protected environment is remote configuration | Local source validation remains partial if its dated remote observation is absent or unsafe. |
| GitHub Actions selected-action allowlist later excludes the pinned App-token action | Producer/delegation fails before mutation; there is no token or workflow fallback. |

## 10. Implementation strategy

1. Add PRD, SPEC, and 2.5 audit; do not create `IMPL.md`.
2. Add Node RED fixtures for public asset cardinality, detached checksum,
   beta/SHA mismatch, idempotent remote inventory, App-token fallback, and
   duplicate direct/reusable CI topology.
3. Extend manifest/checker logic and add the publication planner until the
   same fixtures become green with at least 80% Node line/branch/function
   coverage for new logic.
4. Update release, integrity, publication, and CI contract workflows; run
   actionlint and source-level policy tests. Do not dispatch any workflow.
5. Run documentation checks and hand the new pack to the documentation owner
   for index/inventory reconciliation. Report the protected publication E2E as
   environment-bound. Do not write `IMPL.md`.

## 11. Documents to update

| Document | Action |
| --- | --- |
| `docs/INDEX.md` | Documentation owner reconciles after this pack settles; this slice does not regenerate it. |
| `docs/governance/ci-contract.json` | Extend release topology/policy only. |
| `docs/specs/no-milestone/ci-trust-and-release-integrity/SPEC.md` | Reference this narrower promotion successor if needed. |
| `validation.md` | Do not append without an actual protected dispatch. |

## 12. Out of scope

- Publishing `v0.8.0` or creating `v0.9.0-beta.1` now.
- Windows driver publication, signing, attestation, or public eligibility.
- A production stable release, package-manager publication, or a new release
  channel.
- Remote branch/environment setting mutation, host/lab activity, reboot, swap
  action, pressure, or daemon deployment.

## 13. Acceptance criteria

1. The source tree has the stated three-stage path, and no operation is run.
2. `ci.yml` is reusable-only and direct plus reusable invocation fails a named
   topology regression.
3. Release Please cannot use a PAT or repository-token fallback.
4. Read-only integrity and manual protected publication are separate, use no
   `workflow_run`, bind exact tag/SHA/artifact identity, and admit the protected
   stage only when the exact App bot authored the delegated run.
5. Candidate and release inventory logic accept only the exact asset quartet
   and are idempotent without overwrite.
6. The target policy refuses historical `v0.8.0` and targets
   `v0.9.0-beta.1` only.
7. No `IMPL.md`, live release claim, tag, release asset, remote action, or git
   commit is created by this task.

## 14. Validation plan

- Node unit/refusal tests with RED before Green for manifest, publication-plan,
  and CI topology logic; new Node production files meet ≥80% line, branch, and
  function coverage.
- `node tools/ci/check-ci-contract.mjs --check-local` and focused checker
  suites; actionlint over workflows.
- `cargo fmt --all -- --check`, Clippy, and targeted `ramsharedd` shutdown
  regressions only if Rust is touched. Current source already contains the
  deterministic shutdown wake and passing regression tests, so no Rust change
  is planned absent a fresh RED reproduction.
- Protected stage-1/stage-2 before → action → after evidence is
  environment-bound: no workflow dispatch or publication is authorized here.
