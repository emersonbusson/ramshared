---
slug: documentation-localization-integrity
title: Documentation localization integrity
milestone: —
issues: []
---

# PRD — Documentation localization integrity

> SSDV3 Step 1 only. This slice governs localized user-facing documentation;
> it does not translate or alter normative engineering records.

## 1. Summary

RamShared's canonical engineering documentation is written in English. Users
who prefer Brazilian Portuguese need a complete translation of the root
README and a small Portuguese navigation portal for high-value user tasks.
Without an integrity contract, a translation can silently lag behind its
source, lose its language switch, or present a localized page as a normative
technical authority.

The outcome is a bounded, zero-dependency localization manifest and checker.
The checker pins every required localized file to the SHA-256 of its canonical
English source, verifies local links and reciprocal language switches, and
rejects localized authority claims. It is read-only and has no runtime or host
side effects.

## 2. Technical context

### Confirmed in codebase

- [`README.md`](../../../../README.md) is the current English product entry point,
  including quick start, safe operation, installation, troubleshooting
  pointers, architecture, and status claims.
- [`docs/FAQ.md`](../../../../docs/FAQ.md) contains user troubleshooting guidance;
  [`docs/packaging/INSTALLABLES.md`](../../../../docs/packaging/INSTALLABLES.md)
  describes the installable bundle; [`ARCHITECTURE.md`](../../../../ARCHITECTURE.md)
  is the detailed architecture source.
- [`scripts/docs-check.sh`](../../../../scripts/docs-check.sh) is the local
  documentation gate and the docs job in `.github/workflows/ci.yml` is its CI
  surface.
- Structural documentation rules require English canonical documents and
  repository-local links. SSDV3, PRD, SPEC, IMPL, ADR, CI, evidence, and
  benchmark records are engineering records, not localization targets.

### Inference / proposal

- A manifest with explicit source, localized path, hash, state, and policy is
  sufficient for this user-facing surface; a translation memory or generic
  localization platform would add an unnecessary dependency and authority
  boundary.
- The localized portal should point to canonical technical documents instead
  of copying their requirements. The checker can enforce this by requiring a
  non-normative disclaimer and rejecting positive authority claims.

### Current gaps to close

| Gap | Risk | Required outcome |
| --- | --- | --- |
| No complete Portuguese README | Users miss the current product contract | Full `README.pt-BR.md` translation of the current README |
| No Portuguese user-doc router | Users duplicate or misread technical guidance | `docs/pt-BR/README.md` points to selected canonical sources |
| No source freshness contract | Translation can become stale silently | Manifest SHA-256 pin and `current` state check |
| No language-switch integrity gate | Users can get stranded in one locale | Reciprocal README switch links and local-link validation |
| Localized authority can be ambiguous | A translation may be mistaken for normative engineering truth | Explicit non-normative policy and authority-claim refusal |

## 3. Recommended option

Create one manifest and one checker under the repository's existing Node
documentation tooling. Keep `README.md` authoritative, publish a complete
`README.pt-BR.md`, and publish a concise `docs/pt-BR/README.md` portal. The
checker reads only repository files and prints sanitized, deterministic
findings.

### Alternatives rejected

| Alternative | Reason |
| --- | --- |
| Translate PRD/SPEC/IMPL/ADR/evidence/benchmark logs | Would alter normative and append-only engineering history |
| Duplicate technical docs under `docs/pt-BR/` | Creates competing requirements and unavoidable drift |
| Depend on an external translation service | Adds network, credentials, and non-deterministic output to the docs gate |
| Infer freshness from file timestamps | Timestamps are not content integrity and vary across checkouts |
| Allow a localized page to declare itself canonical | Contradicts the English-source ownership policy |

### Trade-offs

The complete README translation is a maintained artifact and must be updated
when the canonical README changes. The SHA-256 failure is intentional: it
forces an explicit translation update or an explicit manifest state change.

## 4. Functional requirements

### RF-1 — Canonical localization manifest

The repository shall contain a machine-readable manifest with one entry per
required localized file. Each entry records `canonical_source`,
`localized_path`, `source_sha256`, `state`, and `policy`.

Acceptance: every required path exists, is repository-relative, hashes the
current canonical source exactly, has state `current`, and uses the declared
non-normative policy.

### RF-2 — Complete Brazilian Portuguese README

`README.pt-BR.md` shall translate the current `README.md` content, preserving
commands, identifiers, code blocks, links, status boundaries, and safety
meaning. It shall provide a link back to the English README.

Acceptance: the manifest is current, the file is non-empty, and the checker
finds the reciprocal language switch.

### RF-3 — Localized user portal

`docs/pt-BR/README.md` shall be a Portuguese portal for quick start,
installation, safe operation, troubleshooting, and architecture pointers. It
shall link to canonical English sources and state that it is informational and
non-normative.

Acceptance: each required objective has a working repository-local target and
the checker rejects a portal that claims normative authority.

### RF-4 — Link and freshness integrity

The checker shall reject missing localized files, stale source hashes, broken
local Markdown links, missing reciprocal README switches, malformed manifest
entries, and unsupported states or policies.

Acceptance: named positive and negative tests cover each failure class and the
checker returns deterministic exit codes.

### RF-5 — Localization boundary

The checker shall reject localized files that positively claim normative,
canonical, or official authority. Localized content shall not be added for
PRD, SPEC, IMPL, ADR, CI, evidence, or benchmark logs.

Acceptance: a fixture containing a positive authority claim fails without
printing its sensitive or full matched text; the current localized files pass
with an explicit informational disclaimer.

## 5. Non-functional requirements

### NFR-1 — Language and scope

Structural docs, checker code, tests, manifest keys, and comments are English.
Portuguese appears only in localized content. All content remains RamShared
only; no private paths, identities, credentials, or foreign product narrative
may be introduced.

### NFR-2 — Determinism and bounded resources

The checker uses Node built-ins only, reads at most 512 KiB per text file and
2,000 manifest/link files, sorts findings by path/line/rule, and performs no
network access. Identical bytes produce identical output and exit code.

### NFR-3 — Read-only safety

The checker must not write files, run shell commands, mutate runtime/host
state, or execute manifest content. `docs-check.sh` invokes it as a gate.

### NFR-4 — Coverage

The production checker shall meet at least 80% Node coverage for lines,
branches, and functions using the repository's per-file CI command.

## 6. Flows

### Happy path

1. An author updates canonical English README content.
2. The author updates the Portuguese README and portal.
3. The author records the new canonical README SHA-256 in the manifest.
4. The local checker validates hashes, links, switches, and policy.
5. CI repeats the same read-only check and per-file coverage gate.

### Alternate flows

- A translation is temporarily unavailable: the entry is not `current`; the
  checker fails the required-localization gate until the file is restored.
- A source changed before translation: the checker reports a stale hash; no
  automatic translation or manifest rewrite occurs.

### Error flows

| Trigger | Result | State |
| --- | --- | --- |
| Missing or stale localization | Exit 1 with path/rule finding | No promotion |
| Broken local link or switch | Exit 1 with sanitized finding | No promotion |
| Normative authority claim | Exit 1 with sanitized finding | Localized file rejected |
| Invalid CLI argument or manifest JSON | Exit 2 | Configuration error |

## 7. Data / state model

```text
LocalizationManifest
├─ schema_version: 1
├─ canonical_language: en
├─ current_policy: non-normative-localized-content
├─ required_entries[]
│  ├─ canonical_source
│  ├─ localized_path
│  ├─ source_sha256
│  ├─ state: current
│  └─ policy: informational-non-normative
└─ protected_document_classes[]
```

The source hash is over the canonical file bytes, not normalized Markdown.
Localized files are not treated as alternate technical authorities.

## 8. Interfaces

| Interface | Required behavior |
| --- | --- |
| `node tools/ci/check-documentation-localization.mjs --all` | Read-only full localization gate; exit 0/1/2 |
| `docs/localization/manifest.json` | Strict machine-readable source/policy records |
| `scripts/docs-check.sh` | Runs the localization checker and its tests |
| `.github/workflows/ci.yml` | Runs the per-file localization coverage command |

## 9. Dependencies and risks

Dependencies are Node 22 built-ins and existing repository files only. The
primary risk is a false freshness pass; SHA-256 of exact source bytes and
strict entry validation mitigate it.

Rollback trigger: one stale source hash, broken switch, missing localization,
positive authority claim, or checker write/host mutation reaches a passing
gate. Rollback is to the last passing manifest/checker state; localized files
remain recoverable in version control.

## 10. Implementation strategy

1. Create this PRD and its surgical SPEC.
2. Add named RED tests with temporary sanitized fixtures.
3. Implement manifest/link/hash/policy parsing and the CLI.
4. Add the complete translation and Portuguese portal.
5. Wire docs-check and the narrow CI coverage step.
6. Run focused tests, coverage, link/static checks, and record metrics in
   `IMPL.md` without modifying append-only validation history.

## 11. Documents to update

| Path | Action |
| --- | --- |
| `README.md` | Add reciprocal Portuguese switch link only |
| `README.pt-BR.md` | Create full current README translation |
| `docs/pt-BR/README.md` | Create non-normative user portal |
| `docs/localization/manifest.json` | Create source-hash/policy manifest |
| `tools/ci/check-documentation-localization.mjs` | Create checker |
| `tools/ci/check-documentation-localization.test.mjs` | Create named tests |
| `scripts/docs-check.sh` | Add checker and focused tests |
| `.github/workflows/ci.yml` | Add only localization coverage step |
| `docs/specs/no-milestone/documentation-localization-integrity/IMPL.md` | Record implementation metrics |

Protected files are not changed by this slice: `validation.md`,
`docs/INDEX.md`, `docs/governance/claims.json`, existing evidence manifests,
Windows/Rust code, and unrelated CI jobs.

## 12. Out of scope

- Translating PRD, SPEC, IMPL, ADR, CI, evidence, benchmark, validation, or
  other normative engineering records.
- Translating every document under `docs/` or maintaining a parallel technical
  documentation tree.
- Runtime, kernel, driver, daemon, host, network, or external translation
  service behavior.

## 13. Acceptance criteria

- The current README has a complete human-quality Portuguese translation and
  reciprocal language switches.
- The Portuguese portal covers the five required user objectives through
  working pointers and contains no normative authority claim.
- The manifest has current exact hashes, state, policy, and protected classes.
- Named checker tests pass, required negative fixtures fail, and checker output
  is deterministic and sanitized.
- `scripts/docs-check.sh` includes the checker/tests; localization production
  code meets 80% lines/branches/functions in CI.
- No protected append-only, index, governance-claim, evidence, Windows, Rust,
  or unrelated CI files are changed.

## 14. Validation plan

Run the focused Node tests, per-file coverage command, `node --check`,
`node tools/check-broken-links.mjs --check`, and a read-only checker E2E with
positive and refusal fixtures. Run `scripts/docs-check.sh` and report any
pre-existing concurrent-worktree failures separately; do not rewrite protected
history to make the slice pass.
