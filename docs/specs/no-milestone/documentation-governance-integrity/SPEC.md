# SPEC — Documentation governance and evidence integrity

> SSDV3 Step 2 · implements [`PRD.md`](PRD.md).
>
> This SPEC is limited to repository documentation governance. It adds no
> daemon, driver, kernel, uAPI, storage, swap, GPU, Windows, WSL2, or host
> behavior. The operator surface is the local documentation gate itself.

## Closed scope

### In now

- A canonical-source parity matrix at `docs/DOCUMENTATION-PARITY.md`.
- An objective-oriented router at `docs/reference/REFERENCE-INDEX.md`.
- A machine-readable capability-claim registry with fail-closed promotion
  semantics.
- A deterministic, bounded scanner for links, provenance, sensitive material,
  and duplicate normative blocks.
- A small journey/run manifest consumed by the documentation gate and its
  negative fixtures; it is not an execution DSL.
- A schema extension for new `validation.md` entries and a machine-checkable
  postmortem effectiveness closure block. Historical entries remain unchanged.
- Local and CI integration through the existing Node/docs workflow.
- Positive and refusal-path tests, deterministic output checks, and a real
  `scripts/docs-check.sh` E2E run with no runtime or host mutation.

### Out now

- Any runtime, daemon, broker, kernel, driver, protocol, service-control,
  named-pipe, pagefile, swap, LUN, CUDA, WSL2, reboot, signing, VM, or physical
  host operation.
- A benchmark runner, pressure campaign, generic workflow engine, or
  cross-platform test DSL.
- Rewriting or migrating historical validation, benchmark, gap, or postmortem
  records.
- Promotion of any existing product capability to `DONE` merely because this
  governance slice passes.
- Importing names, text, paths, identities, process conventions, or API shapes
  from another repository or product.

### Assumed-ready dependencies

- The existing Node 22 runtime and built-in `node:test` runner.
- `tools/check-broken-links.mjs`, `tools/generate-docs-index.mjs`,
  `tools/ci/check-gap-register.mjs`, and
  `tools/ci/check-public-hygiene.mjs` remain the owners of their current
  checks.
- `tools/ci/check-validation-schema.mjs` remains append-only and continues to
  accept its documented legacy allowlist.
- Existing platform harnesses remain owners of platform evidence. This slice
  consumes only sanitized repository-relative summaries.
- Git is available for the tracked/changed file scopes used by the local and
  CI gate. No network access is required.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-1, ITEM-2, ITEM-3 |
| RF-2 | ITEM-2, ITEM-3 |
| RF-3 | ITEM-3, ITEM-7 |
| RF-4 | ITEM-4, ITEM-5, ITEM-7 |
| RF-5 | ITEM-5, ITEM-6 |
| RF-6 | ITEM-7 |
| RF-7 | ITEM-7, ITEM-8 |
| RF-8 | ITEM-8, ITEM-9 |
| NFR-1 | DT-2, DT-4, ITEM-4 |
| NFR-2 | DT-8, DT-9, ITEM-6, ITEM-9 |
| NFR-3 | DT-1, DT-10, ITEM-9 |
| NFR-4 | DT-7, DT-8, ITEM-4, ITEM-6 |
| NFR-5 | DT-3, DT-5, ITEM-7 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | The slice has one implementation path: `tools/ci/check-documentation-governance.mjs --all`, called by `scripts/docs-check.sh`. Existing specialized tools remain their current owners and are invoked by the same script. | One primary path makes the gate discoverable and prevents a second, divergent checker. |
| DT-2 | Structural documents and machine-readable metadata created by this slice are English, use repository-relative links/paths, and describe RamShared only. External links are accepted only when an explicit standard/vendor rule allows them. | Public documentation must be portable and must not depend on private context or foreign narratives. |
| DT-3 | `docs/governance/claims.json` is the promotion authority. A generated index row may show `DONE` only when a matching registry row has `state: DONE`; an implementation without a qualifying row is rendered `UNQUALIFIED`, never `DONE`. | File presence is not execution proof. Existing unqualified history stays visible without being promoted. |
| DT-4 | Claim states are `PRD`, `SPEC`, `UNQUALIFIED`, `PARTIAL`, `BLOCKED`, `DONE`, and `N/A`. `PARTIAL` and `BLOCKED` require a concrete blocker and next proof. `DONE` requires the complete evidence tuple in the claim schema. | State names separate documentation, implementation, validation, and environment limits. |
| DT-5 | A `DONE` claim requires an existing canonical SPEC, implementation path, owner, every named test from the SPEC matrix, applicable cover evidence, a same-surface validation record, all required refusal cases, and `BINARY_MATCH` evidence when the SPEC declares it. | A claim must be traceable to executable or platform-correct proof. |
| DT-6 | The parity matrix and objective router contain pointers and state semantics only. Normative requirements remain in their canonical PRD/SPEC/runbook/rule; consumers must not copy a normative block. | Pointer documents reduce contradiction and documentation drift. |
| DT-7 | Provenance findings are fail-closed for newly added or edited structural/evidence files. A finite baseline may be supplied only as explicit entries with owner, reason, review date, and expiry; it cannot suppress secrets, credentials, raw addresses, or an entire directory. | Existing debt must remain visible without creating a permanent broad ignore. |
| DT-8 | The scanner uses bounded limits: 512 KiB per text file, 2,000 files per scope, 10,000 normalized tokens per duplicate candidate, and 100,000 pair comparisons. Exceeding a limit is a finding, not silent truncation. | A documentation gate must be predictable in CI and must not hide unscanned content. |
| DT-9 | A journey manifest is declarative. It records a runner reference and assertions, never executable shell, PowerShell, JavaScript, or arbitrary code. Thresholds remain in the owning SPEC or benchmark contract. | A generic executor would erase platform-specific safety ownership. |
| DT-10 | The governance command may read files and invoke Git read operations only. It cannot start processes other than the fixed Git queries, load drivers, alter SCM, alter disks/swap/pagefile, enable pressure, reboot, delete artifacts, or rewrite documentation. | The gate must be safe on a shared development host. |
| DT-11 | Sensitive findings print only repository-relative path, line, rule ID, and a stable reason code. Matched secrets, tokens, usernames, hostnames, private paths, PII, and raw kernel addresses are never printed. | A sanitizer that leaks its match would create a second exposure. |
| DT-12 | New validation and postmortem records opt into schema version `1` with an explicit marker. Legacy records are validated only by their existing rules and remain byte-for-byte append-only. | The new contract can be strict without falsifying historical evidence. |
| DT-13 | The first live E2E is the documentation operator surface: a clean positive run plus manufactured refusal fixtures through the public CLI. `BINARY_MATCH` and platform tools are `N/A — no runtime surface`. | This slice has no daemon or platform artifact whose binary identity could be matched. |
| DT-14 | All checks return exit `0` only for a clean scope, exit `1` for policy findings, and exit `2` for usage/configuration errors. Output order is sorted by path, line, rule, and reason code. | Stable exit and output semantics make local/CI comparison reproducible. |
| DT-15 | Validation diff failures are findings, never an empty successful diff. A line added inside an existing entry is an append-only violation, and a newly added entry cannot inherit a historical date allowlist exemption. | The former parser swallowed an invalid Git base and validated only newly added headers, creating three reproducible false-green paths. |
| DT-16 | Historical validation permits only a controlled public-provenance redaction: a private profile/artifact root becomes an explicit redacted or portable RamShared root, or an unrelated product identity becomes the neutral phrase `unrelated workload`. The normalized line must otherwise preserve every number, verdict marker, command semantic, and evidence claim. | Public release hygiene cannot retain private identities, but append-only evidence must not allow a sanitation edit to rewrite measurements or outcomes. |

### Claim record contract

`docs/governance/claims.json` has this exact top-level shape:

```json
{
  "schema_version": 1,
  "claims": [
    {
      "slug": "documentation-governance-integrity",
      "state": "SPEC",
      "owner_role": "documentation-governance",
      "canonical_spec": "docs/specs/no-milestone/documentation-governance-integrity/SPEC.md",
      "implementation_paths": [],
      "named_tests": [
        {
          "path": "tools/ci/check-documentation-governance.test.mjs",
          "name": "parity_matrix_has_one_canonical_source_per_category"
        }
      ],
      "cover": {
        "mode": "node-built-in",
        "minimum_percent": 80,
        "evidence_path": "tmp/documentation-governance-e2e/coverage.txt"
      },
      "validation": {
        "record_path": "validation.md",
        "verdict": "🟡",
        "source_commit": "0000000000000000000000000000000000000000",
        "evidence_paths": []
      },
      "binary_match_required": false,
      "environment_blocker": null,
      "missing_gate": "implementation not started",
      "next_proof": "complete the named governance test and docs-check gates",
      "rollback_trigger": "one false DONE promotion or one sensitive-output leak"
    }
  ]
}
```

The example is a schema illustration, not an authorization to claim `DONE`.
The implementation must replace the sentinel commit and state with measured
values or keep the row `SPEC`/`PARTIAL`.

Rules for every claim:

- `slug`, canonical paths, test paths, evidence paths, and artifact paths are
  repository-relative POSIX paths; `..`, absolute paths, drive letters, and
  URLs are rejected in these fields.
- `named_tests` is an array of `{path,name}` pairs. The checker verifies that
  the path exists and the exact test name occurs in the file; it does not run
  arbitrary text from the registry.
- `DONE` requires non-empty implementation paths, at least one named test,
  cover evidence (or an explicit `N/A — E2E-only` reason in the owning SPEC),
  a validation record with a positive verdict, all evidence paths present,
  `environment_blocker: null`, and a non-placeholder rollback trigger.
- A claim with `binary_match_required: true` also requires a validation record
  containing `BINARY_MATCH=true` and a repository-relative artifact path.
- `PARTIAL` and `BLOCKED` require `environment_blocker`, `missing_gate`, and
  `next_proof`; neither state is index-quality `DONE`.
- Duplicate slugs, unknown states, missing owners, stale `source_commit`
  fields, missing artifacts, and unknown fields in a strict record are
  findings. Historical records are handled by the legacy policy, not by
  weakening new records.

### Canonical matrix and router contracts

`docs/DOCUMENTATION-PARITY.md` must contain one table with exactly these
columns:

```text
Objective | Canonical source | Owner role | Evidence source | State semantics | Known limitation
```

It must contain one row for each of: architecture/topology, capability state,
PRD/SPEC requirements, operation, empirical validation, benchmark comparison,
reliability gaps, and postmortem closure. A duplicate objective or canonical
source is a finding.

`docs/reference/REFERENCE-INDEX.md` must contain one table with exactly these
columns:

```text
Question | Canonical source | Next-depth references | Boundary
```

It must route build/test, Linux/WSL2 lifecycle, Windows lifecycle, host-safety
campaigns, benchmarks, reliability gaps, architecture decisions, SSDV3
changes, and incident recovery. The checker verifies that every target exists,
that every question has one row, and that the router does not contain a
second normative requirement.

### Provenance and redundancy contract

The scanner checks the structural scope by default and supports explicit
changed-file, evidence, and full tracked-file audits. CI may use the changed
scope with a base revision for incremental review, while the local
`--all` gate uses the full structural scope. Structural scope is:

- `README.md`, `ARCHITECTURE.md`, `CLAUDE.md`, `AGENTS.md`;
- `.claude/rules/`;
- `docs/SSDV3-PROMPTS.md`, `docs/decisions/`, `docs/specs/`,
  `docs/runbooks/`, `docs/reliability/`, `docs/reference/`, and
  `docs/governance/`;
- `validation.md`, `docs/BENCHMARKS.md`,
  `docs/benchmarks/`, and committed slice evidence files.

Locale-specific marketing material and generated build output are not
structural scope. A finding in excluded material may not be used to support a
capability claim.

The scanner rejects, unless a narrowly scoped rule explicitly marks a
sanitized fixture:

- absolute Unix/Windows paths, usernames, hostnames, private workspace paths,
  PII, credentials, tokens, private keys, and password literals;
- raw kernel/KASLR addresses or pointer-like values in committed evidence;
- links to private filesystems, foreign repository origins, or unapproved
  external pages in new/edited public structural documents;
- copied product narrative or service/tenant/API vocabulary that is not a
  RamShared role or an approved standard term.

The allowlist record is `{id, rule, pattern, scope[], reason, owner_role,
review_by, expires}`. `scope` contains file prefixes, never `"**"` or an
entire repository. Secret and raw-address rules cannot be allowlisted.

The only baseline file is
`docs/governance/provenance-baseline.json`. It has
`{schema_version, generated_from_commit, entries[]}`, where each entry is
`{path, rule, fingerprint, file_sha256, reason, owner_role, review_by, expires}`.
`file_sha256` is the hash of the unchanged pre-existing file; changing that
file invalidates the baseline entry. Baseline entries are permitted only for
non-secret provenance or redundancy findings, expire within 90 days, and are
reported in the gate count. A missing file means an empty baseline. The CLI has
no arbitrary baseline-path option, so a caller cannot hide findings elsewhere.

Duplicate detection normalizes whitespace, Markdown link destinations, and
case; removes headings and code fences; then compares contiguous normative
blocks. It reports only a pair when both blocks contain at least 40 tokens and
either normalized similarity is at least `0.92` or the exact block is at least
300 characters. Short labels, table headers, code, and the canonical pointer
sentences are excluded.

### Journey/run manifest contract

The committed smoke record is
`docs/governance/journeys/documentation-governance-smoke.json`. The checker
accepts only `schema_version: 1` and these fields:

```text
journey_id, version, run_id, profile, target_layer, seed, clock_policy,
timeout_seconds, parameters, runner_refs, actions, checkpoints,
legitimate_case, refusal_cases, invariants, reporters, artifacts, cleanup,
consumer_paths, verdict, rollback_trigger
```

Closed values and bounds:

- `journey_id` and `run_id` are lowercase ASCII identifiers of 1–96
  characters; `version` is a positive integer.
- `profile` is one of `smoke`, `baseline`, `stress`, `soak`, `isolation`.
  An unsupported profile uses `profile: "N/A"` plus a non-empty `na_reason`.
- `seed` is a non-negative integer; `clock_policy` is `fixed-utc` or
  `monotonic`; `timeout_seconds` is 1–3,600.
- `parameters` contains at most 32 scalar values; each numeric bound is
  explicit. `runner_refs` and `consumer_paths` contain existing relative
  paths and no executable payload.
- `actions` contain `{id, runner_ref, wait_strategy, timeout_seconds}`;
  `wait_strategy` is `process-exit`, `file-exists`, or `none`. A literal
  sleep is not a valid synchronization strategy.
- `checkpoints` contains `before`, `action`, and `after` reporters. At least
  one legitimate case and two refusal cases are required; each refusal names
  the expected non-zero exit and terminal state.
- `invariants` include no-sensitive-output, no-host-mutation, deterministic
  ordering, and idempotent cleanup. `artifacts` use repository-relative
  paths or `tmp/<slug>-e2e/<run_id>/`; no absolute path is accepted.
- `cleanup.idempotent` is `true`, `cleanup.mode` is `none` or
  `remove-temporary-report`, and cleanup is described as a reporter/ref only;
  the checker never executes it.
- `verdict` is `PASS`, `NO-GO`, or `PARTIAL`; `PARTIAL` requires a blocker.
  `rollback_trigger` is numeric or observable and cannot be `TBD`, `TODO`,
  `none`, or `if it goes wrong`.

Profiles do not carry thresholds. A runner reference may point to
`scripts/docs-check.sh`, but the manifest cannot invoke it or any other
command by parsing JSON.

### Validation and postmortem contracts

New root or slice validation entries opt in with:

```markdown
**Governance schema:** 1
**Slug:** <repo slug>
**Environment/commit:** <sanitized state>
**Scope:** <surface>
**Before:** <state>
**Action:** <command/path, not private input>
**After:** <state>
**Legitimate case:** <observable result>
**Required refusals:** <at least two named refusal results>
**Tests/coverage:** <named tests and measured coverage or N/A reason>
**Platform gates:** <gates or N/A reason>
**Artifacts:** <repository-relative paths>
**Cleanup:** <terminal cleanup result>
**Limitations:** <explicit limitations>
**Rollback trigger:** <numeric or observable condition>
**Verdict:** ✅ works / 🔴 does not work / 🟡 partial
```

The existing `What`, `Category`, `How to measure`, `Measured data`, and
`Next action` labels remain valid. The schema checker requires the governance
labels only when the marker is present and continues to enforce append-only
history.

New postmortem records marked `**Governance schema:** 1` add one effectiveness
row per corrective action:

```markdown
**Action ID:** <stable id>
**Type:** prevent | detect | respond
**Owner role:** <role, never a username>
**Due date:** YYYY-MM-DD
**Regression command:** <repository-relative runner/test path>
**Threshold:** <number and unit or observable state>
**Revalidation:** <legitimate path + critical refusal path>
**Observed result:** <measured result>
**Evidence:** <repository-relative artifact/validation path>
**Closure state:** open | effective | blocked
```

`effective` is rejected unless the regression, threshold, legitimate path,
and critical refusal path are all evidenced. `open` and `blocked` remain open
regardless of prose claiming resolution.

## Interfaces

### Governance CLI

```text
node tools/ci/check-documentation-governance.mjs --all
node tools/ci/check-documentation-governance.mjs --mode parity
node tools/ci/check-documentation-governance.mjs --mode claims
node tools/ci/check-documentation-governance.mjs --mode provenance --scope changed --base-ref <ref>
node tools/ci/check-documentation-governance.mjs --mode journeys
node tools/ci/check-documentation-governance.mjs --mode validation
node tools/ci/check-documentation-governance.mjs --mode postmortems
```

`--all` is the only mode wired into `scripts/docs-check.sh`. `--mode` accepts
`parity`, `reference`, `claims`, `provenance`, `redundancy`, `journeys`,
`validation`, `postmortems`, or `all`; the default scope is `structural`.
`--scope` accepts `structural`, `evidence`, `changed`, or `tracked`. `--base-ref`
is required with `--scope changed` and is passed only to a fixed Git diff query.
Unknown flags, duplicate scope selectors, missing arguments, and a non-existent
base ref return exit `2` without scanning arbitrary command text.

The CLI prints, in this order, `SCOPE`, `FILES`, `FINDINGS`, sorted rule counts,
and `GOVERNANCE_STATUS`. A clean run ends with `GOVERNANCE_STATUS=PASS`; any
finding ends with `GOVERNANCE_STATUS=NO-GO`. Diagnostics contain only the
sanitized tuple defined by DT-11.

### Existing validation CLI

The existing append-only interface remains the authority for root validation:

```text
node tools/ci/check-validation-schema.mjs --diff <base-ref>
node tools/ci/check-validation-schema.mjs --all
```

The governance checker imports its pure exported validation functions for
marked records; it does not spawn the validation command or execute Markdown.

### Generated index interface

```text
node tools/generate-docs-index.mjs
node tools/generate-docs-index.mjs --check
```

The first command is the only intentional documentation write and is run by a
reviewer/implementation step, never by the governance checker. The `--check`
path is read-only and must observe the claims registry when deriving status.

## Atomicity and rollback

### Atomicity frontier

This slice has one frontier: repository documentation and CI metadata. The
checker is read-only; generating `docs/INDEX.md` remains a separately invoked
and reviewable write. No runtime, kernel/module/driver, or host/persistent
state is touched.

### Rollback by frontier

| Frontier | Policy |
| --- | --- |
| Userspace/documentation tooling | Revert the governance checker and its gate wiring to the last known-good command set. Keep append-only evidence and the canonical sources. |
| Kernel/module or Windows driver | N/A — no such file or execution path is in this SPEC. |
| Host/persistent state | N/A — the checker cannot change SCM, swap, pagefile, disks, LUNs, VRAM, leases, watchdogs, or reboot state. |

Rollback is forward-only for already published validation history: failed
records are retained and a superseding sanitized record is appended. Do not
rewrite evidence to hide a failed gate.

Numeric/observable triggers:

- one false `DONE` promotion reaches `docs/INDEX.md` or CI;
- one sensitive value or raw kernel address is printed by a checker;
- one unbounded/malformed manifest or missing cleanup passes;
- two identical-input runs produce different exit codes or sorted output;
- one governance command mutates any file outside an explicitly requested
  report or invokes a runtime/host mutation command.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-3 claims/index | #13 — illusion of validity | Can a file-presence `IMPL.md` become false `DONE`? | `claims_done_requires_all_evidence`, `unqualified_impl_is_not_done`, and generated-index fixture | Any false `DONE` |
| ITEM-4 provenance | #16 — safe default | Can a finding leak the sensitive value while reporting it? | Secret/private-path fixtures assert path/rule only and no matched value | Any echo or allowlist bypass |
| ITEM-5 redundancy | #18 — owning layer | Does the checker point to one source rather than auto-rewrite documents? | Duplicate pair report plus unchanged fixture bytes | Automatic rewrite or silent skip |
| ITEM-6 journey | #15/#17 — bounded retry/idempotency | Are waits bounded and repeated checks equivalent? | Manifest refusal matrix, timeout bounds, two identical runs | Sleep-only sync, missing cleanup, or output drift |
| ITEM-7 validation/postmortem | #9 — executable evidence | Does a closure claim include a measured regression and refusal? | Valid, open, and effective fixtures with named commands | Prose-only closure |
| ITEM-9 live gate | #3 — number not adjective | Is the gate outcome independently measurable? | Exit code, finding count, output hash, expected refusals | Missing numbers or sensitive output |

## Security checklist (pre-implementation)

- [x] Privilege: N/A — the checker uses read-only files and fixed Git metadata.
- [x] User/host copy: N/A — no user buffer, ioctl, device node, or host API.
- [x] Flags/CLI: unknown mode, scope, and argument combinations return exit 2.
- [x] Information leak: findings omit secrets, tokens, usernames, hostnames,
      private paths, PII, and raw KASLR/kernel addresses.
- [x] IRQ/atomic/IRQL: N/A — no kernel or driver code.
- [x] Lifetime/hot-unplug: N/A — no device or runtime resource.
- [x] Host safety: no daemon, driver, SCM, disk, swap, pagefile, GPU,
      pressure, reboot, or destructive cleanup operation is callable.
- [x] Replayable operations: repeated read-only runs produce the same result;
      no manifest action is executed (#17).
- [x] Provenance: new structural docs are English and repository-relative;
      foreign repository/product references are rejected unless an approved
      standard/vendor rule applies.

## Files to CREATE

**`docs/DOCUMENTATION-PARITY.md`**

- Purpose: canonical category-to-source ownership matrix.
- RF / DT: RF-1, DT-2, DT-6.
- Required structure: the six-column table and eight required objectives in
  the contract above; no normative paragraphs duplicated from a source.
- Required tests: `tools/ci/check-documentation-governance.test.mjs` ::
  `parity_matrix_has_one_canonical_source_per_category`,
  `parity_rejects_duplicate_objective`, `parity_rejects_missing_target`.
- Cover target: N/A — declarative documentation; parser logic is covered by
  the Node gate below.

**`docs/reference/REFERENCE-INDEX.md`**

- Purpose: objective-oriented operator/developer/reviewer/incident router.
- RF / DT: RF-2, DT-2, DT-6.
- Required tests: `reference_index_routes_required_objectives`,
  `reference_index_rejects_private_or_missing_target`.
- Cover target: N/A — declarative documentation.

**`docs/governance/claims.json`**

- Purpose: single capability-claim registry consumed by the index and claims
  checker.
- RF / DT: RF-3, DT-3, DT-4, DT-5.
- Types/fields: the exact `ClaimRegistry` contract in this SPEC; strict JSON,
  no unknown fields, no executable values.
- Required tests: `claims_done_requires_all_evidence`,
  `claims_partial_requires_blocker_and_next_proof`,
  `claims_rejects_missing_owner_or_duplicate_slug`,
  `unqualified_impl_is_not_done`.
- Cover target: N/A — data-only; checker logic has an ≥80% Node gate.

**`docs/governance/provenance-allowlist.json`**

- Purpose: narrowly scoped, reviewable standard/vendor and sanitized-fixture
  exceptions.
- RF / DT: RF-4, DT-2, DT-7, DT-11.
- Types/fields: `{schema_version, entries[]}` with the eight allowlist fields;
  secret/raw-address rules are rejected even if listed.
- Required tests: `allowlist_requires_owner_review_and_expiry`,
  `allowlist_cannot_cover_an_entire_directory`.
- Cover target: N/A — data-only.

**`docs/governance/provenance-baseline.json`**

- Purpose: finite, hashed record of pre-existing non-secret findings during a
  bounded migration window.
- RF / DT: RF-4, RF-8, DT-7.
- Types/fields: exact `{schema_version, generated_from_commit, entries[]}`
  contract above; secret/raw-address rules and changed files are invalid.
- Required tests: `baseline_entry_requires_content_hash_and_expiry`,
  `changed_file_cannot_use_legacy_baseline`.
- Cover target: N/A — data-only.

**`docs/governance/journeys/documentation-governance-smoke.json`**

- Purpose: one deterministic, non-mutating smoke run that exercises the public
  documentation gate and its refusal cases.
- RF / DT: RF-5, DT-9, DT-13.
- Types/fields: exact journey/run contract above; `target_layer: documentation`
  and `profile: smoke`.
- Required tests: `journey_manifest_accepts_seeded_bounded_run`,
  `journey_manifest_rejects_implicit_clock_sleep_only_and_missing_cleanup`,
  `journey_manifest_rejects_duplicate_version_missing_consumer`.
- Cover target: N/A — declarative manifest; parser logic has an ≥80% Node gate.

**`docs/governance/README.md`**

- Purpose: short English operator guide for the canonical matrix, claim
  registry, manifest, and commands; pointers only, no duplicated requirements.
- RF / DT: RF-1, RF-2, RF-8, DT-2, DT-6.
- Required tests: `governance_readme_routes_only_existing_canonical_sources`.
- Cover target: N/A — documentation.

**`tools/ci/check-documentation-governance.mjs`**

- Purpose: deterministic zero-dependency parser, scanner, and CLI.
- RF / DT: RF-1..RF-5, RF-7, RF-8, DT-1, DT-7..DT-14.
- Required exports and behavior:
  - `parseMarkdownTable(text, path)` returns sorted rows or sanitized findings.
  - `validateParityDocument(text, root)` enforces one source per objective.
  - `validateReferenceIndex(text, root)` enforces the required objective set.
  - `loadClaims(root)` and `validateClaims(registry, root, asOf)` enforce the
    claim contract without executing values.
  - `scanProvenance(files, allowlist, limits)` returns `{path,line,rule,reason}`
    findings and never includes matched text.
  - `findDuplicateNormativeBlocks(files, limits)` returns source pairs only.
  - `validateJourneyManifest(manifest, root)` enforces all bounded fields,
    refusal cases, reporters, artifacts, and idempotent cleanup.
  - `validatePostmortemEffectiveness(text, path)` validates only marked
    schema-1 actions.
  - `run({root, scope, files, asOf})` returns `{ok, findings, counts}` with
    deterministic ordering; `main()` maps the result to exit 0/1/2.
- Reference patterns: current `tools/check-broken-links.mjs` walker and
  `tools/ci/check-public-hygiene.mjs` tracked-file scan; do not duplicate
  their existing policy or print their sensitive matches.
- Required tests: all `check-documentation-governance.test.mjs` names listed
  in the Test matrix below.
- Cover target: ≥80% lines, branches, and functions using Node's built-in
  coverage threshold command, measured per this production file.
- Kahneman: ITEM-3, ITEM-4, ITEM-5, ITEM-6, and ITEM-9 rows above.

**`tools/ci/check-documentation-governance.test.mjs`**

- Purpose: positive, negative, determinism, and read-only contract tests.
- RF / DT: all checker requirements.
- Required tests: the named matrix below; fixtures are created in temporary
  directories from sanitized inline data and are never persisted as secrets.
- Cover target: test harness only — N/A; it drives the production file's
  ≥80% threshold.

**`tools/generate-docs-index.test.mjs`**

- Purpose: exercise index discovery, frontmatter fallback, claim promotion,
  deterministic rendering, empty repositories, and invalid claim registries
  without writing the real index.
- RF / DT: RF-3, RF-8, DT-3, DT-14.
- Required tests: `build_rows_discovers_grouped_and_legacy_specs`,
  `build_rows_uses_frontmatter_and_heading_fallback`,
  `build_rows_fails_closed_to_unqualified_without_done_claim`,
  `load_claims_rejects_missing_invalid_and_wrong_schema`,
  `render_index_handles_empty_rows_escaping_and_issues`,
  `index_output_is_deterministic`.
- Cover target: test harness only; it drives
  `tools/generate-docs-index.mjs` to ≥80% lines/branches/functions.

## Files to MODIFY

**`tools/generate-docs-index.mjs`**

- What: add `loadClaimsRegistry()`, `qualifiedStatus(slugDir, claims)`, and
  `deriveStatus(slugDir, claims)`; pass the registry from `buildRows()` and
  update `renderIndex()` status help text. Export these pure helpers (plus
  `buildRows` and `renderIndex`) and guard `main()` so Node tests can import
  them without writing `docs/INDEX.md`.
- RF / DT: RF-3, DT-3, DT-4.
- Before → after: an `IMPL.md` alone yields `DONE` → an IMPL without a
  matching `state: DONE` claim yields `UNQUALIFIED`; only a qualified claim
  yields `DONE`. `PRD` and `SPEC` discovery remains unchanged.
- Callers/docs: `scripts/docs-check.sh`, `docs/INDEX.md` generated output.
- Required tests: `unqualified_impl_is_not_done`,
  `qualified_claim_is_the_only_done_path`,
  `index_status_is_deterministic`.
- Cover target: ≥80% lines/branches/functions via the governance Node gate;
  existing index behavior must retain its current tests/smoke command.

**`tools/ci/check-validation-schema.mjs`**

- What: add `hasGovernanceSchema(body)` and exported
  `validateGovernanceEntry(entry)`; call it from `validateEntry()` only when
  `**Governance schema:** 1` is present.
- RF / DT: RF-6, DT-12.
- Before → after: legacy entries keep the existing labels/allowlist behavior →
  new marked entries require the full Before/Action/After, measurement,
  refusals, artifacts, cleanup, limitations, verdict, and rollback fields.
- Required tests: `governance_entry_requires_before_action_after`,
  `governance_entry_requires_refusals_measurement_and_rollback`,
  `legacy_validation_entries_remain_accepted`,
  `validation_log_remains_append_only`, `invalid_base_ref_fails_nonzero`,
  `added_line_inside_existing_entry_is_append_only_violation`,
  `new_entry_with_legacy_timestamp_is_not_allowlisted`,
  `public_provenance_redaction_preserves_metrics_and_verdict`,
  `public_provenance_redaction_cannot_change_measurement_or_claim`.
- Cover target: ≥80% lines/branches/functions using the same Node threshold
  command with `tools/ci/check-validation-schema.mjs` included explicitly.

**`tools/ci/check-validation-schema.test.mjs`**

- What: append the named governance-schema tests without changing historical
  test fixtures or allowlist entries.
- RF / DT: RF-6, DT-12.
- Cover target: N/A — test harness.

**`scripts/docs-check.sh`**

- What: invoke `node tools/ci/check-documentation-governance.mjs --all`
  before the existing index, link, gap-register, and public-hygiene checks.
- RF / DT: RF-8, DT-1, DT-10, DT-14.
- Before → after: specialized checks only → one fail-closed local gate whose
  final output remains `✓ docs-check OK` only after every component passes.
- Required tests: `docs_check_includes_governance_gate`,
  `docs_check_is_read_only`.
- Cover target: N/A — shell orchestration; live E2E is mandatory.

**`.github/workflows/ci.yml`**

- What: retain the existing docs job and add the exact Node governance test
  and coverage commands after `./scripts/docs-check.sh`.
- RF / DT: RF-8, NFR-2, DT-14.
- Required commands:
  - `node --test --experimental-test-coverage --test-coverage-include=tools/ci/check-documentation-governance.mjs --test-coverage-lines=80 --test-coverage-branches=80 --test-coverage-functions=80 tools/ci/check-documentation-governance.test.mjs`
  - `node --test --experimental-test-coverage --test-coverage-include=tools/ci/check-validation-schema.mjs --test-coverage-lines=80 --test-coverage-branches=80 --test-coverage-functions=80 tools/ci/check-validation-schema.test.mjs`
  - `node --test --experimental-test-coverage --test-coverage-include=tools/generate-docs-index.mjs --test-coverage-lines=80 --test-coverage-branches=80 --test-coverage-functions=80 tools/generate-docs-index.test.mjs`
- Cover target: the two production Node files each ≥80% for lines, branches,
  and functions; no workspace-average substitution.

**`docs/postmortems/TEMPLATE.md`**

- What: add the English schema-1 marker and effectiveness fields from this
  SPEC while retaining the existing incident narrative and rollback sections.
- RF / DT: RF-7, DT-2, DT-12.
- Before → after: prose-only corrective action → optional marked action with
  owner role, deadline, executable regression, threshold, revalidation, result,
  evidence, and closure state.
- Required tests: `postmortem_effectiveness_requires_regression_threshold_and_revalidation`,
  `postmortem_open_action_cannot_close`,
  `postmortem_effective_action_requires_both_paths`.
- Cover target: N/A — template; parser logic is covered by the Node gate.

**`docs/INDEX.md`**

- What: regenerate with `node tools/generate-docs-index.mjs` after the claims
  registry and new feature files land; never hand-edit.
- RF / DT: RF-3, RF-8, DT-3.
- Required test: `node tools/generate-docs-index.mjs --check`.
- Cover target: N/A — generated output.

**`validation.md`**

- What: append one schema-1 before/action/after record on close, including
  command exit codes, finding counts, deterministic output hash, refusal
  outcomes, limitations, and the rollback trigger.
- RF / DT: RF-6, RF-8, DT-12.
- Cover target: N/A — append-only empirical log.

## Files to DELETE

None. No compatibility file, alternate checker, generated evidence, or
historical record is deleted.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| Gate result | stdout of `check-documentation-governance.mjs` and `docs-check.sh` | `GOVERNANCE_STATUS=PASS\|NO-GO`, exit 0/1 |
| Scope and counts | stdout | `SCOPE=...`, `FILES=n`, `FINDINGS=n`, sorted rule counts |
| Finding identity | stdout/stderr | `repo/path:line — RULE_ID — reason_code`; no matched value |
| Claim state | `docs/INDEX.md` and claims report | `DONE`, `UNQUALIFIED`, `PARTIAL`, `BLOCKED`, etc. |
| Determinism | E2E artifact under `tmp/documentation-governance-e2e/<run-id>/` | SHA-256 of sanitized stdout and exit code |
| Coverage | CI test output | per-file Node lines/branches/functions percentages |
| Validation closure | root `validation.md` | append-only measured data and ✅/🔴/🟡 verdict |

The checker may write no report by default. The E2E harness may capture stdout
under `tmp/`; it must sanitize before any artifact is committed.

## Test matrix

All tests below are required names, not placeholders. A test that only checks
file existence is insufficient when the requirement is policy behavior.

| File | Named test | Expected proof | Cover |
| --- | --- | --- | --- |
| `tools/ci/check-documentation-governance.test.mjs` | `parity_matrix_has_one_canonical_source_per_category` | valid matrix passes | Node ≥80% production file |
| same | `parity_rejects_duplicate_objective` | duplicate objective exits 1 | same |
| same | `parity_rejects_missing_target` | missing source exits 1 | same |
| same | `reference_index_routes_required_objectives` | all required questions resolve | same |
| same | `reference_index_rejects_private_or_missing_target` | unsafe link exits 1 without echo | same |
| same | `claims_done_requires_all_evidence` | complete claim passes | same |
| same | `claims_partial_requires_blocker_and_next_proof` | incomplete environment claim stays partial | same |
| same | `claims_rejects_missing_owner_or_duplicate_slug` | malformed registry exits 1 | same |
| same | `claims_rejects_stale_artifact_and_missing_binary_match` | stale/identity gap cannot pass | same |
| same | `unqualified_impl_is_not_done` | IMPL without claim is `UNQUALIFIED` | same |
| same | `qualified_claim_is_the_only_done_path` | only evidence-backed claim is `DONE` | same |
| same | `provenance_rejects_private_path_without_echoing_match` | private path is refused and hidden | same |
| same | `provenance_rejects_secret_without_echoing_match` | secret is refused and hidden | same |
| same | `provenance_allows_sanitized_fixture` | redacted fixture passes | same |
| same | `allowlist_requires_owner_review_and_expiry` | incomplete exception refuses | same |
| same | `allowlist_cannot_cover_an_entire_directory` | broad scope refuses | same |
| same | `baseline_entry_requires_content_hash_and_expiry` | migration exception is bounded | same |
| same | `changed_file_cannot_use_legacy_baseline` | changed content is never hidden | same |
| same | `redundancy_reports_long_duplicate_normative_block` | pair and reason are reported | same |
| same | `redundancy_does_not_rewrite_fixture_bytes` | input bytes unchanged | same |
| same | `journey_manifest_accepts_seeded_bounded_run` | valid smoke record passes | same |
| same | `journey_manifest_rejects_implicit_clock_sleep_only_and_missing_cleanup` | all three refusal classes fail | same |
| same | `journey_manifest_rejects_duplicate_version_missing_consumer` | duplicate/version/consumer gaps fail | same |
| same | `validation_entry_requires_before_action_after` | marked record requires states | same |
| same | `postmortem_effectiveness_requires_regression_threshold_and_revalidation` | effective action requires all proof | same |
| same | `postmortem_open_action_cannot_close` | open action remains open | same |
| same | `governance_check_is_deterministic` | two identical runs match exit/output hash | same |
| same | `governance_cli_is_read_only` | source scan finds no mutation command | same |
| same | `docs_check_includes_governance_gate` | shell order includes gate | same |
| same | `docs_check_is_read_only` | shell has no runtime mutation path | same |
| `tools/ci/check-validation-schema.test.mjs` | `governance_entry_requires_before_action_after` | schema-1 missing state fails | Node ≥80% production file |
| same | `governance_entry_requires_refusals_measurement_and_rollback` | missing evidence fields fails | same |
| same | `legacy_validation_entries_remain_accepted` | existing allowlisted history remains readable | same |
| same | `validation_log_remains_append_only` | removal/modification remains non-zero | same |

The canonical Node coverage commands are:

```bash
node --test --experimental-test-coverage \
  --test-coverage-include=tools/ci/check-documentation-governance.mjs \
  --test-coverage-lines=80 --test-coverage-branches=80 \
  --test-coverage-functions=80 \
  tools/ci/check-documentation-governance.test.mjs

node --test --experimental-test-coverage \
  --test-coverage-include=tools/ci/check-validation-schema.mjs \
  --test-coverage-lines=80 --test-coverage-branches=80 \
  --test-coverage-functions=80 \
  tools/ci/check-validation-schema.test.mjs
```

There is no Rust crate or Rust production file in this slice; the canonical
Rust slice-coverage command is therefore `N/A — no Rust business-logic file`.
The Node per-file thresholds above are the applicable business-logic gate.

## Implementation order

`ITEM-1…ITEM-9` is a hard order. Each item compiles/parses, runs its named
tests, and confronts the relevant PRD matrix before the next item starts.

| ITEM | Work | Required early proof |
| --- | --- | --- |
| ITEM-1 | Add the machine-readable claims, allowlist, journey smoke record, and short governance README; add the parity and reference documents with only pointers. | JSON/Markdown fixtures parse; all required rows/objectives exist; no private/foreign content. |
| ITEM-2 | Implement table parsing and parity/reference validation in `check-documentation-governance.mjs`. | `parity_*` and `reference_*` tests; broken-link check. |
| ITEM-3 | Implement claims loading/validation and modify index status derivation to require a qualifying claim. | Fabricated `DONE`, missing evidence, stale artifact, and unqualified-IMPL refusals; regenerated index shows no unqualified `DONE`. |
| ITEM-4 | Implement bounded provenance/sanitization scanning and allowlist validation. | Secret/private-path/KASLR fixtures fail with sanitized diagnostics; exact input bytes unchanged. |
| ITEM-5 | Implement bounded duplicate-normative-block detection. | Duplicate pair is reported; short/common/code blocks are not false positives; no rewrite occurs. |
| ITEM-6 | Implement deterministic journey manifest validation and the smoke record consumer contract. | Seed, clock, timeout, refusal, reporter, artifact, consumer, and cleanup refusal matrix passes. |
| ITEM-7 | Extend validation schema and add postmortem effectiveness parsing. | New marked records are strict; old entries are unchanged; effective closure requires regression and both paths. |
| ITEM-8 | Wire `scripts/docs-check.sh`, CI Node tests/coverage, and existing checks in fail-closed order. | Local and CI command sequences produce the same exit/output for the same checkout. |
| ITEM-9 | Run the real documentation E2E, append sanitized validation, regenerate/check index, and confront rollback/GO-NO-GO. | Before/action/after evidence, positive and refusal exits, deterministic hash, zero sensitive output, and clean expected-file diff. |

No item may introduce a runtime/host mutation or claim platform completion.

## GO/NO-GO readiness (audit 2.5)

This is the pre-implementation readiness gate; it does not create
`AUDIT-2.5.md` in this step.

### GO requires

- Every PRD RF/NFR row maps to an ITEM and a named test or explicit N/A reason.
- The file matrix above is complete; no implementer decision remains about
  ownership, claim state, schema fields, scope, output redaction, or rollback.
- `node --check tools/ci/check-documentation-governance.mjs` and
  `node --check tools/ci/check-validation-schema.mjs` pass after implementation.
- All named Node tests pass with each production file at least 80% lines,
  branches, and functions.
- `node tools/ci/check-documentation-governance.mjs --all` returns exit 0 on
  the legitimate repository scope and exit 1 for every required refusal fixture.
- `./scripts/docs-check.sh` returns exit 0, and
  `node tools/generate-docs-index.mjs --check` is synchronized.
- Two identical runs have identical sorted sanitized output, exit code, and
  finding counts.
- `validation.md` has an appended schema-1 record containing measured command
  exits, counts, output hash, refusal outcomes, limitations, and rollback
  trigger. No prior line was edited.
- `git diff --name-only` contains only the approved matrix files and the
  append-only/generated files listed above; no runtime or host artifact is
  present.
- `BINARY_MATCH`: `N/A — no daemon/driver/runtime surface in this SPEC`.

### NO-GO conditions

- Any missing named test, coverage threshold, evidence/refusal case, or
  repository-relative link.
- Any `DONE` state derived solely from `IMPL.md`, prose, a static test, or a
  different surface's evidence.
- Any scanner output containing a matched secret, identity, private path, or
  raw address.
- Any unbounded wait, shell/PowerShell payload in a manifest, automatic
  rewrite, broad allowlist, or changed historical record.
- Any nondeterministic output, unexpected file mutation, failed index/link/
  gap/public-hygiene gate, or missing rollback trigger.
- Any environment-bound limitation represented as `DONE` instead of
  `PARTIAL`/`BLOCKED`.

## Live E2E protocol for this slice

The live operator surface is documentation tooling, so no Windows, WSL2,
daemon, LKM, or physical drill is valid or required here.

### Before

```bash
git status --short --untracked-files=all
node --version
node tools/ci/check-documentation-governance.mjs --all
```

Record the baseline exit code, file count, finding count, and sanitized output
hash under `tmp/documentation-governance-e2e/<run-id>/before.txt`. The baseline
must be a legitimate current-tree run; it is not a product readiness claim.

### Action

```bash
./scripts/docs-check.sh
node tools/ci/check-documentation-governance.mjs --all
node --test tools/ci/check-documentation-governance.test.mjs
node --test tools/ci/check-validation-schema.test.mjs
```

Run the manufactured positive and refusal cases through the exported test
entry points or the documented fixture mode. The CLI must not execute any
manifest action. Capture only sanitized output in
`tmp/documentation-governance-e2e/<run-id>/action.txt`.

### After

Require all of:

- legitimate gate exit `0`, refusal cases exit `1`, usage errors exit `2`;
- `FINDINGS=0` for the accepted scope;
- two identical accepted runs have equal output SHA-256 and counts;
- no sensitive value appears in stdout/stderr or artifacts;
- expected documentation-only files changed and no runtime/host state changed;
- generated index and broken-link/public-hygiene/gap gates pass;
- validation record has the measured before/action/after state and verdict.

This live proof is `E2E ✓` for the documentation-governance surface only. It
does not close or alter any Windows, Linux, WSL2, driver, daemon, benchmark,
or host gap.

## Living docs

| Document | Action |
| --- | --- |
| `docs/INDEX.md` | Regenerate/check after claim and feature files land; never hand-edit. |
| `docs/DOCUMENTATION-PARITY.md` | Create canonical ownership matrix in ITEM-1. |
| `docs/reference/REFERENCE-INDEX.md` | Create objective router in ITEM-1. |
| `docs/governance/README.md` | Create short command/schema pointer in ITEM-1. |
| `docs/postmortems/TEMPLATE.md` | Add schema-1 effectiveness fields in ITEM-7. |
| `validation.md` | Append one measured close record in ITEM-9. |
| `docs/BENCHMARKS.md` and `docs/benchmarks/results.jsonl` | N/A — no performance decision or runtime benchmark. |
| `docs/reliability/GAP-REGISTER.md` | Update only if the governance claim itself has evidence-backed state; do not close product gaps. |
| `.claude/rules/documentation.md` / `docs/SSDV3-PROMPTS.md` | N/A unless implementation reveals a repository-wide contract change; any such change requires a new SPEC decision. |
