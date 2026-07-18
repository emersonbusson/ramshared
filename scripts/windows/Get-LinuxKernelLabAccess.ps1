#Requires -Version 5.1
<#
.SYNOPSIS
  Discover and optionally smoke-test SSH access to linux-kernel-lab.

.DESCRIPTION
  Hyper-V may leave Get-VMNetworkAdapter.IPAddresses empty for the Ubuntu
  cloud image even when the guest has DHCP. This helper falls back to the
  Windows ARP/neighbor table by the VM MAC address.

  No password is stored or printed. The lab is expected to use SSH keys and
  passwordless sudo for the local-only user documented in ACCESS.txt.
#>
[CmdletBinding()]
param(
    [string]$VMName = "linux-kernel-lab",
    [string]$User = "emedev",
    [switch]$Start,
    [switch]$Smoke
)

$ErrorActionPreference = "Stop"

if ($Start) {
    $vm = Get-VM -Name $VMName -ErrorAction Stop
    if ($vm.State -ne "Running") {
        Start-VM -Name $VMName
        Start-Sleep -Seconds 20
    }
}

$adapter = Get-VMNetworkAdapter -VMName $VMName -ErrorAction Stop
$mac = ($adapter.MacAddress -replace "(.{2})(?=.)", '$1-').ToUpperInvariant()
$directIps = @($adapter.IPAddresses | Where-Object { $_ -match "^(\d+\.){3}\d+$" })

$neighborIps = @(
    Get-NetNeighbor -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object { $_.LinkLayerAddress -and $_.LinkLayerAddress.ToUpperInvariant() -eq $mac } |
        Select-Object -ExpandProperty IPAddress -Unique
)

$ips = @($directIps + $neighborIps | Where-Object { $_ } | Select-Object -Unique)
$smokeResults = @()
if ($Smoke) {
    foreach ($ip in $ips) {
        $output = & ssh.exe -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=no `
            "$User@$ip" "hostname; uname -r; cloud-init status --wait; sudo -n true && echo SUDO_OK" 2>&1
        $smokeResults += [pscustomobject]@{
            ip = $ip
            exit_code = $LASTEXITCODE
            output = ($output -join "`n")
        }
    }
}

[pscustomobject]@{
    vm = $VMName
    state = (Get-VM -Name $VMName).State.ToString()
    user = $User
    mac = $mac
    direct_ips = $directIps
    neighbor_ips = $neighborIps
    ips = $ips
    smoke = $smokeResults
} | ConvertTo-Json -Depth 6
