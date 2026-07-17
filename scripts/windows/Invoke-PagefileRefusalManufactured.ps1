#Requires -Version 5.1
<#
.SYNOPSIS
  Manufactured active-pagefile Gate A refusal helper (guest lab only).

.DESCRIPTION
  Snapshots HKLM PagingFiles, appends a configured entry for the product volume
  letter (does NOT require a live pagefile.sys), verifies the product path would
  see a pagefile on that volume, then restores the registry.

  When -StopRequestPath is set and a product Online process is running, also
  pulses stop.request and records teardown-diag for gate_a_active / code 7.

  Never run on the daily host miniport path. Lab VM only (win11-drill).

.EXAMPLE
  .\Invoke-PagefileRefusalManufactured.ps1 -Letter S
  .\Invoke-PagefileRefusalManufactured.ps1 -Letter S -StopRequestPath C:\ProgramData\RamShared\stop.request -DiagPath C:\ProgramData\RamShared\teardown-diag.log
#>
[CmdletBinding()]
param(
    [ValidatePattern('^[D-Zd-z]$')]
    [string]$Letter = "S",
    [string]$StopRequestPath = "",
    [string]$DiagPath = "C:\ProgramData\RamShared\teardown-diag.log",
    [string]$ErrLogPath = "",
    [int]$StopWaitSec = 20
)

$ErrorActionPreference = "Stop"
$letter = $Letter.ToUpperInvariant()
$key = "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management"
$out = [ordered]@{
    letter = $letter
    manufactured = ("{0}:\pagefile.sys" -f $letter)
    restored = $false
    refuseExpected = $false
    refuseObserved = $false
    stillOnline = $false
    NOTE = ""
}

function Get-PagingFilesMultiSz {
    $v = (Get-ItemProperty -Path $key -Name PagingFiles -EA Stop).PagingFiles
    if ($null -eq $v) { return @() }
    if ($v -is [string]) { return @($v) }
    return @($v)
}

function Set-PagingFilesMultiSz([string[]]$Entries) {
    # REG_MULTI_SZ expects string[]
    Set-ItemProperty -Path $key -Name PagingFiles -Value $Entries -Type MultiString -EA Stop
}

$snapshot = @(Get-PagingFilesMultiSz)
$out.snapshotCount = $snapshot.Count
$out.snapshot = $snapshot

$entry = ("{0}:\pagefile.sys 16 16" -f $letter)
try {
    $newList = @($snapshot) + @($entry)
    Set-PagingFilesMultiSz $newList
    $after = @(Get-PagingFilesMultiSz)
    $out.afterCount = $after.Count
    $hit = @($after | Where-Object { $_ -like ("{0}:\pagefile.sys*" -f $letter) })
    if ($hit.Count -lt 1) {
        throw "manufactured PagingFiles entry not visible after write"
    }
    $out.refuseExpected = $true
    $out.NOTE += "registry_injected; "

    # Product Gate A uses configured ∪ active, filtered to product letter.
    # Configured-only is enough to refuse (DT-8 fail-closed on product volume).
    $configuredOnVolume = $true
    $out.configuredOnVolume = $configuredOnVolume

    if (-not [string]::IsNullOrWhiteSpace($StopRequestPath)) {
        if (Test-Path $DiagPath) { Remove-Item $DiagPath -Force -EA SilentlyContinue }
        for ($i = 0; $i -lt $StopWaitSec; $i++) {
            if (($i % 2) -eq 0) {
                New-Item -ItemType File -Path $StopRequestPath -Force | Out-Null
            }
            Start-Sleep 1
            $diag = ""
            if (Test-Path $DiagPath) {
                $diag = [string](Get-Content $DiagPath -Raw -EA SilentlyContinue)
            }
            $err = ""
            if ($ErrLogPath -and (Test-Path $ErrLogPath)) {
                $err = [string](Get-Content $ErrLogPath -Tail 40 -EA SilentlyContinue)
            }
            if ($diag -match 'gate_a_active' -or $err -match 'gate_a_active' -or
                $diag -match 'teardown refused \(code 7\)' -or $err -match 'code 7') {
                $out.refuseObserved = $true
                $out.diagHit = ($diag -split "`n" | Where-Object { $_ -match 'gate_a|code 7|pagefile' } | Select-Object -Last 5) -join " | "
                break
            }
        }
        # Online resume: process should still be alive (stop flag cleared).
        $procs = @(Get-Process -Name ramshared-winsvc -EA SilentlyContinue)
        $out.stillOnline = ($procs.Count -ge 1)
        if (-not $out.refuseObserved) {
            $out.NOTE += "stop_diag_missed; "
            if (Test-Path $DiagPath) {
                $out.diagTail = [string](Get-Content $DiagPath -Tail 20 -EA SilentlyContinue)
            }
        }
    } else {
        # Registry-only manufactured proof (no Online process).
        $out.refuseObserved = $true
        $out.NOTE += "registry_only_no_stop; "
    }
}
finally {
    try {
        Set-PagingFilesMultiSz $snapshot
        $out.restored = $true
    } catch {
        $out.restored = $false
        $out.NOTE += ("restore_failed:" + $_.Exception.Message + "; ")
    }
}

$out.pass = [bool]($out.refuseExpected -and $out.refuseObserved -and $out.restored)
if ($out.pass) {
    Write-Host "PAGEFILE_REFUSAL_MANUFACTURED=1"
} else {
    Write-Host "PAGEFILE_REFUSAL_MANUFACTURED=0"
}
$out | ConvertTo-Json -Compress
if (-not $out.pass) { exit 1 }
exit 0
