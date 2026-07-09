#Requires -Version 5.1
<#
.SYNOPSIS
  Elevated host: rebuild (optional), sign, deploy INF+sys to win11-drill, start backend.
#>
[CmdletBinding()]
param(
    [string]$RepoRoot = "C:\Users\emedev\ramshared-src",
    [switch]$SkipBuild,
    [string]$PfxPath = "C:\Users\emedev\ramshared-drill\certs\ramshared-test.pfx",
    [string]$PfxPassword = "TestSign!2026",
    [string]$VmName = "win11-drill",
    [string]$GuestUser = ".\drilladmin",
    [string]$GuestPassword = "Drill2026!"
)

$ErrorActionPreference = "Continue"
$log = "C:\Users\emedev\ramshared-drill\install-windrive-vm.log"
Start-Transcript -Path $log -Force

if (-not $SkipBuild) {
    & "$RepoRoot\scripts\windows\Build-Drivers.ps1" -RepoRoot $RepoRoot
    if ($LASTEXITCODE -ne 0 -and -not (Test-Path "$RepoRoot\drivers\windows\ramshared\x64\Release\ramshared.sys")) {
        throw "build failed"
    }
}

$env:RAMSHARED_TESTSIGN_PFX_PASSWORD = $PfxPassword
& "$RepoRoot\scripts\windows\Sign-Drivers.ps1" -RepoRoot $RepoRoot -PfxPath $PfxPath -PfxPassword $PfxPassword

$pass = ConvertTo-SecureString $GuestPassword -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($GuestUser, $pass)
if ((Get-VM $VmName).State -ne "Running") { Start-VM $VmName; Start-Sleep 6 }
$sess = New-PSSession -VMName $VmName -Credential $cred

# Stage files
Invoke-Command -Session $sess -ScriptBlock {
    New-Item -ItemType Directory -Force -Path C:\ramshared\bin, C:\ramshared\package, C:\ramshared\scripts\windows | Out-Null
}
Copy-Item "$RepoRoot\drivers\windows\ramshared\x64\Release\ramshared.sys" -Destination C:\ramshared\package\ramshared.sys -ToSession $sess -Force
Copy-Item "$RepoRoot\drivers\windows\tools\poolstress\x64\Release\poolstress.sys" -Destination C:\ramshared\package\poolstress.sys -ToSession $sess -Force
Copy-Item "$RepoRoot\drivers\windows\ramshared\ramshared.inf" -Destination C:\ramshared\package\ramshared.inf -ToSession $sess -Force
Copy-Item "$RepoRoot\scripts\windows\*.ps1" -Destination C:\ramshared\scripts\windows\ -ToSession $sess -Force
$cer = "C:\Users\emedev\ramshared-drill\certs\ramshared-test.cer"
if (Test-Path $cer) {
    Copy-Item $cer -Destination C:\ramshared\package\ramshared-test.cer -ToSession $sess -Force
}

$result = Invoke-Command -Session $sess -ScriptBlock {
    $ErrorActionPreference = "Continue"
    $o = @()
    if (Test-Path C:\ramshared\package\ramshared-test.cer) {
        Import-Certificate -FilePath C:\ramshared\package\ramshared-test.cer -CertStoreLocation Cert:\LocalMachine\Root -EA SilentlyContinue | Out-Null
        Import-Certificate -FilePath C:\ramshared\package\ramshared-test.cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher -EA SilentlyContinue | Out-Null
    }

    # Tear down legacy sc services if present
    sc.exe stop ramshared 2>$null | Out-Null
    sc.exe stop poolstress 2>$null | Out-Null
    sc.exe delete ramshared 2>$null | Out-Null
    sc.exe delete poolstress 2>$null | Out-Null
    Start-Sleep 2

    # Copy sys into drivers dir for INF
    Copy-Item C:\ramshared\package\ramshared.sys C:\Windows\System32\drivers\ramshared.sys -Force
    Copy-Item C:\ramshared\package\poolstress.sys C:\Windows\System32\drivers\poolstress.sys -Force

    # INF install
    $inf = "C:\ramshared\package\ramshared.inf"
    $pn = pnputil /add-driver $inf /install 2>&1 | Out-String
    $o += "PNPUTIL=$pn"

    # Root device (Win10 2004+)
    $add = pnputil /add-device "Root\RamShared" 2>&1 | Out-String
    $o += "ADD_DEVICE=$add"

    # Fallback: legacy sc for both if INF fails to start
    sc.exe create poolstress type= kernel start= demand binPath= C:\Windows\System32\drivers\poolstress.sys 2>&1 | Out-Null
    sc.exe start poolstress 2>&1 | Out-String | ForEach-Object { $o += "START_POOL=$_" }

    # If INF didn't create ramshared service, sc create
    $q = sc.exe query ramshared 2>&1 | Out-String
    if ($q -match "1060|does not exist|nao existe|não existe") {
        sc.exe create ramshared type= kernel start= demand binPath= C:\Windows\System32\drivers\ramshared.sys 2>&1 | Out-String | ForEach-Object { $o += "SC_CREATE=$_" }
    }
    sc.exe start ramshared 2>&1 | Out-String | ForEach-Object { $o += "START_RAM=$_" }

    $o += "Q_RAM=$((sc.exe query ramshared | Out-String))"
    $o += "Q_POOL=$((sc.exe query poolstress | Out-String))"
    $o += "DISKS=$((Get-Disk | Select Number,FriendlyName,Size | ConvertTo-Json -Compress))"
    $o -join "`n"
}
Write-Output $result

# Start backend loop in background on guest (30s smoke)
$job = Invoke-Command -Session $sess -AsJob -ScriptBlock {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ramshared\scripts\windows\Invoke-WinDriveBackend.ps1 -SizeBytes 268435456 -Seconds 45 2>&1 | Out-String
}
Write-Output "BACKEND_JOB=$($job.Id)"
Wait-Job $job -Timeout 90 | Out-Null
$backendOut = Receive-Job $job
Write-Output "BACKEND_OUT=$backendOut"

$disks = Invoke-Command -Session $sess -ScriptBlock {
    Get-Disk | Select-Object Number, FriendlyName, Size, PartitionStyle, OperationalStatus | ConvertTo-Json -Compress
}
Write-Output "DISKS_AFTER=$disks"

Remove-PSSession $sess
Write-Output "INSTALL_WINDRIVE_DONE"
Stop-Transcript
