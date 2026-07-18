# Hyper-V VM access runbook

This file documents how agents access the RamShared lab VMs without storing
or exposing secrets in git.

## Scope

| VM | Role | Non-interactive access |
| --- | --- | --- |
| `win11-drill` | Isolated Windows StorPort/CUDA lab | PowerShell Direct |
| `win11-wsl2-lab` | Disposable Windows lab for WSL2 freeze campaigns | PowerShell Direct after install |
| `linux-kernel-lab` | Generic Ubuntu/kernel build Hyper-V lab | SSH from Windows host (`emedev@<ip>`) |

Do not use `gha-ubuntu-2404` for RamShared lab validation unless a task
explicitly names it.

## VM inventory policy

The approved RamShared lab inventory is closed:

- `win11-drill`
- `win11-wsl2-lab`
- `linux-kernel-lab`

Do not create additional VMs for RamShared validation without explicit user
approval in the current conversation. Prefer repairing or using one of the
existing lab VMs. If a disposable Windows WSL2 surface is needed, use the
existing `win11-wsl2-lab`; do not create `win11-wsl2-lab-2`, clones, or
replacement VMs.

The reason `win11-wsl2-lab` exists is historical and specific: `win11-drill`
already existed, but its guest WSL runtime was not usable for the freeze
campaign, while `linux-kernel-lab` is a Linux VM and cannot prove Windows guest
WSL2 behavior. `win11-wsl2-lab` was created as the one disposable Windows WSL2
lab so agents can run destructive WSL2 freeze/reclaim campaigns away from the
real Windows desktop and the real daily WSL2 instance.

Lab VM state is not protected product data. It is acceptable to reinstall,
reset, format, or otherwise mutate the guest OS inside `win11-drill`,
`win11-wsl2-lab`, or `linux-kernel-lab` when that is the selected test surface.
The boundary is the VM ownership boundary: never format, replace, or attach a
host disk, the real Windows system volume, the real WSL2 storage, or a VHD owned
by a different VM as a workaround. Only operate on the VHD already owned by the
selected lab VM, and record the action/artifacts.

These lab VMs live on slower host storage than an SSD-backed lab. Treat install,
first boot, Windows Update, WSL install, and PowerShell Direct readiness as
slow-path operations. Do not classify a lab boot as failed from a short wait:
use long bounded waits, progress logs, and the harness defaults before marking a
campaign `PARTIAL`.

## Secrets policy

- The Windows guest password is not documented here and must not be committed.
- Preferred source: host environment variable `RAMSHARED_DRILL_PASSWORD`.
- Local password files such as `.drill-pw` are ignored by git and are
  local-only. Do not print their contents in logs, docs, or chat.
- The canonical Windows lab user is `WIN11-DRILL\drilladmin`. Do not rely on
  `.\drilladmin`; PowerShell Direct can reject that shorthand on this image.

## Elevated host PowerShell from WSL

Use the repository wrapper:

```bash
./scripts/windows/wsl-elevated-ps.sh -NoProfile -ExecutionPolicy Bypass -Command \
  "Get-VM -Name win11-drill,linux-kernel-lab | Select Name,State"
```

## `win11-drill` smoke access

This is a read-only PowerShell Direct probe. It starts the VM if needed and
turns it off at the end.

```bash
./scripts/windows/wsl-elevated-ps.sh -NoProfile -ExecutionPolicy Bypass -Command '
$vm = "win11-drill"
Start-VM -Name $vm -ErrorAction Stop
try {
  $sec = ConvertTo-SecureString $env:RAMSHARED_DRILL_PASSWORD -AsPlainText -Force
  $cred = [pscredential]::new("WIN11-DRILL\drilladmin", $sec)
  Invoke-Command -VMName $vm -Credential $cred -ScriptBlock {
    [pscustomobject]@{ hostname=$env:COMPUTERNAME; whoami=(whoami) }
  }
} finally {
  Stop-VM -Name $vm -TurnOff -Force -ErrorAction SilentlyContinue
}
'
```

Expected identity:

```text
hostname = WIN11-DRILL
whoami   = win11-drill\drilladmin
```

## `win11-drill` product campaign

Run the isolated product Online campaign with the canonical user:

```bash
PS1_PATH=$(wslpath -w scripts/windows/Run-GuestProductOnline.ps1)
./scripts/windows/wsl-elevated-ps.sh -NoProfile -ExecutionPolicy Bypass -Command \
  "& '$PS1_PATH' -VMName win11-drill -User 'WIN11-DRILL\drilladmin'"
```

The harness starts the VM, deploys the current Windows package, runs three
SHA I/O lifecycle rounds, performs graceful teardown, and stops the VM.

## `win11-drill` WSL2 freeze campaign

Use this path for WSL2 freeze-elimination proof instead of the daily desktop WSL:

```bash
pwsh.exe -NoProfile -ExecutionPolicy Bypass -Command \
  "& 'C:\Windows\System32\sudo.exe' powershell.exe -NoProfile -ExecutionPolicy Bypass -File '\\wsl.localhost\Ubuntu-24.04\home\emdev\codespace\ramshared\scripts\windows\Invoke-Win11Wsl2FreezeCampaign.ps1' -Start -Run"
```

2026-07-18 attempt:
`C:\ramshared\artifacts\win11-wsl2-freeze-campaign-20260718-115419` returned
`STATUS=PARTIAL` because PowerShell Direct rejected the current local
credential. Do not reset the VM password in git or document it here; refresh the
local ignored credential source, then rerun the harness.

2026-07-18 follow-up:

- The local Machine `RAMSHARED_DRILL_PASSWORD` works for
  `WIN11-DRILL\drilladmin`; do not print it and do not commit it.
- `scripts/windows/Invoke-Win11Wsl2FreezeCampaign.ps1` now waits for
  PowerShell Direct readiness and reads Machine/User/process/local ignored
  credential sources without persisting the secret.
- WSL and VirtualMachinePlatform optional features were enabled inside
  `win11-drill`.
- A tracked-source tarball was copied to `C:\ramshared\src` in the guest.
- The official Microsoft WSL 2.7.10 MSI from `microsoft/WSL` releases installed
  the Appx package, but guest `wsl.exe` still returned
  `Wsl/CallMsi/Install/REGDB_E_CLASSNOTREG`; removing the Appx made the stub
  return "WSL is not installed" again. The MSIXBundle has the same package
  identity as the MSI and was not installed over it.
- A later `wsl.exe --install --web-download --no-distribution` and
  `wsl.exe --update --web-download` attempt still returned "WSL is not
  installed" from the inbox stub.
- Latest campaign artifact:
  `C:\ramshared\artifacts\win11-wsl2-freeze-campaign-20260718-123613`,
  `STATUS=PARTIAL`, `REASON=powershell_direct_failed` after WSL runtime repair
  attempts.
- Runtime probe artifact:
  `C:\ramshared\artifacts\win11-wsl-runtime-probe-20260718-130619`.
  It confirmed WSL/VMP features enabled, no WSL Appx package, inbox
  `C:\Windows\System32\wsl.exe` returning "WSL is not installed", and a
  highest-privilege scheduled-task probe with no output
  (`last_task_result=267009`).

Next WSL2-freeze unblock is guest WSL runtime repair or reimage to a Windows lab
image with WSL already functional, then rerun the harness. Do not run pressure
on the daily WSL2 desktop as a substitute.

## `win11-wsl2-lab` disposable Windows WSL2 lab

Historical creation command for context only. Do not run this again unless the
user explicitly approves creating a new VM in the current conversation. When
`win11-drill` is not recoverable as a WSL2 guest, use the already-created
`win11-wsl2-lab` for destructive Windows WSL2 campaigns instead of touching the
real desktop WSL2 instance:

```bash
pwsh.exe -NoProfile -ExecutionPolicy Bypass -Command \
  "& 'C:\Windows\System32\sudo.exe' powershell.exe -NoProfile -ExecutionPolicy Bypass -File '\\wsl.localhost\Ubuntu-24.04\home\emdev\codespace\ramshared\scripts\windows\New-Win11Wsl2LabVm.ps1' -Start"
```

2026-07-18: `New-Win11Wsl2LabVm.ps1` created the single disposable VM
`win11-wsl2-lab` with a new
dynamic VHD at `E:\Hyper-V\win11-wsl2-lab\Virtual Hard Disks\win11-wsl2-lab.vhdx`,
attached the Windows 25H2 ISO and autounattend ISO, enabled nested
virtualization, disabled checkpoints, and did not modify existing lab disks.
The VM did not expose heartbeat/PowerShell Direct during the initial unattended
boot window and was turned Off. Next step is console/boot-media inspection or a
no-prompt Windows installer ISO on this same VM; do not create another VM, and
do not touch host disks or real WSL2 storage. Reinstalling this VM's own guest
disk is allowed when needed for the isolated campaign.

No-prompt ISO path:

```bash
pwsh.exe -NoProfile -ExecutionPolicy Bypass -Command \
  "& 'C:\Windows\System32\sudo.exe' powershell.exe -NoProfile -ExecutionPolicy Bypass -File '\\wsl.localhost\Ubuntu-24.04\home\emdev\codespace\ramshared\scripts\windows\New-WindowsNoPromptIso.ps1'"
```

2026-07-18 inspection found both Windows ISOs contain
`efi\microsoft\boot\efisys_noprompt.bin`, and the small
`win11-autounattend.iso` contains `Autounattend.xml`. `New-WindowsNoPromptIso.ps1`
is ready. Later the host installed official Windows ADK Deployment Tools and
generated `E:\Hyper-V\iso\Win11_25H2_English_x64_v2_noprompt_unattend.iso`;
the existing `win11-wsl2-lab` DVD now points at that ISO. All three approved lab
VMs were returned to Off after the audit.

2026-07-18 follow-up: with the no-prompt ISO attached, `win11-wsl2-lab` was
started and observed for 20 minutes. Hyper-V reported `Running`, but heartbeat
and key-value pair integration services stayed `Sem Contato` for the full
window. The VM was turned Off. Next step is console/firmware/boot-media
inspection on this same VM, or reinstalling this VM's own VHD if the installer
state is invalid. Do not create another VM.

## `linux-kernel-lab` access

The lab image is an Ubuntu cloud image with local user `emedev`, SSH keys
installed, passwordless sudo, and console auto-login. Hyper-V can control the
VM power state from WSL:

```bash
./scripts/windows/wsl-elevated-ps.sh -NoProfile -ExecutionPolicy Bypass -Command \
  "Start-VM linux-kernel-lab; Get-VM linux-kernel-lab; Stop-VM linux-kernel-lab -TurnOff -Force"
```

Linux guests do not support PowerShell Direct. Use SSH from the Windows host.
`Get-VMNetworkAdapter.IPAddresses` may be empty even when DHCP succeeded; in
that case, resolve the IP from the Windows ARP/neighbor table by VM MAC.

Probe from WSL:

```bash
PS1_PATH=$(wslpath -w scripts/windows/Get-LinuxKernelLabAccess.ps1)
./scripts/windows/wsl-elevated-ps.sh -NoProfile -ExecutionPolicy Bypass -Command \
  "& '$PS1_PATH' -Start -Smoke"
```

Manual fallback:

```powershell
$adapter = Get-VMNetworkAdapter -VMName linux-kernel-lab
$mac = ($adapter.MacAddress -replace "(.{2})(?=.)", '$1-').ToUpperInvariant()
Get-NetNeighbor -AddressFamily IPv4 |
  ? { $_.LinkLayerAddress -and $_.LinkLayerAddress.ToUpperInvariant() -eq $mac }
ssh.exe emedev@<ip>
```

2026-07-18 live probe: `Get-VMNetworkAdapter.IPAddresses` was empty, but ARP
resolved the VM MAC through `Get-NetNeighbor` to `172.29.50.143`; SSH from the
Windows host returned hostname `linux-kernel-lab`, kernel `6.8.0-134-generic`,
`cloud-init` status `done`, and `sudo -n` succeeded. The same probe found
`/dev/dxg`, `/dev/nvidiactl`, and `/dev/ublk-control` absent; `nvidia-smi` is
not installed; `sudo -n modprobe -n -v ublk_drv` failed because the module is
not present under `/lib/modules/6.8.0-134-generic`. This VM is currently an
access/build lab, not proof of WSL2 GPU reclaim, GPU-PV reclaim, or the
custom-kernel/ublk product transport.

2026-07-18 refreshed capability audit:
`C:\ramshared\artifacts\linux-kernel-lab-capability-20260718-112539` returned
`STATUS=PARTIAL` with SSH and `sudo -n` OK, kernel `6.8.0-134-generic`,
`/dev/ublk-control` absent, `modprobe ublk_drv` missing, and no GPU surface. The
VM was returned to Off after the probe.

2026-07-18 ublk capability repair:
`linux-modules-extra-6.8.0-134-generic` was installed inside the lab, then
`sudo -n modprobe ublk_drv` created `/dev/ublk-control`. Refreshed audit
`C:\ramshared\artifacts\linux-kernel-lab-capability-20260718-131502` returned
`STATUS=PASS` for SSH, passwordless sudo, `ublk_drv`, and `/dev/ublk-control`.
This is only lab capability proof; product ublk transport still needs lifecycle,
swapoff-first teardown, crash/drain, and no-ghost evidence.

If Hyper-V reports insufficient host memory when starting `linux-kernel-lab`,
run `scripts/windows/Harden-LabVms.ps1` elevated. It sets the lab startup memory
to 2 GiB, minimum to 1 GiB, and maximum to 8 GiB without changing disks.

Documented GUI fallback:

```powershell
vmconnect.exe localhost linux-kernel-lab
```

## Safe terminal state

After every campaign, confirm both lab VMs are off unless the task explicitly
requires leaving one running:

```bash
./scripts/windows/wsl-elevated-ps.sh -NoProfile -ExecutionPolicy Bypass -Command \
  "Get-VM -Name win11-drill,win11-wsl2-lab,linux-kernel-lab | Select Name,State"
```
