# SPEC — Campaign evidence lifecycle and custody

## Scope and ownership

This is a documentation and evidence-governance contract owned by
`reliability-evidence`. It is not a kernel, DMA, MMU, uAPI, driver, or runtime
implementation change. Native campaign scripts continue to own execution.

## Canonical paths

| Role | Path |
| --- | --- |
| Lifecycle policy | `docs/governance/campaign-evidence-lifecycle.json` |
| Historical/observed catalog | `docs/governance/campaign-evidence-catalog.generated.json` |
| Checker | `tools/ci/check-campaign-evidence-lifecycle.mjs` |
| Tests | `tools/ci/check-campaign-evidence-lifecycle.test.mjs` |
| Retention policy | `docs/labs/EVIDENCE-RETENTION.md` |
| This SPEC | `docs/specs/no-milestone/campaign-evidence-lifecycle/SPEC.md` |

`docs/INDEX.md` remains an SSDV3 index. The generated catalog is a separate
observed-fact view and never changes an SSDV3 status.

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | The historical catalog and discovered campaign manifests are derived only from repository paths returned by `git ls-files` beneath configured evidence roots. Ignored or untracked local files, directories, symlinks, and logs do not change the catalog or the repository check. Failure to obtain the tracked-path set is a terminal `tracked-files` finding. The prospective `--base` ratchet remains responsible for newly added tracked evidence. | The catalog is committed public metadata and must be byte-identical in a developer workspace and a clean GitHub Actions checkout. Local forensic logs must not create stale generated output or a false hosted failure. |

## Manifest v1

New runs use `campaign-manifest.json` inside a single run directory under an
existing `docs/**/evidence/` directory. The run directory is the only artifact
root visible to the checker.

```text
schema_version: ramshared-campaign-evidence/v1
run_id: lowercase-hyphenated identifier
owner_role: role name, never a person/account
surface: wsl2-freeze | windows-storage | kernel-lab | benchmark | other
lifecycle: writing | complete | failed | blocked
claim_state: PASS | PARTIAL | FAIL | BLOCKED
started_at / finished_at / published_at: RFC3339 UTC instants
source: { commit: 40 lowercase hex, dirty: boolean }
environment: { tier: isolated | shared | physical, sanitization: public }
before / action / after: bounded summaries
legitimate: { name, verdict }
refusals: [{ name, verdict }]
cleanup: { complete: boolean, residue: non-negative integer }
artifacts: [{ path, bytes, sha256, sanitized }]
retention: { class, immutable, review_by? }
rollback_trigger: observable or numeric text
```

`complete` requires `PASS` or `PARTIAL`, a `published_at` not before
`finished_at`, at least one PASS legitimate case, one named refusal, cleanup
complete with zero residue, and a non-empty exact artifact inventory. `failed`
and `blocked` are terminal and may retain partial inventory, but cannot use
claim state `PASS`. `writing` is non-published and cannot claim any final
state.

The manifest does not hash itself. The checker verifies every declared regular
artifact and requires the run directory to contain exactly the manifest plus
the declared inventory. This avoids a self-hash while making additions and
orphan temporary files terminal.

## Historical catalog and ratchet

The catalog generator scans only Git-tracked `docs/**/evidence/**` files with
bounded file and byte limits. Existing groups become immutable
`legacy-unqualified` observations with an owner and reason from the policy.
They are deliberately ineligible for PASS promotion.

With `--base <git-ref>`, a file added beneath an evidence root must belong to a
run containing a valid manifest. This applies prospectively only; it never
rewrites or invalidates historical artifacts. The policy has no broad opt-out.

## Security and custody refusals

The checker fails closed for each of the following:

| Refusal | Required result |
| --- | --- |
| Absolute path, `..`, or duplicate path | NO-GO before artifact read |
| Symlink or non-regular artifact | NO-GO |
| Hash/byte mismatch or undeclared file | NO-GO |
| Private path, credential, private key, raw kernel address, or account identifier | NO-GO with sanitized diagnostic |
| Timestamp in the future or invalid lifecycle transition | NO-GO |
| `PASS` with incomplete cleanup/residue, missing legitimate case, or no refusal | NO-GO |
| New unmanifested evidence in a diff | NO-GO |

## Test matrix

| Test | Purpose |
| --- | --- |
| `valid_complete_run_passes` | Legitimate complete run has a closed, hash-verified inventory. |
| `writing_and_failed_runs_remain_nonpromoting` | In-progress and failed evidence cannot become a PASS. |
| `unsafe_or_sensitive_artifact_is_refused` | Path, symlink, size, and sanitization boundaries fail closed. |
| `inventory_tamper_and_orphan_are_refused` | Hash/bytes and exact-set custody are enforced. |
| `historical_catalog_is_observed_not_qualified` | Legacy evidence remains visible but ineligible for promotion. |
| `catalog_ignores_untracked_local_evidence` | An ignored local artifact does not change the committed catalog. |
| `cli_catalog_generation_uses_tracked_git_paths` | The public `--generate` and `--check` commands use Git-tracked evidence only and reject invalid arguments. |
| `repository_check_refuses_missing_tracked_file_source` | A repository check without a trustworthy tracked-file source is terminal. |
| `diff_ratchet_refuses_new_unmanifested_evidence` | Future evidence adopts the contract without rewriting history. |

The canonical coverage gate for `tools/ci/check-campaign-evidence-lifecycle.mjs`
uses Node's built-in per-file thresholds of at least 80% lines, branches, and
functions. It is a static repository gate; no host or campaign runner is
executed.

## Platform gates and limits

The checker is a static local gate only. It does not qualify a Windows physical
campaign, a WSL2 shared-host campaign, GPU pressure, BINARY_MATCH, or a kernel
drill. Those remain `PARTIAL` until their own approved before/action/after
campaigns run. No host or lab action is authorized by this SPEC.

## Rollback trigger

One accepted unmanifested newly added artifact, one complete run with a missing
or altered inventory member, any sensitive diagnostic echo, or any invocation
that mutates a host/lab boundary is a rollback trigger.
