<#
.SYNOPSIS
  Switches the WSL2 kernel SAFELY with AUTO-RECOVERY.

.DESCRIPTION
  Arms the custom kernel in .wslconfig (with a backup), restarts WSL, and verifies boot
  with a timeout. If the kernel does NOT boot (timeout or incorrect version), RESTORES
  .wslconfig from the backup and restarts → it automatically returns to the Microsoft kernel.
  Reusable for any custom kernel (Phase B+ toolkit).

  Auto-revert criterion = BOOT FAILURE (catastrophic). If the kernel boots but a
  module (for example, ublk_drv) fails, it does NOT revert (the kernel is usable) — it only warns.

  .PARAMETER KernelPath     Windows path to bzImage (default C:\wsl\kernel-ramshared)
  .PARAMETER ExpectedVersion Expected `uname -r` (default 6.6.123.2-microsoft-standard-WSL2+)
.PARAMETER WslConfig      .wslconfig (default $env:USERPROFILE\.wslconfig)
  .PARAMETER CleanConfig    Clean .wslconfig to restore on failure (default C:\wsl\wslconfig-original.txt)
  .PARAMETER TimeoutSec     Boot-check timeout (default 60)
  .PARAMETER CheckModules   Modules to test post-boot with modprobe (default "ublk_drv")
  .PARAMETER DryRunConfig   When set: exercises Arm/Revert logic only in this file and exits (test; does not touch WSL)
  .PARAMETER PreflightOnly  Validates prerequisites and arms/disarms a temporary file. Does not call wsl --shutdown.
#>
param(
  [string]$KernelPath      = "C:\wsl\kernel-ramshared",
  [string]$ExpectedVersion = "6.6.123.2-microsoft-standard-WSL2+",
  [string]$WslConfig       = "$env:USERPROFILE\.wslconfig",
  [string]$CleanConfig     = "C:\wsl\wslconfig-original.txt",
  [int]   $TimeoutSec      = 60,
  [string]$CheckModules    = "ublk_drv",
  [string]$DryRunConfig    = "",
  [switch]$PreflightOnly
)
$ErrorActionPreference = "Stop"

# .wslconfig treats "\" as escape (I:\wsl → invalid escape "w").
# Day-0: always emit forward-slash Windows paths (Microsoft + WSL parser safe).
function To-WslPath([string]$p) {
  if ([string]::IsNullOrWhiteSpace($p)) { return $p }
  return ($p -replace '\\', '/')
}

# Arms kernel= under [wsl2] idempotently (replaces it if present; creates [wsl2] if missing).
function Arm-Config([string]$cfgPath, [string]$kernelWin) {
  $kline = "kernel=" + (To-WslPath $kernelWin)
  $lines = @(); if (Test-Path $cfgPath) { $lines = @(Get-Content $cfgPath) }
  $out = @(); $inWsl2 = $false; $added = $false; $hasWsl2 = $false
  foreach ($l in $lines) {
    if ($l -match '^\s*\[wsl2\]\s*$')      { $inWsl2 = $true; $hasWsl2 = $true; $out += $l; continue }
    if ($l -match '^\s*\[')                { if ($inWsl2 -and -not $added) { $out += $kline; $added = $true }; $inWsl2 = $false; $out += $l; continue }
    if ($inWsl2 -and $l -match '^\s*kernel\s*=') { continue }  # remove the old kernel= (replace it)
    $out += $l
  }
  if ($inWsl2 -and -not $added) { $out += $kline; $added = $true }          # [wsl2] was the final section
  if (-not $hasWsl2)            { $out = @("[wsl2]", $kline) + $out }        # [wsl2] was absent
  if (-not (Set-CfgRetry $cfgPath $out)) { throw "could not write $cfgPath (locked/ACL?)" }
}

# Retried write — transient .wslconfig locks (WSL/editor/antivirus/OneDrive).
function Set-CfgRetry([string]$path, [string[]]$lines) {
  for ($i = 0; $i -lt 6; $i++) {
    try { Set-Content -Path $path -Value $lines -Encoding ASCII -ErrorAction Stop; return $true }
    catch { Start-Sleep -Milliseconds 800 }
  }
  return $false
}

# DETERMINISTIC disarm: removes all kernel= lines (reverts to Microsoft kernel).
# Does NOT rely on backup copy succeeding. Returns $true if disarmed (or if config doesn't exist).
# Never throws (catches everything) — called from finally block to avoid leaving config in a broken/armed state.
function Disarm-Config([string]$cfgPath) {
  try {
    if (-not (Test-Path $cfgPath)) { return $true }
    $kept = @(Get-Content $cfgPath | Where-Object { $_ -notmatch '^\s*kernel\s*=' })
    return (Set-CfgRetry $cfgPath $kept)
  } catch { return $false }
}

# --- TEST mode (does not touch WSL): exercises Arm and shows the result ---
if ($DryRunConfig -ne "") {
  Write-Host "[dry-run] arming kernel in $DryRunConfig ..."
  Arm-Config $DryRunConfig $KernelPath
  Write-Host "--- result ---"; Get-Content $DryRunConfig | ForEach-Object { Write-Host $_ }
  Write-Host "[dry-run] (idempotence) arming again ..."
  Arm-Config $DryRunConfig $KernelPath
  $n = @(Select-String -Path $DryRunConfig -Pattern '^\s*kernel=').Count
  Write-Host "ARM: kernel= lines = $n (expected 1)"
  Write-Host "[dry-run] disarming (deterministic revert) ..."
  $ok = Disarm-Config $DryRunConfig
  $d = @(Select-String -Path $DryRunConfig -Pattern '^\s*kernel=').Count
  Write-Host "DISARM: ok=$ok ; kernel= lines = $d (expected 0)"
  exit 0
}

# --- PREFLIGHT mode (does not touch real WSL): validates inputs and runs an isolated dry run ---
if ($PreflightOnly) {
  Write-Host "PREFLIGHT: launcher=$PSCommandPath"
  Write-Host "PREFLIGHT: kernel=$KernelPath"
  Write-Host "PREFLIGHT: expected=$ExpectedVersion"
  if (-not (Test-Path $KernelPath)) { Write-Error "kernel missing: $KernelPath" }
  $kernelSize = (Get-Item $KernelPath).Length
  if ($kernelSize -le 0) { Write-Error "kernel empty: $KernelPath" }
  Write-Host "PREFLIGHT: kernel-size=$kernelSize"

  if (Test-Path $CleanConfig) {
    if (Select-String -Path $CleanConfig -Pattern '^\s*kernel=' -Quiet) {
      Write-Error "backup '$CleanConfig' contains kernel=; it is not clean"
    }
    Write-Host "PREFLIGHT: clean-config=ok"
  } else {
    Write-Warning "PREFLIGHT: CleanConfig absent; launcher will create it if the current .wslconfig is clean"
  }

  if ((Test-Path $WslConfig) -and (Select-String -Path $WslConfig -Pattern '^\s*kernel=' -Quiet)) {
    Write-Warning "PREFLIGHT: current .wslconfig already contains kernel=; real boot may already be armed"
  } else {
    Write-Host "PREFLIGHT: current-wslconfig=disarmed"
  }

  $tmp = Join-Path $env:TEMP ("ramshared-wslconfig-preflight-" + [guid]::NewGuid() + ".txt")
  if (Test-Path $WslConfig) { Copy-Item $WslConfig $tmp -Force } else { Set-Content $tmp "[wsl2]" -Encoding ASCII }
  try {
    Arm-Config $tmp $KernelPath
    $n = @(Select-String -Path $tmp -Pattern '^\s*kernel=').Count
    if ($n -ne 1) { Write-Error "dry-run arm produced $n kernel= lines (expected 1)" }
    if (-not (Disarm-Config $tmp)) { Write-Error "dry-run disarm failed" }
    $d = @(Select-String -Path $tmp -Pattern '^\s*kernel=').Count
    if ($d -ne 0) { Write-Error "dry-run disarm left $d kernel= lines (expected 0)" }
    Write-Host "PREFLIGHT: arm-disarm=ok"
  } finally {
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
  }

  $active = (wsl.exe -e sh -c "uname -r" 2>&1) -join "`n"
  Write-Host "PREFLIGHT: active-uname=$($active.Trim())"
  Write-Host "PREFLIGHT: OK (no shutdown executed)"
  exit 0
}

# --- 1. guaranteed CLEAN backup (revert ALWAYS restores a bootable state) ---
if (Test-Path $CleanConfig) {
  if (Select-String -Path $CleanConfig -Pattern '^\s*kernel=' -Quiet) {
    Write-Error "backup '$CleanConfig' is NOT clean (contains kernel=). Point -CleanConfig to a .wslconfig WITHOUT a custom kernel."
  }
} else {
  if ((Test-Path $WslConfig) -and (Select-String -Path $WslConfig -Pattern '^\s*kernel=' -Quiet)) {
    Write-Error "No clean backup and the current .wslconfig already has kernel=. Create '$CleanConfig' (a version without kernel=) first."
  }
  if (Test-Path $WslConfig) { Copy-Item $WslConfig $CleanConfig -Force } else { Set-Content $CleanConfig "[wsl2]" -Encoding ASCII }
  Write-Host "clean backup created: $CleanConfig"
}

# --- 2-4. arm, restart, verify, with total FAIL-SAFE: ANY failure/error/exception
# (including wsl --shutdown throwing) → finally reverts to the Microsoft kernel. It never remains broken and armed.
$confirmed = $false; $uname = ""
try {
  Write-Host "arming kernel=$KernelPath in $WslConfig (backup: $CleanConfig)"
  Arm-Config $WslConfig $KernelPath
  Write-Host "wsl --shutdown ..."; wsl --shutdown; Start-Sleep -Seconds 3
  Write-Host "booting and verifying (timeout ${TimeoutSec}s)..."
  $job = Start-Job -ScriptBlock { (wsl.exe -e sh -c "uname -r") 2>&1 }
  if (Wait-Job $job -Timeout $TimeoutSec) {
    $uname = ((Receive-Job $job) -join "`n").Trim()
    if ($uname -match [regex]::Escape($ExpectedVersion)) { $confirmed = $true }
    else { Write-Warning "unexpected uname: '$uname' (expected to contain '$ExpectedVersion')" }
  } else {
    Stop-Job $job; Write-Warning "boot did NOT respond in ${TimeoutSec}s (probable boot failure)"
  }
  Remove-Job $job -Force -ErrorAction SilentlyContinue
} catch {
  Write-Warning "error during the switch: $_"
} finally {
  if (-not $confirmed) {
    Write-Warning "FAILURE → auto-reverting to the Microsoft kernel..."
    # Deterministic disarm (does not depend on the backup) and never escapes finally.
    if (Disarm-Config $WslConfig) {
      try { wsl --shutdown } catch { }
      Write-Host "REVERTED. The next WSL launch uses the Microsoft kernel. No data affected."
    } else {
      Write-Warning ("AUTOMATIC REVERT FAILED while rewriting $WslConfig (locked/ACL?). " +
        "MANUAL ACTION: delete the 'kernel=' line from $WslConfig and run 'wsl --shutdown'. Clean backup: $CleanConfig")
    }
  }
}

# --- 5. result (module is best effort: it does NOT revert; the kernel remains usable if it fails) ---
if ($confirmed) {
  Write-Host "OK: custom kernel booted ($uname)."
  $mod = (wsl.exe -e sh -c "sudo modprobe $CheckModules 2>&1 && ls /dev/ublk-control 2>/dev/null && echo MOD-OK") 2>&1
  if ($mod -match "MOD-OK") { Write-Host "modules OK ($CheckModules loaded)." }
  else { Write-Warning "kernel OK, but module '$CheckModules' did not load: $mod (kernel retained; investigate)" }
  Write-Host "READY. Custom kernel active."
  exit 0
} else {
  exit 1
}
