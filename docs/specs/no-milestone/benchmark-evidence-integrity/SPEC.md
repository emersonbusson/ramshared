# SPEC — Benchmark and validation evidence integrity

## Closed scope

In scope now: a versioned public evidence envelope, deterministic validation of
benchmark records and artifacts, explicit legacy-unqualified mappings for the
five existing human benchmark entries, benchmark prose/registry parity, and a
SPEC evidence-claim manifest checker. The tools are zero-dependency Node.js and
read-only over repository inputs.

Out now: rewriting historical JSONL or validation entries, uploading private
host artifacts, executing Windows/WSL2/kernel workloads, and automatically
promoting any capability. Platform harness adapters remain owned by their
feature SPECs; the current Windows storage harness already emits the context
needed for a future registered record.

Assumed ready: Node.js 22 in CI, `git`, the existing docs gate, and sanitized
repository-relative artifacts. Heavy or host-private artifacts may be described
only as legacy-unqualified and cannot support PASS.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1, NFR-2, NFR-3 | ITEM-1, ITEM-2 |
| RF-2, RF-3, RF-4 | ITEM-2 |
| RF-5, RF-6 | ITEM-3 |
| RF-7, RF-8 | ITEM-4 |
| RF-9 | ITEM-2, ITEM-4 |
| RF-10, NFR-1, NFR-4 | ITEM-5 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | New qualified evidence uses `ramshared-evidence/v1`; old JSONL bytes remain unchanged. | Historical honesty is append-only. |
| DT-2 | Legacy benchmark prose is mapped in `legacy-unqualified.json` by stable section ID and optional old run ID. | Missing context cannot be invented retrospectively. |
| DT-3 | Verdict precedence is `RED > YELLOW > INCOMPARABLE > BASELINE > PASS`; only PASS is promotable. | Prevent a valid measurement from becoming a false regression or release claim. |
| DT-4 | The platform fingerprint includes schema/harness revision, platform, condition and workload semantics, but excludes candidate hash and time. | Candidates must be comparable under the same conditions. |
| DT-5 | A public PASS artifact is repository-relative, exists, has byte size and SHA-256, and passes sanitization. Absolute/private paths are invalid. | A reviewer must be able to re-audit the public claim. |
| DT-6 | `check-benchmark-evidence.mjs` parses before validating, enforces size/count limits, recomputes statistics and fingerprints, and never executes record text. | Fail closed at the public data boundary. |
| DT-7 | Benchmark prose sections use an HTML comment `ramshared-benchmark-id: <id>`; every section maps exactly once to a v1 record or legacy marker. | Deterministic parity without parsing human titles as identity. |
| DT-8 | `check-spec-evidence.mjs` consumes a versioned `evidence-manifest.json`; it never infers DONE from `IMPL.md`. | Evidence quality cannot be derived safely from prose presence. |
| DT-9 | A claim manifest records SPEC path/hash, status, named tests, cover rows, live before/action/after, refusals, cleanup, artifacts and BINARY_MATCH applicability. `DONE` requires all applicable gates; env-bound is PARTIAL. | One explicit fail-closed claim contract. |
| DT-10 | Initial repository integration validates benchmark parity globally and SPEC manifests only when present. Migrating existing feature claims is a separate governance item; absence never upgrades a claim. | Do not fabricate history or block unrelated code on guessed metadata. |
| DT-11 | Findings print relative path, line or record ID, and rule code only; secret material and raw kernel addresses are never echoed. | This repository and its CI logs are public. |

## Atomicity and rollback

- Atomicity frontier: repository documentation and read-only CI tools only.
- Userspace/daemon: N/A — no process or package mutation.
- Kernel/Windows driver: N/A — no load, unload, install or ABI change.
- Host/persistent: N/A — no SCM, swap, pagefile, disk, GPU pressure or reboot.
- Rollback: remove the new docs-check invocations and restore the last known-good
  validator files; retain every historical record and failure artifact.
- Forward-only: published v1 records are append-only. Corrections append a
  superseding record and never rewrite the original.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 parser | #13 | Can malformed or forged evidence pass because fields merely exist? | `check-benchmark-evidence.test.mjs` refusal fixtures | One malformed/hash/statistics case exits 0 |
| ITEM-2 comparison | #9 | Are median, nearest-rank p99, deviation and n recomputed from samples? | `recomputes_statistics_and_rejects_forgery` | Any stored aggregate accepted without equality |
| ITEM-3 parity | #13 | Does every public number have exactly one qualified or legacy identity? | live checker on repository + duplicate/missing fixture | Missing/duplicate mapping passes |
| ITEM-4 claims | #13 | Can IMPL presence, unit-only evidence or env-bound evidence claim DONE? | `done_requires_complete_same_surface_evidence` | Any fabricated DONE passes |
| ITEM-5 integration | #17 | Is the gate deterministic and replayable? | two consecutive `docs-check.sh` runs with byte-identical diagnostics | Exit/output differs on identical tree |

## Security checklist (pre-impl)

- [x] Privilege: N/A — read-only local files.
- [x] User/host copy: bounded UTF-8/JSON bytes, array counts, path lengths and owned strings.
- [x] Flags: reject unknown CLI flags and schema/verdict values.
- [x] Info-leak: sanitized diagnostics; no secret value, PII, private path or raw kernel address.
- [x] IRQ/IRQL: N/A — Node CLI.
- [x] Lifetime: opened files are bounded and closed by synchronous APIs.
- [x] Device-gone: N/A — no device access.
- [x] Host safety: tools cannot invoke platform workloads or mutation commands.
- [x] Replayable ops: validation is read-only and deterministic.

## Files to CREATE

**`docs/benchmarks/evidence.schema.json`**
- Purpose: public v1 evidence contract and limits.
- RF / DT: RF-1–RF-4, RF-9; DT-1, DT-3–DT-6.
- Required tests: `tools/ci/check-benchmark-evidence.test.mjs` :: `valid_v1_record_passes`; `missing_required_group_fails`.
- Cover target: N/A — declarative schema.

**`docs/benchmarks/legacy-unqualified.json`**
- Purpose: exact, honest mapping for legacy human sections and old registry rows.
- RF / DT: RF-6, NFR-4; DT-2, DT-7.
- Required tests: `historical_record_requires_explicit_legacy_unqualified_marker`; `legacy_marker_cannot_promote`.
- Cover target: N/A — data fixture validated live.

**`docs/benchmarks/benchmark-map.json`**
- Purpose: exact one-to-one mapping between human benchmark sections and v1
  records or explicit legacy markers.
- RF / DT: RF-5, RF-6; DT-2, DT-7.
- Required tests: `dated_benchmark_requires_exactly_one_registry_mapping`.
- Cover target: N/A — data fixture validated live.

**`tools/ci/check-benchmark-evidence.mjs`**
- Purpose: export pure validators and provide a CLI for schema, size, sanitization,
  fingerprint, statistics, artifacts, duplicate IDs and prose parity.
- RF / DT: RF-1–RF-6, RF-8, RF-9; DT-1–DT-7, DT-11.
- Functions: `validateRecord(record, context)`, `computeStats(samples)`,
  `platformFingerprint(record)`, `validateRepository(options)`, `main(argv)`.
- Required tests: `rejects_missing_schema_version_duplicate_run_id_or_dirty_provenance`,
  `rejects_absolute_missing_or_hash_mismatched_artifact`,
  `rejects_secret_pii_or_kernel_address_without_echoing_value`,
  `recomputes_statistics_and_rejects_forgery`,
  `incompatible_fingerprint_is_incomparable`,
  `nonpass_verdict_cannot_promote_or_print_pass`,
  `dated_benchmark_requires_exactly_one_registry_mapping`.
- Cover target: N/A — Node business logic has executable unit/refusal tests; Rust slice cover is inapplicable.

**`tools/ci/check-benchmark-evidence.test.mjs`**
- Purpose: Node test-runner positive/refusal fixtures in temporary directories.
- RF / DT: all benchmark validator decisions.
- Cover target: N/A — test file.

**`docs/specs/evidence-manifest.schema.json`**
- Purpose: explicit claim/evidence contract for SSDV3 status.
- RF / DT: RF-7–RF-9; DT-8–DT-10.
- Required tests: `tools/ci/check-spec-evidence.test.mjs` :: `valid_partial_manifest_passes`; `complete_done_manifest_passes`.
- Cover target: N/A — declarative schema.

**`tools/ci/check-spec-evidence.mjs`**
- Purpose: validate explicit claim manifests without inferring status from filenames.
- RF / DT: RF-7–RF-9; DT-8–DT-11.
- Functions: `validateClaimManifest(record, root)`, `discoverClaimManifests(root)`, `main(argv)`.
- Required tests: `partial_status_with_implemented_word_is_not_done`,
  `status_heading_variant_or_missing_manifest_fails_closed_when_claimed`,
  `done_requires_named_tests_cover_live_e2e_refusal_cleanup_and_binary_match`,
  `env_bound_evidence_cannot_publish_done`, `artifact_hash_mismatch_fails`.
- Cover target: N/A — Node business logic tested with built-in runner.

**`tools/ci/check-spec-evidence.test.mjs`**
- Purpose: manufactured positive/negative claim fixtures.
- Cover target: N/A — test file.

## Files to MODIFY

**`docs/BENCHMARKS.md`**
- Add one stable benchmark ID comment to each dated section and state that
  legacy-unqualified entries are not baselines or PASS claims.
- Tests: `dated_benchmark_requires_exactly_one_registry_mapping`.

**`.claude/rules/benchmarks.md`**
- Point new public claims to the v1 envelope and define PASS artifact availability.
- Tests: repository benchmark validator.

**`scripts/docs-check.sh`**
- Run both Node test suites, the repository benchmark validator and optional
  SPEC manifest validator after existing gates.
- Tests: two full live invocations; any validator failure propagates non-zero.

**`docs/INDEX.md`**
- Regenerate after SPEC creation; no status promotion from this file.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| record/section/artifact counts | validator stdout | bounded summary |
| rule violation | stderr | relative path/id + stable rule code, no matched value |
| verdict/promotion | evidence record | enum + boolean |
| platform fingerprint | record/comparison | SHA-256 |
| claim gates | evidence manifest | named booleans and artifact hashes |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | N/A — no runtime architecture |
| ADR | N/A — contract is fully owned here |
| `docs/reliability/DEGRADATION-MATRIX.md` | N/A — no runtime failure mode |
| `validation.md` | Append on close with commands/counts/verdict |
| `docs/BENCHMARKS.md` + JSONL | Add identities/contract; never fabricate rows |
| `.claude/rules/benchmarks.md` | Point to the v1 gate |

## Implementation order

1. ITEM-1 — land schemas and RED parser/statistics/sanitization fixtures.
2. ITEM-2 — implement v1 validation until the unit/refusal suite is GREEN.
3. ITEM-3 — add stable prose IDs, legacy mapping and parity validation.
4. ITEM-4 — implement explicit SPEC evidence-manifest validation and false-DONE fixtures.
5. ITEM-5 — integrate docs-check, run twice, append validation, then write IMPL.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `tools/ci/check-benchmark-evidence.mjs` | `check-benchmark-evidence.test.mjs` :: named tests above | unit/integration | #9/#13 | N/A — Node |
| benchmark registry/prose | same :: `dated_benchmark_requires_exactly_one_registry_mapping` | live docs E2E | #13 | N/A — E2E-only |
| `tools/ci/check-spec-evidence.mjs` | `check-spec-evidence.test.mjs` :: named tests above | unit/integration | #13 | N/A — Node |
| `scripts/docs-check.sh` | two identical repository runs | live CLI E2E | #17 | N/A — orchestration |

## Validation checklist

- [ ] `node --test tools/ci/check-benchmark-evidence.test.mjs`
- [ ] `node --test tools/ci/check-spec-evidence.test.mjs`
- [ ] `node tools/ci/check-benchmark-evidence.mjs --check`
- [ ] `node tools/ci/check-spec-evidence.mjs --check`
- [ ] `./scripts/docs-check.sh` twice with identical exit/output
- [ ] `node tools/generate-docs-index.mjs --check`
- [ ] `git diff --check`
- [ ] Every matrix test name exists and every refusal returns non-zero
- [ ] Live CLI evidence contains before/action/after counts and no public-sensitive values

Rollback trigger: revert the validator integration if one malformed, duplicate,
hash-mismatched, secret-bearing, statistically forged, incomparable or non-PASS
record is accepted; if diagnostics reveal one sensitive value; or if identical
inputs produce different results in two consecutive runs.
