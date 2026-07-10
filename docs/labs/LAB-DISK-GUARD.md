# Lab disk guard — keep host Windows healthy

## Goals

1. **Host Windows (C:) never fills** because of lab VMs.  
2. **win11-drill** and **linux-kernel-lab** do not spawn checkpoint piles.  
3. No “cleanup” scripts that **delete VHDs** or run **Convert-VHD/merge** on failure.

## Where labs live

| VM | Disk | Role |
| --- | --- | --- |
| `win11-drill` | **E:\Hyper-V\…** (ESPANHA) | Windows lab only |
| `linux-kernel-lab` | **R:\Hyper-V\…** (RUSSIA) | Linux lab / kernel build |
| `gha-ubuntu-2404` | **V:\Hyper-V\…** | CI (optional) |
| Host OS | **C:** | Never store lab VHD/ISO here |

New VMs default: `R:\Hyper-V\VMs` + `R:\Hyper-V\VHDs` (`Set-VMHost`).

## Hard limits (applied)

| Setting | win11-drill | linux-kernel-lab |
| --- | --- | --- |
| Automatic checkpoints | **Off** | **Off** |
| Checkpoint type | **Disabled** | **Disabled** |
| Auto-start with host | **Nothing** | StartIfRunning |
| Dynamic RAM max | **8 GB** | **8 GB** |
| Dynamic VHD max | **80 GB** | **40 GB** |
| Snapshots now | **0** | **0** |

Script (safe to re-run):

```powershell
# Elevated
.\scripts\windows\Harden-LabVms.ps1
```

## What fills disks (and what we refuse to automate)

| Cause | Effect | Policy |
| --- | --- | --- |
| Hyper-V **checkpoints** (`.avhdx`) | Tens of GB per snapshot | **Disabled** on lab VMs |
| Leaving **ISO** attached forever | ~8 GB | After Win11 install: eject DVD |
| Dynamic VHD growth | Up to max size only | Caps 40G/80G |
| “Cleanup” Convert-VHD / mass delete | Can **destroy** lab disk | **Forbidden** without explicit human + backup |

## After you finish Windows setup (win11-drill)

In elevated PowerShell (does not delete the VHD):

```powershell
# Boot from disk, free ISO attachment
Set-VMDvdDrive -VMName win11-drill -Path $null
$hd = Get-VMHardDiskDrive -VMName win11-drill
Set-VMFirmware -VMName win11-drill -FirstBootDevice $hd
# Re-apply guards
.\scripts\windows\Harden-LabVms.ps1
```

Optional lab UAC (inside guest only): `E:\Hyper-V\scripts\` or copy `R:\Hyper-V\scripts\Disable-Win11LabUac.ps1`.

## Linux lab

- Checkpoints disabled (same script).  
- Kernel build grows **inside** the 40 GB VHD — watch `df -h` in the guest.  
- Do not enable checkpoints “for safety” without pruning — that was the old 100 GB pile.

## Host C: health

- Free space should stay **>> 40 GB**.  
- If C: drops again, run **inventory only** first (`Measure-CDrivePressure.ps1`) — never blind delete.

## Rollback

These settings are non-destructive. To re-enable checkpoints later (not recommended):

```powershell
Set-VM -Name win11-drill -CheckpointType Production
```
