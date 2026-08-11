# Document lifecycle policy

`document-lifecycle-policy.json` classifies every tracked Markdown document by
an owner, a canonical source, and one lifecycle: `reviewable`, `historical`,
or `immutable`. It also lists deliberate exclusions, each with an owner and a
reason. A path outside both sets is a failure, not a default classification.

Classification is not verification. Every route currently declares
`verification.state: unverified`; the policy therefore makes no claim that an
individual document was recently reviewed, technically exercised, or approved.
Empirical proof remains in the repository's validation and evidence records.

`reviewable` routes set a bounded `freshnessDays` expectation. `historical`
and `immutable` records deliberately have no freshness interval: their age
must remain visible rather than being converted into a current-product claim.
The checker rejects unsafe paths, duplicate route identifiers, future
timestamps, invalid lifecycle metadata, ambiguous routing, and unclassified
Markdown. With `--base <git-revision>`, it also rejects a changed document
whose lifecycle or freshness guarantee was weakened.

The passive inventory is
[`../reference/DOCUMENTATION-INVENTORY.json`](../reference/DOCUMENTATION-INVENTORY.json).
It is a deterministic coverage snapshot, not an alternative navigation index
and not a verification report.

```bash
node tools/ci/check-document-lifecycle.mjs --all
node tools/ci/check-document-lifecycle.mjs --all --base HEAD~1
node tools/ci/generate-documentation-inventory.mjs --check
```
