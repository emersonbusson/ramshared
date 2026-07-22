---
slug: custom-kernel-ublk-product-transport
title: "Custom-kernel ublk product transport gate"
milestone: —
issues: []
---

# PRD — Custom-kernel ublk product transport gate

## Status

**DEFERRED.** NBD remains the day-1 WSL2 product transport. ublk can only become
a product transport after a dedicated isolated lab proves capability, lifecycle,
swapoff-first teardown, crash/drain behavior, and no ghost swap.

## Summary

RamShared needs a repeatable gate that prevents an old custom-kernel success from
being reported as current product readiness. The gate must distinguish three
claims:

| Claim | Product status |
| --- | --- |
| Hyper-V/WSL lab is reachable | Audit evidence only |
| Kernel exposes ublk capability | Capability evidence only |
| Product cascade can safely use ublk as VRAM transport | Still deferred until full lifecycle proof |

## Requirements

| ID | Requirement |
| --- | --- |
| RF-1 | Provide a read-only capability audit for `linux-kernel-lab`. |
| RF-2 | Require SSH reachability, passwordless sudo, `/dev/ublk-control`, and dry-run `modprobe ublk_drv` before claiming ublk capability. |
| RF-3 | Optionally require a GPU surface (`/dev/dxg`, `/dev/nvidiactl`, or `nvidia-smi`) when the test is meant to support GPU reclaim claims. |
| RF-4 | Keep ublk product transport deferred until a separate full up/down wire-up, crash/drain drill, swapoff-first proof, and no-ghost terminal state exists. |
| NFR-1 | Do not create, resize, attach, format, initialize, merge, or delete disks. |
| NFR-2 | Do not run swap, pressure, or product `ramshared up` workloads in this capability audit. |
| NFR-3 | Do not read or persist credentials. Use SSH keys and documented local-only access. |

## Risks

| Risk | Mitigation |
| --- | --- |
| Old custom-kernel GREEN docs become a false product claim | Current gap register remains authoritative; capability audit produces PASS/PARTIAL only. |
| Disk mutation repeats prior operator damage | Static test bans disk mutation commands in the audit harness. |
| VM access becomes tribal knowledge | Use `docs/labs/HYPERV-VM-ACCESS.md` and the shared access helper. |

Rollback trigger: revert if the audit can mutate disks, run swap/pressure
workloads, read secrets, or mark product ublk transport ready from capability
alone.
