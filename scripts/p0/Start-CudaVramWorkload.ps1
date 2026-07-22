#Requires -Version 5.1
<#
.SYNOPSIS
  Generic CUDA VRAM pressure workload for validation campaigns.

.DESCRIPTION
  Allocates a bounded amount of VRAM through nvcuda.dll, touches it, holds it
  for a fixed duration, and releases it. This is a synthetic external GPU
  workload used to prove aggregate WDDM/CUDA VRAM pressure without naming one
  application as architecture.
#>
[CmdletBinding()]
param(
    [ValidateRange(1, 16384)][int]$MiB = 1024,
    [ValidateRange(1, 3600)][int]$HoldSec = 30,
    [ValidateRange(0, 32)][int]$Device = 0
)

$ErrorActionPreference = "Stop"

if (-not ("RamSharedCudaVramWorkload" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class RamSharedCudaVramWorkload {
  [DllImport("nvcuda.dll", CallingConvention = CallingConvention.Cdecl)]
  static extern int cuInit(uint flags);

  [DllImport("nvcuda.dll", CallingConvention = CallingConvention.Cdecl)]
  static extern int cuDeviceGet(out int device, int ordinal);

  [DllImport("nvcuda.dll", CallingConvention = CallingConvention.Cdecl)]
  static extern int cuCtxCreate_v2(out IntPtr pctx, uint flags, int dev);

  [DllImport("nvcuda.dll", CallingConvention = CallingConvention.Cdecl)]
  static extern int cuCtxDestroy_v2(IntPtr ctx);

  [DllImport("nvcuda.dll", CallingConvention = CallingConvention.Cdecl)]
  static extern int cuMemAlloc_v2(out ulong dptr, UIntPtr bytesize);

  [DllImport("nvcuda.dll", CallingConvention = CallingConvention.Cdecl)]
  static extern int cuMemFree_v2(ulong dptr);

  [DllImport("nvcuda.dll", CallingConvention = CallingConvention.Cdecl)]
  static extern int cuMemsetD8_v2(ulong dstDevice, byte uc, UIntPtr N);

  static void Check(string op, int rc) {
    if (rc != 0) throw new Exception(op + " failed cuda_rc=" + rc);
  }

  public static void Run(int ordinal, ulong bytes, int holdSec) {
    IntPtr ctx = IntPtr.Zero;
    ulong ptr = 0;
    try {
      Check("cuInit", cuInit(0));
      int dev;
      Check("cuDeviceGet", cuDeviceGet(out dev, ordinal));
      Check("cuCtxCreate", cuCtxCreate_v2(out ctx, 0, dev));
      Check("cuMemAlloc", cuMemAlloc_v2(out ptr, new UIntPtr(bytes)));
      Check("cuMemsetD8", cuMemsetD8_v2(ptr, 0xA5, new UIntPtr(bytes)));
      System.Threading.Thread.Sleep(holdSec * 1000);
    } finally {
      if (ptr != 0) cuMemFree_v2(ptr);
      if (ctx != IntPtr.Zero) cuCtxDestroy_v2(ctx);
    }
  }
}
'@
}

$bytes = [uint64]$MiB * 1024 * 1024
Write-Host ("[cuda-vram-workload] allocate_mib={0} hold_sec={1} device={2}" -f $MiB, $HoldSec, $Device)
[RamSharedCudaVramWorkload]::Run($Device, $bytes, $HoldSec)
Write-Host "[cuda-vram-workload] released"
