<#
.SYNOPSIS
  Boot the VMBus drill kernel as a disposable Gen2 Hyper-V guest and capture
  its serial console.

.DESCRIPTION
  Builds a 128 MiB FAT32 ESP VHDX holding only \EFI\BOOT\BOOTX64.EFI, defines
  a Gen2 Hyper-V VM around it with a COM1 named pipe, boots, streams the
  console until the guest prints HYPERV_DRILL_DONE, then deletes the VM and
  the VHDX.

  This is a capability-and-drill harness, not a production host path. It never
  runs on the daily WSL2 host (guard below), never activates swap, never
  applies memory pressure to the host, and never changes RamShared lifecycle
  state. The only mutation is a VM that exists for the length of the job.

.PARAMETER BzImage
  Path to the bootable drill bzImage (built by build-hyperv-drill-kernel.sh).

.PARAMETER WorkDir
  Scratch directory for the VHDX and logs. Defaults to .\drill-vm-work.

.PARAMETER VmName
  Hyper-V VM name. Defaults to vmbus-drill-$PID.

.PARAMETER TimeoutSec
  Maximum wall-clock to wait for the guest to finish. Default 420.

.EXAMPLE
  ./new-hyperv-drill-vm.ps1 -BzImage ./drill-out/BOOTX64.EFI
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$BzImage,

  [string]$WorkDir = (Join-Path (Get-Location) 'drill-vm-work'),

  [string]$VmName = "vmbus-drill-$PID",

  [int]$TimeoutSec = 420,

  [int]$MemoryMB = 2048,

  [int]$CpuCount = 2
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Note($k, $v) {
  '{0}={1}' -f $k, $v | Tee-Object -FilePath $script:report -Append
  Write-Host "DRILL-VM $k=$v"
}

function AsText($raw) {
  if ($null -eq $raw) { return '' }
  $s = ($raw | Out-String)
  return (($s -replace "`0", '') -replace "\s+", ' ').Trim()
}

# --- host-safety guard -------------------------------------------------------
if (Test-Path '/proc/version') {
  $pv = ''
  try { $pv = Get-Content '/proc/version' -Raw } catch {}
  if ($pv -match 'microsoft-standard-WSL2') {
    Write-Host 'REFUSE: this is the daily WSL2 host. The drill guest must be a disposable Hyper-V VM.'
    exit 2
  }
}

if (-not $env:GITHUB_ACTIONS -and -not $env:HYPERV_DRILL_OK) {
  Write-Host 'REFUSE: set HYPERV_DRILL_OK=1 to confirm this is a disposable lab/CI host.'
  Write-Host 'This harness defines and deletes a Hyper-V VM. It must not run unattended on a shared machine.'
  exit 2
}

# --- inputs ------------------------------------------------------------------
if (-not (Test-Path $BzImage)) {
  Write-Host "ERROR: bzImage not found at $BzImage"
  exit 2
}
$BzImage = (Resolve-Path $BzImage).Path

New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$WorkDir = (Resolve-Path $WorkDir).Path
$script:report = Join-Path $WorkDir 'drill-vm-report.txt'
$consoleLog = Join-Path $WorkDir 'drill-console.log'
$vhdPath = Join-Path $WorkDir 'drill-esp.vhdx'
$pipeName = "vmbus-drill-$PID"
$pipePath = "\\.\pipe\$pipeName"

Set-Content -Path $script:report -Value "DRILL-VM start $(Get-Date -Format o)"
Set-Content -Path $consoleLog -Value ''

Note runner $env:RUNNER_OS
Note bzimage_bytes (Get-Item $BzImage).Length
Note vm_name $VmName
Note pipe_path $pipePath
Note memory_mb $MemoryMB
Note cpu_count $CpuCount
Note timeout_sec $TimeoutSec

# --- module preflight --------------------------------------------------------
if (-not (Get-Module -ListAvailable -Name Hyper-V)) {
  Note hyperv_module unavailable
  Write-Host 'ERROR: Hyper-V PowerShell module is not available.'
  exit 1
}
Import-Module Hyper-V
Note hyperv_module imported

if (-not (Get-Command New-VM -ErrorAction SilentlyContinue)) {
  Note new_vm_cmdlet unavailable
  Write-Host 'ERROR: New-VM cmdlet is not available.'
  exit 1
}
Note new_vm_cmdlet present

# --- build the ESP VHDX ------------------------------------------------------
$vhd = $null
$mounted = $false
try {
  Note esp_build begin
  if (Test-Path $vhdPath) { Remove-Item $vhdPath -Force }

  # Fixed 128 MiB: one kernel image, nothing else. Small enough to keep the
  # runner's C: footprint bounded and large enough for any bzImage we build.
  $vhd = New-VHD -Path $vhdPath -SizeBytes 128MB -Fixed
  $disk = $vhd | Mount-VHD -PassThru
  $mounted = $true
  Note esp_mounted_disk $disk.Number

  $disk | Initialize-Disk -PartitionStyle GPT -Confirm:$false

  # GptType is the EFI System Partition GUID. A plain data partition will not
  # be offered to the Gen2 UEFI firmware as a boot target.
  $ESP_GPT = '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}'
  $part = New-Partition -DiskNumber $disk.Number -Size 100MB -AssignDriveLetter -GptType $ESP_GPT
  $vol = $part | Format-Volume -FileSystem FAT32 -NewFileSystemLabel 'DRILLESP' -Confirm:$false
  $esp = '{0}:\' -f $vol.DriveLetter
  Note esp_drive $esp

  New-Item -ItemType Directory -Force -Path (Join-Path $esp 'EFI\BOOT') | Out-Null
  Copy-Item $BzImage (Join-Path $esp 'EFI\BOOT\BOOTX64.EFI') -Force

  $onEsp = Get-Item (Join-Path $esp 'EFI\BOOT\BOOTX64.EFI')
  Note esp_bootx64_bytes $onEsp.Length
  Note esp_bootx64_sha256 (Get-FileHash $onEsp.FullName -Algorithm SHA256).Hash

  Dismount-VHD -Path $vhdPath
  $mounted = $false
  Note esp_build ok
}
catch {
  Note esp_build ("failed: " + $_.Exception.Message)
  if ($mounted) {
    try { Dismount-VHD -Path $vhdPath -ErrorAction SilentlyContinue } catch {}
  }
  Write-Host "ERROR: could not build the ESP VHDX: $($_.Exception.Message)"
  exit 1
}

# --- define the guest --------------------------------------------------------
$vm = $null
try {
  Note vm_define begin
  $vm = New-VM -Name $VmName -Generation 2 -MemoryStartupBytes ($MemoryMB * 1MB) -VHDPath $vhdPath
  Set-VM -Name $VmName -AutomaticCheckpointsEnabled $false -CheckpointType Disabled
  Set-VMProcessor -VMName $VmName -Count $CpuCount
  Set-VMFirmware -VMName $VmName -EnableSecureBoot Off
  Set-VMComPort -VMName $VmName -Number 1 -Path $pipePath

  # One synthetic NIC. The lifecycle and fragmentation drills rebind it to
  # force a fresh ring allocation; without it neither drill can run. The NIC
  # does not need to reach anything: a netvsc device bound to a private switch
  # is enough for the rebind cycle.
  #
  # "Default Switch" exists on Windows client Hyper-V only. Hosted runners are
  # Windows Server, so a private switch is the normal path and the default
  # switch is the convenience.
  $switchName = $null
  foreach ($candidate in 'Default Switch', 'vmbus-drill-private') {
    if (Get-VMSwitch -Name $candidate -ErrorAction SilentlyContinue) {
      $switchName = $candidate
      break
    }
  }
  if (-not $switchName) {
    try {
      New-VMSwitch -Name 'vmbus-drill-private' -SwitchType Private -ErrorAction Stop | Out-Null
      Note drill_switch created
      $switchName = 'vmbus-drill-private'
    }
    catch {
      Note drill_switch ("failed: " + $_.Exception.Message)
    }
  }
  else {
    Note drill_switch existing
  }

  Get-VMNetworkAdapter -VMName $VmName | Remove-VMNetworkAdapter -ErrorAction SilentlyContinue
  if ($switchName) {
    Add-VMNetworkAdapter -VMName $VmName -SwitchName $switchName -Name 'drill-nic'
    Note vm_network_adapter $switchName
  }
  else {
    Note vm_network_adapter none
  }

  Note vm_define ok
}
catch {
  Note vm_define ("failed: " + $_.Exception.Message)
  Write-Host "ERROR: could not define the VM: $($_.Exception.Message)"
  try { Remove-VHD -Path $vhdPath -Force -ErrorAction SilentlyContinue } catch {}
  exit 1
}

# --- console reader ----------------------------------------------------------
# Hyper-V owns the named pipe as the server the moment the VM starts. The host
# connects as a client. Connecting before Start-VM would block with no server,
# so the reader runs on a thread job that blocks in Connect() and the VM start
# unblocks it. Doing it in that order keeps the earliest guest output.
$readerScript = {
  param($pipeName, $logPath, $timeoutSec)

  $deadline = [DateTime]::UtcNow.AddSeconds($timeoutSec)
  $pipe = $null
  for ($i = 0; $i -lt 30 -and -not $pipe; $i++) {
    try {
      $p = New-Object System.IO.Pipes.NamedPipeClientStream(
        '.', $pipeName,
        [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::None)
      $p.Connect(10000)
      $pipe = $p
    }
    catch {
      try { $p.Dispose() } catch {}
      Start-Sleep -Milliseconds 500
    }
  }
  if (-not $pipe) {
    Add-Content -Path $logPath -Value 'DRILL-VM console_connect=failed'
    return 'CONNECT_FAILED'
  }
  Add-Content -Path $logPath -Value 'DRILL-VM console_connect=ok'

  # NamedPipeClientStream.ReadTimeout is not supported on every .NET this
  # harness runs on — the property setter throws "Timeouts are not supported
  # on this stream" and, with the default ErrorActionPreference, that error
  # becomes the job's whole output while a subsequent blocking Read parks the
  # loop until the outer Wait-Job kills it. Do not depend on the property.
  # ReadAsync + Wait(1000) gives a real deadline on every runtime and still
  # keeps a quiet guest from stalling the reader.
  $buf = New-Object byte[] 4096
  $pending = ''
  $seenDone = $false
  $readTask = $pipe.ReadAsync($buf, 0, $buf.Length)

  while ([DateTime]::UtcNow -lt $deadline -and -not $seenDone) {
    try {
      if (-not $readTask.Wait(1000)) { continue }
      $n = $readTask.Result
      if ($n -le 0) { break }
      $chunk = [System.Text.Encoding]::ASCII.GetString($buf, 0, $n)
      $pending += $chunk
      Add-Content -Path $logPath -Value $chunk -NoNewline
      if ($pending -match 'HYPERV_DRILL_DONE') {
        $seenDone = $true
      }
      else {
        $readTask = $pipe.ReadAsync($buf, 0, $buf.Length)
      }
    }
    catch {
      # A faulted read is the pipe closing under us. Record it and stop;
      # rethrowing here would lose everything already written to the log.
      Add-Content -Path $logPath -Value ("`nDRILL-VM console_read=error " + $_.Exception.Message)
      break
    }
  }

  try { $pipe.Dispose() } catch {}
  return $(if ($seenDone) { 'DONE' } else { 'TIMEOUT' })
}

$readerJob = $null
$readerStatus = 'NOT_STARTED'
try {
  if (Get-Command Start-ThreadJob -ErrorAction SilentlyContinue) {
    $readerJob = Start-ThreadJob -ScriptBlock $readerScript -ArgumentList $pipeName, $consoleLog, $TimeoutSec
    Note reader_job threadjob
  }
  else {
    $readerJob = Start-Job -ScriptBlock $readerScript -ArgumentList $pipeName, $consoleLog, $TimeoutSec
    Note reader_job job
  }
}
catch {
  Note reader_job ("failed: " + $_.Exception.Message)
}

# --- boot --------------------------------------------------------------------
$booted = $false
try {
  Note vm_start begin
  Start-VM -Name $VmName -ErrorAction Stop
  $booted = $true
  Note vm_start ok
  Start-Sleep -Seconds 3
  Note vm_state (Get-VM -Name $VmName).State
}
catch {
  Note vm_start ("failed: " + $_.Exception.Message)
}

# --- wait for the guest ------------------------------------------------------
if ($readerJob) {
  try {
    $finished = Wait-Job -Job $readerJob -Timeout ($TimeoutSec + 30)
    if ($finished) {
      $readerStatus = Receive-Job -Job $readerJob
    }
    else {
      $readerStatus = 'HOST_TIMEOUT'
      Stop-Job -Job $readerJob -ErrorAction SilentlyContinue
    }
    Note reader_status $readerStatus
  }
  catch {
    Note reader_status ("failed: " + $_.Exception.Message)
    $readerStatus = 'READER_ERROR'
  }
  finally {
    Remove-Job -Job $readerJob -Force -ErrorAction SilentlyContinue
  }
}

# --- console + verdict -------------------------------------------------------
$console = ''
if (Test-Path $consoleLog) { $console = Get-Content $consoleLog -Raw }
if (-not $console) { $console = '' }
$console = $console -replace "`0", ''

Write-Host '======== GUEST CONSOLE ========'
Write-Host $console
Write-Host '==============================='

$resultLine = ($console -split "`n" | Where-Object { $_ -match 'HYPERV_DRILL_RESULT status=' } | Select-Object -Last 1)
$verdict = 'NO_RESULT'
if ($resultLine -match 'HYPERV_DRILL_RESULT status=(\S+)') { $verdict = $Matches[1] }
Note drill_result $verdict
Note console_bytes $console.Length

# --- teardown ----------------------------------------------------------------
try {
  if (Get-VM -Name $VmName -ErrorAction SilentlyContinue) {
    if ($booted) {
      Stop-VM -Name $VmName -TurnOff -Force -ErrorAction SilentlyContinue
    }
    Remove-VM -Name $VmName -Force -ErrorAction SilentlyContinue
    Note vm_remove ok
  }
  else {
    Note vm_remove nothing_to_remove
  }
}
catch {
  Note vm_remove ("failed: " + $_.Exception.Message)
}

try {
  if (Test-Path $vhdPath) {
    try { Dismount-VHD -Path $vhdPath -Force -ErrorAction SilentlyContinue } catch {}
    Remove-Item $vhdPath -Force -ErrorAction SilentlyContinue
    Note vhd_remove ok
  }
}
catch {
  Note vhd_remove ("failed: " + $_.Exception.Message)
}

# Leave the switch only if this run created it. A pre-existing Default Switch
# is infrastructure and must survive.
try {
  $sw = Get-VMSwitch -Name 'vmbus-drill-private' -ErrorAction SilentlyContinue
  if ($sw) {
    Remove-VMSwitch -Name 'vmbus-drill-private' -Force -ErrorAction SilentlyContinue
    Note drill_switch_remove ok
  }
}
catch {
  Note drill_switch_remove ("failed: " + $_.Exception.Message)
}

# --- exit --------------------------------------------------------------------
Write-Host '---- drill-vm report ----'
Get-Content $script:report | Write-Host

# PASS and PARTIAL both mean the measurement ran. PARTIAL is green for the job
# and PARTIAL for the gate -- it is the honest "drill executed, gate not closed"
# outcome and must not be confused with a failure to measure. FAIL and
# NO_RESULT redden: either the candidate was damaged or we never got a verdict.
if ($verdict -eq 'PASS' -or $verdict -eq 'PARTIAL') {
  Note exit_code 0
  Note verdict $verdict
  exit 0
}
Note exit_code 1
Note verdict $verdict
exit 1
