# SPEC — Comment-language integrity

> SSDV3 Step 3 activation (2026-08-09). This SPEC closes the language-gate
> contract for RamShared comments and human-readable repository text and now
> authorizes its bounded, read-only implementation plus eleven English-only
> mutable migration batches. It does not alter protected records.

## Step 3 activation record

This revision changes the execution state, not the language-policy contract:

| ITEM | Step 3 state | Authorized result in this change |
| --- | --- | --- |
| ITEM-1 — Contract | complete | Reconfirm the dated inventory, exact allowlist, protected paths, and read-only frontier. |
| ITEM-2 — Pure scanner | in scope | Implement the deterministic sanitized scanner and every named extraction, bounds, ordering, and read-only test in this SPEC. |
| ITEM-3 — Diff and path gate | in scope | Implement exact diff selection, base/path refusals, localized allowlist handling, protected-history refusal, CLI exit statuses, and the workflow invocation. |
| ITEM-4 — Ratchet and coverage | partial | Implement the strict schema/parser, base-record comparison, batch limits, named tests, coverage gate, and eleven bounded English migration batches. Materialize a repository baseline and activate the workflow only after protected-inventory stabilization. |

### DT-9 decision closure

DT-9 is closed as a ratchet design and parser decision, but it is not yet an
active repository baseline. The eventual ratchet record is exactly
`tools/ci/comment-language-baseline.json`; its contract is expressed by
`tools/ci/comment-language-baseline.schema.json` and enforced by the
no-dependency parser. The exact schema has only a
format identifier, a non-negative revision, one fixed review-role declaration,
an immutable initial snapshot, and a current snapshot. A snapshot consists
only of aggregate mutable/protected counts and a SHA-256 digest of the bounded
protected inventory. It cannot contain a path list, line list, marker,
exception, allowlist, waiver, or free-form reason; unknown keys fail closed.

The approving owner is the `repository-maintainer` role. Each baseline update
must be approved through the repository's normal pull-request review before
merge. CI validates that this exact role and review channel are declared; it
does not fabricate a claim that a human review occurred or mutate remote
branch-protection settings.

`--ratchet <base>` requires the fixed record to exist in the validated base
commit, then scans the working tree and validates its working-tree record. An
edited record in a pull request cannot mask a regression: its immutable initial
snapshot and review declaration must match the base record; its current snapshot
must exactly match the scan; a changed snapshot must be a strict, bounded
decrease from the base current snapshot. The protected inventory count and
digest must remain unchanged for a ratchet update. The independent `--diff
<base>` gate still rejects every new language finding before this aggregate
check runs.

No repository baseline is created or activated in this change. Protected
records are still receiving legitimate qualification updates, so pinning a
fingerprint now would turn honest drift into a false regression. The dated
2026-08-09 measurements remain candidate observations only; no protected
digest is hard-coded into the production checker. Isolated fixtures exercise
the one-time bootstrap transition. After the protected inventory stabilizes, a
maintainer-reviewed bootstrap PR must record a fresh snapshot; only a later PR
whose base already contains that record may activate the workflow ratchet.
Until then, a missing base record is a configuration error and the active
`--diff` gate remains the no-new-debt control.

## Closed scope

### In now

- The canonical language is English for mutable RamShared source, comments,
  source messages, scripts, workflows, and structural documentation.
- Portuguese is allowed only in the exact localized paths
  `README.pt-BR.md` and `docs/pt-BR/README.md`.
- Existing `tools/ci/check-comment-language.mjs` is the implementation anchor;
  its current two-marker, leading-comment behavior is recorded as a legacy
  gap, not accepted as the final contract.
- The scanner contract is read-only, bounded, deterministic, sanitized, and
  split into a full inventory mode, an added-line diff mode, and a base-ref
  ratchet-validation mode.
- The measured candidate observations and protected history/evidence boundary
  in [`PRD.md`](PRD.md) are inputs for this bounded implementation, not an
  activated ratchet baseline.
- `tools/ci/check-comment-language.mjs`, its named Node test module, the
  strict ratchet schema/parser, and the existing diff-gate workflow may be
  modified to implement ITEM-2 through ITEM-4 at the read-only frontier.
- Ten named English-only migration batches are in scope after the ratchet test
  suite is green. Each may touch at most 10 mutable files and 100 candidate
  lines, and must not touch a protected path. A protected-inventory fingerprint
  is not declared until the stabilization PR.

### Out now

- Editing or translating code, comments, source strings, or scripts outside
  the named migration batches; or editing validation, the docs index,
  governance claims, evidence, history, or IMPL.
- Editing `README.pt-BR.md`, `docs/pt-BR/README.md`, or the existing
  documentation-localization slice.
- Adding a suppression file, a per-path/line exception, a waiver, a second
  scanner, a translation service, or an automatic rewrite path.
- Runtime, kernel, driver, daemon, host, GPU, swap, disk, network, or external
  service behavior.

### Assumed-ready dependencies

- Node 22 built-ins, the repository's existing `git` checkout, and the current
  checker path.
- The existing pull-request workflow remains the owner of the diff invocation;
  this Step 3 change updates only that invocation to the implemented exact CLI
  contract.
- The documentation-localization checker remains the owner of localized source
  freshness, reciprocal links, portal objectives, and non-normative policy.
- The current checkout metrics in [`PRD.md`](PRD.md) are dated candidate
  observations. The future bootstrap record must be measured only after the
  protected inventory stabilizes; it must never be inferred from these values.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | DT-1, ITEM-3 |
| RF-2 | DT-2, DT-3, ITEM-2 |
| RF-3 | DT-4, ITEM-2 |
| RF-4 | DT-5, ITEM-3 |
| RF-5 | DT-6, DT-9, ITEM-4 |
| RF-6 | DT-7, ITEM-2, ITEM-4 |
| RF-7 | DT-8, ITEM-1 |
| NFR-1 | DT-2 |
| NFR-2 | DT-3, DT-5 |
| NFR-3 | DT-4, DT-8 |
| NFR-4 | DT-7 |
| NFR-5 | DT-1, DT-8 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Use English as the only mutable repository language. Permit Portuguese only at the exact two localized paths; a new `*.pt-BR.md` path is rejected until explicitly selected. | An exact allowlist prevents locale drift and competing technical authorities. |
| DT-2 | Enumerate sorted repository-relative tracked text, with a 512 KiB per-file and 2,000-path run bound. Skip binary payloads and reject invalid/over-limit input. | The current 621-path checkout is bounded without scanning evidence blobs or invoking a language service. |
| DT-3 | Keep the current marker vocabulary as reviewed machine data, add accent-aware and single high-confidence detection, and extract leading, multiline, and trailing comments without evaluating source. Scan declared human-readable text extensions; do not infer language from filenames. | It closes the current two-marker false-negative without introducing an AST or probabilistic classifier. |
| DT-4 | Findings contain only `path`, `line`, `rule`, and `scope`; no source line, marker, excerpt, absolute path, environment value, or diff payload is retained or printed. | Sanitization is a correctness and privacy boundary, not a presentation preference. |
| DT-5 | `--diff <base>` selects only added line numbers from a validated git base ref. It fails on new findings outside the allowlist, including new additions under protected history paths. | Existing legacy can be migrated without allowing new debt; protected history is never rewritten. |
| DT-6 | `--all` reports mutable and protected counts separately. Mutable findings ratchet down in batches of at most 10 files or 100 lines and terminate at zero; protected history/evidence is inventory-only. | The migration is finite and observable while honoring the no-translation rule for evidence/history. |
| DT-7 | Refactor the current module behind pure exports and require per-file Node coverage of at least 80% for lines, branches, and functions. | Named tests and file-level coverage catch scanner blind spots that workspace averages hide. |
| DT-8 | This Step 3 change modifies only the scanner, its named test module, the strict schema, the existing comment-language workflow, this SPEC, eleven bounded mutable English batches, and the exact DT-11/DT-12 map/planner tests; it preserves the read-only frontier. | The bounded implementation closes ITEM-2 and the implemented portion of ITEM-4 without translating or altering protected records. |
| DT-9 | Define exactly one eventual aggregate-only ratchet record at `tools/ci/comment-language-baseline.json`, constrained by the adjacent strict schema and no-dependency parser. The record declares the `repository-maintainer` pull-request-review protocol, has no suppression-capable field, and is compared with the record from the validated PR base by `--ratchet <base>`. No record is materialized while protected inventory is changing; after stabilization, every update must match the current scan, preserve protected digest/counts, and be a strict decrease of at most 10 files or 100 lines. | A base-anchored, aggregate-only record makes progress reviewable without turning legacy findings into permanent per-path permissions. |
| DT-10 | In `--all`, an invalid-UTF-8 body under the exact protected-history/evidence classes is treated as opaque historical evidence and excluded from content inventory. Invalid UTF-8 outside that exact class, an over-limit/read/path error anywhere, or an invalid protected file selected by `--diff` returns exit 2. | Immutable binary evidence is explicitly not a language input, while changed or mutable text must fail closed rather than becoming an unreviewed exception. |
| DT-11 | The seven measured Rust paths listed in the canonical coverage-owner section receive exactly one `rust-line-coverage` owner at 80%, with the command and package/file argv tokens aligned between this SPEC and `docs/governance/rust-slice-coverage.json`. The former lower-coverage production paths listed there have exact feature-SPEC owners; zero paths from this historical set remain low-coverage or unmapped, and neither set receives a localization exemption. | Measured per-file line coverage can close only the paths that meet the canonical threshold, while preserving DT-18's fail-closed ownership rule for every future unmapped business path. |
| DT-12 | `crates/ramshared-cuda/src/lib.rs` may receive the one exact DT-24 `rust-test-only-localization-differential` owner declared below, but only if the planner proves from a full immutable base that every non-comment difference is inside the declared root `#[cfg(test)] mod tests` region and the projected production text is otherwise identical. The owner runs the exact package `cargo test -p ramshared-cuda --lib` command and requires the already-recorded ignored GPU test evidence from the immutable base; it is never a line-coverage or N/A exemption. The WSL2 backend is intentionally absent from this no-behavior-change owner and remains owned by memory-broker. | This measured-low CUDA file changed only test diagnostics, so a fail-closed test-boundary proof preserves the production coverage rule without treating test text as production behavior. |
| DT-13 | `--diff` permits one opaque protected-file public-provenance redaction only when the current first line is exactly the ASCII placeholder `'<repo-root>'`, the base first line is a private WSL repository root, and every byte after the first line is identical between base and current. Any second-line change, different placeholder, non-private base line, missing base blob, or invalid UTF-8 protected change remains exit 2. The exception produces no language finding because it validates bytes rather than decoding historical text. | A public repository must remove a private root from legacy OEM-encoded evidence without translating or re-encoding the historical payload. A byte-exact suffix proof keeps DT-10 fail-closed for every broader rewrite. |

## Scanner contract

### Corpus and path policy

The implementation shall use POSIX repository-relative paths from a
fixed `git ls-files -z` result for `--all`. It shall sort paths before reading
them and reject absolute paths, `..` traversal, NUL bytes, symlink escapes, and
unsupported file arguments. It shall skip binary data after a bounded NUL
probe and decode only valid UTF-8 text.

The policy sets are exact:

```text
localized_allowlist = {
  README.pt-BR.md,
  docs/pt-BR/README.md,
}

protected_history = {
  CHANGELOG.md,
  validation.md,
  docs/**/evidence/**,
  docs/postmortems/**,
  docs/reliability/**,
}

protected_impl = { docs/specs/**/IMPL.md }

machine_policy_data = { tools/ci/check-comment-language.mjs }
```

The scanner excludes the localized allowlist from language findings but still
lets the documentation-localization gate validate those files. It scans valid
UTF-8 protected history for inventory visibility but does not include it in
the mutable exit decision. Under DT-10, an invalid-UTF-8 protected historical
body is opaque evidence in `--all`; it is not a language input or a mutable
exception. A Portuguese line added in a protected path is a diff finding and
blocks the change; no historical line is rewritten.

`machine_policy_data` is an exact, narrow exception for the checker's marker
table; it is not a localized path and is not available to any other file. The
test module must construct candidate fixtures without embedding prose, so it
does not need a second data exception.

The declared source extensions remain `.rs`, `.c`, `.h`, `.ps1`, `.sh`, and
`.mjs`. The declared prose extensions are `.md`, `.txt`, `.yaml`, `.yml`,
`.toml`, `.json`, and `.svg` when they contain human-readable text. Binary
evidence, generated result payloads, and opaque fixtures are not language
inputs. The exact scanner implementation may narrow a prose extension only by
recording the reason in its tests; it may not silently expand the path
allowlist.

### Extraction

The current symbols `isCommentLine`, `cleanCommentText`, `getPtMarkers`,
`checkFile`, and `getDiffAddedLines` are the refactor anchors. The module
shall expose pure equivalents and guard CLI execution behind `main()`.

- Leading line comments, block-comment continuation lines, and trailing
  comments are normalized without executing the surrounding code.
- Human-readable text is scanned as UTF-8 lines after Unicode normalization;
  identifiers, hashes, URLs, and machine-only keys are not treated as prose by
  themselves.
- A high-confidence marker is sufficient for a candidate; low-confidence
  stopwords require two distinct markers on the same extracted text unit. The
  marker vocabulary is data, not diagnostic content.
- The scanner does not claim to prove a natural language. Its deterministic
  result is a migration candidate that is reviewed by path owner. A mutable
  candidate is removed by an English rewrite, not by an inline suppression.
- The checker source's marker table and sanitized temporary test fixtures are
  machine data. They are covered by explicit tests and are not treated as
  user-facing Portuguese content.

### Finding and exit contract

The only finding shape is:

```json
{"path":"relative/file.ext","line":12,"rule":"LANG-PT-001","scope":"comment"}
```

`scope` is one of `comment`, `source-text`, or `document`. The implementation
must not add `content`, `markers`, `excerpt`, `absolute_path`, or environment
fields. Findings sort by `path`, numeric `line`, `rule`, then `scope`.

The CLI accepts exactly `--all`, `--diff <base>`, or `--ratchet <base>`, plus
no other arguments. `--ratchet` reads the fixed baseline path from the
working tree and, after validating `<base>`, from that base tree. It never
writes either record.
It returns:

| Exit | Meaning |
| ---: | --- |
| 0 | No new diff findings, a full inventory with zero mutable findings, or a valid ratchet transition |
| 1 | Policy finding, mutable inventory remains, or ratchet violation |
| 2 | Usage/configuration error, invalid base, invalid text, path escape, or resource bound |

`--all` emits only sanitized findings and aggregate counts. A full inventory
may show protected findings while returning 0 when the mutable count is zero.
The summary fields are stable: `mutable_files`, `mutable_lines`,
`protected_files`, `protected_lines`, and `finding_count`. No source content is
part of either the summary or the finding stream.

### Ratchet record and transition contract

The schema is versioned as `ramshared-comment-language-ratchet/v1` and has
these exact top-level keys:

```text
$schema, schema, revision, approval, initial, current
```

`approval` has exactly `approver_role=repository-maintainer` and
`channel=pull-request-review`. `initial` and `current` have exactly
`mutable_files`, `mutable_lines`, `protected_files`, `protected_lines`,
`protected_paths`, and `protected_inventory_sha256`. The protected digest is
the SHA-256 of sorted protected inventory entries, each represented by its
repository-relative path and the SHA-256 of its bounded file bytes. The
protected inventory is bounded to 2 MiB total; exceeding that bound fails
closed. The digest is a comparison value only and is not an input exception.

No bootstrap snapshot is compiled into the production checker. The dated
2026-08-09 values are candidate observations, not a trusted digest. The
one-time bootstrap transition is exercised only with isolated fixture data
until a stabilized, maintainer-reviewed repository record exists.

For an unchanged PR, the working `current` snapshot and revision equal the
base record and must match the fresh current scan. For a migration PR, the
working revision is exactly base revision plus one, `initial` and `approval`
are byte-for-byte equivalent in their parsed value, both mutable dimensions
do not grow, at least one decreases, the decrease is at most 10 files and 100
lines, and protected counts, paths, and digest equal the base current snapshot
and the fresh scan. The production CLI refuses a missing base record with a
configuration error; it never lets a pull request self-bootstrap the policy. A
current snapshot with zero mutable findings is the terminal state; it is not a
reason to add a suppression.

## Atomicity and rollback

### Atomicity frontier

The implementation frontier is the read-only Node process and its repository
inputs. It reads source bytes and git metadata, produces stdout/stderr, and
exits. It never writes a baseline, translation, evidence artifact, working
tree, host state, or remote service.

### Rollback by frontier

| Frontier | Policy |
| --- | --- |
| Scanner and tests | Revert to the last deterministic contract and passing test set; preserve repository bytes. |
| Source/script/document migration | Revert only the bounded English batch; do not revert or rewrite evidence/history. |
| Kernel/module or Windows driver | N/A — no runtime or driver files are in scope. |
| Host/persistent state | N/A — no daemon, swap, disk, VRAM, SCM, or host operation is callable. |

Rollback trigger: one new mutable finding passes the diff gate, the mutable
baseline grows, a batch exceeds 10 files or 100 lines, protected history is
rewritten, diagnostics echo content, or the process writes/calls network or
host state. Revert the scanner/migration change to the last passing state and
leave protected bytes untouched.

## Kahneman map

| ITEM / stage | # | Question | Minimum evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 extraction | #9 — number before adjective | Does the scanner cover one-marker, trailing, and multiline cases rather than only the happy comment shape? | `single_high_confidence_marker_is_reported`, `leading_and_trailing_comments_are_scanned`, `multiline_comment_state_is_bounded` | Any known legacy fixture passes silently |
| ITEM-2 diagnostics | #16 — fail-safe curator | Can a finding reveal source text or private values? | `sanitized_finding_omits_source_and_marker_text` | Any content echo |
| ITEM-3 diff selection | #13 — refusal plus legitimate | Does the gate refuse new Portuguese while accepting an unchanged legacy baseline and the exact localized paths? | `diff_reports_added_lines_only`, `localized_allowlist_is_accepted`, `new_protected_history_line_is_rejected` | New non-localized line passes |
| ITEM-4 ratchet | #17 — idempotent operation | Does the fixture ratchet reduce mutable findings and refuse protected drift without allowing a pull request to self-bootstrap? | `ratchet_rejects_growth_and_accepts_strict_decrease`, `ratchet_uses_base_record_not_pr_record`, `ratchet_refuses_protected_inventory_drift`, and `ratchet_requires_anchored_base_record_before_activation` | Count grows, repeats, protected bytes change, or a missing base passes |
| ITEM-4 final gate | #18 — right-layer sunset | Is zero reached by English migration rather than a new suppression layer? | `batch_limit_rejects_more_than_ten_files_or_hundred_lines`, `zero_mutable_findings_is_terminal_state`, and strict schema test | Any mutable exception remains or suppression is added |

## Security checklist (pre-implementation)

- [x] Privilege: N/A — regular repository reads only.
- [x] User/host copy: N/A — no user buffers, kernel APIs, or host devices.
- [x] Flags and arguments: unknown arguments, unsafe base refs, and extra paths
  return exit 2.
- [x] Path traversal: repository-relative normalization rejects absolute,
  parent, NUL, and symlink-escape paths.
- [x] Information leak: findings and summaries omit source, markers, secrets,
  absolute paths, and environment values.
- [x] Resource bounds: file, path, finding, and binary-probe limits are fixed;
  DT-10 limits the opaque-evidence exception to invalid UTF-8 in unchanged
  protected inventory; DT-13 permits only a byte-exact first-line private-root
  redaction in diff mode.
- [x] Runtime safety: no shell pipeline, network, daemon, driver, GPU, swap,
  disk, reboot, or host mutation.
- [x] Replayable ops: repeated runs over identical bytes and base ref are
  deterministic and read-only.
- [x] Protected records: evidence, history, reliability, and IMPL files are
  not rewritten or translated; additions are rejected by the diff gate.

## Files to CREATE / MODIFY / DELETE

### Existing documentation records

**`docs/specs/no-milestone/comment-language-integrity/PRD.md`**

- Purpose: measured requirements, language boundary, migration target, and
  resource/safety limits. It is not translated or otherwise changed by Step 3.
- RF / DT: RF-1..RF-7; DT-1..DT-10.
- Required validation: scoped Markdown-link and syntax checks.
- Cover target: N/A — documentation only.

**`docs/specs/no-milestone/comment-language-integrity/SPEC.md`**

- Purpose: surgical scanner, path, diff, sanitization, ratchet, test, and
  rollback contract. Step 3 changes only its activation state, DT-9, and
  DT-11/DT-12/DT-13.
- RF / DT: RF-1..RF-7; DT-1..DT-13.
- Required validation: scoped Markdown-link and syntax checks.
- Cover target: N/A — documentation only.

### MODIFY — bounded Step 3 implementation

**`tools/ci/check-comment-language.mjs`**

- Purpose: implement the pure scanner and CLI contract above while preserving
  the read-only boundary.
- Types / functions: refactor `isCommentLine`, `cleanCommentText`,
  `getPtMarkers`, `checkFile`, `getDiffAddedLines`, and `main` into tested
  exports; no finding retains source content.
- Required tests: `tools/ci/check-comment-language.test.mjs` :: every
  non-blocked name in the matrix below.
- Cover target: ≥80% lines, branches, and functions for this file.

**`tools/ci/check-comment-language.test.mjs`**

- Purpose: sanitized temporary fixtures and CLI tests only; no repository
  content is copied into diagnostics.
- Cover target: tests the production file at ≥80% lines, branches, and
  functions; test code itself is not used to claim production coverage.

**`.github/workflows/comment-language.yml`**

- Purpose: runs the exact `--diff origin/<base>` gate. It must not invoke
  `--ratchet` until the reviewed baseline is already present in the PR base.
- Change boundary: no new permissions, network service, or migration rewrite.
- Cover target: N/A — workflow-only.

**`docs/governance/rust-slice-coverage.json`**

- Purpose: add the one exact `rust-line-coverage` ownership entry declared in
  DT-11 and the exact DT-12 test-only ownership entry; neither is a generic
  exemption and neither owns the separately feature-owned production paths.
- Required validation: `tools/ci/plan-rust-slice-coverage.test.mjs` ::
  `comment_language_measured_rust_files_keep_exact_ownership_boundaries`.
- Cover target: N/A — declarative map; the canonical command gates each named
  production file at ≥80% line coverage.

**`tools/ci/plan-rust-slice-coverage.test.mjs`**

- Purpose: prove the actual DT-11 map selects the seven covered paths while
  exact feature owners cover every former lower-coverage path; prove the
  actual DT-12 map binds the immutable-base proof; and prove DT-12 accepts
  only declared `cfg(test)` localization and refuses every spoofed or
  production difference.
- Required tests: `comment_language_test_only_localization_requires_immutable_base_proof`,
  `test_only_localization_differential_accepts_declared_cfg_test_change`, and
  `test_only_localization_differential_refuses_spoofed_or_production_change`.
- Cover target: exercises `tools/ci/plan-rust-slice-coverage.mjs` at ≥80%
  lines, branches, and functions.

**`tools/ci/plan-rust-slice-coverage.mjs`**

- Purpose: add the bounded DT-12 lexical projection and tokenized package-test
  runner; it must never execute an ignored GPU command.
- Required tests: `test_only_localization_differential_accepts_declared_cfg_test_change`
  and `test_only_localization_differential_refuses_spoofed_or_production_change`.
- Cover target: ≥80% lines, branches, and functions through Node's built-in
  runner.

### CREATE — ratchet schema

**`tools/ci/comment-language-baseline.schema.json`**

- Purpose: strict, English JSON Schema for the one aggregate-only ratchet
  record. It rejects every unknown key and contains no path, finding, marker,
  waiver, or suppression list.
- Required validation: the scanner's record parser and named schema-refusal
  test; no JSON-schema package or network service.
- Cover target: N/A — declarative data; parser branches are covered through
  `check-comment-language.mjs`.

### DEFERRED — aggregate-only ratchet record

**`tools/ci/comment-language-baseline.json`**

- Status: intentionally absent until protected-inventory stabilization.
- Future purpose: reviewed initial/current aggregate snapshot and exact
  maintainer-review protocol. It is manually updated only in the same pull
  request as a bounded English migration; the scanner never writes it.
- Future validation: `--ratchet <base>` reads the base record and rejects a
  snapshot that does not exactly match the working-tree scan.
- Cover target: N/A — declarative data.

### MODIFY — English migration batches

#### Batch 1

The Rust source batches below define linguistic migration scope only. They do
not create a Rust coverage exemption or prove behavior equivalence. Under
`ci-trust-and-release-integrity` DT-23, a listed source path remains blocked
by Rust slice ownership unless an immutable-base differential proves
byte-identical non-comment Rust text. A changed string literal, identifier,
wire token, control token, or whitespace outside a comment is semantic for
that contract and cannot be auto-classified as localization-only.

| Path | Allowed change |
| --- | --- |
| `crates/ramshared-agent/src/main.rs` | English-only CLI diagnostics, comments, and test text in Batch 1. The later public CLI contract and process tests are owned only by `memory-broker` ITEM-9 / DT-45; Batch 1 does not authorize that behavior change. |
| `crates/ramshared-cuda/src/lib.rs` | English-only comments and test text; no CUDA behavior change. |
| `crates/ramshared-wsl2d/src/canary_probe.rs` | English-only test text; no probe behavior change. |
| `crates/ramshared-wsl2d/src/backend.rs` | English-only comments and test text; no backend behavior change. |
| `crates/ramshared-wsl2d/src/conn.rs` | English-only comments, test text, and local identifier spelling in Batch 1. Later transport behavior and its coverage gate are owned only by `wsl2-cascade-swap`; Batch 1 does not authorize that behavior change. |
| `crates/ramshared-wsl2d/src/telemetry.rs` | English-only test text; no telemetry behavior change. |

#### Batch 2

| Path | Allowed change |
| --- | --- |
| `crates/ramshared-broker/src/arbiter.rs` | English-only test identifiers; no arbitration behavior change. |
| `crates/ramshared-broker/src/protocol.rs` | English-only telemetry comment and diagnostic text; no broker protocol behavior change. |
| `crates/ramshared-block/src/handshake.rs` | English-only handshake diagnostics and test text; no wire behavior change. |
| `crates/ramshared-block/src/protocol.rs` | English-only protocol diagnostic and test text; no wire behavior change. |

#### Batch 3

| Path | Allowed change |
| --- | --- |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | English-only cascade comments, diagnostics, and test text; no command or safety behavior change. Later bounded transport behavior and coverage are owned only by `cascade-transport-policy` ITEM-5 / DT-T1..DT-T6. |
| `crates/ramshared-cli/src/cascade/mod.rs` | English-only cascade comments and test text; no policy behavior change. |
| `crates/ramshared-cli/src/main.rs` | English-only CLI comments, diagnostics, and test text; no CLI contract change. Later lifecycle behavior and coverage are owned only by `cascade-lifecycle-observability`. |
| `crates/ramshared-wsl2d/src/broker_srv.rs` | English-only broker comments, diagnostics, and test text; preserve compatibility tokens and wire values. |
| `crates/ramshared-wsl2d/src/main.rs` | English-only daemon comments, diagnostics, and test text; preserve compatibility tokens and runtime behavior. |

#### Batch 4

| Path | Allowed change |
| --- | --- |
| `docs/decisions/ADR-0001-vram-cascade-tiering.md` | English-only canonical engineering prose; preserve code, identifiers, URLs, dates, historic measurements, and technical decision. |
| `docs/decisions/ADR-0002-rust-userspace-port.md` | English-only canonical engineering prose; preserve code, identifiers, URLs, dates, historic measurements, and technical decision. |
| `docs/decisions/ADR-0003-page-state-swap-safety.md` | English-only canonical engineering prose; preserve code, identifiers, URLs, dates, historic measurements, and technical decision. |
| `docs/decisions/ADR-0004-ublk-io-uring-crate.md` | English-only canonical engineering prose; preserve code, identifiers, URLs, dates, historic measurements, and technical decision. |
| `docs/decisions/ADR-0005-broker-protocol-jsonl.md` | English-only canonical engineering prose; preserve code, identifiers, URLs, dates, historic measurements, and technical decision. |
| `docs/decisions/README.md` | English-only ADR catalog prose and titles; preserve ADR numbers, status values, and link targets. |

#### Batch 5

| Path | Allowed change |
| --- | --- |
| `docs/LIBRARIES.md` | English-only canonical engineering prose and tables; preserve code, identifiers, URLs, dates, versions, measurements, technical decisions, and links. |

The following proposed documentation paths are explicitly excluded from Batch 5:

- `docs/reliability/BLACK-BOX-FORENSICS.md` (8 current candidates) and
  `docs/reliability/DEGRADATION-MATRIX.md` (16) remain protected by the exact
  `docs/reliability/**` history/evidence policy.
- `docs/BENCHMARKS.md` (9) is scanner-mutable but declares an append-only
  historical benchmark log; this batch must not rewrite it.

Neither a protected-path exception nor a scanner reclassification is authorized
here. A future policy decision must define a reviewed boundary before either
category can be translated.

#### Batch 6

| Path | Allowed change |
| --- | --- |
| `README.md` | English-only language-switch label; preserve its reciprocal target and all product content. |
| `docs/labs/WSL-KERNEL-LAB.md` | English-only canonical lab prose; preserve commands, identifiers, paths, and safety boundaries. |
| `docs/runbooks/FASE-B-KERNEL.md` | English-only canonical runbook prose; preserve commands, paths, technical meaning, and reboot boundary. |
| `docs/runbooks/REVIEW-ADR.md` | English-only canonical review-runbook prose; preserve links, thresholds, and postmortem boundary. |
| `docs/marketing/LAUNCH-KIT.md` | English-only canonical launch-index prose; preserve asset paths, links, and product-status limits. |
| `docs/marketing/posts/01-reddit-rust-en.md` | English-only operator instructions for the English-targeted post; preserve copy delimiters, URLs, and public post text. |
| `docs/localization/manifest.json` | Data-only refresh of the two exact `README.md` source SHA-256 values after the canonical label changes; preserve schema, policy, paths, localized content, and every non-hash field. |

Batch 6 has six translation paths and 31 scanner findings. The manifest refresh
has zero scanner findings and is a required canonical-README parity update, so
the batch changes seven mutable files, within the 10-file limit.

#### Batch 7

| Path | Allowed change |
| --- | --- |
| `docs/specs/no-milestone/broker-telemetry-reconciliation/PRD.md` | English-only canonical engineering prose; preserve RF/DT/test names, commands, identifiers, metrics, contracts, tables, links, and technical meaning. |
| `docs/specs/no-milestone/broker-telemetry-reconciliation/SPEC.md` | English-only canonical engineering prose; preserve RF/DT/test names, commands, identifiers, metrics, contracts, tables, links, and technical meaning. |
| `docs/specs/no-milestone/cascade-desktop-app/PRD.md` | English-only canonical engineering prose; preserve RF names, command/script identifiers, interfaces, tables, links, and technical meaning. |
| `docs/specs/no-milestone/kernel-native-language/PRD.md` | English-only canonical engineering prose; preserve policy identifiers, ADR references, technical terms, tables, links, and technical meaning. |
| `docs/specs/no-milestone/wsl2-cascade-boot/PRD.md` | English-only canonical engineering prose; preserve RF/NFR names, commands, identifiers, metrics, contracts, tables, links, and technical meaning. |

Batch 7 has five translation paths and 83 scanner findings, within the
10-file/100-finding-line limit. It excludes the protected broker telemetry
`IMPL.md`, validation, evidence, benchmarks, source, CI, host, runtime, and
all reboot or commit actions.

#### Batch 8

| Path | Allowed change |
| --- | --- |
| `docs/specs/no-milestone/kernel-vram-as-memory/PRD.md` | English-only canonical architecture prose; preserve architecture/refusals, RF/DT/test names, host-contract boundaries, commands, metrics, tables, URLs, upstream terminology, and technical meaning. |
| `docs/specs/no-milestone/mainline-vram-tiering/PRD.md` | English-only canonical architecture prose; preserve architecture/refusals, RF/DT/test names, host-contract boundaries, commands, metrics, tables, URLs, upstream terminology, and technical meaning. |
| `docs/specs/no-milestone/wsl2-native-vram-tier/PRD.md` | English-only canonical architecture prose; preserve architecture/refusals, RF/DT/test names, host-contract boundaries, commands, metrics, tables, URLs, upstream terminology, and technical meaning. |

Batch 8 has three translation paths and 90 scanner findings, within the
10-file/100-finding-line limit. It excludes runtime, source, CI, protected
records, validation, IMPL, evidence, benchmarks, host, reboot, and commit
actions.

#### Batch 9

| Path | Allowed change |
| --- | --- |
| `docs/specs/no-milestone/windows-swap-driver/SPEC.md` | English-only canonical engineering prose; preserve every RF/DT/ITEM/TestName, command, Windows/WDK/SCM/uAPI token, metric, matrix semantic, rollback trigger, evidence reference, and technical meaning. |

Batch 9 has one translation path and 95 scanner findings, within the
10-file/100-finding-line limit. It excludes the protected `IMPL.md`,
driver/source/scripts, validation, evidence, benchmarks, CI, host, VM, reboot,
and commit actions.

#### Batch 10

| Path | Allowed change |
| --- | --- |
| `scripts/kernel/boot-kernel-logged.ps1` | English-only human help text and log prose; preserve parameters, paths, command invocation, log framing, exit propagation, and PowerShell behavior. |
| `scripts/kernel/boot-kernel-safe.ps1` | English-only comments and human diagnostics; preserve `.wslconfig` syntax, parameters, cmdlets, `wsl --shutdown` boundary, recovery order, timeout values, exit behavior, and module command tokens. |
| `scripts/kernel/build-wsl-kernel.sh` | English-only comments and diagnostics; preserve kernel source/tag/config identifiers, command argv, build/install behavior, resource bounds, and exit behavior. |
| `scripts/kernel/qemu-broker-drill.sh` | English-only comments and human diagnostics; preserve shell/heredoc command text, broker/agent/NBD arguments, `KTEST-*` protocol labels, timeout values, verdict logic, and exit behavior. |
| `scripts/kernel/qemu-ublk-crash-e1.sh` | English-only comments and human diagnostics; preserve shell/heredoc command text, experiment parameters, `KTEST-*` protocol labels, timeout values, verdict logic, and exit behavior. |
| `scripts/kernel/qemu-ublk-crash-e1b.sh` | English-only comments and human diagnostics, including embedded C comments; preserve shell/heredoc/C behavior, experiment parameters, `KTEST-*` protocol labels, timeout values, verdict logic, and exit behavior. |
| `scripts/kernel/qemu-ublk-daemon.sh` | English-only comments and human diagnostics; preserve shell/heredoc command text, daemon argv, `KTEST-*` protocol labels, timeout values, teardown/verdict logic, and exit behavior. |
| `scripts/kernel/qemu-validate.sh` | English-only comments and human diagnostics; preserve shell/heredoc command text, kernel/module argv, `KTEST-*` protocol labels, timeout values, verdict logic, and exit behavior. |

Batch 10 has exactly eight translation paths and 40 scanner findings measured
against `HEAD`; the working-tree scanner reports 0 for those paths. It remains
within the 10-file/100-finding-line cap. It excludes live QEMU/kernel/WSL/host
execution, reboot, validation, IMPL, evidence, CI, remote changes, and commits.
Parser/static validation must prove the PowerShell and Bash syntax plus the
existing bounded/refusal contracts without executing a launcher, QEMU, kernel,
or host operation. Any machine-facing token, command, protocol label, exit
behavior, or experiment contract ambiguity stops the batch for a new decision.

#### Batch 11

| Path | Allowed change |
| --- | --- |
| `scripts/p0/measure-cascade-demote.sh` | English-only human comments and diagnostics; preserve cascade command argv, metric names, JSON shape/keys, status/refusal behavior, cleanup order, timeout values, and exit codes. |
| `scripts/p0/measure-nbd-tcp.sh` | English-only human comments and diagnostics; preserve NBD/TCP command argv, metric names, JSON shape/keys, bounds, cleanup/refusal behavior, and exit codes. |
| `scripts/p0/measure-net.sh` | English-only human comments and diagnostics; preserve network command argv, metric names, JSON shape/keys, bounds, cleanup/refusal behavior, and exit codes. |
| `scripts/p0/measure-psi-load.sh` | English-only human comments and diagnostics; preserve pressure command argv, metric names, JSON shape/keys, bounds, cleanup/refusal behavior, and exit codes. |
| `scripts/p0/measure-psi.sh` | English-only human comments and diagnostics; preserve PSI read command argv, metric names, JSON shape/keys, bounds, cleanup/refusal behavior, and exit codes. |
| `scripts/p0/measure-vram-headroom.sh` | English-only human comments and diagnostics; preserve GPU/VRAM command argv, metric names, JSON shape/keys, bounds, cleanup/refusal behavior, and exit codes. |

Batch 11 has exactly six translation paths and 17 scanner findings measured
against `HEAD`; it remains within the 10-file/100-finding-line cap. It excludes
live measurement, pressure, cascade/network/host execution, reboot,
validation, IMPL, evidence, CI, remote changes, and commits. Bash parser and
static checks must prove metric/JSON keys, command and exit/refusal boundaries,
cleanup, and bounded-operation controls without executing a measurement or
mutating pressure, network, cascade, GPU, or host state. Any machine-facing
token, metric/JSON contract, command, exit behavior, or safety-bound ambiguity
stops the batch for a new decision.

### Canonical Rust coverage owner for measured localization paths

The following exact command is the sole DT-11 owner for the seven Rust paths
whose fresh report already clears the per-file 80% line threshold. It is a
real coverage gate, not an exemption or a comment-only differential:

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-block,ramshared-broker,ramshared-wsl2d --files crates/ramshared-block/src/handshake.rs,crates/ramshared-block/src/protocol.rs,crates/ramshared-broker/src/arbiter.rs,crates/ramshared-broker/src/protocol.rs,crates/ramshared-wsl2d/src/broker_srv.rs,crates/ramshared-wsl2d/src/canary_probe.rs,crates/ramshared-wsl2d/src/telemetry.rs,crates/ramshared-broker/src/model.rs,crates/ramshared-broker/src/slices.rs,crates/ramshared-wsl2d/src/residency.rs --min 80 --report-json tmp/comment-language-rust-cov.json
```

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-config --files crates/ramshared-config/src/lib.rs,crates/ramshared-config/src/error.rs --min 80 --report-json tmp/config-management-core-cov.json
```

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-integrity --files crates/ramshared-integrity/src/hash.rs,crates/ramshared-integrity/src/pattern.rs --min 80 --report-json tmp/integrity-block-verification-cov.json
```

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-vram --files crates/ramshared-vram/src/lib.rs --min 80 --report-json tmp/vram-provider-abstraction-cov.json
```

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cuda --files crates/ramshared-cuda/src/probe.rs,crates/ramshared-cuda/src/driver.rs,crates/ramshared-cuda/src/ffi.rs --min 80 --report-json tmp/cuda-probe-planning-cov.json
```

| Covered path | Fresh measured line coverage |
| --- | ---: |
| `crates/ramshared-block/src/handshake.rs` | 95.8% |
| `crates/ramshared-block/src/protocol.rs` | 94.5% |
| `crates/ramshared-broker/src/arbiter.rs` | 99.5% |
| `crates/ramshared-broker/src/protocol.rs` | 96.0% |
| `crates/ramshared-broker/src/model.rs` | 96.2% |
| `crates/ramshared-broker/src/slices.rs` | 92.2% |
| `crates/ramshared-wsl2d/src/broker_srv.rs` | 88.8% |
| `crates/ramshared-wsl2d/src/canary_probe.rs` | 93.6% |
| `crates/ramshared-wsl2d/src/residency.rs` | 99.0% |
| `crates/ramshared-wsl2d/src/telemetry.rs` | 100.0% |
| `crates/ramshared-config/src/lib.rs` | 90.6% |
| `crates/ramshared-config/src/error.rs` | 100.0% |
| `crates/ramshared-integrity/src/hash.rs` | 86.2% |
| `crates/ramshared-integrity/src/pattern.rs` | 81.0% |
| `crates/ramshared-vram/src/lib.rs` | 95.0% |
| `crates/ramshared-cuda/src/probe.rs,crates/ramshared-cuda/src/driver.rs,crates/ramshared-cuda/src/ffi.rs` | 82.8% |

<!-- rust-slice-structural-contract-v1
{
  "schema_version": 1,
  "id": "integrity-block-verification-structural",
  "kind": "rust-structural-contract",
  "files": [
    "crates/ramshared-integrity/src/lib.rs"
  ],
  "verifications": [
    {
      "source": "crates/ramshared-integrity/src/lib.rs",
      "package": "ramshared-integrity",
      "cargo_test": [
        "cargo",
        "test",
        "-p",
        "ramshared-integrity",
        "--lib"
      ]
    }
  ]
}
-->

### Strict test-only Rust localization owner

DT-12 covers only the exact CUDA file below. Its owner is not line coverage:
it must lex the immutable-base and working-tree Rust source, recognize only the
declared root `#[cfg(test)] mod tests` spans, and prove that the production
projection differs only in comments. A malformed source, spoofed attribute,
unbalanced delimiter, undeclared path, changed production token/string/control
flow, or moved module header is a refusal.

The package tests are executable local checks. Each named GPU function must
carry adjacent `#[test]` and `#[ignore]` attributes. The ignored GPU commands
are historical live evidence already recorded as terminal `**PASS**` commands
in the immutable base's `validation.md`; the planner validates that record but
never reruns them. The WSL2 backend is intentionally absent from this
no-behavior-change owner; its production behavior and one-time ignored-test
relocation have independent memory-broker contracts.

<!-- rust-slice-test-only-localization-differential-v1
{
  "schema_version": 1,
  "id": "comment-language-rust-test-only-localization",
  "kind": "rust-test-only-localization-differential",
  "files": [
    "crates/ramshared-cuda/src/lib.rs"
  ],
  "verifications": [
    {
      "source": "crates/ramshared-cuda/src/lib.rs",
      "package": "ramshared-cuda",
      "test_module": "tests",
      "cargo_test": ["cargo", "test", "-p", "ramshared-cuda", "--lib"],
      "ignored_gpu_tests": [
        {
          "name": "gpu_roundtrip_256mib",
          "command": ["cargo", "test", "-p", "ramshared-cuda", "--", "--ignored", "--test-threads=1"],
          "evidence": "validation.md"
        }
      ]
    }
  ]
}
-->

```bash
cargo test -p ramshared-cuda --lib
```

No business path from this historical localization set remains outside a
line-coverage or test-only map entry. Every former lower-coverage production
path now has an exact feature owner; any future unmapped business path remains
blocked. DT-11 and DT-12 do not authorize an N/A row or a generic exemption.

The following former blocked paths have their own canonical feature owners.
They are not part of DT-11 or DT-12, and neither owner is a localization
exemption.

| Path | Canonical owner | Feature SPEC |
| --- | --- | --- |
| `crates/ramshared-agent/src/main.rs` | `memory-broker-agent-cli` | `docs/specs/no-milestone/memory-broker/SPEC.md` DT-45 / ITEM-9 |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | `cascade-transport-orchestration` | `docs/specs/no-milestone/cascade-transport-policy/SPEC.md` ITEM-5 / DT-T1..DT-T6 |
| `crates/ramshared-cli/src/main.rs` | `cascade-lifecycle-observability` | `docs/specs/no-milestone/cascade-lifecycle-observability/SPEC.md` |
| `crates/ramshared-wsl2d/src/conn.rs` | `wsl2-cascade-connection-transport` | `docs/specs/no-milestone/wsl2-cascade-swap/SPEC.md` |
| `crates/ramshared-wsl2d/src/main.rs` | `memory-broker-wsl2d-daemon` | `docs/specs/no-milestone/memory-broker/SPEC.md` ITEM-8 / DT-46; canonical 81.5% (2723/3340) report: `tmp/memory-broker-wsl2d-daemon-cov.json` |

### DELETE

None. Do not modify `README.md`, localized files, scripts other than the
scanner, validation, `docs/INDEX.md`, governance claims, evidence, history, or
any existing IMPL.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| Sanitized finding | CLI stdout | `path`, `line`, `rule`, `scope`; no source text |
| Mutable inventory count | CLI summary | Deterministic integer fields |
| Protected inventory count | CLI summary | Deterministic integer fields, non-blocking in `--all` |
| Exit status | CLI process | 0 clean, 1 policy, 2 configuration |
| Ratchet transition | Fixture and, after activation, CLI stdout | Sanitized pass/no-go code, revision, and aggregate counts; no content or path list |
| Ratchet batch size | Future reviewed baseline transition | Number of files and candidate lines; no content |

No dmesg, metric endpoint, daemon status, host state, or runtime artifact is
created by this slice. No ratchet record is persisted during this phase; once
materialized, the declarative record is the only persistent ratchet state and
the checker never writes it.

## Living docs

| Document | Action |
| --- | --- |
| `README.md` | N/A — canonical product text is protected in this slice |
| `README.pt-BR.md` | N/A — owned by documentation-localization-integrity |
| `docs/pt-BR/README.md` | N/A — owned by documentation-localization-integrity |
| `docs/INDEX.md` | Defer — user explicitly forbids index changes in this slice |
| `validation.md` | N/A — append-only history is protected |
| `docs/governance/claims.json` | N/A — claims are not touched |
| `docs/**/evidence/**` and existing `IMPL.md` | N/A — protected, no translation or rewrite |
| `.claude/rules/*`, `CLAUDE.md`, `AGENTS.md` | N/A — existing policy is audited, not synchronized here |

## Implementation order

`ITEM-1…ITEM-4` is a hard order for this bounded Step 3 implementation.

1. **ITEM-1 — Contract:** review this PRD/SPEC; confirm the dated inventory,
   exact allowlist, protected paths, and no-modification boundary.
2. **ITEM-2 — Pure scanner:** add sanitized extraction, resource limits,
   deterministic ordering, and named RED/GREEN fixtures without touching
   repository content.
3. **ITEM-3 — Diff and path gate:** implement validated base selection, exact
   localized allowlist, protected-history refusal, and exit statuses.
4. **ITEM-4 — Ratchet and coverage:** add named RED tests, then implement the
   strict schema/parser/CLI transition. Run per-file coverage and make only the
   named bounded English migration batches after the ratchet fixtures are green.
   Defer baseline materialization and workflow activation until a stabilized
   protected inventory can be maintainer-reviewed.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `tools/ci/check-comment-language.mjs` | `check-comment-language.test.mjs` :: `single_high_confidence_marker_is_reported` | unit | #9 | ≥80% |
| same | `leading_and_trailing_comments_are_scanned` | unit | #9 | ≥80% |
| same | `multiline_comment_state_is_bounded` | unit | #9 | ≥80% |
| same | `english_comment_and_identifier_are_ignored` | unit | — | ≥80% |
| same | `localized_allowlist_is_accepted` | unit | #13 | ≥80% |
| same | `forbidden_localized_path_is_rejected` | unit | #13 | ≥80% |
| same | `new_protected_history_line_is_rejected` | unit | #13 | ≥80% |
| same | `diff_reports_added_lines_only` | integration | #13 | ≥80% |
| same | `sanitized_finding_omits_source_and_marker_text` | unit | #16 | ≥80% |
| same | `output_is_sorted_and_deterministic` | integration | #17 | ≥80% |
| same | `resource_bound_and_invalid_utf8_fail_closed` | unit | #16 | ≥80% |
| same | `opaque_protected_private_root_redaction_is_byte_exact` | integration/refusal | #13/#16 | ≥80% |
| same | `invalid_cli_argument_and_base_return_two` | integration | — | ≥80% |
| same | `ratchet_rejects_growth_and_accepts_strict_decrease` | integration | #17 | ≥80% |
| same | `ratchet_uses_base_record_not_pr_record` | integration | #17 | ≥80% |
| same | `ratchet_refuses_protected_inventory_drift` | integration | #17 | ≥80% |
| same | `ratchet_schema_rejects_suppression_capability` | unit | #18 | ≥80% |
| same | `ratchet_schema_file_is_strict_and_suppression_free` | unit | #18 | ≥80% |
| same | `batch_limit_rejects_more_than_ten_files_or_hundred_lines` | integration | #18 | ≥80% |
| same | `zero_mutable_findings_is_terminal_state` | integration | #18 | ≥80% |
| same | `ratchet_requires_anchored_base_record_before_activation` | integration | #13 | ≥80% |
| `tools/ci/plan-rust-slice-coverage.mjs` | `plan-rust-slice-coverage.test.mjs` :: `comment_language_measured_rust_files_keep_exact_ownership_boundaries` | unit/refusal | #13/#16 | ≥80% |
| same | same :: `comment_language_test_only_localization_requires_immutable_base_proof` | unit/refusal | #13/#16 | ≥80% |
| same | same :: `test_only_localization_differential_accepts_declared_cfg_test_change` | unit | #13/#16 | ≥80% |
| same | same :: `test_only_localization_differential_refuses_spoofed_or_production_change` | refusal | #13/#16 | ≥80% |

The fixture builder must construct candidate text without printing or embedding
source snippets in failure messages. The test suite must also prove that a
repeated run does not modify fixture bytes or repository files.

## Validation checklist

### Step 3 implementation gate

- [ ] This SPEC activation record and existing PRD have valid English Markdown
  and resolve their scoped links.
- [ ] Every Markdown link in these two files resolves to an existing repository
  path or an explicitly permitted external standard.
- [ ] `git diff --check -- docs/specs/no-milestone/comment-language-integrity/`
  passes.
- [ ] Only this SPEC, the scanner, its named test module, the strict ratchet
  schema, the existing diff workflow, the exact DT-11 coverage map/planner
  test, the DT-12 test-only map/planner contract, and the named bounded mutable
  English batches are modified by this Step 3 implementation.

- [ ] `node --check tools/ci/check-comment-language.mjs`
- [ ] Focused `node --test tools/ci/check-comment-language.test.mjs`
- [ ] Per-file coverage:

  ```bash
  node --test --experimental-test-coverage \
    --test-coverage-include=tools/ci/check-comment-language.mjs \
    --test-coverage-lines=80 --test-coverage-branches=80 \
    --test-coverage-functions=80 \
    tools/ci/check-comment-language.test.mjs
  ```

- [ ] `node tools/ci/check-comment-language.mjs --diff <base>` rejects a new
  finding and accepts an English or exact localized addition.
- [ ] `node tools/ci/check-comment-language.mjs --all` reports deterministic
  mutable/protected counts.
- [ ] Fixture `--ratchet <base>` tests prove that a working record matches the
  current scan and either equals or makes one bounded strict decrease from its
  committed base record; a repository invocation remains fail-closed until
  baseline stabilization materializes that base record.
- [ ] No diagnostic echoes source, marker, secret, absolute path, or diff text.
- [ ] No protected evidence/history/IMPL file is translated, rewritten, or
  counted as a mutable exception. The named migration batches have no protected
  path; the first repository digest comparison is deferred to stabilization.
- [ ] The workflow invokes the exact implemented diff command only. Ratchet
  activation is deferred until its baseline is present in the PR base. Full
  docs/index/claims gates remain out of scope and are not claimed here.
- [ ] DT-9 design is closed: the fixed future record path, strict schema,
  maintainer-review protocol, base comparison, batch limits, fixture bootstrap,
  and missing-base refusal are tested. Material baseline and workflow activation
  remain pending stabilization.
- [ ] DT-11's exact canonical command passes with every named production file
  at ≥80% line coverage, and planner tests prove every former lower-coverage
  path has its exact separate feature owner; zero historical paths remain
  low-coverage or unmapped.
- [ ] DT-12 accepts only the two exact test-only source paths after a full-base
  lexical projection proves their production text unchanged, package tests pass,
  and the immutable base contains all named ignored-GPU PASS commands. No GPU
  command is rerun by this slice.

`BINARY_MATCH`: N/A — no daemon, driver, or runtime surface.
