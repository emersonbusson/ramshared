---
slug: windows-task-manager-disk-counters
title: Windows virtual disk identity, counters, and performance matrix
milestone: —
issues: []
---

# PRD - Windows virtual disk identity, counters, and performance matrix

## Summary

RamShared must expose an honest Windows storage identity and provide reliable
activity, capacity, integrity, latency, and throughput evidence for the
`RAMSHARE VRAMDISK` LUN. The operator surface must distinguish actual device
properties from Task Manager rounding or cache behavior. Acceptance uses
locale-safe Windows storage/CIM APIs plus direct checksum I/O; Task Manager
screenshots remain secondary human evidence.

## Technical Context

- `Measure-RamSharedDiskIo.ps1` samples
  `Win32_PerfFormattedData_PerfDisk_PhysicalDisk` and performs direct checksum
  I/O.
- Existing host lifecycle scripts format only an exact, non-boot, non-system
  `RAMSHARE VRAMDISK` of the configured size.
- On the physical host on 2026-07-25, `MSFT_Disk` reported the active 64 MiB
  LUN as exactly 67,108,864 bytes with 4 KiB sectors; `MSFT_Volume` reported
  healthy NTFS; `MSFT_PhysicalDisk` reported SSD/non-rotating. Task Manager
  displayed a 1.0 GB capacity floor, `0 MB` formatted, and `HDD (SAS)`.
- The miniport already reports VPD B1 rotation rate `0x0001`, but does not
  override StorPort's default bus property.
- The live checksum harness produced a PowerShell 5.1 locale conversion error
  and still exited zero. Measurement errors therefore do not currently fail
  closed.

## Recommended Option

Extend the audit into a VM-first, physical-supervised matrix. Report the
virtual bus honestly as `BusTypeVirtual`, retain SCSI direct-access semantics
and non-rotating media, and verify exact values through Windows storage APIs.
Exercise 64 MiB, 256 MiB, 1 GiB and 2 GiB configurations plus 512-byte/4 KiB
logical sectors and queue-depth/workload profiles. Every benchmark cell has at
least three runs, fixed parameters, direct integrity, context capture, and
idle/loaded tags where safe.

Rejected alternatives:

- Pretend the virtual miniport is NVMe, SATA, or physical SAS. Those are false
  transport claims; Windows has a `BusTypeVirtual` identity.
- Treat Task Manager screenshots as the sole gate. They are manual,
  locale/UI-dependent, cacheable, and rounded for small devices.
- Format directly from the audit script. Product lifecycle scripts remain the
  only formatting authority.

## Requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Produce reproducible disk property/counter artifacts. | Each run emits raw properties, samples, context, commands and verdict. |
| RF-2 | Reuse exact-identity lifecycle safety. | Matrix contains no broad disk selection and delegates create/format/teardown to product-safe routines. |
| RF-3 | Prove activity with counters and checksum I/O. | Direct read/write checksum matches and at least one PerfDisk activity signal is non-zero. |
| RF-4 | Report an honest virtual/non-rotating identity. | Windows reports `BusType=Virtual`, exact vendor/product/serial, `MediaType=SSD`, `SpindleSpeed=0`, and exact size/sectors. |
| RF-5 | Exercise supported sizes and I/O qualities. | Matrix covers 64 MiB, 256 MiB, 1 GiB, 2 GiB; 512/4096-byte sectors; QD 1/4/16; sequential, random 4 KiB, mixed, flush and integrity. |
| RF-6 | Detect hangs and regressions. | Cells are bounded; ≥3 runs report median/p99/deviation; stability requires one identity/lease, forward-moving counters, matching hashes and clean teardown. |
| RF-7 | Fail closed on measurement errors. | Locale/parser/tool errors produce non-zero exit and cannot emit PASS. |
| NFR-1 | Be safe by default. | Default is plan-only; live physical mode requires `-Run -ApprovePhysicalHost`. |
| NFR-2 | Keep UI claims honest and useful. | Docs explain UI rounding/cache, use APIs as canonical, and collect a refreshed 1 GiB screenshot as secondary evidence. |
| NFR-3 | Preserve the daily host. | VM + Verifier precede physical deployment; physical load is bounded/watchdog-supervised, pagefile-free on the product volume, and never touches a foreign disk. |

## Flow

1. Plan-only: emit the exact matrix and safety requirements.
2. Baseline: capture active manifest, services, GPU/RAM, disks, volumes,
   pagefiles, storage properties and counters.
3. VM: WDK build/install, identity/property checks, Driver Verifier, I/O,
   refusal and teardown drills.
4. Physical: preflight GPU/storage/pagefile state, install one immutable
   configuration per cell, run ≥3 fixed workload repetitions, and perform
   supported teardown between cells.
5. Aggregate median/p99/deviation and regression thresholds. Only an accepted
   demand-start configuration may be left active.

## Risks

- A bus-property change can break enumeration. Any missing PDO, changed
  identity, or Verifier finding blocks physical deployment.
- Larger VRAM allocations can starve the desktop GPU. Unsafe cells are refused
  when current free VRAM is below size plus reserve.
- Task Manager may still round or cache values. API truth and direct I/O remain
  authoritative; UI disagreement stays visible in evidence.

Rollback trigger: revert if an audit passes after any tool/parser error,
without checksum match, with size/sector mismatch, with a non-virtual or
rotating identity, with a Driver Verifier finding, after a foreign-volume
mutation, or when supported stop exceeds 45 seconds.

## Validation Plan

- Static PowerShell contract tests.
- WDK `/W4 /WX`, Code Analysis, InfVerif, and disposable-VM Driver Verifier.
- Live VM and approved physical matrix with BINARY_MATCH, before/action/after,
  exact refusals, registered benchmarks and final active-state observation.
- `./scripts/docs-check.sh`.
