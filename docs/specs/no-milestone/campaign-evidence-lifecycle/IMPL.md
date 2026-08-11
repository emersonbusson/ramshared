# IMPL — Campaign evidence lifecycle and custody

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/campaign-evidence-lifecycle/SPEC.md`

## Status

implemented · cover N/A for Rust · static repository E2E ✓ · BINARY_MATCH N/A

This slice implements a read-only documentation and custody gate. It does not
run or qualify a WSL2, Windows, VM, GPU, driver, storage, or kernel campaign.
Those native paths remain environment-bound under their own owners.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `docs/governance/campaign-evidence-lifecycle.json` | ITEM-1 / RF-1–RF-4 | Explicit roots, roles, retention classification, and bounded artifact limits. |
| `tools/ci/check-campaign-evidence-lifecycle.mjs` | ITEM-2–ITEM-5 / RF-1–RF-9 | Read-only manifest validation, historical catalog generation, integrity and prospective-diff ratchet. |
| `tools/ci/check-campaign-evidence-lifecycle.test.mjs` | ITEM-2–ITEM-5 / RF-1–RF-9 | Legitimate fixture plus lifecycle, traversal, symlink, tamper, orphan, sensitive-content, bounds, and ratchet refusals. |
| `docs/governance/campaign-evidence-catalog.generated.json` | ITEM-3 / RF-5 | Deterministic observed catalog; legacy records are non-promotable. |
| `docs/labs/EVIDENCE-RETENTION.md` | ITEM-4 / RF-6 | Public, historical, CI, release, and protected-local retention boundaries. |
| `scripts/docs-check.sh`, `.github/workflows/ci.yml` | ITEM-6 / RF-10 | Static admission and pull-request diff checks. |

## Validation (numbers)

- lifecycle tests: `node --test tools/ci/check-campaign-evidence-lifecycle.test.mjs`
  → 9 passed, 0 failed.
- legitimate repository gate:
  `node tools/ci/check-campaign-evidence-lifecycle.mjs --check` → exit 0;
  181 observed entries, all existing historical evidence marked
  `legacy-unqualified` unless a future valid v1 manifest says otherwise.
- deterministic catalog: generate then check produces byte-identical output
  for identical repository input.
- refusals: invalid state/time, unsafe path, sensitive artifact, symlink,
  checksum/byte tamper, orphan file, oversize historical file, stale catalog,
  and newly added unmanifested evidence all return NO-GO in named fixtures.
- live E2E: N/A by design. No hardware, guest, GPU, service, driver, VM, swap,
  disk, or kernel path is invoked by this slice.

## Gaps

The repository starts with historical observations, not retroactive campaign
promotion. Native producers must still publish a new, approved v1 manifest
after their own surface-specific before/action/after, binary identity,
legitimate/refusal, and cleanup gates complete. A local hash checks integrity
of a checked-out artifact; it does not authenticate an experiment author.

## Rollback trigger

One accepted unmanifested new evidence artifact, one accepted unsafe or
undeclared complete-run member, one sensitive diagnostic echo, a stale catalog
accepted as current, or any host/lab mutation caused by this checker.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1–RF-10 | ITEM-1–ITEM-6 | pending — no automatic commit |
