#Requires -Version 5.1
<#
.SYNOPSIS
  Restore PowerShell Direct access for the disposable win11-wsl2-lab VM.

.DESCRIPTION
  This lab-only helper mounts the existing VM-owned VHD, registers a one-shot
  LocalSystem bootstrap service in every offline ControlSet, and dismounts the
  VHD. On the next guest boot the service creates or resets the lab user, enables
  local admin access, writes C:\ramshared-bootstrap.log, and removes itself.

  The script is intentionally scoped to the approved disposable WSL2 lab VM. It
  never creates a VM, never formats disks, and refuses VHD paths outside the
  selected VM directory.
#>
[CmdletBinding()]
param(
    [string]$VMName = "win11-wsl2-lab",
    [string]$VhdPath = "C:\ramshared-hyperv\win11-wsl2-lab\Virtual Hard Disks\win11-wsl2-lab.vhdx",
    [string]$LabUser = "drilladmin",
    [string]$Password = "",
    [string]$PasswordFile = "C:\ramshared\bin\.drill-pw",
    [string]$ArtifactRoot = "C:\ramshared\artifacts"
)

$ErrorActionPreference = "Stop"

function Get-LocalDrillPassword {
    param(
        [string]$InitialPassword,
        [string]$LocalPasswordFile
    )
    if (-not [string]::IsNullOrEmpty($InitialPassword)) {
        return $InitialPassword
    }
    foreach ($scope in @("Machine", "User")) {
        $value = [Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)
        if (-not [string]::IsNullOrEmpty($value)) {
            return $value
        }
    }
    if (-not [string]::IsNullOrEmpty($env:RAMSHARED_DRILL_PASSWORD)) {
        return $env:RAMSHARED_DRILL_PASSWORD
    }
    if (Test-Path -LiteralPath $LocalPasswordFile) {
        return (Get-Content -LiteralPath $LocalPasswordFile -Raw).Trim()
    }
    return ""
}

function Assert-LabBoundary {
    param(
        [string]$Name,
        [string]$Path
    )
    if ($Name -ne "win11-wsl2-lab") {
        throw "Refusing VM '$Name'. This repair helper is scoped to win11-wsl2-lab only."
    }
    $expectedPrefix = "C:\ramshared-hyperv\win11-wsl2-lab\"
    $full = [IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($expectedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing VHD outside the win11-wsl2-lab directory: $full"
    }
    if (-not (Test-Path -LiteralPath $full)) {
        throw "VHD not found: $full"
    }
}

function New-ArtifactDir {
    param([string]$Root)
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $dir = Join-Path $Root "win11-wsl2-lab-offline-access-repair-$stamp"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function New-EncodedBootstrap {
    param(
        [string]$User,
        [string]$PlainPassword
    )
    $script = @"
`$ErrorActionPreference = "Continue"
`$log = "C:\ramshared-bootstrap.log"
New-Item -ItemType Directory -Force -Path "C:\ramshared" | Out-Null
"started=`$(Get-Date -Format o)" | Out-File -Encoding ASCII -Append `$log
`$pw = @'
$PlainPassword
'@
`$user = "$User"
cmd.exe /c "net user `$user ""`$pw"" /add /expires:never" | Out-File -Encoding ASCII -Append `$log
cmd.exe /c "net user `$user ""`$pw""" | Out-File -Encoding ASCII -Append `$log
cmd.exe /c "net localgroup Administrators `$user /add" | Out-File -Encoding ASCII -Append `$log
cmd.exe /c "net accounts /maxpwage:unlimited" | Out-File -Encoding ASCII -Append `$log
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name LocalAccountTokenFilterPolicy -Type DWord -Value 1
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" -Name AutoAdminLogon -Type String -Value "1"
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" -Name DefaultUserName -Type String -Value `$user
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" -Name DefaultPassword -Type String -Value `$pw
Set-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" -Name DefaultDomainName -Type String -Value `$env:COMPUTERNAME
"finished=`$(Get-Date -Format o)" | Out-File -Encoding ASCII -Append `$log
sc.exe delete RamSharedBootstrap | Out-File -Encoding ASCII -Append `$log
"@
    return [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
}

Assert-LabBoundary -Name $VMName -Path $VhdPath
$artifactDir = New-ArtifactDir -Root $ArtifactRoot
$Password = Get-LocalDrillPassword -InitialPassword $Password -LocalPasswordFile $PasswordFile
if ([string]::IsNullOrEmpty($Password)) {
    throw "Missing local lab credential. Set RAMSHARED_DRILL_PASSWORD or provide an ignored PasswordFile."
}

$vm = Get-VM -Name $VMName -ErrorAction Stop
if ($vm.State -ne "Off") {
    Stop-VM -Name $VMName -TurnOff -Force -ErrorAction Stop
}

$mounted = $false
try {
    Mount-VHD -Path $VhdPath -ErrorAction Stop
    $mounted = $true
    $disk = Get-Disk | Where-Object { $_.Location -like "*$VhdPath*" } | Select-Object -First 1
    if (-not $disk) {
        $disk = Get-Disk | Sort-Object Number -Descending | Select-Object -First 1
    }
    $winPart = Get-Partition -DiskNumber $disk.Number |
        Where-Object { $_.DriveLetter -and (Test-Path ("$($_.DriveLetter):\Windows\System32")) } |
        Select-Object -First 1
    if (-not $winPart) {
        throw "Windows partition not found in $VhdPath"
    }
    $winDrive = "$($winPart.DriveLetter):"
    reg.exe load HKLM\RS_SYSTEM "$winDrive\Windows\System32\config\SYSTEM" |
        Out-File (Join-Path $artifactDir "reg-load-system.txt")
    try {
        $encoded = New-EncodedBootstrap -User $LabUser -PlainPassword $Password
        $imagePath = "%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -ExecutionPolicy Bypass -EncodedCommand $encoded"
        $controlSets = Get-ChildItem "HKLM:\RS_SYSTEM" |
            Where-Object { $_.PSChildName -match "^ControlSet\d{3}$" }
        foreach ($controlSet in $controlSets) {
            $servicePath = Join-Path $controlSet.PSPath "Services\RamSharedBootstrap"
            New-Item -Path $servicePath -Force | Out-Null
            New-ItemProperty -Path $servicePath -Name Type -PropertyType DWord -Value 16 -Force | Out-Null
            New-ItemProperty -Path $servicePath -Name Start -PropertyType DWord -Value 2 -Force | Out-Null
            New-ItemProperty -Path $servicePath -Name ErrorControl -PropertyType DWord -Value 1 -Force | Out-Null
            New-ItemProperty -Path $servicePath -Name ImagePath -PropertyType ExpandString -Value $imagePath -Force | Out-Null
            New-ItemProperty -Path $servicePath -Name DisplayName -PropertyType String -Value "RamShared one-shot lab bootstrap" -Force | Out-Null
            New-ItemProperty -Path $servicePath -Name ObjectName -PropertyType String -Value "LocalSystem" -Force | Out-Null
        }
        foreach ($controlSet in $controlSets) {
            $regPath = "HKLM\RS_SYSTEM\$($controlSet.PSChildName)\Services\RamSharedBootstrap"
            reg.exe query $regPath |
                ForEach-Object { $_ -replace [Regex]::Escape($encoded), "<encoded-redacted>" } |
                Out-File -Append (Join-Path $artifactDir "services-redacted.txt")
        }
        [pscustomobject]@{
            STATUS = "PASS"
            VM = $VMName
            VHD = $VhdPath
            WIN_DRIVE = $winDrive
            CONTROL_SETS = @($controlSets | ForEach-Object { $_.PSChildName })
            ARTIFACT = $artifactDir
            SECRET_PERSISTED_IN_REPO = $false
            NEXT_STEP = "Start the VM and authenticate as .\$LabUser after heartbeat is OK."
        } | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $artifactDir "summary.json")
    } finally {
        reg.exe unload HKLM\RS_SYSTEM | Out-Null
    }
} finally {
    if ($mounted) {
        Dismount-VHD -Path $VhdPath -ErrorAction SilentlyContinue
    }
}

Get-Content -LiteralPath (Join-Path $artifactDir "summary.json") -Raw
