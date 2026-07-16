# GPU-PV offline UMD copy and bounded validation

Date: 2026-07-16 10:38–10:42 America/Sao_Paulo  
Surface: isolated Hyper-V VM `win11-drill`  
Verdict: **PARTIAL / BLOCKED** — no real CUDA execution proof

## Before

- VM: `Off`
- Guest virtual disk: `E:\Hyper-V\win11-drill\Virtual Hard Disks\win11-drill.vhdx`
- Host NVIDIA display device: `OK`
- Host `nvidia-smi -L`: RTX 2060 present
- Host NVIDIA package: `oem27.inf`, version `32.0.16.1074`
- Exact package directory:
  `C:\Windows\System32\DriverStore\FileRepository\nv_dispi.inf_amd64_b26cc1edfbb8f4d0`
- Source inventory: 214 files, 2,818,282,276 bytes
- Guest `C:\Windows\System32\HostDriverStore\FileRepository` was absent.
- The virtual NVIDIA device used Microsoft `vrd.inf` and reported
  `CM_PROB_FAILED_POST_START`; no NVIDIA INF was installed in the guest.

## Action

With the VM off, its OS VHDX was mounted directly. Exact identity gates required the largest basic
partition to be NTFS, label `Windows`, and contain `Windows\System32\config\SYSTEM`.

The single host NVIDIA driver-store directory was copied to:

`C:\Windows\System32\HostDriverStore\FileRepository\nv_dispi.inf_amd64_b26cc1edfbb8f4d0`

This follows the Microsoft GPU-PV full-VM layout documented at
<https://learn.microsoft.com/windows-hardware/drivers/display/gpu-paravirtualization#driverstore-in-the-vm>.
No download, DDA, host driver replacement, host reboot, or guest NVIDIA INF installation occurred.

Copy evidence:

- `robocopy` exit: 1 (files copied successfully)
- Destination inventory: 214 files, 2,818,282,276 bytes
- Destination inventory exactly matched the source count and byte sum.
- VHDX detached after the action.

## Bounded validation

The VM was started once for a bounded probe. PowerShell Direct did not become usable within the
bounded validation window and the remote session ended. No retry was made.

Host Hyper-V Worker events captured the stronger blocker:

- Event 33101 at 10:35:21: guest requested incompatible virtual PCI protocol `0x10006`; negotiated
  channel was `0x10005`; error `0x8007051A` (revision mismatch).
- Event 18590 at 10:39:34: guest reported a fatal error with `ErrorCode0: 0x10` during the first boot
  after the offline copy; the guest subsequently initialized an OS at 10:39:44, but did not become
  available for the bounded CUDA probe.
- Host build: `26200.8655`.
- Most recently proven guest build: `26200.8037`.

Because `nvidia-smi` and `ramshared-winsvc probe-cuda` did not execute successfully in the guest,
file presence is not counted as CUDA proof.

## Terminal state

- VM: `Off`
- VHDX attached: `False`
- Host NVIDIA display device: `OK`
- Host `nvidia-smi -L`: RTX 2060 present
- Guest copied directory remains present and inventory-verified.
- No dump file was present in the mounted guest OS volume.

## Rollback and next executable gate

Rollback is limited to removing the one copied guest directory while the VHDX is mounted offline:

`C:\Windows\System32\HostDriverStore\FileRepository\nv_dispi.inf_amd64_b26cc1edfbb8f4d0`

Do not retry the same boot/probe until the virtual PCI protocol revision mismatch is resolved. The
next gate is to align or otherwise prove compatibility of the host and guest Hyper-V/GPU-PV OS
builds using already-authorized media/update state, then run one bounded before→action→after probe:

1. virtual NVIDIA PnP status `OK`;
2. bounded `nvidia-smi -L` exit 0;
3. bounded `ramshared-winsvc probe-cuda` exit 0 with allocate/write/read/free restoration evidence;
4. VM `Off`, VHDX detached, and host RTX 2060 `OK` after cleanup.

