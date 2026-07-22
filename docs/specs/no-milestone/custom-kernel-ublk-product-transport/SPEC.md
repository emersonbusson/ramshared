# SPEC — custom-kernel-ublk-product-transport

> Passo 2 SSDV3. Implements [`PRD.md`](PRD.md).

## Scope

In now:

- Read-only `linux-kernel-lab` access and capability audit.
- Static safety test for disk, secret, and host-pressure exclusions.
- Gap-register wording that keeps ublk product transport **DEFERRED**.

Out now:

- Formatting, attaching, creating, resizing, merging, or deleting disks.
- Running swap pressure, `ramshared up`, or ublk product workloads.
- Declaring ublk product transport ready from capability alone.

## Design

| Decision | Spec |
| --- | --- |
| DT-1 | Use `scripts/windows/Get-LinuxKernelLabAccess.ps1` for VM/IP/SSH discovery. |
| DT-2 | New audit emits `STATUS=PASS` only when SSH, sudo, `/dev/ublk-control`, and dry-run `modprobe ublk_drv` pass. |
| DT-3 | `-RequireGpuSurface` additionally requires `/dev/dxg`, `/dev/nvidiactl`, or `nvidia-smi`. |
| DT-4 | Missing capability is `STATUS=PARTIAL` / exit 2, not a harness crash. |
| DT-5 | The audit writes only artifact JSON under `C:\ramshared\artifacts`. |

## Files

| Path | Action |
| --- | --- |
| `scripts/windows/Invoke-LinuxKernelLabCapabilityAudit.ps1` | Create read-only capability audit. |
| `scripts/windows/Test-LinuxKernelLabCapabilityAuditStatic.ps1` | Create static safety test. |
| `docs/reliability/GAP-REGISTER.md` | Keep product transport open with the new audit as current evidence. |
| `docs/specs/no-milestone/wsl2-custom-kernel-p1/IMPL.md` | Clarify old GREEN was historical capability evidence, not current product transport closure. |

## Validation

Static:

```powershell
pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/windows/Test-LinuxKernelLabCapabilityAuditStatic.ps1
```

Live capability audit:

```powershell
pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/windows/Invoke-LinuxKernelLabCapabilityAudit.ps1 -Start
```

Live GPU-surface audit:

```powershell
pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/windows/Invoke-LinuxKernelLabCapabilityAudit.ps1 -Start -RequireGpuSurface
```

## Close Evidence

The product transport gate stays open until a future SPEC adds full up/down
wire-up, swapoff-first teardown, crash/drain drills, and terminal no-ghost
evidence in an isolated lab.

Rollback trigger: revert if a PASS from this capability audit is treated as
product ublk transport readiness.
