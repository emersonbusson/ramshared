---
slug: documentation-governance-integrity
title: Documentation governance and evidence integrity
milestone: —
issues: []
---

# PRD — Documentation governance and evidence integrity

> SSDV3 Step 1 only. This PRD defines a RamShared-native documentation and
> evidence-governance capability. It does not change the runtime, kernel,
> driver, daemon, protocol, benchmark workload, or host state.

## 1. Summary

RamShared has strong local rules for SSDV3, append-only validation,
environment-bound claims, per-file Rust coverage, supervised pressure, and
benchmark registration. The repository can already generate `docs/INDEX.md`,
check relative links, check the gap register, and check public hygiene.

The remaining integrity risk is that these controls are not joined into one
machine-verifiable documentation contract. The index status is derived mainly
from file presence; there is no general gate proving that an implemented claim
has a named local test and runtime/evidence links. There is no repository-wide
parity matrix, objective-oriented reference router, duplicate-prose gate, or
dedicated documentation-provenance gate. Campaigns also lack one canonical
profile/seed/run/invariant manifest, and postmortem closure does not require a
measured effectiveness proof.

The outcome is a small, zero-dependency governance layer that makes document
ownership, claims, evidence, deterministic campaigns, rollback, and closure
machine-checkable without importing another product's vocabulary, architecture,
paths, identities, or process text.

## 2. Technical context

### Confirmed in codebase

- [`docs/SSDV3-PROMPTS.md`](../../../SSDV3-PROMPTS.md) defines the RamShared-only PRD → SPEC → optional
  audit 2.5 → IMPL process, named test matrices, per-file coverage, platform
  gates, live `before → action → after` evidence, `BINARY_MATCH` when relevant,
  and `PARTIAL` for environment-bound gaps.
- [`.claude/rules/ssdv3.md`](../../../../.claude/rules/ssdv3.md) and
  [`.claude/rules/documentation.md`](../../../../.claude/rules/documentation.md) require one
  in-repo primary path, repository-relative links, regenerated `docs/INDEX.md`,
  and no foreign process or API shapes.
- [`tools/generate-docs-index.mjs`](../../../../tools/generate-docs-index.mjs) generates `docs/INDEX.md` and currently derives
  `DONE` from `IMPL.md` presence, with a limited `PARTIAL` check in the IMPL
  status block. The index itself warns that file presence is not cover/E2E
  proof.
- [`scripts/docs-check.sh`](../../../../scripts/docs-check.sh) runs the index check,
  `tools/check-broken-links.mjs`, `tools/ci/check-gap-register.mjs`, and
  `tools/ci/check-public-hygiene.mjs`.
- [`validation.md`](../../../../validation.md) is an append-only empirical log with measured data and a
  verdict schema. Slice-local validation files also exist, including
  `docs/specs/no-milestone/windows-autonomous-broker-service/validation.md`.
- [`.claude/rules/benchmarks.md`](../../../../.claude/rules/benchmarks.md),
  [`docs/BENCHMARKS.md`](../../../BENCHMARKS.md), and
  [`docs/benchmarks/results.jsonl`](../../../benchmarks/results.jsonl) require at least three runs per cell,
  median/p99/deviation, same-snapshot comparison, automatic context capture,
  raw output, run IDs, and append-only storage.
- [`docs/reliability/GAP-REGISTER.md`](../../../reliability/GAP-REGISTER.md) separates open claims from closed claims
  and explicitly keeps environment-bound or externally signed surfaces from
  becoming false `DONE` claims.
- [`docs/postmortems/TEMPLATE.md`](../../../postmortems/TEMPLATE.md) records technical cause, measurable blast
  radius, rollback triggers, owners, deadlines, and a required regression
  action, but it does not define a machine-checkable effectiveness result.
- Existing Windows and WSL2 campaigns already emit hashes, lifecycle states,
  residue checks, watchdog outcomes, and platform-specific evidence. Those
  scripts and artifacts are the authoritative runtime surfaces; this PRD adds
  governance around them and does not replace their gates.

### Confirmed in documentation

- `docs/INDEX.md` states that its status is a filesystem classification and is
  not proof of coverage or live E2E.
- The root documentation rules require English structural documents and
  repository-local cross-links, while public-hygiene rules prohibit secrets,
  private machine details, and unsafe evidence.
- The current benchmark and validation rules already distinguish a measured
  observation from a release or production claim.

### Inference / proposal

- A claims gate, provenance/redaction gate, redundancy gate, and parity matrix
  should be added as documentation tooling rather than as runtime code.
- Deterministic campaign manifests should be deliberately small and typed. They
  should describe an existing runner's contract, not create a generic test DSL
  or move business logic out of platform-specific harnesses.
- Postmortem effectiveness can be checked with a compact result record that
  points to an executable regression, a measured threshold, and a revalidation
  verdict.

### Current gaps to close

| Gap | Current evidence | Risk | Required outcome |
| --- | --- | --- | --- |
| `DONE` claim can be inferred from `IMPL.md` presence | `tools/generate-docs-index.mjs`; `docs/INDEX.md` | A document can look complete while named tests, cover, live evidence, or BINARY_MATCH are missing | A fail-closed claim checker distinguishes `PRD`, `SPEC`, `PARTIAL`, and evidence-backed `DONE` |
| No objective-oriented documentation router | `README.md` and `docs/INDEX.md` provide links but no complete goal→source map | Agents and operators may read the wrong source or duplicate facts | A short reference index routes each common task to one canonical source and its depth links |
| No documentation parity matrix | No `docs/DOCUMENTATION-PARITY.md` exists | Architecture, capability, SPEC, runbook, validation, benchmark, and gap state can drift | A matrix names owner, canonical source, evidence source, state, and known limitation for each RamShared surface |
| No general redundancy/provenance gate | `scripts/docs-check.sh` has no duplicate-prose or foreign-provenance step | Repeated or imported narrative can diverge and violate repository boundaries | Read-only scanners fail on unsafe provenance, private paths, secrets, KASLR material, and excessive duplicate normative text |
| Campaign setup is distributed across scripts | `scripts/windows/`, `scripts/p0/`, `scripts/safety/`, and per-slice `evidence/` | Same surface can be measured with different bounds, context, or cleanup claims | A versioned profile/run manifest records inputs and invariants while runners remain native |
| Postmortem effectiveness is prose-only | `docs/postmortems/TEMPLATE.md` | A closed action may have no proof that the fault is prevented or detected | Closure requires a regression command, measured result, and post-recovery state |

## 3. Recommended option

Add one documentation-governance toolchain under `tools/` and a small set of
canonical Markdown/JSON schemas under `docs/`. Keep the implementation
zero-dependency and read-only over source documents except for explicit,
append-only writers already authorized by the benchmark and validation rules.

The option has six connected parts:

1. **Canonical-source parity.** Add a matrix and objective-oriented router that
   point to existing sources without copying their bodies.
2. **Claim integrity.** Extend index validation with explicit states, owner,
   runtime/configuration links, named test links, evidence links, and honest
   `PARTIAL`/blocked semantics.
3. **Document hygiene.** Add duplicate-normative-text detection and a
   provenance/sanitization scan for repository-relative paths, private
   identities, secrets, hostnames, usernames, raw addresses, KASLR material,
   and foreign repository references.
4. **Deterministic campaign contracts.** Define a small manifest for profile,
   seed, run ID, clock/bounds, environment identity, actions, invariants,
   reporters, refusal cases, artifact paths, and idempotent cleanup.
5. **Validation and rollback taxonomy.** Strengthen the root and slice
   validation records with explicit `before`, `action`, `after`, measured
   state, legitimate/refusal cases, artifacts, limitations, verdict, and
   numeric/observable rollback trigger. Classify rollback by userspace,
   kernel/driver, and host/persistent state.
6. **Postmortem effectiveness.** Require every corrective action to identify an
   executable prevention/detection/response proof and a measured closure result.

The toolchain must be integrated into `scripts/docs-check.sh` and a CI job only
after its initial baseline is explicit and clean. A baseline exception must be
dated, scoped, owned, and fail closed when it changes.

### Alternatives rejected

| Alternative | Reason |
| --- | --- |
| Make `docs/INDEX.md` the only source of truth | It is a generated catalog and cannot own runtime state, evidence, or rollback policy |
| Create a single large governance document | It would duplicate architecture, SPEC, runbook, validation, and benchmark facts and increase drift |
| Add a universal journey/benchmark DSL | One generic executor would hide Linux, WSL2, Windows, and kernel-specific safety requirements without two proven consumers |
| Infer `DONE` from filenames or commit messages | Presence and history are not execution proof |
| Import document text or process vocabulary from another repository | This violates RamShared-only scope and can introduce foreign identities, paths, and unsafe claims |
| Rewrite old validation or benchmark entries | Append-only history is required to preserve failed attempts and temporal truth |
| Scan only Markdown prose | Evidence JSONL, scripts, manifests, logs, and paths can also contain secrets or private host material |

### Trade-offs

The governance layer adds a small amount of metadata and CI runtime and may
initially expose existing documentation debt. This is accepted because a
visible `PARTIAL` or `NO-GO` is safer than a filesystem-derived `DONE`. The
scanners will use bounded heuristics and explicit allowlists; ambiguous matches
must be review findings, not automatic deletion or rewriting.

## 4. Functional requirements

### RF-1 — Canonical-source and parity matrix

Define one canonical source for each documentation question relevant to
RamShared: topology/ownership, capability state, durable decisions, PRD/SPEC,
operation, empirical validation, benchmarks, reliability gaps, and postmortems.

Acceptance:

- A repository-relative `docs/DOCUMENTATION-PARITY.md` (or a path explicitly
  chosen by the later SPEC) lists category, canonical source, owner, evidence
  source, state semantics, and known limitations.
- Every category has one primary source and links to satellites instead of
  repeating normative blocks.
- The matrix distinguishes “documented”, “implemented”, “validated”,
  “partial”, “blocked”, and “not applicable”. Documentation coverage never
  promotes runtime capability.
- A negative fixture proves that two conflicting canonical sources are
  rejected or reported as a P1 finding.

### RF-2 — Objective-oriented reference index

Provide a short router for common operator, developer, reviewer, and incident
questions. It must point to the canonical source and the next-depth references
for each goal.

Acceptance:

- The router covers at least: build/test, Linux/WSL2 lifecycle, Windows driver
  lifecycle, host-safety campaigns, benchmarks, reliability gaps, architecture
  decisions, SSDV3 changes, and incident recovery.
- Every link is repository-relative or points to an explicitly approved
  external standard/vendor manual; private filesystem links are rejected.
- The router contains no duplicated requirements or state claims.
- A link/coverage test proves each objective has an existing canonical target.

### RF-3 — Evidence-backed capability claims

Extend index/governance validation so `DONE` is never inferred from
`IMPL.md` alone. Claims must point to the runtime/configuration surface and a
named local test or platform drill; `PARTIAL` must identify its environment
blocker.

Acceptance:

- A checker validates canonical states and requires an owner for every listed
  capability.
- `DONE` requires the applicable evidence set: named SPEC tests, cover gate or
  justified E2E-only classification, live evidence for the same surface,
  validation record, and `BINARY_MATCH` when the daemon/driver contract
  requires it.
- `PARTIAL` requires a concrete blocker, missing gate, and next proof; it is
  never treated as release-quality `DONE`.
- Negative fixtures cover missing runtime link, missing named test, missing
  live evidence, stale evidence, and fabricated `DONE`.
- The checker does not execute untrusted Markdown, YAML, JSON, or shell text.

### RF-4 — Redundancy and provenance integrity

Add read-only scanners for documentation and committed evidence. They must
detect empty documents, broken repository-relative links, dead-path mentions,
duplicate normative prose, foreign repository references, usernames,
hostnames, private filesystem paths, secrets, tokens, raw addresses, and
KASLR/kernel-address material.

Acceptance:

- The scanner reports source path and line for every finding and exits non-zero
  for policy violations.
- It supports an explicit, reviewed allowlist for approved external standards,
  vendor manuals, protocol names, kernel symbols, and intentionally redacted
  test fixtures. The allowlist cannot suppress an entire directory.
- Secret detection is content-based and scans Markdown, JSONL, YAML, shell,
  PowerShell, and evidence files; it does not print the matched secret.
- Provenance checks reject other repository names, copied private paths,
  agent/user identities, or imported product narratives in new or edited
  documents.
- Duplicate detection reports a source pair and similarity reason but never
  rewrites files automatically.
- A sanitized report is reproducible without logging secrets or raw KASLR
  addresses.

### RF-5 — Deterministic campaign profile and run manifest

Define a compact, versioned manifest consumed by existing native runners. It
must describe what was measured without becoming a cross-platform execution
DSL.

Acceptance:

- Each campaign records `journey_id`, `version`, `profile`, `seed`, `run_id`,
  base time/clock policy, target layer, toolchain/commit state, hardware and
  kernel/driver identity where applicable, bounded parameters, action list,
  checkpoints, legitimate case, refusal cases, invariants, reporters,
  artifact paths, retention class, and idempotent cleanup.
- Profiles distinguish at least `smoke`, `baseline`, `stress`, `soak`, and
  `isolation` where the target surface supports them; unsupported profiles are
  explicitly `N/A` with justification.
- A runner must not infer a threshold from a profile. Thresholds and abort
  triggers remain in the owning SPEC or benchmark contract.
- The manifest records before/action/after state and does not contain secret,
  PII, raw kernel address, private path, or real-user data.
- A checker rejects duplicate journey versions, missing run IDs, unbounded
  waits, implicit clocks, sleep-only synchronization, missing cleanup, or
  missing consumers.

### RF-6 — Validation record schema

Strengthen the root and slice validation schemas without rewriting historical
entries.

Acceptance:

- New entries contain: date/timezone, slug, environment/commit state, scope,
  `Before`, `Action`, `After`, measured data with units, legitimate case,
  required refusals, tests/coverage, platform gates, artifact paths, cleanup,
  limitations, verdict (`✅`, `🔴`, or `🟡`), and numeric/observable rollback
  trigger.
- Windows entries can record SCM state, driver/package identity,
  `BINARY_MATCH`, reboot count/authorization, pagefile/LUN/lease state, and
  residue without persisting secrets or raw addresses.
- Linux/WSL2 entries can record daemon identity, `/proc/swaps`, dmesg summary,
  GPU/RAM context, watchdog result, and terminal state without committing
  KASLR addresses.
- Old entries remain byte-for-byte unchanged; a schema checker validates only
  new entries or explicitly marked legacy records.
- A validation entry cannot claim `DONE` when a declared environment-bound gate
  is absent.

### RF-7 — Rollback taxonomy and effectiveness closure

Make rollback and postmortem closure explicit across the three RamShared
atomicity frontiers:

1. userspace/daemon and package configuration;
2. kernel module/Windows driver and protocol/uAPI;
3. host/persistent state such as swap, lease, VRAM, pagefile, LUN, SCM, and
   watchdog state.

Acceptance:

- Every non-trivial PRD/SPEC/ADR/benchmark decision has a numeric or observable
  rollback trigger and target known-good state.
- A postmortem action names owner, deadline, prevention/detection/response
  type, executable regression/drill, threshold, and evidence location.
- Closure is rejected until the regression is run after the fix and the
  affected legitimate path plus critical refusal path are revalidated.
- If a rollback crosses an unsafe or irreversible frontier, the document marks
  it `forward-only` and defines operator recovery instead of promising a file
  reversal.
- Failed attempts and rejected campaigns remain retained as sanitized evidence.

### RF-8 — CI and local enforcement

Integrate the new checks into `scripts/docs-check.sh` and CI only after their
baseline is recorded and the checks are deterministic.

Acceptance:

- Local and CI commands produce the same exit code and sanitized report for the
  same commit.
- The index check, broken-link check, gap-register check, public-hygiene check,
  claims check, parity check, redundancy check, provenance check, and manifest
  schema tests are all named in the governance contract.
- Any new allowlist entry requires a reason, owner, scope, review date, and
  removal/expiry condition.
- The toolchain remains zero-dependency Node or existing shell tooling unless a
  later SPEC justifies an alternative with measured cost and rollback.

## 5. Non-functional requirements

### NFR-1 — RamShared-only boundaries and language

- Structural documentation and code-facing metadata are written in English.
- All public links and paths are repository-relative, except approved external
  standards or vendor manuals that are explicitly identified as such.
- No document, test fixture, artifact, manifest, prompt, or report may identify
  another repository/product, import its narrative, or introduce its service,
  tenant, API, or workflow vocabulary.
- No usernames, hostnames, private workstation paths, credentials, tokens,
  environment secrets, PII, or raw KASLR/kernel addresses may enter committed
  docs or evidence.
- Evidence is sanitized before commit; the scanner must not echo sensitive
  matches in its output.

### NFR-2 — Determinism and reproducibility

- Scans and generated indexes are deterministic for identical input bytes.
- Run manifests include commit/dirtiness state, tool command, versioned
  parameters, and bounded timeout policy.
- Time, randomness, and concurrency synchronization are explicit; fixed sleeps
  cannot be the only correctness mechanism.

### NFR-3 — Safety and non-interference

- Governance tooling is read-only unless an explicitly named append-only
  validation/benchmark writer is invoked.
- It never starts a daemon, loads a driver, changes SCM, formats a disk,
  changes swap, enables pressure, reboots a host, or deletes artifacts.
- It never treats a documentation scan, QEMU result, unit test, or static
  script result as a substitute for the platform-specific live gate required by
  a SPEC.

### NFR-4 — Performance and bounded resources

- Documentation checks complete within a bounded local CI budget recorded by
  the SPEC; they do not traverse dependency caches or generated build trees
  unless explicitly included.
- Duplicate detection uses bounded file size, token, and pair limits and
  reports an explicit truncation finding rather than silently skipping data.
- Manifest parsing enforces maximum document, entry, path, field, and artifact
  sizes before processing.

### NFR-5 — Compatibility and history

- Existing root validation, benchmark, gap, and postmortem history is
  append-only and remains readable.
- Existing platform-specific scripts remain their own owners; governance tools
  consume their sanitized summaries rather than reimplementing Windows, Linux,
  WSL2, or kernel behavior.
- No compatibility shim, dual reader/writer, or alternate evidence path is
  introduced without a later SPEC containing reason, removal date, rollback,
  and proof.

## 6. Flows

### Happy path

1. An author adds or changes a canonical document in English under the
   repository's documented path.
2. The author updates the parity/reference metadata only when ownership or
   navigation changes.
3. The author runs the local docs checks and manifest/schema tests.
4. A runtime or campaign change runs its own platform gate and writes a
   sanitized append-only validation/benchmark record.
5. CI re-runs deterministic scans, claims, links, index, parity, and hygiene.
6. A reviewer compares the claim to the canonical source and evidence paths.

### Alternate flows

- A new platform surface has no live environment: mark it `PARTIAL`, record the
  blocker and next proof, and keep the capability out of `DONE`.
- A visual or heavy campaign has no small committed artifact: retain the
  sanitized manifest and repository-relative pointer while storing heavy output
  in the documented ignored evidence directory.
- A baseline legacy document violates a newly adopted scanner: record the
  bounded baseline, owner, date, and remediation path; do not silently allow
  future violations.
- A postmortem action is not yet effective: keep the incident/action open and
  do not promote the related capability.

### Error flows

| Trigger | Result | Required signal | State |
| --- | --- | --- | --- |
| Missing canonical source or broken local link | Non-zero docs check | source path and target, no secret content | No promotion |
| `DONE` claim lacks runtime/test/live evidence | Non-zero claims check | missing evidence category | `PARTIAL`/`NO-GO` |
| Foreign/protected/private material detected | Non-zero provenance check | sanitized path/line/reason | Commit blocked |
| Duplicate normative prose detected | Non-zero or review finding per threshold | source pair and similarity class | Owner resolves source-of-truth |
| Manifest has missing seed/bounds/refusal/cleanup | Non-zero schema check | field name and journey ID | Campaign not runnable |
| Validation lacks after-state or rollback trigger | Non-zero schema check | missing field | Evidence incomplete |
| Postmortem action has no executable effectiveness proof | Non-zero closure check | action ID and missing proof | Incident remains open |

## 7. Data and state model

The feature stores documentation metadata and sanitized evidence only. It adds
no runtime database, kernel structure, driver ABI, uAPI, swap state, or network
service.

### Canonical-source record

```text
DocumentSource
├─ category
├─ canonical_path
├─ owner_role
├─ state_semantics
├─ evidence_paths[]
├─ limitations[]
└─ review_due
```

### Capability claim record

```text
CapabilityClaim
├─ id
├─ state: PRD | SPEC | PARTIAL | DONE | BLOCKED | N/A
├─ owner
├─ runtime_or_config_paths[]
├─ named_test_or_drill_paths[]
├─ validation_paths[]
├─ benchmark_paths[]
├─ binary_match_required
├─ environment_blocker
└─ rollback_trigger
```

### Deterministic journey/run record

```text
JourneyRun
├─ journey_id + version
├─ profile + seed + run_id
├─ base_clock + timeout_policy
├─ target_layer + commit/toolchain identity
├─ hardware/kernel/driver context (when applicable)
├─ bounded parameters
├─ before/action/after checkpoints
├─ legitimate_case + refusal_cases[]
├─ invariants[] + reporters[]
├─ artifacts[] + sanitization_class
├─ cleanup_result
└─ verdict + rollback_trigger
```

### Postmortem effectiveness record

```text
Effectiveness
├─ action_id + owner + due_date
├─ regression_command_or_drill
├─ expected_threshold
├─ observed_result
├─ revalidation_paths[]
├─ residual_limitations[]
└─ closure_state
```

Historical validation and benchmark records remain immutable. New schema fields
apply to new entries or explicitly marked legacy migrations only.

## 8. Interfaces

The exact command names, schemas, and files are closed by the future SPEC. The
PRD requires these observable interfaces:

| Interface | Required behavior |
| --- | --- |
| `node tools/generate-docs-index.mjs --check` | Deterministic index drift check |
| `./scripts/docs-check.sh` | One local fail-closed documentation gate |
| Claims checker | Validates capability state, owner, runtime/test/evidence links and `PARTIAL` blockers |
| Parity checker | Validates one canonical source per category and valid local links |
| Redundancy/provenance checker | Reports duplicate normative prose and prohibited/private provenance without echoing sensitive content |
| Journey/manifest checker | Validates profile, seed, run ID, bounds, refusals, invariants, reporters, artifacts, and cleanup |
| Validation schema checker | Validates new `Before → Action → After` records, measurements, verdict, limitations, and rollback trigger |
| Postmortem closure checker | Validates action owner/deadline and executable effectiveness proof |
| CI job | Runs the same deterministic read-only gates as local docs-check |

No interface may execute document content, accept arbitrary code in a manifest,
or mutate runtime/host state.

## 9. Dependencies and risks

### Dependencies

- Existing `tools/generate-docs-index.mjs`, `tools/check-broken-links.mjs`,
  `tools/ci/check-gap-register.mjs`, and
  `tools/ci/check-public-hygiene.mjs`.
- Existing `docs/SSDV3-PROMPTS.md`, `.claude/rules/*`, `validation.md`,
  `docs/BENCHMARKS.md`, `docs/benchmarks/results.jsonl`, gap register, runbooks,
  and postmortem template.
- Existing platform-specific harnesses and evidence directories; this feature
  must not replace their ownership.

### Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| False positive provenance match | Bounded allowlist with owner/review date; report sanitized finding for review |
| Duplicate detector masks legitimate repeated contract terms | Detect long normative blocks, not short vocabulary; require source owner decision |
| Claims checker overstates evidence | Require explicit live/evidence categories and reject unknown states |
| Manifest becomes a generic DSL | Keep it declarative and runner-neutral; require two real consumers before sharing a helper |
| CI blocks on old debt | Establish a dated, finite baseline and fail on any new finding; never use a broad permanent ignore |
| Sensitive scanner output leaks the match | Print only path, line, category, and stable reason code |
| Generated index claims a capability from a file | Keep index descriptive and let the claims checker own promotion evidence |
| Governance tooling changes production behavior | Enforce read-only process and test that no daemon/driver/SCM/swap command is invoked |

### Rollback taxonomy

- **Tooling/docs:** revert the new checker or metadata while retaining prior
  canonical sources and append-only history; restore the last known-good docs
  gate command set.
- **Runtime/daemon:** not touched by this PRD; any later consumer must have a
  separate application rollback trigger.
- **Kernel/driver/uAPI:** not touched by this PRD; platform gates remain owned
  by their SPEC.
- **Host/persistent state:** no action is authorized by this PRD. A future
  campaign must define recovery for swap, lease, VRAM, pagefile, LUN, SCM,
  watchdog, and reboot state separately.

Numeric/observable rollback triggers for this documentation slice:

- Any false `DONE` claim reaches the generated index or CI once.
- Any scanner prints a secret, token, PII, private path, hostname, username, or
  raw KASLR/kernel address once.
- Any source document with a broken link, unbounded manifest, missing cleanup,
  or missing rollback trigger passes the gate once.
- Any deterministic check returns different results twice for identical input.
- Any governance command mutates runtime or host state.

## 10. Implementation strategy

The future SPEC must split this PRD into small test-first items and re-audit
before implementation. The recommended order is:

1. Inventory existing source families, claims, validation entries, benchmark
   records, scripts, and CI; freeze the file/owner matrix.
2. Add the canonical-source parity matrix and objective reference router with
   link/structure tests.
3. Add capability-state/claims validation and negative fixtures for fabricated
   `DONE` and invalid `PARTIAL`.
4. Add redundancy/provenance/sanitization scanners with sanitized diagnostics,
   allowlist governance, and regression fixtures.
5. Add deterministic journey/run manifest schema and checker; connect only two
   existing native campaign consumers after the schema is proven.
6. Add validation schema and postmortem effectiveness checks without rewriting
   old records.
7. Run local/CI docs gates, record baseline and measured runtime, then update
   the capability/gap state only if evidence satisfies the claims gate.

No production code is authorized by this PRD. If discovery reveals a decision
about ownership, evidence authority, schema compatibility, or execution
semantics that is not closed here, the SPEC must stop, update this PRD, and
return through SSDV3 before implementation.

## 11. Documents to update

| Path | Action |
| --- | --- |
| `docs/INDEX.md` | Regenerate after this PRD is accepted; never edit manually |
| `docs/DOCUMENTATION-PARITY.md` | Create as the canonical source/ownership matrix |
| `docs/reference/REFERENCE-INDEX.md` | Create as the objective-oriented router |
| `docs/SSDV3-PROMPTS.md` | Alter only if the new gates require a RamShared-wide contract change |
| `.claude/rules/documentation.md` | Alter only to point to the canonical governance tools and schemas |
| `.claude/rules/governance.md` | Alter only if claim/postmortem closure changes the repo gate |
| `.claude/rules/benchmarks.md` | Alter only if journey manifest fields add a mandatory benchmark invariant |
| `scripts/docs-check.sh` | Extend with the final fail-closed tools after baseline tests pass |
| `tools/` and `scripts/tests/` | Create zero-dependency checkers and negative/positive fixtures in the SPEC |
| `validation.md` | Append measured governance-tool results; do not rewrite history |
| `docs/BENCHMARKS.md` / `docs/benchmarks/results.jsonl` | Append only if tooling performance or a benchmark decision is measured |
| `docs/postmortems/TEMPLATE.md` | Alter only to add machine-checkable effectiveness closure fields |
| `docs/reliability/GAP-REGISTER.md` | Update only when this governance capability has evidence-backed state |
| `CAPABILITIES.md` | Update only after the claims gate and all applicable tests/evidence pass |

## 12. Out of scope

- Runtime, daemon, broker, kernel, driver, uAPI, protocol, SCM, named-pipe,
  pagefile, swap, CUDA, WSL2, LUN, or storage behavior.
- Windows signing, Secure Boot, WDK, Driver Verifier, VM, physical-host, or
  reboot operations.
- Creating or modifying `SPEC.md`, `AUDIT-2.5.md`, `IMPL.md`, runbooks, or
  validation entries in this PRD step.
- Rewriting historical `validation.md`, benchmark, MEMORY, gap, or postmortem
  records.
- Automatic deletion, redaction, migration, or rewriting of documents.
- A universal journey runner, benchmark runner, or cross-platform test DSL.
- Importing code, text, names, entities, paths, identities, API shapes, or
  process templates from another repository.
- Adding telemetry, dashboards, a database, external service, hosted docs, or
  a dependency manager solely for governance.
- Claiming production readiness, normal-Windows support, or closing any
  existing technical gap.

## 13. Acceptance criteria

The PRD is ready for SPEC only when:

- one option and its boundaries are accepted;
- all RF/NFR acceptance criteria are measurable and have an owner in the SPEC;
- canonical document ownership and the parity matrix have no unresolved
  collision;
- claims states and evidence requirements are closed, including `PARTIAL`;
- provenance policy explicitly covers English docs, repository-relative paths,
  no foreign references, no usernames/hostnames/private paths/secrets, and no
  KASLR addresses;
- deterministic journey fields, bounded resources, refusal cases, and cleanup
  semantics are closed;
- validation and postmortem schemas preserve append-only history;
- rollback triggers are numeric/observable and separated by frontier;
- tooling commands are read-only and cannot touch runtime/host state;
- the implementation file/test matrix, CI placement, and baseline migration
  policy are fully decidable without invention by the implementer.

## 14. Validation plan

This PRD has no runtime implementation gate. The future SPEC must prove:

- **Tooling unit tests:** parser, index, parity, claims, provenance,
  redundancy, manifest, validation, and postmortem fixtures; positive and
  negative cases named per file.
- **Determinism:** repeated checks on identical checkout/input produce identical
  output and exit code.
- **Repository checks:** `node tools/generate-docs-index.mjs --check`,
  `node tools/check-broken-links.mjs --check`, `./scripts/docs-check.sh`, and
  all new governance tests pass without sensitive output.
- **Boundary safety:** static inspection/test proves no governance command can
  invoke daemon, driver, SCM, swap, disk, pressure, reboot, or destructive
  cleanup operations.
- **Evidence schema:** a valid sanitized record passes; missing before/after,
  refusal, measurement, artifact, limitation, verdict, or rollback fields
  fail; historical records remain untouched.
- **Claims:** fabricated `DONE`, stale evidence, absent BINARY_MATCH where
  required, and environment-bound closure all fail or remain `PARTIAL`.
- **Journey manifests:** deterministic profiles, seeded runs, refusal cases,
  bounded waits, reporters, artifacts, and cleanup validate; unsupported
  platform profiles remain explicitly `N/A`.
- **Postmortem closure:** an action without a named executable regression or
  measured threshold remains open; a passing effectiveness record closes only
  after the legitimate and critical refusal paths are revalidated.

The future IMPL must report command, exit code, counts, changed files, artifact
paths, limitations, and rollback trigger. It must not claim that this PRD makes
any Windows, Linux, WSL2, kernel, driver, or benchmark capability complete.
