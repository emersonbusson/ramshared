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
    [ValidateRange(0, 32)][int]$Device = 0,
    [string]$ReadyFile = "",
    [string]$ReleaseFile = "",
    [switch]$HandshakeSelfTest,
    [switch]$CleanupSelfTest,
    [switch]$LiveCampaign
)

$ErrorActionPreference = "Stop"

if (-not ("RamSharedCudaVramWorkload" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
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

  public static void WriteReadyFile(string readyFile) {
    if (String.IsNullOrWhiteSpace(readyFile)) {
      throw new ArgumentException("ready file is required");
    }
    byte[] ready = System.Text.Encoding.UTF8.GetBytes("cuda_allocation_ready\n");
    using (System.IO.FileStream stream = new System.IO.FileStream(
        readyFile, System.IO.FileMode.CreateNew, System.IO.FileAccess.Write,
        System.IO.FileShare.None)) {
      stream.Write(ready, 0, ready.Length);
      stream.Flush();
    }
  }

  static Exception TerminalFailure(
      Exception primaryFailure,
      int memFreeRc,
      Exception memFreeFailure,
      int ctxDestroyRc,
      Exception ctxDestroyFailure) {
    List<string> cleanup = new List<string>();
    if (memFreeFailure != null) {
      cleanup.Add("cuMemFree_v2 exception=" + memFreeFailure.GetType().Name);
    } else if (memFreeRc != 0) {
      cleanup.Add("cuMemFree_v2 rc=" + memFreeRc);
    }
    if (ctxDestroyFailure != null) {
      cleanup.Add("cuCtxDestroy_v2 exception=" + ctxDestroyFailure.GetType().Name);
    } else if (ctxDestroyRc != 0) {
      cleanup.Add("cuCtxDestroy_v2 rc=" + ctxDestroyRc);
    }
    if (cleanup.Count == 0) {
      return primaryFailure;
    }
    string cleanupMessage = "cuda_cleanup_failure=" + String.Join(",", cleanup.ToArray());
    if (primaryFailure != null) {
      return new Exception(primaryFailure.Message + "; " + cleanupMessage, primaryFailure);
    }
    return new Exception(cleanupMessage);
  }

  public static void ThrowManufacturedCleanupFailure() {
    throw TerminalFailure(
        new InvalidOperationException("manufactured_primary_failure"),
        7001,
        null,
        7002,
        null);
  }

  public static void Run(int ordinal, ulong bytes, int holdSec, string readyFile, string releaseFile) {
    IntPtr ctx = IntPtr.Zero;
    ulong ptr = 0;
    Exception primaryFailure = null;
    int memFreeRc = 0;
    int ctxDestroyRc = 0;
    Exception memFreeFailure = null;
    Exception ctxDestroyFailure = null;
    try {
      Check("cuInit", cuInit(0));
      int dev;
      Check("cuDeviceGet", cuDeviceGet(out dev, ordinal));
      Check("cuCtxCreate", cuCtxCreate_v2(out ctx, 0, dev));
      Check("cuMemAlloc", cuMemAlloc_v2(out ptr, new UIntPtr(bytes)));
      Check("cuMemsetD8", cuMemsetD8_v2(ptr, 0xA5, new UIntPtr(bytes)));
      if (!String.IsNullOrWhiteSpace(readyFile)) {
        WriteReadyFile(readyFile);
      }
      if (String.IsNullOrWhiteSpace(releaseFile)) {
        System.Threading.Thread.Sleep(holdSec * 1000);
      } else {
        DateTime deadline = DateTime.UtcNow.AddSeconds(holdSec);
        while (!System.IO.File.Exists(releaseFile)) {
          if (DateTime.UtcNow >= deadline) {
            throw new TimeoutException("CUDA release file deadline elapsed");
          }
          System.Threading.Thread.Sleep(200);
        }
      }
    } catch (Exception failure) {
      primaryFailure = failure;
    } finally {
      if (ptr != 0) {
        try {
          memFreeRc = cuMemFree_v2(ptr);
        } catch (Exception failure) {
          memFreeFailure = failure;
        }
      }
      if (ctx != IntPtr.Zero) {
        try {
          ctxDestroyRc = cuCtxDestroy_v2(ctx);
        } catch (Exception failure) {
          ctxDestroyFailure = failure;
        }
      }
    }
    Exception terminalFailure = TerminalFailure(
        primaryFailure, memFreeRc, memFreeFailure, ctxDestroyRc, ctxDestroyFailure);
    if (terminalFailure != null) throw terminalFailure;
  }
}
'@
}

if ($HandshakeSelfTest -and $CleanupSelfTest) { throw "cuda_self_test_mode_conflict" }

if ($HandshakeSelfTest) {
    if ($LiveCampaign) { throw "handshake_self_test_live_campaign_conflict" }
    [RamSharedCudaVramWorkload]::WriteReadyFile($ReadyFile)
    Write-Host "[cuda-vram-workload] handshake-ready"
    return
}

if ($CleanupSelfTest) {
    if ($LiveCampaign) { throw "cleanup_self_test_live_campaign_conflict" }
    $preserved = $false
    try {
        [RamSharedCudaVramWorkload]::ThrowManufacturedCleanupFailure()
    } catch {
        $messages = @()
        $failure = $_.Exception
        while ($null -ne $failure) {
            $messages += [string]$failure.Message
            $failure = $failure.InnerException
        }
        $allMessages = $messages -join "`n"
        $preserved = $allMessages.Contains("manufactured_primary_failure") -and
            $allMessages.Contains("cuda_cleanup_failure=") -and
            $allMessages.Contains("cuMemFree_v2 rc=7001") -and
            $allMessages.Contains("cuCtxDestroy_v2 rc=7002")
    }
    if (-not $preserved) { throw "cuda_cleanup_self_test_primary_failure_not_preserved" }
    Write-Host "[cuda-vram-workload] cuda_cleanup_self_test_pass"
    return
}

$bytes = [uint64]$MiB * 1024 * 1024
Write-Host ("[cuda-vram-workload] allocate_mib={0} hold_sec={1} device={2}" -f $MiB, $HoldSec, $Device)
[RamSharedCudaVramWorkload]::Run($Device, $bytes, $HoldSec, $ReadyFile, $ReleaseFile)
Write-Host "[cuda-vram-workload] released"
