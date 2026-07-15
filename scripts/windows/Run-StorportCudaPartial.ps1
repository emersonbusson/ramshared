#Requires -Version 5.1
# Elevated host: start win11-drill, deploy package+winsvc, load driver, basic CREATE/REGISTER, probe verdicts.
param(
  [string]$VMName = "win11-drill",
  [string]$User = ".\drilladmin",
  [string]$Password = $env:RAMSHARED_DRILL_PASSWORD
)
$ErrorActionPreference = "Stop"
if ([string]::IsNullOrEmpty($Password)) {
  if (Test-Path "C:\ramshared\bin\.drill-pw") {
    $Password = (Get-Content "C:\ramshared\bin\.drill-pw" -Raw).Trim()
  }
}
if ([string]::IsNullOrEmpty($Password)) { throw "RAMSHARED_DRILL_PASSWORD / .drill-pw required" }
$sec = ConvertTo-SecureString $Password -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($User, $sec)

$vm = Get-VM -Name $VMName -EA Stop
if ($vm.State -ne "Running") {
  Write-Host "Starting $VMName..."
  Start-VM -Name $VMName
  $t=0
  while ((Get-VM $VMName).State -ne "Running" -and $t -lt 90) { Start-Sleep 2; $t+=2 }
}
# Wait for integration services
$t=0
while ($t -lt 120) {
  try {
    $r = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock { "PSD_OK " + $env:COMPUTERNAME } -ErrorAction Stop
    Write-Host $r
    break
  } catch {
    Start-Sleep 3
    $t += 3
  }
}
if ($t -ge 120) { throw "PSD not ready" }

$artifact = "C:\Users\emedev\ramshared-drill\agent-storport-cuda-$(Get-Date -Format yyyyMMdd-HHmmss)"
New-Item -Force -ItemType Directory $artifact | Out-Null

# Deploy files into guest via PSD Copy-VMFile if available, else base64
function Send-GuestFile([string]$Local, [string]$RemoteDir, [string]$Name) {
  $bytes = [IO.File]::ReadAllBytes($Local)
  $b64 = [Convert]::ToBase64String($bytes)
  Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
    param($d,$n,$b)
    New-Item -Force -ItemType Directory $d | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $d $n), [Convert]::FromBase64String($b))
  } -ArgumentList $RemoteDir,$Name,$b64
}

$pkg = "C:\ramshared\package"
$gdir = "C:\ramshared\package"
Send-GuestFile (Join-Path $pkg "ramshared.sys") $gdir "ramshared.sys"
Send-GuestFile (Join-Path $pkg "poolstress.sys") $gdir "poolstress.sys"
Send-GuestFile (Join-Path $pkg "ramshared.inf") $gdir "ramshared.inf"
if (Test-Path (Join-Path $pkg "ramshared.cat")) {
  Send-GuestFile (Join-Path $pkg "ramshared.cat") $gdir "ramshared.cat"
}
Send-GuestFile "C:\ramshared\bin\ramshared-winsvc.exe" "C:\ramshared\bin" "ramshared-winsvc.exe"
Send-GuestFile "C:\ProgramData\RamShared\winsvc.toml" "C:\ProgramData\RamShared" "winsvc.toml"

$guestResult = Invoke-Command -VMName $VMName -Credential $cred -ScriptBlock {
  $ErrorActionPreference = "Continue"
  $out = [ordered]@{}
  $out.hostname = $env:COMPUTERNAME
  $out.testsigning = (bcdedit /enum '{current}' | Out-String) -match 'testsigning\s+Yes'
  # stop old services
  sc.exe stop ramshared 2>$null | Out-Null
  sc.exe stop poolstress 2>$null | Out-Null
  Start-Sleep 2
  sc.exe delete ramshared 2>$null | Out-Null
  sc.exe delete poolstress 2>$null | Out-Null
  Start-Sleep 1
  # install kernel services (path that worked historically)
  sc.exe create poolstress type= kernel binPath= C:\ramshared\package\poolstress.sys | Out-Null
  sc.exe create ramshared type= kernel binPath= C:\ramshared\package\ramshared.sys | Out-Null
  $s1 = sc.exe start poolstress 2>&1 | Out-String
  $s2 = sc.exe start ramshared 2>&1 | Out-String
  $out.poolstress = (sc.exe query poolstress | Out-String)
  $out.ramshared = (sc.exe query ramshared | Out-String)
  $out.start_pool = $s1
  $out.start_ram = $s2
  $out.running = ($out.ramshared -match 'RUNNING')
  # probe-cuda if GPU present (often not in VM)
  $winsvc = "C:\ramshared\bin\ramshared-winsvc.exe"
  $cfg = "C:\ProgramData\RamShared\winsvc.toml"
  if ((Test-Path $winsvc) -and (Test-Path $cfg)) {
    $p = & $winsvc probe-cuda --config $cfg 2>&1 | Out-String
    $out.probe = $p
    $out.probe_ok = ($LASTEXITCODE -eq 0)
  } else {
    $out.probe = "missing winsvc/config"
    $out.probe_ok = $false
  }
  # Lab backend smoke if driver running (RAM lab path for IOCTL legitimate path)
  $out.ctl = Test-Path "\\.\RamSharedCtl"
  $out | ConvertTo-Json -Depth 4
}

$guestResult | Set-Content (Join-Path $artifact "guest.json") -Encoding utf8
Write-Host "ARTIFACT=$artifact"
Write-Host $guestResult

# Leave VM On for further work? Spec: leave Off after. Stop for safety.
Stop-VM -Name $VMName -Force -EA SilentlyContinue
Write-Host "VM stopped"
