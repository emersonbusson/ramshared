#Requires -Version 5.1
<#
.SYNOPSIS
  Audit linux-kernel-lab readiness for custom-kernel/ublk product transport work.

.DESCRIPTION
  This is a read-only capability gate. It may start the VM when -Start is
  supplied, but it does not create, format, resize, merge, or attach disks and
  does not run swap or pressure workloads.

  PASS means the lab is reachable over SSH, sudo is non-interactive, the running
  kernel exposes ublk, and the NVIDIA/WSL GPU surfaces are present when requested.
#>
[CmdletBinding()]
param(
    [string]$VMName = "linux-kernel-lab",
    [string]$User = "emedev",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [switch]$Start,
    [switch]$RequireGpuSurface
)

$ErrorActionPreference = "Stop"

function New-ArtifactDir {
    param([string]$Root)
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $dir = Join-Path $Root "linux-kernel-lab-capability-$stamp"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function Invoke-SshText {
    param(
        [string]$Target,
        [string]$Command
    )
    $old = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $output = & ssh.exe -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=no `
        $Target $Command 2>&1 | ForEach-Object { $_.ToString() }
    $exit = $LASTEXITCODE
    $ErrorActionPreference = $old
    return [pscustomobject]@{
        exit_code = $exit
        output = ($output -join "`n")
    }
}

function Parse-Kv {
    param([string]$Text)
    $map = @{}
    foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match '^([A-Z0-9_]+)=(.*)$') {
            $map[$Matches[1]] = $Matches[2]
        }
    }
    return $map
}

$artifactDir = New-ArtifactDir -Root $ArtifactRoot
$accessScript = Join-Path $PSScriptRoot "Get-LinuxKernelLabAccess.ps1"
$accessArgs = @{
    VMName = $VMName
    User = $User
    Smoke = $true
}
if ($Start) { $accessArgs.Start = $true }

try {
    $access = & $accessScript @accessArgs | ConvertFrom-Json
    $access | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $artifactDir "access.json") -Encoding UTF8
} catch {
    $capability = [ordered]@{
        ok = $false
        vm = $VMName
        user = $User
        artifact_dir = $artifactDir
        ip = $null
        state = "unknown"
        require_gpu_surface = [bool]$RequireGpuSurface
        checks = [ordered]@{
            access = "failed"
        }
        reason = "access_failed"
        error = $_.Exception.Message
    }
    $capability | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $artifactDir "capability.json") -Encoding UTF8
    Write-Host "STATUS=PARTIAL"
    Write-Host "REASON=$($capability.reason)"
    Write-Host "ARTIFACT_DIR=$artifactDir"
    exit 2
}

$successfulSmoke = @($access.smoke | Where-Object { $_.exit_code -eq 0 })
$targetIp = $null
if ($successfulSmoke.Count -gt 0) {
    $targetIp = $successfulSmoke[0].ip
}

$capability = [ordered]@{
    ok = $false
    vm = $VMName
    user = $User
    artifact_dir = $artifactDir
    ip = $targetIp
    state = $access.state
    require_gpu_surface = [bool]$RequireGpuSurface
    checks = [ordered]@{}
    reason = ""
}

if (-not $targetIp) {
    $capability.reason = "ssh_smoke_failed"
    $capability | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $artifactDir "capability.json") -Encoding UTF8
    Write-Host "STATUS=PARTIAL"
    Write-Host "REASON=$($capability.reason)"
    Write-Host "ARTIFACT_DIR=$artifactDir"
    exit 2
}

$probe = @'
printf 'HOSTNAME=%s\n' "$(hostname)"
printf 'UNAME=%s\n' "$(uname -r)"
if sudo -n true >/dev/null 2>&1; then echo SUDO_NOPASSWD=ok; else echo SUDO_NOPASSWD=missing; fi
if [ -e /dev/ublk-control ]; then echo UBLK_CONTROL=present; else echo UBLK_CONTROL=absent; fi
if sudo -n modprobe -n -v ublk_drv >/tmp/ramshared-ublk-probe.txt 2>&1; then echo UBLK_MODPROBE=ok; else echo UBLK_MODPROBE=missing; fi
sed 's/^/UBLK_MODPROBE_TEXT=/' /tmp/ramshared-ublk-probe.txt 2>/dev/null | head -5
if [ -e /dev/dxg ]; then echo DEV_DXG=present; else echo DEV_DXG=absent; fi
if [ -e /dev/nvidiactl ]; then echo NVIDIACTL=present; else echo NVIDIACTL=absent; fi
if command -v nvidia-smi >/dev/null 2>&1; then echo NVIDIA_SMI=present; else echo NVIDIA_SMI=absent; fi
'@

$ssh = Invoke-SshText -Target "$User@$targetIp" -Command "sh -lc '$($probe.Replace("'", "'\''"))'"
$ssh | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $artifactDir "ssh-probe.json") -Encoding UTF8
$kv = Parse-Kv -Text $ssh.output

$capability.checks.ssh = if ($ssh.exit_code -eq 0) { "ok" } else { "failed" }
$capability.checks.sudo_nopasswd = $kv["SUDO_NOPASSWD"]
$capability.checks.uname = $kv["UNAME"]
$capability.checks.ublk_control = $kv["UBLK_CONTROL"]
$capability.checks.ublk_modprobe = $kv["UBLK_MODPROBE"]
$capability.checks.dev_dxg = $kv["DEV_DXG"]
$capability.checks.nvidiactl = $kv["NVIDIACTL"]
$capability.checks.nvidia_smi = $kv["NVIDIA_SMI"]

$needsGpu = -not $RequireGpuSurface -or (
    $kv["DEV_DXG"] -eq "present" -or
    $kv["NVIDIACTL"] -eq "present" -or
    $kv["NVIDIA_SMI"] -eq "present"
)

$capability.ok = (
    $ssh.exit_code -eq 0 -and
    $kv["SUDO_NOPASSWD"] -eq "ok" -and
    $kv["UBLK_CONTROL"] -eq "present" -and
    $kv["UBLK_MODPROBE"] -eq "ok" -and
    $needsGpu
)

if ($capability.ok) {
    $capability.reason = "pass"
    $exitCode = 0
    Write-Host "STATUS=PASS"
} else {
    $missing = @()
    if ($ssh.exit_code -ne 0) { $missing += "ssh_probe" }
    if ($kv["SUDO_NOPASSWD"] -ne "ok") { $missing += "sudo_nopasswd" }
    if ($kv["UBLK_CONTROL"] -ne "present") { $missing += "ublk_control" }
    if ($kv["UBLK_MODPROBE"] -ne "ok") { $missing += "ublk_modprobe" }
    if (-not $needsGpu) { $missing += "gpu_surface" }
    $capability.reason = ($missing -join ",")
    $exitCode = 2
    Write-Host "STATUS=PARTIAL"
    Write-Host "REASON=$($capability.reason)"
}

$capability | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $artifactDir "capability.json") -Encoding UTF8
Write-Host "ARTIFACT_DIR=$artifactDir"
exit $exitCode
