#Requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet("All", "FreshInstall", "Repair", "ManufacturedRollback",
        "UninstallRefusal", "CleanUninstall")]
    [string]$Case = "All",
    [string]$VMName = "win11-drill",
    [string]$User = "WIN11-DRILL\drilladmin",
    [string]$Password = "",
    [string]$HostBinDir = "C:\ramshared\bin",
    [string]$DriverPackage = "C:\ramshared\artifacts\driver-package-build",
    [string]$ArtifactRoot = "C:\ramshared\artifacts",
    [ValidatePattern("^[A-Z]$")]
    [string]$VolumeLetter = "R"
)

$ErrorActionPreference = "Stop"
function Get-DrillPassword {
    if ($Password) { return $Password }
    foreach ($scope in @("Machine", "User")) {
        $candidate = [Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD", $scope)
        if ($candidate) { return $candidate }
    }
    if ($env:RAMSHARED_DRILL_PASSWORD) { return $env:RAMSHARED_DRILL_PASSWORD }
    throw "Missing RAMSHARED_DRILL_PASSWORD."
}
function Add-Artifact([Collections.Generic.List[object]]$Rows, [string]$Role,
    [string]$Path, [string]$Root) {
    $Rows.Add([ordered]@{
            role          = $Role
            relative_path = $Path.Substring($Root.Length).TrimStart("\")
            sha256        = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
        })
}
function New-Package([string]$Root, [string]$Version, [string]$Commit) {
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
    Copy-Item (Join-Path $HostBinDir "ramshared-winbroker.exe") $Root -Force
    Copy-Item (Join-Path $HostBinDir "ramshared-winsvc.exe") $Root -Force
    Copy-Item (Join-Path $DriverPackage "ramshared.inf") $Root -Force
    Copy-Item (Join-Path $DriverPackage "ramshared.cat") $Root -Force
    Copy-Item (Join-Path $DriverPackage "ramshared.sys") $Root -Force
    [IO.File]::WriteAllLines((Join-Path $Root "broker.toml"), [string[]]@(
            "[local_broker]", "schema = 1", "capacity_bytes = 67108864",
            'allowed_tenant = "windows-drive"',
            'evidence_path = "C:\\ProgramData\\RamShared\\evidence\\broker.jsonl"'
        ), [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllLines((Join-Path $Root "winsvc.toml"), [string[]]@(
            "[win_drive]", "size_bytes = 67108864", "block_size = 4096",
            "cuda_device = 0", "reserve_bytes = 536870912", "queue_depth = 4",
            "max_io_bytes = 1048576",
            'evidence_path = "C:\\ProgramData\\RamShared\\evidence\\winsvc.jsonl"',
            "volume_letter = `"$VolumeLetter`"", 'broker_pipe = "named_pipe_v1"',
            "broker_ready_timeout_secs = 30", 'tenant = "windows-drive"',
            "heartbeat_secs = 5"
        ), [Text.UTF8Encoding]::new($false))
    $artifacts = [Collections.Generic.List[object]]::new()
    Add-Artifact $artifacts "broker_exe" (Join-Path $Root "ramshared-winbroker.exe") $Root
    Add-Artifact $artifacts "broker_config" (Join-Path $Root "broker.toml") $Root
    Add-Artifact $artifacts "winsvc_exe" (Join-Path $Root "ramshared-winsvc.exe") $Root
    Add-Artifact $artifacts "winsvc_config" (Join-Path $Root "winsvc.toml") $Root
    Add-Artifact $artifacts "driver_inf" (Join-Path $Root "ramshared.inf") $Root
    Add-Artifact $artifacts "driver_cat" (Join-Path $Root "ramshared.cat") $Root
    Add-Artifact $artifacts "driver_sys" (Join-Path $Root "ramshared.sys") $Root
    $manifest = [ordered]@{
        schema       = 1
        version      = $Version
        commit       = $Commit
        architecture = "x86_64-pc-windows-msvc"
        start_policy = "demand"
        services     = [ordered]@{
            broker_name     = "RamSharedBroker"
            broker_account  = "NT SERVICE\RamSharedBroker"
            consumer_name   = "RamSharedWinSvc"
            consumer_account = "LocalSystem"
        }
        artifacts    = $artifacts
    } | ConvertTo-Json -Depth 8
    [IO.File]::WriteAllText((Join-Path $Root "product-manifest.json"), $manifest,
        [Text.UTF8Encoding]::new($false))
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$hostRun = Join-Path $ArtifactRoot "product-package-$stamp"
$oldPackage = Join-Path $hostRun "candidate-old"
$newPackage = Join-Path $hostRun "candidate-new"
if ($VolumeLetter -eq "R") {
    $oldVersion = "0.1.0-vm"
    $oldCommit = "1111111111111111111111111111111111111111"
    $newVersion = "0.1.1-vm"
    $newCommit = "2222222222222222222222222222222222222222"
}
else {
    $oldVersion = "0.1.0-physical"
    $oldCommit = "3333333333333333333333333333333333333333"
    $newVersion = "0.1.1-physical"
    $newCommit = "4444444444444444444444444444444444444444"
}
New-Package $oldPackage $oldVersion $oldCommit
New-Package $newPackage $newVersion $newCommit
$oldVersionRootName = "$oldVersion-$($oldCommit.Substring(0, 12))"

$credential = [pscredential]::new($User,
    (ConvertTo-SecureString (Get-DrillPassword) -AsPlainText -Force))
if ((Get-VM $VMName).State -ne "Running") { Start-VM $VMName | Out-Null }
$session = New-PSSession -VMName $VMName -Credential $credential
$guestInput = "C:\ramshared\product-package-input"
$guestResults = "C:\ramshared\product-package-results.json"

try {
    Invoke-Command -Session $session -ScriptBlock {
        param($inputRoot)
        $ramDisks = @(Get-Disk -ErrorAction Stop | Where-Object FriendlyName -Match "RAMSHARE")
        if ($ramDisks.Count -ne 0) { throw "preflight refuses existing RamShared disk" }
        foreach ($name in @("RamSharedWinSvc", "RamSharedBroker")) {
            Stop-Service $name -ErrorAction SilentlyContinue
            & sc.exe delete $name 2>&1 | Out-Null
        }
        Start-Sleep -Milliseconds 750
        $versions = "C:\Program Files\RamShared\versions"
        if (Test-Path $versions) {
            & takeown.exe /F $versions /A /R /D Y 2>&1 | Out-Null
            & icacls.exe $versions /grant "*S-1-5-32-544:(OI)(CI)F" /T /C 2>&1 | Out-Null
            Remove-Item $versions -Recurse -Force
        }
        Remove-Item "C:\ProgramData\RamShared\active-manifest.*" -Force `
            -ErrorAction SilentlyContinue
        Remove-Item $inputRoot -Recurse -Force -ErrorAction SilentlyContinue
        New-Item $inputRoot -ItemType Directory -Force | Out-Null
    } -ArgumentList $guestInput
    Copy-Item $oldPackage (Join-Path $guestInput "old") -ToSession $session -Recurse
    Copy-Item $newPackage (Join-Path $guestInput "new") -ToSession $session -Recurse

    $results = Invoke-Command -Session $session -ScriptBlock {
        param($inputRoot, $selectedCase, $resultPath, $expectedOldRoot)
        $ErrorActionPreference = "Stop"
        $rows = [Collections.Generic.List[object]]::new()
        function Pass([string]$Name, [string]$Detail) {
            $rows.Add([pscustomobject]@{ test = $Name; verdict = "PASS"; detail = $Detail })
        }
        function Invoke-Controller([string]$Controller, [string[]]$Arguments,
            [bool]$ExpectSuccess = $true) {
            $savedPreference = $ErrorActionPreference
            $ErrorActionPreference = "Continue"
            try {
                $text = & $Controller @Arguments 2>&1 | Out-String
                $code = $LASTEXITCODE
            }
            finally { $ErrorActionPreference = $savedPreference }
            if ($ExpectSuccess -and $code -ne 0) { throw "controller failed ($code): $text" }
            if (-not $ExpectSuccess -and $code -eq 0) { throw "controller unexpectedly passed: $text" }
            [pscustomobject]@{ code = $code; text = $text }
        }
        $old = Join-Path $inputRoot "old"
        $new = Join-Path $inputRoot "new"
        $controller = Join-Path $old "ramshared-winsvc.exe"
        $oldManifest = Join-Path $old "product-manifest.json"
        $newManifest = Join-Path $new "product-manifest.json"
        $active = "C:\ProgramData\RamShared\active-manifest.json"

        if ($selectedCase -in @("All", "FreshInstall")) {
            Invoke-Controller $controller @("install", "--manifest", $oldManifest) | Out-Null
            $broker = Get-CimInstance Win32_Service -Filter "Name='RamSharedBroker'"
            $consumer = Get-CimInstance Win32_Service -Filter "Name='RamSharedWinSvc'"
            if ($broker.State -ne "Stopped" -or $consumer.State -ne "Stopped") {
                throw "installation started a service"
            }
            $dependencies = @((Get-ItemProperty `
                        "HKLM:\SYSTEM\CurrentControlSet\Services\RamSharedWinSvc").DependOnService)
            if ($dependencies -notcontains "RamSharedBroker") {
                throw "consumer dependency mismatch"
            }
            $activeDoc = Get-Content $active -Raw | ConvertFrom-Json
            Pass "FreshInstall" "version=$($activeDoc.version); services=Stopped; dependency=match"
        }
        if ($selectedCase -in @("All", "Repair")) {
            $before = (Get-FileHash $active -Algorithm SHA256).Hash
            Invoke-Controller $controller @("repair", "--manifest", $oldManifest) | Out-Null
            $after = (Get-FileHash $active -Algorithm SHA256).Hash
            if ($before -ne $after) { throw "idempotent repair changed active manifest" }
            Pass "Repair" "active_manifest_sha256=$after; idempotent=true"
        }
        if ($selectedCase -in @("All", "ManufacturedRollback")) {
            $before = Get-Content $active -Raw | ConvertFrom-Json
            $env:RAMSHARED_TEST_FAIL_AFTER_BROKER = "1"
            try {
                Invoke-Controller (Join-Path $new "ramshared-winsvc.exe") `
                    @("install", "--manifest", $newManifest) $false | Out-Null
            }
            finally { Remove-Item Env:\RAMSHARED_TEST_FAIL_AFTER_BROKER -ErrorAction SilentlyContinue }
            $after = Get-Content $active -Raw | ConvertFrom-Json
            $brokerImage = (Get-ItemProperty `
                    "HKLM:\SYSTEM\CurrentControlSet\Services\RamSharedBroker").ImagePath
            $consumerImage = (Get-ItemProperty `
                    "HKLM:\SYSTEM\CurrentControlSet\Services\RamSharedWinSvc").ImagePath
            if ($after.commit -ne $before.commit -or
                $brokerImage -notmatch [regex]::Escape($expectedOldRoot) -or
                $consumerImage -notmatch [regex]::Escape($expectedOldRoot)) {
                throw "manufactured rollback left mixed state"
            }
            Pass "ManufacturedRollback" "active=$($after.commit); both_image_paths=old"
        }
        if ($selectedCase -in @("All", "UninstallRefusal")) {
            $saved = [IO.File]::ReadAllBytes($active)
            [IO.File]::WriteAllText($active, "{corrupt")
            try {
                Invoke-Controller $controller @("uninstall") $false | Out-Null
                if (-not (Get-Service RamSharedBroker -ErrorAction SilentlyContinue) -or
                    -not (Get-Service RamSharedWinSvc -ErrorAction SilentlyContinue)) {
                    throw "uninstall refusal deleted a service"
                }
                Pass "UninstallRefusal" "corrupt active pointer refused before SCM mutation"
            }
            finally { [IO.File]::WriteAllBytes($active, $saved) }
        }
        if ($selectedCase -in @("All", "CleanUninstall")) {
            Invoke-Controller $controller @("uninstall") | Out-Null
            if ((Get-Service RamSharedBroker -ErrorAction SilentlyContinue) -or
                (Get-Service RamSharedWinSvc -ErrorAction SilentlyContinue) -or
                (Test-Path $active)) {
                throw "clean uninstall left active SCM/pointer state"
            }
            Pass "CleanUninstall" "services=absent; active_pointer=absent"
        }
        $rows | ConvertTo-Json -Depth 6 | Set-Content $resultPath -Encoding UTF8
        $rows
    } -ArgumentList $guestInput, $Case, $guestResults, $oldVersionRootName

    Copy-Item $guestResults (Join-Path $hostRun "results.json") -FromSession $session -Force
    $results
    Write-Host "EVIDENCE=$hostRun"
}
finally {
    if ($session) { Remove-PSSession $session }
}
