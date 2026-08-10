---
slug: benchmark-evidence-integrity
title: Benchmark and validation evidence integrity
milestone: —
issues: []
---

# PRD - Benchmark and validation evidence integrity

## Summary

RamShared needs one fail-closed contract for performance claims and validation
evidence across Windows, WSL2, Linux kernel, CUDA, and userspace slices. A
benchmark number is admissible only when its workload, platform, loaded
binary, source revision, raw artifacts, statistical treatment, and cleanup
state can be independently reconstructed. Documentation status must agree
with the machine-readable evidence registry; a prose claim alone cannot mark
a feature DONE or establish a regression baseline.

## Problem and confirmed evidence

- `docs/BENCHMARKS.md` has dated benchmark narratives that do not all have a
  corresponding entry in `docs/benchmarks/results.jsonl`.
- Existing JSONL entries have no `schema_version` and no repository validator
  for mandatory context, artifact existence, hashes, or identity.
- Benchmark comparison is implemented independently by individual harnesses,
  so two records can be compared without proving that their OS, GPU, driver,
  configuration, workload definition, warm-up, and run count are compatible.
- The exploratory Windows storage matrix could calculate RED/YELLOW rows but
  still return zero and print `STATUS=PASS`; it also did not persist the live
  hashes that it had verified.
- SSDV3 requires every named SPEC test and live evidence, but the repository
  has no general SPEC-to-test/evidence completeness checker.

## Recommended option

Introduce a RamShared evidence manifest schema plus deterministic validators.
Keep raw platform harnesses responsible for measurement, but require them to
emit a common envelope and register accepted results through one append-only
path. Generate human benchmark tables from, or validate them against, the
registry. Add a claims gate that prevents `DONE`, regression PASS, or candidate
promotion when required evidence is missing, incomparable, yellow, red, or
environment-bound.

Rejected alternatives:

- Keep prose and JSONL as independent best-effort logs. This preserves the
  current possibility of drift and unauditable claims.
- Use workspace-average coverage or test counts as evidence completeness.
  Those metrics do not prove the named slice, live path, or platform gates.
- Require identical candidate hashes for baseline comparison. Candidate
  identity must be recorded, but excluding it from the platform fingerprint
  is necessary to compare two candidate builds under the same conditions.
- Rewrite historical records with guessed fields. Legacy gaps remain explicit
  and cannot be upgraded to qualified evidence without original artifacts.

## Requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Define a versioned evidence envelope. | JSON Schema requires run ID, UTC interval, source commit/dirty state, sanitized command, harness revision, platform condition, loaded identities, workload/config, metrics, verdict, cleanup, artifact paths and SHA-256 hashes. |
| RF-2 | Qualify comparison context. | A deterministic fingerprint binds environment and workload fields while excluding candidate identity and timestamp; mismatches are `INCOMPARABLE`. |
| RF-3 | Enforce verdict and promotion semantics. | Ordered verdicts are RED, YELLOW, INCOMPARABLE, BASELINE, PASS; only PASS may promote, and all non-PASS outcomes return non-zero from promotion-capable harnesses. |
| RF-4 | Validate benchmark statistics. | Each metric declares unit, samples, `n`, aggregation, median, tail method, deviation/range, and threshold; n=3 nearest-rank p99 is explicitly the maximum observation. |
| RF-5 | Keep an append-only canonical registry. | A tool validates and appends one record without rewriting prior entries, duplicate run IDs, or unverifiable artifact references. |
| RF-6 | Enforce documentation parity. | Every dated benchmark claim maps to exactly one registry/evidence record or an explicit legacy-unqualified marker; every registered public claim appears in the benchmark index. |
| RF-7 | Check SSDV3 traceability. | For a selected SPEC, a checker confirms every file-matrix `TestName`, cover row, platform gate and before/action/after artifact required for the claimed status. |
| RF-8 | Separate measurement validity from product acceptance. | A complete BASELINE may become a qualified comparator, but is not a regression PASS; env-bound evidence remains PARTIAL/yellow and never becomes index-quality DONE. |
| RF-9 | Preserve failure and cleanup evidence. | Timeout, integrity error, BINARY_MATCH failure, residue, forced kill, unsafe recovery, or incomplete rollback is RED and retained with before/action/after state. |
| RF-10 | Make stable journeys reusable. | Repeated physical or VM drills live under `scripts/safety`, `scripts/p0`, or `scripts/windows`, support plan-only mode where destructive, and emit the common evidence envelope. |
| NFR-1 | Remain product-specific. | Schema fields and documentation describe RamShared surfaces only; no foreign service names, narratives, or copied process templates. |
| NFR-2 | Be deterministic and offline-verifiable. | Validators require no network and produce stable output for the same repository and artifact set. |
| NFR-3 | Protect secrets and host identity. | Sanitization rejects tokens, passwords, KASLR addresses, and unnecessary personal identifiers before evidence can be committed. |
| NFR-4 | Preserve historical honesty. | Legacy entries are never silently backfilled or promoted; corrections append a superseding record linked to the original run ID. |

## Evidence data model

The canonical record contains these top-level groups:

| Group | Required content |
| --- | --- |
| `identity` | `schema_version`, unique `run_id`, surface/slug, harness behavior revision |
| `source` | commit, dirty flag, optional package revision, sanitized invocation |
| `platform` | OS/kernel/build, CPU/RAM, GPU/driver/VRAM condition, relevant services/devices |
| `candidate` | manifest/config/binary/driver hashes and observed loaded hashes |
| `workload` | exact profile, size, sector/block size, queue depth, warm-up, runs, bounds |
| `comparison` | platform fingerprint, baseline run ID/fingerprint, qualification reason |
| `metrics` | samples, units, aggregation, median/tail/deviation/range and thresholds |
| `lifecycle` | before/action/after, BINARY_MATCH, refusals, cleanup and residue |
| `artifacts` | relative path, byte size, SHA-256, retention/commit status |
| `decision` | verdict, promotion eligibility, gaps, rollback trigger |

## Verdict state machine

1. Structural/schema or integrity failure is RED.
2. A complete measurement with no comparator is BASELINE.
3. A supplied but unqualified or fingerprint-mismatched comparator is
   INCOMPARABLE.
4. A comparable threshold warning is YELLOW; a hard threshold violation is
   RED.
5. PASS requires comparable metrics plus all identity, live-path, refusal,
   cleanup, and artifact gates.
6. Candidate activation/promotion occurs only after PASS. Every other state
   selects the documented LKG or clean stopped state.

## Documentation parity

The validator maintains an objective-oriented matrix rather than duplicating
the same narrative across files:

| Claim | Canonical source | Consumers |
| --- | --- | --- |
| Feature requirements and named tests | feature `SPEC.md` | `IMPL.md`, validation gate, docs index |
| Raw run and machine context | evidence manifest/artifacts | registry, `validation.md`, benchmark table |
| Benchmark comparison | registered qualified record | `docs/BENCHMARKS.md`, release/architecture claims |
| Current implementation status | `IMPL.md` plus validated evidence | generated `docs/INDEX.md` |
| Operator workflow | stable safety/platform script | README/runbook references |

## Implementation strategy

1. Inventory existing benchmark prose, JSONL entries, validation records, and
   harness artifacts without changing historical claims.
2. Specify the JSON Schema, verdict/fingerprint rules, legacy-unqualified
   representation, and sanitizer.
3. Implement schema, artifact-hash, duplicate-ID, documentation-parity, and
   SPEC-traceability validators with manufactured false-green tests.
4. Migrate current Windows storage and active WSL2 benchmark harnesses to emit
   the envelope; keep legacy rows explicitly unqualified.
5. Add the validators to `scripts/docs-check.sh` and CI only after the current
   tree passes with honest legacy markers.
6. Register new results only after their platform-specific live gates pass.

## Acceptance criteria

- [ ] Malformed, incomplete, duplicate, secret-bearing, hash-mismatched, or
      missing-artifact records fail non-zero.
- [ ] Fingerprint-compatible and incompatible manufactured baselines produce
      PASS and INCOMPARABLE respectively.
- [ ] RED/YELLOW/BASELINE/INCOMPARABLE cannot print PASS or promote a candidate.
- [ ] Every dated benchmark section has one validated registry mapping or an
      explicit legacy-unqualified record.
- [ ] A SPEC fixture with a missing named test, cover path, refusal, or E2E
      artifact fails the traceability checker.
- [ ] Windows, WSL2, and pure-userspace fixtures demonstrate the same envelope
      without erasing their platform-specific gates.
- [ ] `scripts/docs-check.sh` and CI run the validators offline.

## Risks and rollback

- Strict rollout can expose many historical gaps. Land explicit legacy markers
  first; never weaken the schema for new runs to make old prose pass.
- Platform fingerprints can be too strict or too weak. Version their field set
  and require an explicit migration when it changes.
- Artifact roots can be large or host-private. Register bounded, sanitized
  evidence subsets and hashes rather than raw dumps containing secrets.

Rollback trigger: disable registry promotion and revert the validator release
if it accepts a missing/mismatched artifact, compares different platform
fingerprints, emits PASS for a non-PASS verdict, rewrites historical rows, or
leaks a secret/host-sensitive value.

## Documents and tools in scope

- `docs/benchmarks/evidence.schema.json`
- `docs/benchmarks/results.jsonl`
- `docs/BENCHMARKS.md`
- `tools/ci/check-benchmark-evidence.mjs`
- `tools/ci/check-spec-evidence.mjs`
- `scripts/docs-check.sh`
- platform harnesses selected by the future SPEC matrix
- root and feature `validation.md` / `IMPL.md`
