# Documentation governance

RamShared documentation ownership is indexed by
[`DOCUMENTATION-PARITY.md`](../DOCUMENTATION-PARITY.md) and the
objective router in [`REFERENCE-INDEX.md`](../reference/REFERENCE-INDEX.md).
Capability state is explicit in [`claims.json`](claims.json); an `IMPL.md`
without a qualified claim is `UNQUALIFIED`, not `DONE`.

Each record in [`claim-closures.json`](claim-closures.json) binds the complete
claim object through `claim_sha256`. Canonical JSON recursively sorts object
keys by UTF-16 code-unit order, preserves array order, serializes JSON
primitives without whitespace, and hashes the resulting UTF-8 bytes with
SHA-256. Owner, state, rollback, and evidence changes therefore invalidate a
stale or replayed closure. The exact shape is defined by
[`claim-closures.schema.json`](claim-closures.schema.json).

The repository also keeps a bounded lifecycle policy for every Markdown file
in [`DOCUMENT-LIFECYCLE.md`](DOCUMENT-LIFECYCLE.md), a passive
[`../reference/DOCUMENTATION-INVENTORY.json`](../reference/DOCUMENTATION-INVENTORY.json),
and temporal contracts for [`../../TASK.md`](../../TASK.md) and
[`../../validation.md`](../../validation.md) in
[`RECORD-SCHEMAS.md`](RECORD-SCHEMAS.md). Classification or an inventory entry
is never a technical verification claim.

Campaign evidence has its own custody policy in
[`campaign-evidence-lifecycle.json`](campaign-evidence-lifecycle.json) and a
deterministic observed catalog. Retention boundaries, including the prohibition
on automatic lab/data deletion, are in
[`../labs/EVIDENCE-RETENTION.md`](../labs/EVIDENCE-RETENTION.md). Historical
workstation-space observations live in the separately non-promotable receipt
ledger documented by
[`../runbooks/WORKSTATION-SPACE-RECOVERY.md`](../runbooks/WORKSTATION-SPACE-RECOVERY.md).

[`CAPABILITY-OBSERVATIONS.md`](CAPABILITY-OBSERVATIONS.md) explains the passive
capability catalog: each entry remains `OBSERVED`, even when reconciled with a
separate claim. The cross-cutting authority and custody boundaries are recorded
in [`../security/THREAT-MODEL.md`](../security/THREAT-MODEL.md) and
[`../decisions/ADR-0008-evidence-and-document-lifecycle.md`](../decisions/ADR-0008-evidence-and-document-lifecycle.md).

The repository-local CI contract is
[`ci-contract.json`](ci-contract.json). Its strict checker keeps an
administrator-only remote-control gap non-zero; its local admission mode may
report `PARTIAL` only when that remote gap is the sole remaining condition.
The topology and release boundaries are documented in
[`../ci/CI-TOPOLOGY.md`](../ci/CI-TOPOLOGY.md) and
[`../ci/RELEASE-INTEGRITY.md`](../ci/RELEASE-INTEGRITY.md). Repository source
never mutates branch protection, token defaults, action policy, environment
reviewers, retention defaults, or signing authority.

Run the complete read-only gate:

```bash
node tools/ci/check-documentation-governance.mjs --all
./scripts/docs-check.sh
```

Journey metadata is declarative and never executed. Provenance exceptions are
finite, reviewed records; secrets, private identities, and raw kernel addresses
cannot be allowlisted.
