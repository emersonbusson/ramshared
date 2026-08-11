---
slug: campaign-evidence-lifecycle
title: Campaign evidence lifecycle and custody
milestone: —
issues: []
---

# PRD — Campaign evidence lifecycle and custody

> SSDV3 governance slice. It changes neither the Linux cascade, kernel module,
> CUDA allocation, Windows driver, VM, swap, disk, service, nor host state.

## Problem

RamShared has platform-specific evidence with strong local checks, but its
historical campaign directories do not share one custody and lifecycle contract.
An artifact can therefore be present without saying whether it is a current,
complete, failed, blocked, or historical observation. A filesystem discovery
must never turn that presence into a promotion claim.

The repository contains Linux/WSL2, Windows, kernel-lab, benchmark, and
documentation evidence. Their valid proof remains native to each surface:
before → action → after, legitimate operation, named refusal, cleanup, and
platform-specific identity checks. This slice adds a common *description and
verification boundary* around those artifacts; it does not replace their
runners or manufacture live proof.

## Outcome

New campaign evidence can be published only as a bounded, sanitized,
repository-relative run with a lifecycle, immutable source identity, owner,
before/action/after summary, cleanup result, refusal coverage, and a closed
hash inventory. Historical files are classified as immutable
`legacy-unqualified`; they remain readable but cannot be used as a current PASS
or baseline merely because they exist.

## Constraints

- No web screenshot gallery, browser test, tenant model, API model, or foreign
  product vocabulary belongs in this slice.
- The checker is read-only. It must not start a VM, run a driver, mutate swap,
  remove a disk, delete an artifact, invoke Docker, or call a campaign runner.
- A new campaign producer remains owned by its Linux, WSL2, Windows, or kernel
  harness. A generic executor is deliberately out of scope.
- No private path, account, credential, raw kernel address, or unredacted host
  identifier may enter a public run manifest or catalog.
- Evidence failure and environment blocking are retained as states, never
  overwritten as a success.

## Acceptance criteria

1. A schema and checker validate new `campaign-manifest.json` files with a
   bounded lifecycle, owner, exact UTC timestamps, source revision/dirtiness,
   surface, environment classification, custody fields, before/action/after,
   legitimate and refusal outcomes, cleanup/residue, and a hash inventory.
2. The checker refuses absolute/traversing paths, symlinks, oversized or
   duplicate inventory entries, malformed hashes, sensitive text, lifecycle
   inconsistency, incomplete publication, future timestamps, and artifacts not
   in the declared closed set.
3. Existing evidence is catalogued as historical and immutable without being
   retroactively recertified. A new unmanifested campaign file in a diff fails
   the prospective ratchet.
4. The generated catalog distinguishes observed artifact from qualified proof.
5. A retention policy documents repository evidence, protected local export,
   CI plans, and release evidence. It offers inventory/preview only; VHD, VM,
   swap, active service, and volume deletion remain outside this capability.
6. Named Node tests exercise legitimate validation and each critical refusal.
   Static documentation checks and index generation remain green.

## Rollback trigger

Rollback this governance slice if one unmanifested new campaign file is
accepted, one `complete` run can publish with an unverified artifact, one
private/sensitive value is emitted by diagnostics, or a checker causes a host
or lab mutation.
