# IMPL — custom-kernel-ublk-product-transport

## Status

**PARTIAL / DEFERRED.** The capability audit and static safety gate exist.
Product ublk transport remains deferred; NBD is still day-1 on WSL2.

## Implemented

| Item | Result |
| --- | --- |
| Read-only lab capability audit | `scripts/windows/Invoke-LinuxKernelLabCapabilityAudit.ps1` |
| Static disk/secret/pressure safety test | `scripts/windows/Test-LinuxKernelLabCapabilityAuditStatic.ps1` |
| Current product status | ublk capability does not close product transport readiness |

## Required Future Evidence

Product ublk transport requires a separate isolated campaign with:

- full `ramshared up/down` ublk wire-up;
- swapoff-first teardown proof;
- crash/drain drill;
- no ghost swap terminal state;
- no daily-host WSL2 pressure.

Rollback trigger: revert if this implementation marks ublk product transport
ready without the full lifecycle campaign.
