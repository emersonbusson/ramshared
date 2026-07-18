# Hyper-V VM access runbook

This file documents how agents access the RamShared lab VMs without storing
or exposing secrets in git.

## Scope

| VM | Role | Non-interactive access |
| --- | --- | --- |
| `win11-drill` | Isolated Windows StorPort/CUDA lab | PowerShell Direct |
| `linux-kernel-lab` | Generic Ubuntu/kernel build Hyper-V lab | SSH from Windows host (`emedev@<ip>`) |

Do not use `gha-ubuntu-2404` for RamShared lab validation unless a task
explicitly names it.

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

2026-07-17 live probe: `Get-VMNetworkAdapter.IPAddresses` was empty, but ARP
resolved MAC `00-15-5D-00-FA-04` to `172.23.18.42`; SSH from Windows returned
hostname `linux-kernel-lab`, kernel `6.8.0-134-generic`, `cloud-init` status
`done`, and `sudo -n` succeeded.

Documented GUI fallback:

```powershell
vmconnect.exe localhost linux-kernel-lab
```

## Safe terminal state

After every campaign, confirm both lab VMs are off unless the task explicitly
requires leaving one running:

```bash
./scripts/windows/wsl-elevated-ps.sh -NoProfile -ExecutionPolicy Bypass -Command \
  "Get-VM -Name win11-drill,linux-kernel-lab | Select Name,State"
```
