# Documentation governance

RamShared documentation ownership is indexed by
[`DOCUMENTATION-PARITY.md`](../DOCUMENTATION-PARITY.md) and the
objective router in [`REFERENCE-INDEX.md`](../reference/REFERENCE-INDEX.md).
Capability state is explicit in [`claims.json`](claims.json); an `IMPL.md`
without a qualified claim is `UNQUALIFIED`, not `DONE`.

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
