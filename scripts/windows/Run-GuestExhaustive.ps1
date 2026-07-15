#Requires -Version 5.1
# Elevated: win11-drill deploy + IOCTL + optional Verifier reboot pass
param(
    [string]$VMName = "win11-drill",
    [string]$User = ".\drilladmin",
    [string]$Password = $env:RAMSHARED_DRILL_PASSWORD,
    [switch]$SkipVerifier,
    [switch]$LeaveVmOn
)
$ErrorActionPreference = "Stop"
if ([string]::IsNullOrEmpty($Password) -and (Test-Path "C:\ramshared\bin\.drill-pw")) {
    $Password = (Get-Content "C:\ramshared\bin\.drill-pw" -Raw).Trim()
}
if ([string]::IsNullOrEmpty($Password)) { throw "RAMSHARED_DRILL_PASSWORD / .drill-pw required" }
$sec = ConvertTo-SecureString $Password -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($User, $sec)

$art = "C:\ramshared\artifacts\guest-exhaustive-$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -Force -ItemType Directory $art | Out-Null
function W($m) {
    $t = "[{0}] {1}" -f (Get-Date -Format HH:mm:ss), $m
    $t | Tee-Object -FilePath (Join-Path $art "host-side.log") -Append
}

$vm = Get-VM -Name $VMName -ErrorAction Stop
if ($vm.State -ne "Running") {
    W ("Starting " + $VMName + " StartupBytes=" + $vm.MemoryStartup)
    Start-VM -Name $VMName
    $t = 0
    while ((Get-VM $VMName).State -ne "Running" -and $t -lt 120) { Start-Sleep 2; $t += 2 }
}
W ("VM state=" + (Get-VM $VMName).State)

$t = 0
while ($t -lt 180) {
    try {
        $r = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
            "PSD_OK " + $env:COMPUTERNAME + " " + [Environment]::OSVersion.VersionString
        } -ErrorAction Stop
        W $r
        break
    } catch {
        Start-Sleep 3
        $t += 3
    }
}
if ($t -ge 180) { throw "PSD not ready after 180s" }

function Send-GuestFile([string]$Local, [string]$RemoteDir, [string]$Name) {
    if (-not (Test-Path $Local)) { throw ("missing " + $Local) }
    $bytes = [IO.File]::ReadAllBytes($Local)
    $b64 = [Convert]::ToBase64String($bytes)
    Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
        param($d, $n, $b)
        New-Item -Force -ItemType Directory $d | Out-Null
        [IO.File]::WriteAllBytes((Join-Path $d $n), [Convert]::FromBase64String($b))
    } -ArgumentList $RemoteDir, $Name, $b64
}

$pkg = "C:\ramshared\package"
Send-GuestFile (Join-Path $pkg "ramshared.sys") "C:\ramshared\package" "ramshared.sys"
Send-GuestFile (Join-Path $pkg "poolstress.sys") "C:\ramshared\package" "poolstress.sys"
Send-GuestFile (Join-Path $pkg "ramshared.inf") "C:\ramshared\package" "ramshared.inf"
if (Test-Path (Join-Path $pkg "ramshared.cat")) {
    Send-GuestFile (Join-Path $pkg "ramshared.cat") "C:\ramshared\package" "ramshared.cat"
}
Send-GuestFile "C:\ramshared\bin\Invoke-WinDriveIoctlValidation.ps1" "C:\ramshared\bin" "Invoke-WinDriveIoctlValidation.ps1"
W "files deployed"

$load = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
    $ErrorActionPreference = "Continue"
    $o = [ordered]@{}
    $o.testsigning = ((bcdedit /enum "{current}" | Out-String) -match "testsigning\s+Yes")
    sc.exe stop ramshared 2>$null | Out-Null
    sc.exe stop poolstress 2>$null | Out-Null
    Start-Sleep 2
    sc.exe delete ramshared 2>$null | Out-Null
    sc.exe delete poolstress 2>$null | Out-Null
    Start-Sleep 1
    sc.exe create poolstress type= kernel binPath= C:\ramshared\package\poolstress.sys | Out-Null
    sc.exe create ramshared type= kernel binPath= C:\ramshared\package\ramshared.sys | Out-Null
    $o.start_pool = (sc.exe start poolstress 2>&1 | Out-String)
    $o.start_ram = (sc.exe start ramshared 2>&1 | Out-String)
    $o.pool = (sc.exe query poolstress | Out-String)
    $o.ram = (sc.exe query ramshared | Out-String)
    $o.running = ($o.ram -match "RUNNING")
    $o.sysLen = (Get-Item C:\ramshared\package\ramshared.sys).Length
    $o.sysSha = (Get-FileHash C:\ramshared\package\ramshared.sys -Algorithm SHA256).Hash
    $o | ConvertTo-Json -Compress
}
$load | Set-Content (Join-Path $art "guest-load.json")
W ("load=" + $load)

$ioctl1 = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
    $ErrorActionPreference = "Continue"
    New-Item -Force -ItemType Directory C:\ramshared\artifacts\ioctl-validation | Out-Null
    & C:\ramshared\bin\Invoke-WinDriveIoctlValidation.ps1 -ArtifactDir C:\ramshared\artifacts\ioctl-validation 2>&1 | Out-String
    "EXIT=" + $LASTEXITCODE
}
$ioctl1 | Set-Content (Join-Path $art "ioctl-pass1.txt")
W "ioctl pass1 done"

$verdictJson = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
    $v = Get-ChildItem C:\ramshared\artifacts\ioctl-validation\verdict-*.json |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($v) { Get-Content $v.FullName -Raw } else { "{}" }
}
$verdictJson | Set-Content (Join-Path $art "verdict-pass1.json")

$verifierDone = $false
$ioctl2 = ""
if (-not $SkipVerifier) {
    W "Enabling Driver Verifier for ramshared.sys (guest reboot required)"
    Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
        $ErrorActionPreference = "Continue"
        verifier /standard /driver ramshared.sys 2>&1 | Out-String
        verifier /query 2>&1 | Out-String
    } | Set-Content (Join-Path $art "verifier-enable.txt")

    W "Restarting guest for Verifier..."
    Restart-VM -Name $VMName -Force
    Start-Sleep 15
    $t = 0
    while ($t -lt 300) {
        try {
            if ((Get-VM $VMName).State -ne "Running") { Start-Sleep 3; $t += 3; continue }
            $null = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock { "PSD_OK" } -ErrorAction Stop
            break
        } catch { Start-Sleep 3; $t += 3 }
    }
    if ($t -ge 300) { throw "PSD not ready after Verifier reboot" }
    W "PSD after reboot OK"

    $load2 = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
        $ErrorActionPreference = "Continue"
        $o = [ordered]@{}
        $o.verifier = (verifier /query 2>&1 | Out-String)
        sc.exe start poolstress 2>$null | Out-Null
        sc.exe start ramshared 2>$null | Out-Null
        Start-Sleep 2
        $o.ram = (sc.exe query ramshared | Out-String)
        $o.running = ($o.ram -match "RUNNING")
        $o.dumps = @(Get-ChildItem C:\Windows\Minidump -Filter *.dmp -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
        $o | ConvertTo-Json -Depth 4 -Compress
    }
    $load2 | Set-Content (Join-Path $art "guest-load-verifier.json")
    W ("load2=" + $load2)

    $ioctl2 = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
        $ErrorActionPreference = "Continue"
        & C:\ramshared\bin\Invoke-WinDriveIoctlValidation.ps1 -ArtifactDir C:\ramshared\artifacts\ioctl-validation -Verifier 2>&1 | Out-String
        "EXIT=" + $LASTEXITCODE
    }
    $ioctl2 | Set-Content (Join-Path $art "ioctl-pass2-verifier.txt")
    $verdict2 = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
        $v = Get-ChildItem C:\ramshared\artifacts\ioctl-validation\verdict-*.json |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($v) { Get-Content $v.FullName -Raw } else { "{}" }
    }
    $verdict2 | Set-Content (Join-Path $art "verdict-pass2-verifier.json")
    $verifierDone = $true
    W "ioctl pass2 under Verifier done"
}

function Parse-Status([string]$text) {
    if ($text -match "STATUS=PASS") { return "PASS" }
    if ($text -match "STATUS=FAIL") { return "FAIL" }
    return "UNKNOWN"
}
$s1 = Parse-Status $ioctl1
$s2 = if ($verifierDone) { Parse-Status $ioctl2 } else { "SKIPPED" }

$sum = [ordered]@{
    ARTIFACT       = $art
    IOCTL_PASS1    = $s1
    IOCTL_VERIFIER = $s2
    VERIFIER_RAN   = $verifierDone
    LEAVE_VM_ON    = [bool]$LeaveVmOn
}
$sum | ConvertTo-Json | Set-Content (Join-Path $art "summary.json")
W ("SUMMARY " + ($sum | ConvertTo-Json -Compress))
Write-Host ("ARTIFACT=" + $art)

if (-not $LeaveVmOn) {
    try {
        Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
            verifier /reset 2>&1 | Out-Null
        } -ErrorAction SilentlyContinue
    } catch {}
    Stop-VM -Name $VMName -Force -ErrorAction SilentlyContinue
    W "VM stopped; verifier reset best-effort"
}

if ($s1 -eq "PASS" -and ($s2 -eq "PASS" -or $s2 -eq "SKIPPED")) { exit 0 }
exit 2
