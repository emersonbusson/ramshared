---
slug: public-repository-hygiene
title: "Public repository candidate integrity"
milestone: —
---

# PRD — Public repository candidate integrity

> SSDV3 Step 1 · RamShared public-source hygiene only.

## Problem

The existing hygiene gate enumerates tracked paths but reads working-tree
content. It therefore cannot prove the content that would actually be
published: a staged unsafe blob may be hidden by a clean unstaged copy, and a
new non-ignored file is not checked before its first commit. Several public
operator scripts also contain workstation-specific default paths. Extension-
limited public decoding creates another false-green class: invalid UTF-8,
format controls, or bidi-split tokens in `.txt`, `.svg`, `.log`, or another
public text artifact can be mistaken for binary and skipped.
The former image check stopped at PNG chunk framing and JPEG marker framing:
a CRC-valid raw IDAT payload did not have to be a zlib stream, decompressed
scanlines were never bounded or checked, and an abbreviated JPEG with arbitrary
entropy could pass. Coverage-map relocation ownership was also checked only for
the selected diff, allowing a duplicate owner to remain latent in `--all`.
PNG ancillary chunks could carry uninspected textual payloads, while a JPEG
and its digest manifest could be changed together and redefine the supposed
baseline. The coverage planner also accepted duplicate pure line-coverage
owners, followed symlinked SPEC/source/config inputs, and trimmed changed-path
records before validating BOM or control characters. Reusing symbolic `HEAD`
and live index lookups within one checker run also left authority and staged
selection exposed to concurrent movement. High-bit iTXt language bytes, an
indexed PNG `bKGD` outside its PLTE, and UTF-16 or loosely recognized C2PA JPEG
metadata were further parser gaps.

## Goal

Make the public-source gate inspect the selected publication candidate,
fail closed on bounded scan errors, report no sensitive match value, and keep
all shipped defaults portable across Windows and Linux hosts. Make reviewed
public JPEG identity, all line-coverage ownership, and every planner trust
input explicit, immutable, and independently machine checked.

## Requirements

| ID | Requirement |
| --- | --- |
| RF-1 | Candidate mode resolves `HEAD` exactly once to an immutable commit OID and scans the publishable delta against that OID: dirty worktrees use the combined commit-to-worktree delta plus non-ignored untracked files; clean commits use the commit against its first parent, with the empty tree for a root commit. Git topology failures refuse instead of reporting zero files. |
| RF-2 | Staged mode binds path/mode/blob selection to one captured index snapshot, reads captured blob OIDs rather than live `:<path>` names or the working tree, and refuses if the index differs at final revalidation. |
| RF-3 | Every changed artifact in public scope is text by default, independent of extension. Public text decodes as strict UTF-8 and refuses every Cc control except normalized CR/LF/tab plus every Cf/bidi format character before identity or activation matching. |
| RF-4 | Reject private home/profile paths, personal e-mail addresses, credentials/tokens/private keys, and raw kernel addresses. |
| RF-5 | Diagnostics contain only path, line, rule ID, and stable reason; never the matched value. |
| RF-6 | File-count and file-size limits fail closed. Deleted and ignored files do not create false findings. |
| RF-7 | Public operator scripts derive paths from parameters, environment, `$PSScriptRoot`, `$env:USERPROFILE`, or repository-relative locations. |
| RF-8 | `scripts/docs-check.sh` exercises candidate mode; the clean-checkout CI job also invokes candidate mode directly so a newly committed unsafe artifact cannot hide behind local dirty-state semantics. |
| RF-9 | Public binary assets may bypass text decoding only through a bounded explicit extension, signature, and decoder-level contract. PNG must have legal non-interlaced IHDR/PLTE/chunk order, exactly one fully consumed bounded zlib image stream, and only the strict ancillary allowlist; an indexed `bKGD` must name an existing PLTE entry, while `tEXt`, `zTXt`, and `iTXt` are parsed, bounded, decompressed when applicable, privacy-scanned, and validate iTXt language bytes as raw ASCII. JPEG must be a conservative standalone baseline/JFIF stream, reject unsupported APPn/COM profiles, scan Latin-1 and bounded UTF-16LE/BE metadata for private markers, and match an exact tracked regular-file path, size, and SHA-256 in the immutable reviewed digest baseline. The sole non-JFIF metadata profile is one exact structurally validated C2PA APP11/JUMBF tree; only UUIDs inside its credential fields are exempt, while its remaining bytes are privacy-scanned. A candidate cannot redefine the baseline by changing JPEG bytes and the manifest together. New, changed, unlisted, progressive, arithmetic, external-table, unsupported-metadata, or malformed JPEGs refuse. `--check` remains a safe compatibility alias for candidate mode; unknown options refuse. |
| RF-10 | A Git symlink is scanned as its publishable link blob/readlink in candidate, staged, and tracked modes. The checker never follows the final target; an absolute or repository-escaping public target refuses. |
| RF-11 | Coverage-map validation and static `--all` enforce global ownership: every production source has at most one active pure line-coverage owner; a relocation source may additionally have at most one ignored-test proof; each integration source/target and source/target/test-name tuple has exactly one relocation owner; any other overlap refuses before READY. |
| RF-12 | Every planner map, changed-path list, SPEC, production source, integration test, Windows named-test source, and wrapper is a bounded regular non-symlink file whose canonical path remains inside the repository. Changed-path records are decoded without trimming and reject BOM, Cc, or Cf characters anywhere. |

## Non-functional requirements

- NFR-1: zero third-party runtime dependencies; Node 22 and Git only.
- NFR-2: deterministic sorted output and stable exit codes (`0` clean, `1`
  finding, `2` usage or scan failure).
- NFR-3: at least 80% lines, branches, and functions on the production gate.
- NFR-4: read-only; the checker never stages, rewrites, deletes, or publishes.
- NFR-5: no host, driver, WSL, VM, pressure, or reboot operation.

## Acceptance

The slice is complete only when named positive and refusal tests pass, the
production-file cover gate is at least 80% for every metric, candidate and
staged E2E fixtures demonstrate the correct content source, repository
candidate mode reports zero findings, `.txt` plus another public text extension
refuse invalid UTF-8, U+202E split tokens, and C0 controls in both dirty and
clean committed fixtures, leading/interior BOMs and a BOM-prefixed Git path
refuse, legitimate PNG and reviewed manifest-bound baseline JPEG assets pass,
raw/trailing/invalid zlib, decompression bombs, invalid scanline/filter data,
two concatenated zlib streams, illegal PNG headers/palettes/backgrounds/order,
non-ASCII iTXt language bytes, unsupported or sensitive ancillary
metadata, arbitrary or malformed JPEG entropy or metadata, missing tables,
UTF-16 private JPEG metadata, malformed C2PA APP11, same-candidate
JPEG/manifest changes, digest/schema/path/symlink tampering, and
bookended image payloads refuse. Duplicate pure line or relocation owners must
make both direct validation and CLI `--all` BLOCKED rather than READY. External
symlink planner inputs and BOM/Cc/Cf changed paths must fail before selection.
Symlink modes agree on link-blob truth, and two identical runs have identical
output and exit status.

## Rollback trigger

Rollback on one missed staged/untracked sensitive fixture, one public text
artifact skipped because of its extension or invalid UTF-8, one leading BOM or
unsafe Git path normalized away, one run that reuses symbolic `HEAD` or a moved
index without refusal, one binary asset accepted without its declared
signature/decoded-structure/size contract, one JPEG accepted without its exact
tracked immutable manifest identity or strict metadata profile, one duplicate line or relocation owner
accepted globally, one planner trust input followed through a symlink, one
changed path normalized before validation, one final symlink target followed, one clean
commit silently treated as an empty candidate, one diagnostic that prints a
matched value, or one portable script default that resolves to a specific
developer identity.
