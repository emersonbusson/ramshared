---
slug: comment-language-integrity
title: Canonical English and comment-language integrity
milestone: —
issues: []
---

# PRD — Comment-language integrity

> SSDV3 Step 1 only. This is a RamShared documentation slice for the
> language policy and its read-only gate. It does not translate files or
> change the checker, workflow, validation history, index, claims, or IMPL.

## 1. Summary

RamShared uses English as its canonical authoring language. The rule applies to
source text, comments, user-facing source messages, scripts, workflows,
structural documentation, and engineering records. Brazilian Portuguese is
allowed only in the explicitly localized user surfaces:
`README.pt-BR.md` and the selected `docs/pt-BR/` portal files. A localized file
is informational and is never a second technical authority.

The existing comment gate is a useful narrow starting point, but it is not a
repository-wide language contract. It scans only a fixed set of code/script
extensions, recognizes only comment lines, and requires two distinct marker
words on one line. That shape can pass a Portuguese comment with one strong
marker, a trailing comment, a source diagnostic, or a Markdown paragraph.

The outcome of this slice is a bounded contract for a deterministic,
read-only scanner with a pull-request diff gate and a finite migration ratchet.
The ratchet blocks new legacy findings immediately and reduces the measured
mutable baseline to zero in small reviewable batches. Historical evidence and
append-only history remain untouched; they are inventoried as protected
records, not translated or used as permission for new content.

## 2. Technical context

### Confirmed in codebase

- [`AGENTS.md`](../../../../AGENTS.md), [`CLAUDE.md`](../../../../CLAUDE.md),
  and [`.claude/rules/coding.md`](../../../../.claude/rules/coding.md) require
  English code, comments, filenames, and structural documentation. The current
  coding rule also permits Portuguese issue and PR body text, while the root
  policy says English across the project; this slice treats the canonical
  repository language as the controlling policy for files in the tree and
  does not rewrite governance files.
- [`tools/ci/check-comment-language.mjs`](../../../../tools/ci/check-comment-language.mjs)
  currently scans `.rs`, `.c`, `.h`, `.ps1`, `.sh`, and `.mjs` files. It uses
  `git ls-files` for `--all`, reads only lines that begin with a comment marker,
  and reports a finding only when at least two distinct Portuguese markers are
  present on one line. `--diff` parses added line numbers from a git diff.
- [`.github/workflows/comment-language.yml`](../../../../.github/workflows/comment-language.yml)
  runs the current checker on pull-request additions. It is a diff gate, not a
  full-corpus baseline gate.
- [`docs/specs/no-milestone/documentation-localization-integrity/PRD.md`](../documentation-localization-integrity/PRD.md)
  and [`SPEC.md`](../documentation-localization-integrity/SPEC.md) define the
  localized README and portal contract. This slice reuses their boundary and
  does not duplicate technical documentation in Portuguese.
- [`docs/SSDV3-PROMPTS.md`](../../../../docs/SSDV3-PROMPTS.md),
  [`.claude/rules/documentation.md`](../../../../.claude/rules/documentation.md),
  and [`.claude/rules/ssdv3.md`](../../../../.claude/rules/ssdv3.md) require
  RamShared-only scope, named tests, deterministic evidence, and at least 80%
  per-file coverage for business logic when implementation is claimed.

### Measured legacy inventory

The following snapshot was measured on 2026-08-09 from the current checkout.
It is a candidate inventory produced with the existing marker vocabulary; a
candidate line is not asserted to be Portuguese until the migration owner
classifies it. The counts deliberately exclude `README.pt-BR.md` and every
file under `docs/pt-BR/`.

| Surface | Corpus | Candidate baseline | Meaning |
| --- | ---: | ---: | --- |
| Existing checker target files | 236 files / 57,497 lines | `--all`: 0 findings; `--diff HEAD`: 0 findings | Current gate result, not proof of a clean corpus |
| Target comments, one or more existing markers | 24 files / 72 lines | 72 candidate lines | Current parser misses these because none has two markers on one line |
| All non-localized tracked text, one or more existing markers | 89 files / 1,168 lines | 89 candidate files | Includes source strings, scripts, docs, templates, and tool data |
| All non-localized tracked text, two or more markers | 57 files / 297 lines | 297 candidate lines | Shows the current two-marker rule is broader than its comment result |

The target-file count is 92 Rust, 5 C, 5 headers, 85 PowerShell, 41 shell,
and 8 JavaScript-module files. The candidate inventory is partitioned for safe
migration as follows:

| Partition | Files / candidate lines | Treatment |
| --- | ---: | --- |
| Mutable source | 16 / 88 | Owner migration; no new findings after the diff gate |
| Mutable scripts | 26 / 157 | Owner migration; preserve operational semantics and exit codes |
| Mutable tools | 2 / 14 | Owner migration; scanner data is reviewed separately from prose |
| Mutable docs and templates | 32 / 788 | Canonical English migration in bounded batches |
| Other mutable text | 3 / 14 | Review individually; no new path exception |
| Protected history/evidence | 8 / 60 | Read-only inventory; do not translate or rewrite |
| Protected IMPL records | 2 / 47 | Read-only for this slice; do not edit or translate here |

The partitions are path classes, not a claim that every marker is a language
violation. Protected history/evidence currently includes `CHANGELOG.md`,
`validation.md`, `docs/**/evidence/**`, `docs/postmortems/**`, and
`docs/reliability/**`. Existing `docs/specs/**/IMPL.md` files are also frozen
for this task. Future work may classify a protected record without rewriting
it; the migration denominator is mutable findings only.

## 3. Recommended option

Keep one small Node gate at the existing checker path and evolve its pure
scanner contract in place. The gate has two modes:

1. `--diff <base>` checks only added human-language lines and rejects any new
   non-localized finding, including an attempted addition to a protected
   history path.
2. `--all` scans the bounded tracked corpus and emits a deterministic,
   sanitized inventory. During migration it compares mutable findings with the
   previous recorded baseline; after migration it requires zero mutable
   findings.

The scanner uses a small, reviewed marker vocabulary and line-oriented
comment/text extraction. It is not a probabilistic language detector, a
translation engine, or a formatter. Ambiguous marker matches are classified by
the migration owner and removed from the mutable baseline only by an English
rewrite or by a path that is genuinely protected by this policy. No per-line
ignore syntax is introduced.

### Alternatives rejected

| Alternative | Reason |
| --- | --- |
| Keep the two-marker comment rule | It misses single-marker comments, trailing comments, source diagnostics, and prose files already present in the corpus. |
| Add a network translation or language-detection service | It adds credentials, non-determinism, and a foreign authority to a local CI gate. |
| Auto-translate code, scripts, evidence, or history | It can change safety meaning and would rewrite protected engineering records. |
| Add an unrestricted suppression list | It turns legacy exceptions into permanent policy and defeats a zero target. |
| Scan binary evidence or parse every programming language | It is unbounded and can echo or corrupt evidence; the gate remains text- and path-bounded. |

### Trade-offs

A lexical gate can produce candidate findings and therefore needs a short
human classification step during migration. In return it is reproducible,
offline, cheap to review, and easy to roll back. The allowlist is intentionally
exact: adding a new localized path is a policy change reviewed in English, not
an implicit filename convention.

## 4. Functional requirements

### RF-1 — Canonical language and exact localization allowlist

The policy shall treat English as canonical for all mutable RamShared text.
Portuguese shall be accepted only at `README.pt-BR.md` and explicitly selected
files under `docs/pt-BR/`; the initial portal is
`docs/pt-BR/README.md`. A file named `*.pt-BR.md` elsewhere is not localized by
default.

Acceptance: the scanner accepts the two current localized surfaces, rejects a
new Portuguese line in any other mutable path, and rejects an unregistered
localized-looking path. The localization integrity checker remains the owner
of source freshness, local links, and non-normative disclaimers.

### RF-2 — Bounded deterministic scan

The scanner shall enumerate repository-relative tracked paths in sorted POSIX
order, skip binary data and the exact localized allowlist, and inspect only
bounded UTF-8 text files. It shall cover comments and human-readable text in
the existing source/script extensions plus Markdown and other explicitly
declared text extensions. It shall recognize leading, multiline, and trailing
comment forms without evaluating code.

Acceptance: the same bytes and base ref produce the same ordered findings and
exit code; no network, shell pipeline, runtime/host operation, or file write is
performed; an unreadable, invalid, or over-limit file fails closed with a
configuration result rather than being silently ignored.

### RF-3 — Sanitized diagnostics

Every finding shall contain only a repository-relative path, one-based line
number, stable rule code, and scope (`comment`, `source-text`, or `document`).
Diagnostics shall never echo source lines, matched words, excerpts, absolute
paths, environment values, credentials, or diff payloads.

Acceptance: a fixture containing a secret-like value and Portuguese prose
returns a finding without either value; repeated runs have byte-identical
output.

### RF-4 — Diff gate

`--diff <base>` shall validate only added lines from a validated git base ref.
An added line outside the localization allowlist that contains a language
finding shall exit non-zero. Removed lines and unchanged legacy lines do not
create new findings in this mode. A malformed or missing base ref, unsupported
argument, or unsafe path exits with configuration code 2.

Acceptance: a fixture proves that an old finding does not fail a no-op diff, a
new finding fails, an English replacement passes, a localized README passes,
and a protected-history addition fails without being translated.

### RF-5 — Bounded legacy ratchet

Migration shall begin from the measured candidate baseline and use a finite
batch budget of at most 10 mutable files or 100 candidate lines per change.
Each accepted batch shall make the mutable file count or mutable line count
strictly smaller; neither count may increase. No mutable-path suppression is
allowed. The terminal condition is zero mutable findings in `--all` while the
protected history/evidence set remains unchanged and outside the migration
denominator.

Acceptance: the ratchet test rejects an increase, accepts a strict decrease,
rejects a batch over either bound, and reports the remaining protected count
without requiring a translation.

### RF-6 — Named test and coverage gate

The implementation shall have named unit and integration tests for extraction,
allowlist, diff selection, deterministic output, sanitization, protected paths,
ratchet bounds, and CLI errors. Every production business-logic file in the
slice shall meet at least 80% lines, branches, and functions coverage. A
coverage claim is not closed by workspace-average coverage.

### RF-7 — Read-only and scope safety

This slice shall not alter source, scripts, workflows, validation, index,
claims, evidence, history, or IMPL files. It shall not translate protected
records or call a daemon, driver, GPU, swap, disk, host, or network service.

Acceptance: a before/after tree comparison around the checker shows no file
mutation, and the only files created by this Step 1/2 slice are its PRD and
SPEC.

## 5. Non-functional requirements

### NFR-1 — Resource bounds

The checker shall read at most 512 KiB per text file and 2,000 paths per run,
with a fixed maximum of 10,000 findings retained before deterministic truncation
to an error. The current 621-path checkout is therefore within the declared
bound. Binary files are identified without decoding their payload.

### NFR-2 — Determinism

Path normalization, Unicode normalization, marker matching, finding ordering,
exit codes, and summary counts shall be stable across supported Node 22 runs.
The base ref is passed as an argument to a fixed git executable invocation; no
user-provided shell text is evaluated.

### NFR-3 — Safety and privacy

The checker is read-only and fail-closed. It does not print source content,
matched markers, secrets, private paths, or host state. It never edits a
translation, baseline, manifest, evidence file, or working tree.

### NFR-4 — Coverage and reviewability

The per-file Node coverage gate shall require ≥80% lines, branches, and
functions for `tools/ci/check-comment-language.mjs`. Tests use sanitized
temporary fixtures and do not copy repository content into diagnostics.

### NFR-5 — RamShared-only language

Names, examples, paths, and acceptance evidence refer only to RamShared,
existing repository surfaces, and the local Node/CI tooling. No external
product process or service contract is introduced.

## 6. Flows

### Happy path

1. The author runs the diff gate against the pull request base.
2. The scanner normalizes tracked paths, applies the exact localization and
   protected-path policy, and extracts bounded human-language lines.
3. Findings are sorted and emitted without content; exit 0 permits review.
4. A migration owner runs the full inventory, updates at most 10 files or 100
   mutable candidate lines in English, and records a strict ratchet decrease.
5. Once the mutable count reaches zero, the full inventory is a clean gate;
   localized portal content is checked by the separate localization gate.

### Alternate flows

- A localized README or portal line is accepted by the language gate and is
  still subject to the localization manifest, link, switch, and disclaimer
  checks.
- A protected evidence/history file is scanned for inventory visibility but is
  not translated. A new Portuguese addition to that path fails the diff gate.
- An ambiguous marker is classified as a false positive only by removing or
  rewriting the mutable line; no inline ignore is added.

### Error flows

| Trigger | Result | State |
| --- | --- | --- |
| New non-localized finding in a diff | Exit 1 with sanitized path/line/rule | Review blocked |
| Mutable baseline grows or batch exceeds 10 files/100 lines | Exit 1 | Migration blocked |
| Malformed base, argument, UTF-8, or file bound | Exit 2 | Configuration error |
| New protected-history addition | Exit 1 | Protected record preserved; change rejected |
| Scanner write, host, network, or content echo observed | Exit 1 and rollback | Gate invalid |

## 7. Data / state model

```text
LanguagePolicy
├─ canonical_language: en
├─ localized_allowlist: [README.pt-BR.md, docs/pt-BR/README.md]
├─ protected_history: [CHANGELOG.md, validation.md, docs/**/evidence/**,
│                      docs/postmortems/**, docs/reliability/**]
├─ protected_impl: docs/specs/**/IMPL.md
└─ mutable_paths: tracked text not in the sets above

Finding
├─ path: repository-relative POSIX path
├─ line: positive integer
├─ rule: stable LANG-* code
└─ scope: comment | source-text | document

Ratchet
├─ baseline_mutable_files: measured snapshot
├─ baseline_mutable_lines: measured snapshot
├─ batch_limit: 10 files or 100 lines
└─ terminal_mutable_findings: 0
```

The scanner does not persist a mutable baseline or rewrite files. A future
implementation may store a reviewed baseline as a separate English CI record,
but this Step 1/2 slice intentionally creates no inventory, claims, evidence,
or index file.

## 8. Interfaces

| Interface | Required behavior |
| --- | --- |
| `node tools/ci/check-comment-language.mjs --diff <base>` | Added-line gate; exit 0 clean, 1 policy finding, 2 configuration |
| `node tools/ci/check-comment-language.mjs --all` | Full bounded inventory and ratchet check with sanitized output |
| `.github/workflows/comment-language.yml` | Future owner wires the same diff command; no workflow change in this slice |
| `README.pt-BR.md` and `docs/pt-BR/README.md` | Exact localized allowlist; freshness and links remain localization-gate responsibilities |

## 9. Dependencies and risks

Dependencies are the existing Node runtime, `git` diff/path metadata, and the
repository text corpus. The main risks are false positives from short marker
words and accidental policy drift in the localized allowlist. Context-aware
comment extraction, exact path matching, sanitized findings, and mandatory
human review of the finite baseline mitigate those risks.

Rollback trigger: any non-zero mutable finding count increase, any new
Portuguese diff line passing outside the allowlist, any protected record being
rewritten, any diagnostic containing source content, or any observed write,
network call, or host mutation. Revert to the last passing scanner contract and
leave source/history bytes unchanged.

## 10. Implementation strategy

1. Keep this PRD and its surgical SPEC as the only files changed in this slice.
2. Refactor the current scanner behind pure functions without changing its
   read-only boundary; add the exact path policy and sanitized finding shape.
3. Add RED tests with temporary fixtures for one-marker comments, trailing and
   multiline comments, source/document text, allowlist, protected paths, diff
   selection, sanitization, bounds, and ratchet behavior.
4. Implement the deterministic scanner and run the full candidate inventory;
   classify only mutable candidates and preserve the protected count.
5. Wire the diff gate and coverage in a later implementation change, then
   migrate no more than the declared batch budget per review.
6. Close only when the mutable inventory is zero; environment-bound or
   historical records are not silently reclassified as DONE.

## 11. Documents to update

| Path | Action |
| --- | --- |
| `docs/specs/no-milestone/comment-language-integrity/PRD.md` | Create in this slice |
| `docs/specs/no-milestone/comment-language-integrity/SPEC.md` | Create in this slice |
| `tools/ci/check-comment-language.mjs` | Future implementation owner; not modified here |
| `tools/ci/check-comment-language.test.mjs` | Future named-test owner; not modified here |
| `.github/workflows/comment-language.yml` | Future diff-gate owner; not modified here |
| `README.pt-BR.md` | Protected localized surface; not translated here |
| `docs/pt-BR/README.md` | Protected localized portal; not translated here |
| `validation.md`, `docs/INDEX.md`, claims, evidence, history, and IMPL | Protected; no changes |

## 12. Out of scope

- Translating or editing code, scripts, workflows, validation, the docs index,
  governance claims, evidence, history, or any IMPL file.
- Translating PRD/SPEC/ADR/reliability records in this turn.
- Adding Portuguese content to marketing, issue, PR, or other paths outside
  the exact localized allowlist.
- Runtime, kernel, driver, daemon, GPU, swap, host, network, or translation
  service behavior.
- Probabilistic language identification, automatic rewriting, or broad
  per-path suppression frameworks.

## 13. Acceptance criteria

- This PRD and SPEC contain only English structural documentation and point to
  existing RamShared paths.
- The measured baseline records the current checker result and the broader
  candidate inventory without echoing source content.
- The future scanner contract is bounded, deterministic, read-only, sanitized,
  exact about localized paths, and fail-closed on new diff findings.
- The migration ratchet has explicit numeric limits and a zero mutable target;
  protected history/evidence remains unmodified and outside that denominator.
- The SPEC names tests and the ≥80% per-file coverage gate.
- No file outside this new directory is modified by this slice.

## 14. Validation plan

- Run a scoped Markdown-link check against these two files and their repository
  targets; do not regenerate or validate the shared index in this slice.
- Run `git diff --check` against the new directory.
- Run a Markdown/frontmatter syntax check that confirms both files have the
  required English SSDV3 frontmatter and headings.
- Do not claim scanner, workflow, coverage, migration, or full-repository
  closure until the future implementation and its named tests exist.
