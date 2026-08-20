# Incident — WSL2 control-plane timeout under concurrent workload

Date: 2026-08-20
Status: containment complete; causal reproduction open

## Impact

The WSL2 VM stopped responding twice during one development session. Terminals,
containers, builds, and the interactive development session were interrupted.
Windows reported `HCS_E_CONNECTION_TIMEOUT`, `0x8007274c`, and `0x8007000e`.

## Evidence

- First boot: final healthy sample at 03:45:36, zram 1,041,436/1,048,572 KiB,
  VRAM 0, disk swap 0; journald reported memory pressure at 03:46:14.
- Second boot: zram rose from 394,712 to 854,320 KiB in 61 seconds; VRAM and
  disk remained 0. The collector then stopped for 6m32s before VM termination.
- The second boot recorded many long-running udev workers for VMBus
  `hv_sock` devices and Docker health-check timeouts.
- No Linux OOM-killer event, NBD error, or completed terminal teardown was
  found.
- The incident ran on WSL 2.7.11. WSL 2.7.12 was installed only after the
  second crash.

## Causal assessment

High confidence: concurrent unmanaged workloads exceeded the protection model,
the static swap priority did not exercise VRAM in time, and the topology-only
health result was misleading.

Moderate confidence: WSL Relay/VMBus degradation amplified the pressure. The
signature is consistent with microsoft/WSL#40795 and #41242, and the timeout
change in #41252 is absent from the inspected 2.7.12 tag.

Not established: direct causation by NBD, the custom kernel, the recurring dxg
warning, or one specific process. The upstream report must preserve this
uncertainty.

## Containment

- Deactivated NBD and zram through the sealed swapoff-first lifecycle while
  both had zero use.
- Disabled `ramshared-cascade.service` and the checkout-based
  `ramshared-beta-health.service`.
- Retained disk swap only.
- Heavy concurrent workloads remain paused until managed-scope and watchdog
  gates pass.
