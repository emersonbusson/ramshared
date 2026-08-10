# SPEC — Documentation localization integrity

> SSDV3 Step 2 · implements [`PRD.md`](PRD.md).
>
> This SPEC is limited to localized user documentation and a read-only Node
> gate. It does not alter runtime, kernel, driver, daemon, host, or
> append-only engineering records.

## Closed scope

### In now

- `README.md` gets one reciprocal link to `README.pt-BR.md`.
- `README.pt-BR.md` is a complete Brazilian Portuguese translation of the
  current root README, preserving code, identifiers, links, and safety intent.
- `docs/pt-BR/README.md` is a Portuguese, non-normative portal for quickstart,
  installation, safe operation, troubleshooting, and architecture pointers.
- `docs/localization/manifest.json` pins each localized file to the exact
  SHA-256 of `README.md` and records state and policy.
- `tools/ci/check-documentation-localization.mjs` is the single checker CLI.
- Focused named tests run from `scripts/docs-check.sh`; `.github/workflows/ci.yml`
  adds only the per-file Node coverage command.

### Out now

- Translation of PRD, SPEC, IMPL, ADR, CI, evidence, benchmark, validation,
  reliability, or other normative engineering records.
- Any duplicate Portuguese technical specification or runtime/host behavior.
- Network calls, external translation services, generated translation output,
  or automatic file/manifest rewrites.

### Assumed-ready dependencies

- Node 22 built-in ESM, `node:test`, `node:crypto`, and `node:fs`.
- Existing `scripts/docs-check.sh` and Markdown link checker remain their own
  owners; this checker verifies the localization contract only.
- Existing concurrent governance changes may add other docs gates. This slice
  does not remove or reorder them.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | DT-1, ITEM-1, ITEM-2 |
| RF-2 | DT-2, ITEM-3 |
| RF-3 | DT-2, DT-4, ITEM-3 |
| RF-4 | DT-3, ITEM-2, ITEM-4 |
| RF-5 | DT-4, ITEM-2, ITEM-3 |
| NFR-1 | DT-2, DT-4 |
| NFR-2 | DT-3, ITEM-2 |
| NFR-3 | DT-3, ITEM-4 |
| NFR-4 | ITEM-4 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | The manifest is `docs/localization/manifest.json`; its top-level `canonical_language` is `en`, `current_policy` is `informational-non-normative`, and every entry has exactly `canonical_source`, `localized_path`, `source_sha256`, `state`, and `policy`. | One strict location and shape prevent competing freshness sources. |
| DT-2 | Both localized files use `README.md` as their canonical source. The full translation and portal therefore become stale together when the root README changes. | The portal is a navigation projection of the root product entry point, not a second technical source. |
| DT-3 | Hashes are SHA-256 over exact canonical bytes; local link checks resolve repository-relative paths and ignore external URLs. | Content integrity is independent of timestamps and network availability. |
| DT-4 | `state` must be `current` and `policy` must be `informational-non-normative`; any other state or policy is a finding for required entries. | Required localizations cannot silently opt out of freshness or authority policy. |
| DT-5 | The checker recognizes reciprocal language switches by links whose local targets are the counterpart README files; labels are not trusted as proof alone. | A visible, working switch is required in both directions. |
| DT-6 | Authority checks reject positive claims such as “this document is normative/canonical/official” and Portuguese equivalents; explicit negative disclaimers are allowed. | Localized pages may explain their boundary without becoming a competing authority. |
| DT-7 | Protected classes are declared in the manifest as `prd`, `spec`, `impl`, `adr`, `ci`, `evidence`, and `benchmark`; the checker rejects manifest localized paths under those classes. | The slice cannot accidentally create translations of normative or append-only records. |
| DT-8 | The CLI accepts exactly `--all`, returns 0 for clean, 1 for policy findings, and 2 for usage/configuration errors. Findings are sorted by path, line, rule, and reason and never include matched content. | Local and CI output remains deterministic and sanitized. |

## Manifest contract

The exact top-level shape is:

```json
{
  "schema_version": 1,
  "canonical_language": "en",
  "current_policy": "informational-non-normative",
  "protected_document_classes": ["prd", "spec", "impl", "adr", "ci", "evidence", "benchmark"],
  "entries": [
    {
      "canonical_source": "README.md",
      "localized_path": "README.pt-BR.md",
      "source_sha256": "<64 lowercase hex characters>",
      "state": "current",
      "policy": "informational-non-normative"
    }
  ]
}
```

The manifest contains exactly two required entries: the full README and the
Portuguese portal. Paths are POSIX, repository-relative, non-empty, and cannot
contain `..`, absolute roots, drive letters, URLs, or NUL/control characters.
The checker rejects duplicate localized paths, duplicate canonical/path pairs,
unknown top-level or entry keys, bad hashes, missing paths, and stale hashes.

## Link and authority contract

The checker scans Markdown links in every manifest localized file. A local
target must exist after removing its fragment; an external HTTP(S) target is
accepted without a network request. It requires:

- `README.md` → `README.pt-BR.md` language switch;
- `README.pt-BR.md` → `README.md` language switch;
- the portal → `README.pt-BR.md` and all five objective pointers;
- an informational/non-normative disclaimer in every localized file.

The five portal objectives are `quickstart`, `installation`, `safe operation`,
`troubleshooting`, and `architecture`. The checker recognizes a pointer when
the portal contains a heading or link text containing the objective and a
local link to the selected canonical source. It does not require translated
copies of those sources.

Positive authority patterns include English and Portuguese forms of a localized
document being normative, canonical, official, authoritative, or the source of
truth. A line explicitly negating those claims (for example, “não é normativo”
or “not normative”) is not a finding. Diagnostics report only the file, line,
rule, and stable reason code.

## Atomicity and rollback

### Atomicity frontier

The only frontier is repository documentation and the read-only checker. The
checker never writes the manifest or translation. A translation update and its
manifest hash update are one reviewable source-control change.

### Rollback by frontier

| Frontier | Policy |
| --- | --- |
| Userspace/documentation | Revert the checker, localized files, and manifest to the last clean revision. |
| Kernel/module or Windows driver | N/A — no such files or execution paths are in scope. |
| Host/persistent state | N/A — no daemon, swap, disk, VRAM, SCM, or host operation is callable. |

Rollback trigger: one stale hash, missing required localization, broken switch,
positive authority claim passing the gate, sensitive diagnostic leak, or any
checker write/runtime mutation. Already-published localized history is not
rewritten; a corrected revision supersedes it.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 manifest/hash | #9 — number before adjective | Is “current” backed by exact source bytes? | `manifest_current_hashes_pass`; stale-hash refusal | Any stale hash passes |
| ITEM-2 links | #13 — refusal plus legitimate | Does every switch and pointer resolve on this tree? | `language_switches_and_portal_links_pass`; missing-link fixture | Broken local link passes |
| ITEM-3 authority | #16 — safe default | Can a localized page claim technical authority? | `authority_claim_is_rejected_without_echo`; disclaimer fixture | Any positive claim passes |
| ITEM-4 E2E | #17 — idempotent read | Do repeated checks produce one result without mutation? | CLI determinism and read-only source test | Output drift or mutation |

## Security checklist (pre-implementation)

- [x] Privilege: N/A — regular read-only files only.
- [x] User/host copy: N/A — no user buffers or host APIs.
- [x] Flags: unknown CLI arguments return exit 2.
- [x] Information leak: findings omit matched text and private values.
- [x] Lifetime/hot-unplug: N/A — no runtime resource.
- [x] Host safety: no daemon, driver, SCM, disk, swap, pressure, or reboot path.
- [x] Replayable ops: repeated checks are read-only and deterministic.
- [x] Protected records: no localized path may be in PRD/SPEC/IMPL/ADR/CI/evidence/benchmark classes.

## Files to CREATE

**`docs/specs/no-milestone/documentation-localization-integrity/PRD.md`**

- Purpose: requirements and boundary for the localization slice.
- Required tests: documentation artifact and link gates.

**`docs/specs/no-milestone/documentation-localization-integrity/SPEC.md`**

- Purpose: closed manifest, checker, link, authority, and validation contract.

**`docs/specs/no-milestone/documentation-localization-integrity/IMPL.md`**

- Purpose: English implementation metrics, named tests, coverage, and gaps.

**`docs/specs/no-milestone/documentation-localization-integrity/evidence/validation-summary.json`**

- Purpose: sanitized repository-only before/action/after measurements for the
  localization checker, named tests, coverage, and deterministic output.
- Schema: `ramshared-validation-summary/v1`; it contains no private paths,
  identities, secrets, runtime state, or host artifacts.

**`docs/specs/no-milestone/documentation-localization-integrity/evidence-manifest.json`**

- Purpose: evidence-backed Step 3 claim manifest for this slice.
- Schema: `ramshared-spec-evidence/v1`; its spec, test, coverage, artifact,
  refusal, cleanup, and rollback fields are validated by
  `tools/ci/check-spec-evidence.mjs`.

**`docs/localization/manifest.json`**

- Purpose: current source hashes and non-normative policy entries.
- Cover: data-only; parser is covered in the Node checker file.

**`README.pt-BR.md`**

- Purpose: complete Brazilian Portuguese translation of the current README.
- Policy: informational/non-normative; canonical English switch required.

**`docs/pt-BR/README.md`**

- Purpose: concise Portuguese user portal with canonical pointers.
- Policy: informational/non-normative; no copied normative block.

**`tools/ci/check-documentation-localization.mjs`**

- Purpose: zero-dependency deterministic checker and CLI.
- Required exports: `loadManifest`, `validateManifest`, `scanMarkdownLinks`,
  `validateLocalizations`, `run`, and `main`.
- Cover: ≥80% lines, branches, and functions.

**`tools/ci/check-documentation-localization.test.mjs`**

- Purpose: RED/GREEN unit and CLI tests using temporary sanitized fixtures.

## Files to MODIFY

**`README.md`**

- Add one language switch link to `README.pt-BR.md`; do not alter English
  requirements or status claims.

**`scripts/docs-check.sh`**

- Run the localization checker and its focused tests in the existing docs gate.

**`.github/workflows/ci.yml`**

- Add only the localization per-file Node coverage step after docs check.

## Required tests matrix

| Production path | Test | Kind | Cover |
| --- | --- | --- | --- |
| `tools/ci/check-documentation-localization.mjs` | `manifest_current_hashes_pass` | unit | ≥80% |
| same | `missing_required_localization_fails` | unit | ≥80% |
| same | `stale_source_hash_fails` | unit | ≥80% |
| same | `language_switches_and_portal_links_pass` | unit | ≥80% |
| same | `broken_language_switch_fails` | unit | ≥80% |
| same | `authority_claim_is_rejected_without_echo` | unit | ≥80% |
| same | `protected_normative_localization_path_fails` | unit | ≥80% |
| same | `invalid_manifest_state_policy_fails` | unit | ≥80% |
| same | `checker_output_is_deterministic_and_read_only` | integration | ≥80% |
| same | `cli_usage_error_returns_two` | integration | ≥80% |

## Implementation order

`ITEM-1…ITEM-4` is a hard order:

1. ITEM-1 — create PRD/SPEC, named RED tests, and sanitized fixture helpers.
2. ITEM-2 — implement strict manifest, exact hash, and Markdown link checks.
3. ITEM-3 — implement language-switch, portal-objective, disclaimer, and
   authority checks; add localized documents and manifest.
4. ITEM-4 — wire docs-check/CI, run coverage/static gates, and write IMPL
   metrics without modifying protected append-only files.

## Validation checklist

- [ ] `node --check tools/ci/check-documentation-localization.mjs`
- [ ] Focused `node --test tools/ci/check-documentation-localization.test.mjs`
- [ ] Per-file Node lines/branches/functions coverage ≥80%.
- [ ] `node tools/ci/check-documentation-localization.mjs --all` exits 0.
- [ ] Missing, stale, broken-switch, authority, and protected-path fixtures exit 1.
- [ ] `node tools/check-broken-links.mjs --check` passes for localized docs.
- [ ] `scripts/docs-check.sh` includes and runs the checker/tests.
- [ ] No protected history/index/claims/evidence/Windows/Rust files changed.

`BINARY_MATCH`: N/A — no daemon, driver, or runtime surface.
