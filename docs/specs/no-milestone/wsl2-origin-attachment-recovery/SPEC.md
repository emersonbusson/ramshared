# SPEC — Attended recovery of the sealed WSL2 origin attachment

## Closed scope

In: one attended Windows origin-manager action that reattaches only the sealed
VHDX as a bare WSL disk and proves the sealed PARTUUID is visible. Out: VHDX
creation/deletion, formatting, swap/NBD lifecycle, Guardian mutation, automatic
startup, WSL shutdown, and broad unmount. Dependencies: the existing manifest
schema-3 reader, `Get-OriginVhdxOwnershipProof`, and WSL bare-VHD mount support.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | DT-1, ITEM-2 |
| RF-2 | DT-2, ITEM-2 |
| RF-3 | DT-3, ITEM-2 |
| RF-4 | DT-2, DT-4, ITEM-2 |
| RF-5 | DT-4, DT-5, ITEM-2 |
| RF-6 | DT-5, ITEM-3 |
| NFR-1..3 | DT-4, ITEM-2 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | `attach` uses the existing exact attended approval and additionally requires an administrator token. | A WSL VHD mount is host storage mutation. |
| DT-2 | The action reads the sealed manifest and matches `Get-OriginVhdxOwnershipProof` before the one mount operation. | A path and size alone cannot select the origin. |
| DT-3 | A bounded root WSL PARTUUID probe is performed before mutation; exact presence returns `ALREADY_ATTACHED`. | Replaying attach is 1× and does not create a second attachment. |
| DT-4 | Native WSL calls use only validated no-whitespace argument tokens, a 15-second mount deadline, 5-second probes, and five post-mount observation attempts. | Windows PowerShell 5.1 precomposed command lines can preserve literal quotes; bounded read observations are safe after one mount attempt. |
| DT-5 | The action never writes a guest block device, detaches a disk, publishes `/etc/ramshared/origin.conf`, or invokes a WSL lifecycle command. | Attachment is not origin authority or cascade activation. |

## Atomicity and rollback

- **Windows host:** VHDX proof is read-only. The sole effect is one bare WSL
  attach after proof. There is no automatic rollback because an ambiguous mount
  cannot be safely associated with a disk number for a broad unmount.
- **Guest:** probes are read-only. `ramshared-host-gate.sh` independently
  verifies the resulting block/GPT/swap identity before publishing authority.
- **Daemon/kernel:** N/A; the action does not start/stop the daemon, swap, or a
  driver.
- **Forward-only frontier:** after a bounded mount attempt, absence of the exact
  PARTUUID is `BLOCKED` and requires a later attended inspection; no retry or
  cleanup is inferred.

## Kahneman map

| ITEM / stage | # | Question | Minimum evidence | Abort |
| --- | --- | --- | --- | --- |
| Pre-mount identity | #13 | Does absence authorize a different disk? | `origin_attach_decision_is_idempotent_and_fail_closed` | Manifest/proof mismatch or foreign PARTUUID. |
| Mount/postcondition | #15 | Is repetition justified by a transient signature? | Bounded one-mount live receipt plus five read-only probes | Timeout, non-zero result, or no exact PARTUUID; no mount retry. |
| Persistent boundary | #16 | Can recovery damage an active disk or fallback swap? | Static forbidden-action contract and host-gate refusal tests | Any format, detach, fallback-swap, or authority publication path. |
| Replay | #17 | Does 2× equal 1×? | Manufactured already-attached decision test | A present exact PARTUUID invokes mount. |

## Security checklist

- [x] Privilege: explicit approval plus administrator token.
- [x] User/host input: path comes from the sealed manifest and is whitespace-refused for this command boundary.
- [x] Flags: fixed bare-VHD attachment mode; no caller-provided disk number or mount options.
- [x] Info leak: success output contains only sealed identifiers.
- [x] Lifetime: no driver/kernel handle is retained.
- [x] Hot-unplug/device-gone: failure preserves `BLOCKED`; host gate revalidates identity.
- [x] Host safety: no pressure, swap, WSL shutdown, or broad unmount.
- [x] Replayable ops: presence before mutation returns `ALREADY_ATTACHED`.

## Files to create / modify / delete

**`scripts/windows/Manage-RamSharedOrigin.ps1`**
- Modify action validation and add bounded native invocation, exact guest
  PARTUUID probe, pure attachment decision, and attended `attach` flow.
- RF / DT: RF-1..6; DT-1..5.
- Required tests: `Invoke-OriginManufacturedTests` ::
  `origin_attach_decision_is_idempotent_and_fail_closed`; static suite.
- Cover target: N/A — PowerShell host orchestration; live E2E required.

**`scripts/windows/Test-RamSharedOriginStatic.ps1`**
- Modify static requirements for the new action and forbid broad WSL/storage
  operations.
- RF / DT: RF-1, RF-5; DT-1, DT-5.

**`docs/specs/no-milestone/wsl2-origin-attachment-recovery/*`**
- Create this bounded storage-recovery specification and implementation record.

Delete: none.

## Observability

| Signal | Where | Pass condition |
| --- | --- | --- |
| Attachment result | Origin-manager JSON | `ALREADY_ATTACHED` or `ATTACHED` with sealed identifiers. |
| Guest visibility | bounded PARTUUID probe | Exact `/dev/disk/by-partuuid/<sealed>` is present. |
| Authority | `ramshared-host-gate.sh` | Independent host-gate receipt succeeds. |

## Living docs

| Document | Action |
| --- | --- |
| `docs/reliability/DEGRADATION-MATRIX.md` | Update only after live proof. |
| `validation.md` | Append only after live proof. |
| `README.md` | N/A; this is an operator recovery detail. |
| Agent rules | N/A; no convention changes. |

## Implementation order

1. Add source/static RED coverage for the attachment decision and forbidden
   host actions.
2. Implement the action and run the focused PowerShell suites.
3. Run an attended live before → attach → host-gate proof.
4. Record the environment-bound result in `IMPL.md`; update public/reliability
   records only if the full live proof passes.

## Required tests matrix

| Production path | Test | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| attachment decision | `Manage-RamSharedOrigin.ps1` :: `origin_attach_decision_is_idempotent_and_fail_closed` | manufactured | #13/#17 | N/A — PowerShell orchestration |
| native mount boundary | `Test-RamSharedOriginStatic.ps1` :: static attach contract | static | #15/#16 | N/A — PowerShell orchestration |
| host attachment | exact PARTUUID before → `attach` → `ramshared-host-gate.sh` | live E2E | #13/#15 | environment-bound |

## Validation checklist

- [ ] Focused manufactured origin-manager test.
- [ ] `Test-RamSharedOriginStatic.ps1`.
- [ ] `./scripts/docs-check.sh`.
- [ ] Live VHDX proof, exact PARTUUID visibility, and host-gate receipt.
- [ ] No swap/cascade/stress action as part of attachment validation.
