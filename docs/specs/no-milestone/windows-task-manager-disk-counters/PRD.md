---
slug: windows-task-manager-disk-counters
title: Windows virtual disk counter audit
milestone: —
issues: []
---

# PRD - Windows virtual disk counter audit

## Summary

RamShared must provide reliable Windows disk activity evidence for the
`RAMSHARE VRAMDISK` LUN. Windows Task Manager can misrepresent virtual StorPort
devices, so product evidence must use locale-safe PerfDisk counters plus direct
checksum I/O. Task Manager visual parity with a physical SSD is explicitly out
of scope for product correctness.

## Technical Context

- Confirmed in codebase: `scripts/windows/Measure-RamSharedDiskIo.ps1` samples
  `Win32_PerfFormattedData_PerfDisk_PhysicalDisk` and performs checksum I/O.
- Confirmed in codebase: `scripts/windows/Run-HostExhaustive.ps1` formats only a
  disk whose identity is exactly `RAMSHARE VRAMDISK`, non-boot, non-system, and
  matching the requested size.
- Confirmed in docs: `docs/reliability/GAP-REGISTER.md` records the supported
  disk-counter evidence gate as closed when CIM/direct-I/O audit passes.
- Inference: Task Manager UI screenshots are useful operator evidence, but they are
  not stable enough to be the only acceptance surface.

## Recommended Option

Create a Windows audit harness that runs plan-only by default and, when explicitly
approved, delegates live disk creation to `Run-HostExhaustive.ps1`. The audit parses
the generated artifact and requires `DISK_IO_MEASURE_OK`, direct load during sampling,
checksum match, and at least one non-zero PerfDisk activity signal.

Discarded alternatives:

- Directly format from the audit script. Rejected: this would duplicate disk-safety
  gates and increase risk to physical disks.
- Treat Task Manager screenshots as the gate. Rejected: screenshots are manual,
  locale/UI dependent, and cannot safely close low-level correctness.

## Requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Produce a reproducible Windows disk-counter audit artifact. | `Invoke-WindowsDiskCounterAudit.ps1 -Run -ApprovePhysicalHost` emits `audit-summary.json`. |
| RF-2 | Use only existing exact-identity formatting gates. | Audit script contains no `Initialize-Disk` or `Format-Volume`; live creation is delegated to `Run-HostExhaustive.ps1`. |
| RF-3 | Prove activity with machine-readable counters and checksum I/O. | Audit requires `DISK_IO_MEASURE_OK=true`, `Direct load during sampling`, `match=True`, and non-zero busy/write/queue evidence. |
| NFR-1 | Be safe by default. | Default mode is plan-only; live mode requires `-Run -ApprovePhysicalHost`. |
| NFR-2 | Keep Task Manager claim honest. | Docs state CIM/direct metrics are authoritative; UI parity is not a correctness requirement. |

## Flow

1. Plan-only: collect expected stages and write `audit-plan.json`.
2. Live: run storage-only preflight.
3. Live: invoke `Run-HostExhaustive.ps1` with explicit `SizeBytes`.
4. Live: parse the generated `summary.json` and `disk-io.out`.
5. Live: emit pass/fail `audit-summary.json` and leave the LUN torn down.

## Risks

- Physical disk formatting risk is contained by delegating to the existing host
  exhaustive harness.
- PerfDisk may lag or report partial fields. The audit accepts non-zero activity
  from busy, write, or queue plus direct checksum I/O.
- Task Manager UI may still disagree; this is expected and not a correctness gate.

Rollback trigger: revert if the audit can pass without direct checksum match, without
`DISK_IO_MEASURE_OK`, or if it performs direct formatting.

## Validation Plan

- Static: `scripts/windows/Test-WindowsDiskCounterAuditStatic.ps1`.
- Live: `scripts/windows/Invoke-WindowsDiskCounterAudit.ps1 -Run -ApprovePhysicalHost`
  on the physical Windows lab host after clean storage-only preflight.
- Docs: `./scripts/docs-check.sh`.
