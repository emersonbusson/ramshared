---
slug: wsl2-origin-attachment-recovery
title: Attended recovery of the sealed WSL2 origin attachment
milestone: —
issues: []
---

# PRD — Attended recovery of the sealed WSL2 origin attachment

## 1. Summary

Add one attended `attach` action to `Manage-RamSharedOrigin.ps1`. It restores
the already-sealed origin VHDX as a bare WSL2 disk only after proving its fixed
VHDX and GPT identities. It never creates, formats, swaps on, detaches, or
selects the ordinary WSL fallback swap device.

## 2. Technical context

- **Confirmed in codebase:** `ramshared-host-gate.sh` requires the manifest
  PARTUUID to resolve to a distinct WSL block partition before it may publish
  origin authority.
- **Confirmed in codebase:** `Manage-RamSharedOrigin.ps1` can create and prove
  the fixed VHDX and sealed GPT identity, but has no action to re-present that
  VHDX to WSL after a WSL restart.
- **Confirmed locally:** the installed WSL runtime supports bare VHD attachment,
  which exposes a disk without mounting a filesystem.
- **Inference:** a restart can detach an otherwise valid VHDX, so the absence
  of its PARTUUID is an attachment-state problem, not permission to rewrite the
  sealed origin.

## 3. Recommended option

Extend the existing plan-first origin manager with a one-shot `attach` action.
It first checks whether the sealed PARTUUID is already visible to the declared
WSL distro. If it is not visible, the action proves the detached VHDX against
the manifest, executes exactly one bounded bare WSL mount, then polls only the
read-only PARTUUID probe. A missing postcondition is a refusal; it does not
retry the mount, detach any disk, or publish origin authority.

Discarded alternatives:

- Let the Guardian attach storage. Rejected: the Guardian is outside the guest
  failure domain and is intentionally observation/containment-only.
- Recreate the origin on PARTUUID absence. Rejected: absence does not prove
  ownership of a replacement target and would risk the fallback swap.
- Stop all WSL workloads or perform a broad disk unmount. Rejected: both affect
  unrelated WSL state and are not needed to attach one sealed VHDX.

## 4. Functional requirements

- **RF-1:** `attach` requires `-Run`, `-AttendedOriginApply`, the exact origin
  approval token, and an elevated Windows token.
- **RF-2:** The action derives the VHDX, PARTUUID, and GPT disk GUID from the
  sealed manifest; caller-supplied identity never selects a disk.
- **RF-3:** A pre-existing matching WSL PARTUUID returns `ALREADY_ATTACHED`
  without a host mutation.
- **RF-4:** When absent, the action verifies the detached fixed VHDX identity,
  invokes one bounded bare attachment of the sealed VHDX, then requires that
  exact PARTUUID to appear in the declared distro.
- **RF-5:** A timeout, non-zero result, failed postcondition, malformed
  manifest, identity mismatch, or unavailable distro returns a stable refusal.
  It never formats or writes a guest block device, uses the existing fallback
  swap VHDX, stops all WSL workloads, or issues a broad unmount.
- **RF-6:** The action only restores attachment visibility. `ramshared-host-gate.sh`
  remains the owner that may publish origin authority, and cascade activation
  remains a separate attended operation.

## 5. Non-functional requirements

- **NFR-1:** One mount attempt is bounded to 15 seconds. Each read-only WSL
  PARTUUID probe is bounded to 5 seconds; at most five post-mount probes occur.
- **NFR-2:** Native-process diagnostics are not emitted in normal success JSON.
  The result contains only state, action, and sealed identifiers.
- **NFR-3:** The sealed VHDX path must have no whitespace for this action. This
  is a deliberate fail-closed boundary for Windows PowerShell 5.1 native
  command serialization; the default managed origin path satisfies it.

## 6. Flows

1. The operator invokes the origin manager with the attended approval.
2. The manager parses and authenticates the sealed manifest.
3. It performs a bounded root WSL probe for `/dev/disk/by-partuuid/<sealed>`.
4. If present, it returns `ALREADY_ATTACHED`.
5. If absent, it validates the detached VHDX fixed size, GPT GUID, and
   PARTUUID against the manifest.
6. It invokes the one bare VHDX mount and performs bounded read-only probes.
7. On the exact postcondition it returns `ATTACHED`; otherwise it refuses and
   leaves authority unpublished.

## 7. Data and interface model

The manager adds `attach` to its `Action` set. Its result is a JSON object with
`state` (`ALREADY_ATTACHED` or `ATTACHED`), `action`, `partuuid`, `disk_guid`,
and `host_mutation`. No new daemon protocol, kernel ABI, or persistent runtime
record is introduced.

## 8. Dependencies and risks

- WSL 2 must support bare VHD mounts and the target distro must answer bounded
  root probes.
- The VHDX must be detached before the manager performs its ownership proof.
- A mount can take effect near its deadline. A non-proven result is treated as
  ambiguous and requires a new attended invocation; it never triggers a broad
  cleanup.

Rollback trigger: the exact sealed PARTUUID is not visible within five bounded
post-mount probes, or the final `ramshared-host-gate.sh` rejects origin identity.

## 9. Implementation strategy

1. Add the PRD, SPEC, and audit.
2. Add a manufactured decision regression and static contract; validate RED.
3. Implement the bounded, identity-bound attach action and validate GREEN.
4. On the approved host, prove before → `attach` → PARTUUID → host gate.

## 10. Documents to update

- `docs/specs/no-milestone/wsl2-origin-attachment-recovery/{PRD,SPEC,AUDIT-2.5,IMPL}.md`
- `docs/reliability/DEGRADATION-MATRIX.md` after live qualification only.
- `validation.md` after live qualification only.

## 11. Out of scope

- VHDX creation, replacement, formatting, and deletion.
- Automatic startup attachment, Guardian mutation, WSL shutdown, or broad
  unmount.
- NBD lifecycle, swap activation, GPU allocation, and pressure testing.

## 12. Acceptance criteria

- A missing sealed origin partition is restored by one attended, bounded bare
  attachment without modifying the fallback WSL swap.
- A present matching partition is idempotent.
- Identity or postcondition uncertainty fails closed and does not publish
  origin authority.

## 13. Validation plan

- PowerShell manufactured and static source tests for legitimate, already
  attached, malformed, and forbidden-action cases.
- Windows live before → action → after: detached state, VHDX proof, exact
  PARTUUID visibility, and a successful `ramshared-host-gate.sh` receipt.
- The later cascade and stress qualification remains separate.
