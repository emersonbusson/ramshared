# Incident — WSL2 control-plane timeout under concurrent workload

Date: 2026-08-20 through 2026-08-22
Status: host storage trigger identified; independent containment hardening open

## Impact

The WSL2 VM stopped responding three times across the investigation. Terminals,
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
- The third incident ran on WSL 2.7.12. Its last heartbeat showed roughly
  1 GiB zram, 26 MiB VRAM, and no SSD growth, so the guest became inaccessible
  before VRAM capacity was exhausted.
- The dxg `[ cut here ]` warning occurred during boot, hours before the incident
  window. It is retained as `kernel_warning_at_boot`, not `kernel_crash`.
- The Windows System log records NTFS Event ID 137 for volume `I:` at
  2026-08-22 07:06:17 -03:00. Its binary payload contains little-endian
  `0xC000007F`; Windows decodes that NTSTATUS as `STATUS_DISK_FULL` (the
  operation failed because the disk or backing physical storage was full).
- No `Microsoft-Windows-Resource-Exhaustion-Detector` event was found from
  2026-08-20 through the evidence check. This is negative evidence against a
  recorded Windows resource-exhaustion event, not proof that memory pressure
  never occurred.
- The current VHDX and its parents are neither compressed nor sparse. The SSD
  and NTFS currently report healthy; no present filesystem-corruption signal
  was found.

## Causal assessment

The incident trigger is classified as `host_volume_exhausted`. The dated NTFS
event and `STATUS_DISK_FULL` payload are direct host evidence that the backing
volume could not accept the operation. The earlier working classification of
guest control-plane starvation is superseded for this incident.

This evidence does **not** establish RamShared as the cause of the volume
exhaustion. Concurrent builds, caches, images, VHDX growth, and other writers
were not individually attributed by the event. RamShared lifecycle, identity,
swapoff-first, and pressure hardenings continue as independent safeguards; they
must not be presented as root-cause fixes for this event.

The storage-full event is a confounder for any upstream WSL attribution. This
incident must not be submitted as evidence of microsoft/WSL#40795 unless a new,
storage-sufficient reproduction independently demonstrates that failure class.

Not established: direct causation by NBD, RamShared, the custom kernel, the
recurring dxg warning, memory exhaustion, or one specific process.

## Containment

- Deactivated NBD and zram through the sealed swapoff-first lifecycle while
  both had zero use.
- Disabled `ramshared-cascade.service` and the checkout-based
  `ramshared-beta-health.service`.
- Retained disk swap only.
- Heavy concurrent workloads remain paused until managed-scope and watchdog
  gates pass.
