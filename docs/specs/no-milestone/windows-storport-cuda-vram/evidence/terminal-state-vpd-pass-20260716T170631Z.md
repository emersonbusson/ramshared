# Terminal state after exact-VPD closeout — 2026-07-16

Read-only host capture at `2026-07-16T14:06:31.9538922-03:00`, after campaign
`guest-exhaustive-20260716-120459`. The command queried Hyper-V, GPU-PV, DDA, Windows PnP, and
`nvidia-smi`; it did not start the VM, modify an adapter, install a driver, or run product Online.

```powershell
Get-VM -Name "win11-drill"
Get-VMGpuPartitionAdapter -VMName "win11-drill"
Get-VMHostAssignableDevice
Get-PnpDevice -Class Display | Where-Object FriendlyName -Match "RTX 2060"
nvidia-smi.exe --query-gpu=name,pci.bus_id,driver_version --format=csv,noheader
```

```text
VM.Name=win11-drill
VM.State=Off
GpuPartitionCount=1
GpuPartition[0].InstancePath=[]
GpuPartition[0].MinPartitionVRAM=[]
GpuPartition[0].MaxPartitionVRAM=[]
GpuPartition[0].OptimalPartitionVRAM=[]
DdaCount=0
HostDisplay.Status=OK
HostDisplay.FriendlyName=NVIDIA GeForce RTX 2060
NvidiaSmi=NVIDIA GeForce RTX 2060, 00000000:06:00.0, 610.74
```

This closes only the terminal-state evidence gap. It does not change the product status: physical
host BINARY_MATCH/Online, real guest CUDA over GPU-PV, and the isolated WSL2 hang campaign remain
open.
