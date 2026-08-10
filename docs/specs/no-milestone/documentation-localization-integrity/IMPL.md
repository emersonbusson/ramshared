# IMPL — Documentation localization integrity

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/documentation-localization-integrity/SPEC.md`

## Status

implemented · cover ✓ · E2E ✓ · BINARY_MATCH N/A — no daemon, driver, or
runtime surface

The repository-only localization contract is implemented. The checker is
read-only, zero-dependency Node, and owns source-hash, link, language-switch,
portal-objective, disclaimer, protected-path, and localized-authority checks.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `README.md` | ITEM-3 / RF-2 | Added the reciprocal Portuguese language switch only. |
| `README.pt-BR.md` | ITEM-3 / RF-2 | Added the complete Brazilian Portuguese translation of the current root README. |
| `docs/pt-BR/README.md` | ITEM-3 / RF-3 | Added a concise Portuguese portal with five canonical user-document pointers. |
| `docs/localization/manifest.json` | ITEM-2 / RF-1 | Added exact README SHA-256 entries, current state, policy, and protected classes. |
| `tools/ci/check-documentation-localization.mjs` | ITEM-2..4 / RF-1..5 | Added the deterministic read-only checker CLI and exported validation functions. |
| `tools/ci/check-documentation-localization.test.mjs` | ITEM-1..4 | Added 15 named positive, refusal, determinism, and CLI tests. |
| `scripts/docs-check.sh` | ITEM-4 / RF-4 | Added the localization checker and focused test invocation. |
| `.github/workflows/ci.yml` | ITEM-4 / NFR-4 | Added the per-file Node coverage command with 80% lines/branches/functions thresholds. |
| `docs/specs/no-milestone/documentation-localization-integrity/evidence/validation-summary.json` | ITEM-4 | Added sanitized repository-only measured results. |
| `docs/specs/no-milestone/documentation-localization-integrity/evidence-manifest.json` | ITEM-4 | Added the `ramshared-spec-evidence/v1` claim manifest with hashes and named tests. |

`validation.md`, `docs/INDEX.md`, `docs/governance/claims.json`, existing
evidence manifests, unrelated workflows, Windows code, and Rust code were not
modified for this closeout.

## Validation (numbers)

- Syntax: `node --check tools/ci/check-documentation-localization.mjs` and the
  focused test file → exit `0`.
- Named tests: `node --test tools/ci/check-documentation-localization.test.mjs`
  → 15 passed, 0 failed, exit `0`.
- Coverage: the production checker measured **98.59% lines, 86.90% branches,
  and 100.00% functions** with the per-file Node threshold command; exit `0`.
- Localization gate: `node tools/ci/check-documentation-localization.mjs --all`
  → `FILES=2`, `FINDINGS=0`, `LOCALIZATION_STATUS=PASS`, exit `0`.
- Determinism: two identical checker runs returned exit `0` and the same
  sanitized output SHA-256:
  `f1964e7db9763a1028e20c5dfdaee6c13e85a2d81ba0685b1205c50a45b7300c`.
- Refusals: stale source hash, missing localization, broken switch, positive
  authority claim, invalid state/policy, protected path, missing portal
  objective, and missing canonical source fixtures all failed as expected;
  diagnostics contained only path/line/rule/reason fields.
- Evidence: [`validation-summary.json`](evidence/validation-summary.json) and
  [`evidence-manifest.json`](evidence-manifest.json).

The integrated docs gate had already passed before this Step 3 evidence write.
This closeout intentionally did not rerun or rewrite the protected generated
index after adding `IMPL.md`; index/claim promotion remains owned by the parent
integration change.

## SPEC matrix

| Test | Result |
| --- | --- |
| `manifest_current_hashes_pass` | PASS |
| `missing_required_localization_fails` | PASS |
| `stale_source_hash_fails` | PASS |
| `language_switches_and_portal_links_pass` | PASS |
| `broken_language_switch_fails` | PASS |
| `authority_claim_is_rejected_without_echo` | PASS |
| `protected_normative_localization_path_fails` | PASS |
| `invalid_manifest_state_policy_fails` | PASS |
| `checker_output_is_deterministic_and_read_only` | PASS |
| `cli_usage_error_returns_two` | PASS |

The five additional regression tests in the same test file also passed:
`manifest_schema_and_path_guards_fail`, `portal_missing_objective_fails`,
`missing_canonical_readme_fails_closed`,
`cli_all_returns_zero_for_current_repository`, and
`repository_localization_gate_passes`.

## E2E: before → action → after

- **Before:** the current manifest contained two required localized entries,
  the canonical README SHA-256 was
  `e4acb2f93861e05bca1935421396d8a44b5670ca4ff7326053d31df51335d3c8`, and the
  checker had no findings.
- **Action:** ran the public localization CLI and its 15-test suite. The
  checker performed no writes, network requests, process launches, or host
  operations.
- **After:** two accepted runs matched byte-for-byte, both returned `0`, and
  the accepted scope remained at two files with zero findings. Manufactured
  stale, missing, broken-link, authority, and protected-path cases returned
  policy findings without echoing their matched content.

## Gaps

No localization-integrity gaps remain in this repository-only surface. The
generated index and promotion registry are intentionally outside this closeout
because the task forbids modifying them; the parent integration must regenerate
or qualify those files separately.

## Rollback trigger

Revert this slice if one stale hash, missing localization, broken switch,
positive authority claim, sensitive diagnostic, or nondeterministic result
passes the gate, or if the checker mutates any file or host state.

## Traceability

| RF | ITEM | Evidence |
| --- | --- | --- |
| RF-1 | ITEM-2 | Current manifest and `manifest_current_hashes_pass` |
| RF-2 | ITEM-3 | Full translation, reciprocal switch, and source hash |
| RF-3 | ITEM-3 | Portuguese portal and `language_switches_and_portal_links_pass` |
| RF-4 | ITEM-2, ITEM-4 | Refusal tests and zero-finding CLI run |
| RF-5 | ITEM-3 | Authority/disclaimer and protected-path refusals |
| NFR-2 | ITEM-4 | Deterministic output hash |
| NFR-3 | ITEM-2..4 | Read-only source test and no host operations |
| NFR-4 | ITEM-4 | Per-file Node coverage report |
