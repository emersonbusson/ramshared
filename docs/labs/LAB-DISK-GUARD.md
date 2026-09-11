# Lab disk guard — keep host Windows healthy

## Goals

1. **Host Windows (C:) never fills** because of lab VMs.  
2. **isolated-win-vm** and **isolated-linux-lab** do not spawn checkpoint piles.  
3. No “cleanup” scripts that **delete VHDs** or run **Convert-VHD/merge** on failure.

## Where labs live

| VM / surface | Disk | Role |
| --- | --- | --- |
| `isolated-win-vm` | **`<lab-drive-1>:\Hyper-V\…`** (Secondary Storage 1) | Windows lab only |
| `isolated-linux-lab` | **`<lab-drive-2>:\Hyper-V\…`** (Secondary Storage 2) | Linux lab / kernel build (Hyper-V) |
| `RamShared-Kernel` (WSL2) | **`<lab-drive-2>:\WSL\RamShared-Kernel\`** | Throwaway host WSL lab (break kernel) |
| WSL lab backup | **`<backup-drive>:\WSL-backup\RamShared-Kernel\`** | `wsl --export` base tar (not C:) |
| `ci-runner-vm` | **`<ci-drive>:\Hyper-V\…`** | CI (optional) |
| Host OS | **C:** | Never store lab VHD/ISO/export here |

Access procedure for agents: [`HYPERV-VM-ACCESS.md`](HYPERV-VM-ACCESS.md).

New VMs default: `<lab-drive>:\Hyper-V\VMs` + `<lab-drive>:\Hyper-V\VHDs` (`Set-VMHost`).

## Hard limits (applied)

| Setting | isolated-win-vm | isolated-linux-lab |
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

## After you finish Windows setup (isolated-win-vm)

In elevated PowerShell (does not delete the VHD):

```powershell
# Boot from disk, free ISO attachment
Set-VMDvdDrive -VMName isolated-win-vm -Path $null
$hd = Get-VMHardDiskDrive -VMName isolated-win-vm
Set-VMFirmware -VMName isolated-win-vm -FirstBootDevice $hd
# Re-apply guards
.\scripts\windows\Harden-LabVms.ps1
```

Optional lab UAC (inside guest only): `<lab-drive>:\Hyper-V\scripts\` or copy `<lab-drive>:\Hyper-V\scripts\Disable-Win11LabUac.ps1`.

## Linux lab

- Checkpoints disabled (same script).  
- Kernel build grows **inside** the 40 GB VHD — watch `df -h` in the guest.  
- Do not enable checkpoints “for safety” without pruning — that was the old 100 GB pile.

## WSL kernel lab distro (`RamShared-Kernel`)

- Live: `<lab-drive>:\WSL\<lab-distro>\ext4.vhdx` (dynamic, import cap ~40 GB).  
- Backup: `<backup-drive>:\WSL-backup\<lab-distro>\base.tar`.  
- Product default stays **`Ubuntu-24.04`** — never make the lab distro default.  
- Details: [`WSL-KERNEL-LAB.md`](WSL-KERNEL-LAB.md).

## Host C: health

- Free space should stay **>> 40 GB**.  
- If C: drops again, run **inventory only** first (`Measure-CDrivePressure.ps1`) — never blind delete.

## Rollback

These settings are non-destructive. To re-enable checkpoints later (historical; non-current; do not execute):

```powershell
Set-VM -Name isolated-win-vm -CheckpointType Production
```
