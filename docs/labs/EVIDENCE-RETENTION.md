# Evidence retention and custody

## Purpose

This policy distinguishes a public repository observation from a qualified
campaign claim and from a protected local export. It is owned by
`reliability-evidence`; it authorizes neither a campaign run nor deletion.

The common static contract is the campaign lifecycle policy in
[`../governance/campaign-evidence-lifecycle.json`](../governance/campaign-evidence-lifecycle.json).
Its generated catalog is an observed-fact view only. Existing files classified
`legacy-unqualified` remain readable but cannot become a PASS, baseline, or
release assertion merely because they are retained.

## Classes

| Class | Location and purpose | Retention | Promotion rule |
| --- | --- | --- | --- |
| `repository-evidence` | Sanitized, hash-inventoried public campaign run | Immutable after terminal publication | Only a valid `complete` manifest with its native surface proof can support a claim. |
| `historical-immutable` | Existing historical evidence observed by the catalog | Preserve; no automatic expiry | Never promotable without a new, independently qualified campaign. |
| `ci-plan` | Sanitized workflow plan or diagnostic emitted by CI | Follow the workflow retention setting; currently 14 days where declared | Never a live campaign result. |
| `release-evidence` | Release manifest and integrity evidence | Retain with the release | Requires its own release-integrity contract. |
| `protected-local-export` | Local, access-controlled source export used by a lab owner | Bound by the owning lab procedure and explicit authorization | Never publish directly; first produce a separate sanitized public envelope. |

## Publication and failure handling

1. A producer writes a new run below an approved `docs/**/evidence/` root.
2. The run remains `writing` until its exact artifact set, hashes, sanitization,
   native before/action/after proof, refusal, and cleanup result have been
   checked.
3. A public `complete` run has no undeclared sibling artifact. A `failed` or
   `blocked` run is retained as its terminal truth and cannot claim `PASS`.
4. The manifest does not self-hash. Its closed sibling inventory is the custody
   boundary; any unlisted temporary file, symlink, non-regular file, checksum
   mismatch, private path, credential, account identifier, or raw kernel
   address is a NO-GO.
5. `node tools/ci/check-campaign-evidence-lifecycle.mjs --check` validates
   repository state. With `--base <git-ref>`, it also rejects a newly added
   evidence artifact that lacks a valid run manifest.

The static checker does not authenticate an author, and a hash alone is not a
source of trust. It checks local integrity and custody shape only. Native
Windows, WSL2, kernel, GPU, binary identity, or physical-host qualification
remains with its owning campaign.

## Deletion boundary

This document creates no deletion mechanism. Do not use campaign retention to
remove a VHD/VHDX, VM, checkpoint, swap, pagefile, volume, active service,
database, container volume, source tree, credential, or protected export.
For workstation-space reasoning, follow
[`../runbooks/WORKSTATION-SPACE-RECOVERY.md`](../runbooks/WORKSTATION-SPACE-RECOVERY.md)
and [`LAB-DISK-GUARD.md`](LAB-DISK-GUARD.md): inventory first, choose an exact
rebuildable target only after separate human approval, and preserve an
append-only receipt.

## Review triggers

Review this policy before adding a producer, a new evidence root, a new
retention class, a public binary artifact, a different host/lab classification,
or any proposal to delete retained data. A proposal that changes Windows,
WSL2, VM, driver, swap, storage, or kernel execution returns to SSDV3 and the
surface owner before implementation.
