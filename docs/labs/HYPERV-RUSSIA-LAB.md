# Lab on R: (RUSSIA) — three paths

> Host: Windows + Hyper-V. Disk **R:** label **RUSSIA** (~466 GB, ~140 GB free).  
> Mechanical HDD (Hitachi) — installs work; expect slower I/O than SSD.

## Honest map

| # | Path | What it is | GPU for kernel-true? | Status on this host (2026-07-10) |
| --- | --- | --- | --- | --- |
| 1 | Hyper-V `linux-kernel-lab` | Ubuntu VM, disks on R: | **No** (generic kernel lab) | **READY** — ISO + VM created; connect with `vmconnect` |
| 2 | DDA | Assign host NVIDIA to that VM | **Maybe** (host loses GPU; GeForce experimental) | **Inventory only** — Apply needs `-Force` + second display |
| 3 | Dual-boot | Ubuntu on unallocated space on RUSSIA | **Yes** (best Gate A) | **BLOCKED** — NTFS shrink only ~2.7 GB (immovable files); free space exists but not at end of volume |

Mainline destination: [`docs/specs/no-milestone/mainline-vram-tiering/PRD.md`](../specs/no-milestone/mainline-vram-tiering/PRD.md).

### What landed on R:

| Path | Role |
| --- | --- |
| `R:\Hyper-V\iso\ubuntu-24.04.2-live-server-amd64.iso` | Installer (~3 GB) |
| `R:\Hyper-V\linux-kernel-lab\` | VM + VHDX + `dda-inventory.txt` |
| `R:\Hyper-V\dual-boot\RUNBOOK-dualboot-RUSSIA.txt` | Dual-boot status / next steps |

### DDA facts (inventory)

- GPU: `NVIDIA GeForce RTX 2060` Status=OK  
- LocationPath: `PCIROOT(0)#PCI(0301)#PCI(0000)`  
- Report: `R:\Hyper-V\linux-kernel-lab\dda-inventory.txt`  
- **Do not** `-Apply -Force` unless you accept host without the 2060.

## From WSL2 (elevated PowerShell)

```bash
# Repo on Windows path recommended for -File:
REPO_WIN='C:\Users\emedev\ramshared-src'   # or your mirror
# Mirror if needed:
# rsync -a --delete --exclude target --exclude .git ./ /mnt/c/Users/emedev/ramshared-src/

./scripts/windows/wsl-elevated-ps.sh -File "${REPO_WIN}\scripts\windows\New-LinuxKernelLabVm.ps1" -Start

./scripts/windows/wsl-elevated-ps.sh -File "${REPO_WIN}\scripts\windows\Prepare-DdaGpu.ps1" -Inventory

# Dual-boot: plan, then shrink (shuts nothing by itself; stop VMs using R: first)
./scripts/windows/wsl-elevated-ps.sh -File "${REPO_WIN}\scripts\windows\Prepare-DualBootRussia.ps1" -DownloadIso
# After review:
./scripts/windows/wsl-elevated-ps.sh -File "${REPO_WIN}\scripts\windows\Prepare-DualBootRussia.ps1" -ApplyShrink -DownloadIso
```

## After VM exists

```powershell
vmconnect.exe localhost linux-kernel-lab
# Install Ubuntu (OpenSSH). Then:
Set-VMDvdDrive -VMName linux-kernel-lab -Path $null
```

## DDA Apply (dangerous)

Only with spare display / accept black host GPU:

```powershell
Stop-VM -Name linux-kernel-lab -Force
.\Prepare-DdaGpu.ps1 -Apply -Force -VmName linux-kernel-lab
```

Undo: `.\Prepare-DdaGpu.ps1 -Release -Force`

## Dual-boot finish

See `R:\Hyper-V\dual-boot\RUNBOOK-dualboot-RUSSIA.txt` after prep script.  
Install **only** into unallocated space; never “erase entire disk” on C:.
