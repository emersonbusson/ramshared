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
operator scripts also contain workstation-specific default paths.

## Goal

Make the public-source gate inspect the selected publication candidate,
fail closed on bounded scan errors, report no sensitive match value, and keep
all shipped defaults portable across Windows and Linux hosts.

## Requirements

| ID | Requirement |
| --- | --- |
| RF-1 | Candidate mode scans tracked working-tree files plus non-ignored untracked files. |
| RF-2 | Staged mode reads every path from the Git index, not from the working tree. |
| RF-3 | Text detection includes supported extensions, known extensionless source files, and UTF-8/shebang text without scanning binary blobs. |
| RF-4 | Reject private home/profile paths, personal e-mail addresses, credentials/tokens/private keys, and raw kernel addresses. |
| RF-5 | Diagnostics contain only path, line, rule ID, and stable reason; never the matched value. |
| RF-6 | File-count and file-size limits fail closed. Deleted and ignored files do not create false findings. |
| RF-7 | Public operator scripts derive paths from parameters, environment, `$PSScriptRoot`, `$env:USERPROFILE`, or repository-relative locations. |
| RF-8 | `scripts/docs-check.sh` exercises candidate mode; CI additionally proves the checked-out tree. |

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
candidate mode reports zero findings, and two identical runs have identical
output and exit status.

## Rollback trigger

Rollback on one missed staged/untracked sensitive fixture, one diagnostic that
prints a matched value, or one portable script default that resolves to a
specific developer identity.
