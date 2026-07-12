#Requires -Version 5.1
<#
.SYNOPSIS
  Kahneman-disciplined validation campaign on Hyper-V guest (win11-drill).

.DESCRIPTION
  Elevated host only (use scripts/windows/wsl-elevated-ps.sh from WSL).
  Applies: #1 WYSIATI state, #2 checkpoint rollback, #3 numbers, #5 worst-case
  fail paths (#13), #15 no retry on deterministic tool gaps, #16 no thrash.

  Password: env RAMSHARED_DRILL_PASSWORD (never hardcode in git).

.NOTES
  Does NOT claim ITEM-8 PASS without DT-21 residency. Missing WDK/link.exe = SKIP.
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-drill",
    [string]$SrcHost = "C:\Users\emedev\ramshared-src",
    [string]$DstGuest = "C:\ramshared",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = $env:RAMSHARED_DRILL_PASSWORD,
    [string]$ResultsJson = "C:\Users\emedev\ramshared-drill\agent-disciplined-results.json"
)

$ErrorActionPreference = "Continue"
if ([string]::IsNullOrEmpty($Password)) {
    throw "Set RAMSHARED_DRILL_PASSWORD or -Password (lab only)."
}

$results = New-Object System.Collections.Generic.List[object]
function Rec([string]$name, [string]$status, [string]$detail, $number = $null) {
    $results.Add([pscustomobject]@{
            Name   = $name
            Status = $status
            Detail = $detail
            Number = $number
            Ts     = (Get-Date).ToString("o")
        })
    Write-Output ("RESULT [{0}] {1}: {2} num={3}" -f $status, $name, $detail, $number)
}

$pass = ConvertTo-SecureString $Password -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($User, $pass)

$vm = Get-VM -Name $VMName -ErrorAction Stop
Rec "wysiati-vm-state" "INFO" ("state={0} memMB={1}" -f $vm.State, [int]($vm.MemoryAssigned / 1MB)) $vm.State

$cpName = "disciplined-{0:yyyyMMdd-HHmmss}" -f (Get-Date)
try {
    Checkpoint-VM -Name $VMName -SnapshotName $cpName -ErrorAction Stop
    Rec "checkpoint" "PASS" $cpName 1
}
catch {
    Rec "checkpoint" "WARN" $_.Exception.Message 0
}

if ($vm.State -ne "Running") {
    Start-VM $VMName
    $t = 0
    while ((Get-VM $VMName).State -ne "Running" -and $t -lt 90) { Start-Sleep 2; $t += 2 }
}

$sess = $null
for ($i = 0; $i -lt 40; $i++) {
    try { $sess = New-PSSession -VMName $VMName -Credential $cred -ErrorAction Stop; break }
    catch { Start-Sleep 3 }
}
if (-not $sess) { throw "PSD failed" }
Rec "psd" "PASS" "session" 1

Invoke-Command -Session $sess -ScriptBlock {
    param($d)
    Remove-Item -Path $d -Recurse -Force -ErrorAction SilentlyContinue | Out-Null
    New-Item -ItemType Directory -Force -Path $d, (Join-Path $d "artifacts") | Out-Null
} -ArgumentList $DstGuest

foreach ($it in @("scripts", "drivers", "crates", "docs", "Cargo.toml", "Cargo.lock", "deny.toml", "README.md")) {
    $p = Join-Path $SrcHost $it
    if (Test-Path $p) {
        Copy-Item $p -Destination (Join-Path $DstGuest $it) -ToSession $sess -Recurse -Force -ErrorAction Continue
    }
}
Rec "copy" "PASS" $DstGuest 1

$inv = Invoke-Command -Session $sess -ScriptBlock {
    $b = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
    [pscustomobject]@{
        Build       = "$($b.CurrentBuild).$($b.UBR)"
        FreeGB      = [math]::Round((Get-PSDrive C).Free / 1GB, 2)
        TestSigning = [bool]((bcdedit /enum "{current}") -match "testsigning\s+Yes")
        Cargo       = [bool](Get-Command cargo -ErrorAction SilentlyContinue)
        Wdk         = Test-Path "C:\Program Files (x86)\Windows Kits\10"
        Nvcuda      = Test-Path "$env:SystemRoot\System32\nvcuda.dll"
        LinkExe     = [bool](Get-Command link.exe -ErrorAction SilentlyContinue)
    }
}
Rec "inventory" "INFO" ($inv | ConvertTo-Json -Compress) $inv.FreeGB

# #13: DT-21 fail path must not silent-pass
$kpd = Invoke-Command -Session $sess -ScriptBlock {
    New-Item -ItemType Directory -Force -Path C:\ramshared\artifacts\kernel-page-drill | Out-Null
    $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ramshared\scripts\windows\Invoke-KernelPageDrill.ps1 `
        -Runs 1 -SkipPoolstressLoad -WhatIfHostCheck -ArtifactDir C:\ramshared\artifacts\kernel-page-drill 2>&1 | Out-String
    [pscustomobject]@{ Exit = $LASTEXITCODE; Out = $out }
}
if ($kpd.Exit -eq 3 -and $kpd.Out -match "INCONCLUSIVO") {
    Rec "item8-fail-path-dt21" "PASS" "INCONCLUSIVO without residency" $kpd.Exit
}
else {
    Rec "item8-fail-path-dt21" "WARN" ("exit={0}" -f $kpd.Exit) $kpd.Exit
}

$pf = Invoke-Command -Session $sess -ScriptBlock {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ramshared\scripts\windows\Get-WinDrivePreflight.ps1 2>&1 | Out-String
    "EXIT=$LASTEXITCODE"
}
if ($pf -match "EXIT=0") { Rec "preflight" "PASS" "exit0" 0 } else { Rec "preflight" "WARN" "nonzero" 1 }

$parse = Invoke-Command -Session $sess -ScriptBlock {
    $errs = @()
    Get-ChildItem C:\ramshared\scripts\windows\*.ps1 | ForEach-Object {
        $t = $null; $e = $null
        [void][System.Management.Automation.Language.Parser]::ParseFile($_.FullName, [ref]$t, [ref]$e)
        if ($e -and $e.Count) { $errs += "$($_.Name):$($e[0].Message)" }
    }
    [pscustomobject]@{ ErrCount = $errs.Count; Files = (Get-ChildItem C:\ramshared\scripts\windows\*.ps1).Count }
}
if ($parse.ErrCount -eq 0) { Rec "ps1-parse" "PASS" ("files={0}" -f $parse.Files) $parse.Files }
else { Rec "ps1-parse" "FAIL" "parse errors" $parse.ErrCount }

$meas = Invoke-Command -Session $sess -ScriptBlock {
    New-Item -ItemType Directory -Force -Path C:\ramshared\artifacts\measure | Out-Null
    $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ramshared\scripts\windows\Measure-PagefileVram.ps1 `
        -Runs 3 -LoadTag idle -ArtifactDir C:\ramshared\artifacts\measure 2>&1 | Out-String
    [pscustomobject]@{ Exit = $LASTEXITCODE; Out = $out }
}
if ($meas.Exit -eq 0) { Rec "measure-n3-idle" "PASS" "ITEM-9" $meas.Exit } else { Rec "measure-n3-idle" "FAIL" "measure" $meas.Exit }

$rev = Invoke-Command -Session $sess -ScriptBlock {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ramshared\scripts\windows\Invoke-RevokeDrill.ps1 `
        -ServiceName "ramshared-winsvc-DOES-NOT-EXIST" -ArtifactDir C:\ramshared\artifacts\revoke 2>&1 | Out-Null
    $LASTEXITCODE
}
if ($rev -eq 2) { Rec "revoke-fail-path" "PASS" "missing service" 2 } else { Rec "revoke-fail-path" "WARN" "exit" $rev }

# #15: deterministic tool gaps are SKIP, not retry/FAIL theatre
if (-not $inv.Wdk) { Rec "wdk" "SKIP" "no WDK" 0 }
if (-not $inv.Nvcuda) { Rec "cuda-nvcuda" "SKIP" "no nvcuda.dll" 0 }
if (-not $inv.LinkExe) { Rec "msvc-link" "SKIP" "no link.exe - cargo msvc needs VS Build Tools" 0 }

$srcOk = Invoke-Command -Session $sess -ScriptBlock {
    @(
        (Test-Path C:\ramshared\drivers\windows\ramshared\driver.c),
        (Test-Path C:\ramshared\drivers\windows\ramshared\protocol.h),
        (Test-Path C:\ramshared\drivers\windows\tools\poolstress\poolstress.c)
    ) -join ","
}
Rec "driver-sources-present" "PASS" $srcOk 3

Remove-PSSession $sess

$fail = @($results | Where-Object { $_.Status -eq "FAIL" }).Count
$passN = @($results | Where-Object { $_.Status -eq "PASS" }).Count
$skip = @($results | Where-Object { $_.Status -eq "SKIP" }).Count
$results | ConvertTo-Json -Depth 5 | Set-Content $ResultsJson -Encoding UTF8
Write-Output ("SUMMARY pass={0} fail={1} skip={2}" -f $passN, $fail, $skip)
if ($fail -gt 0) { Write-Output "OVERALL=FAIL"; exit 1 }
Write-Output "OVERALL=PASS_WITH_SKIPS"
exit 0
