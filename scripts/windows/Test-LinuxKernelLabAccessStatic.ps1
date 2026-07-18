#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$HarnessPath
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) "Get-LinuxKernelLabAccess.ps1"
}
$text = Get-Content -LiteralPath $HarnessPath -Raw

if ($text -notmatch '\[string\]\$VMName = "linux-kernel-lab"') {
    throw "default_vm: helper must target linux-kernel-lab by default"
}
if ($text -notmatch '\[string\]\$User = "emedev"') {
    throw "default_user: helper must use the documented Linux lab user"
}
if ($text -notmatch 'Get-VMNetworkAdapter -VMName \$VMName') {
    throw "hyperv_adapter: helper must inspect the VM network adapter"
}
if ($text -notmatch '\$adapter\.MacAddress -replace') {
    throw "mac_normalization: helper must normalize the Hyper-V MAC address"
}
if ($text -notmatch 'Get-NetNeighbor -AddressFamily IPv4') {
    throw "arp_fallback: helper must use Windows neighbor table fallback"
}
if ($text -notmatch '\$directIps \+ \$neighborIps') {
    throw "ip_sources: helper must merge direct Hyper-V IPs and ARP fallback IPs"
}
if ($text -notmatch 'ssh\.exe' -or $text -notmatch 'BatchMode=yes' -or $text -notmatch 'ConnectTimeout=10') {
    throw "ssh_smoke: helper must use bounded non-interactive SSH"
}
if ($text -notmatch 'sudo -n true') {
    throw "sudo_smoke: helper must verify passwordless sudo without prompting"
}
if ($text -match 'Password\\s*=' -or $text -match 'ConvertTo-SecureString' -or $text -match 'Get-Content .*drill-pw') {
    throw "no_secret_handling: Linux lab helper must not read or store credentials"
}

Write-Output "PASS Test-LinuxKernelLabAccessStatic"
