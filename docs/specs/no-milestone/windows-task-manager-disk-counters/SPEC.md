# SPEC - Windows virtual disk counter audit

## Closed Scope

In now:

- Plan-only and approved live Windows audit harness.
- Parsing existing `Run-HostExhaustive.ps1` artifacts.
- Documentation that Task Manager UI parity remains a separate claim.

Out now:

- Automating Task Manager UI screenshots.
- Changing StorPort driver counter behavior.
- Formatting any disk directly from the audit harness.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-1, ITEM-3 |
| RF-2 | ITEM-1, ITEM-2 |
| RF-3 | ITEM-3 |
| NFR-1 | ITEM-1 |
| NFR-2 | ITEM-4 |

## Technical Decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | The audit script must be plan-only unless `-Run -ApprovePhysicalHost` is supplied. | Prevent accidental live host mutation. |
| DT-2 | Live disk lifecycle is delegated to `Run-HostExhaustive.ps1`. | Reuse exact RAMSHARE identity gates and avoid duplicate formatting logic. |
| DT-3 | The pass gate is CIM/direct-I/O evidence, not Task Manager UI. | Task Manager is a human UI and can misreport virtual StorPort disks. |

## Atomicity And Rollback

- Atomicity frontier: Windows host script and generated artifact only.
- Driver/kernel: unchanged.
- Host/persistent: only the delegated exhaustive harness may create and format the
  exact `RAMSHARE VRAMDISK` LUN; teardown remains its responsibility.
- Forward-only: none.

## Kahneman Map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-1 live approval | #13 | Can the script mutate the host without explicit approval? | Static test checks `-Run` and `-ApprovePhysicalHost`. | Any direct format or missing approval gate. |
| ITEM-3 pass parse | #9 | Are the counters numeric and tied to checksum I/O? | Live artifact with `DISK_IO_MEASURE_OK`, direct load, `match=True`, non-zero signal. | All counters zero or checksum absent. |

## Security Checklist

- [x] Privilege: live mode requires explicit physical-host approval.
- [x] User/host copy: N/A — no untrusted buffers.
- [x] Flags/IOCTL codes: N/A — no IOCTLs in this script.
- [x] Info-leak: artifacts contain disk metrics and hashes only.
- [x] IRQ/atomic or IRQL: N/A — userspace PowerShell.
- [x] Lifetime: delegated harness owns create/destroy.
- [x] Hot-unplug / device-gone: delegated harness post-checks LUN/Win32/PnP gone.
- [x] Host safety: plan-only default; live path uses storage-only preflight.
- [x] Replayable ops: bounded run with unique artifact directory.

## Files To Create / Modify

**CREATE — `scripts/windows/Invoke-WindowsDiskCounterAudit.ps1`**

- Purpose: plan/live audit for Windows virtual disk activity counters.
- RF / DT: RF-1, RF-2, RF-3, NFR-1, DT-1, DT-2, DT-3.
- Required tests: `scripts/windows/Test-WindowsDiskCounterAuditStatic.ps1`.
- Cover target: N/A — Windows host E2E harness.

**CREATE — `scripts/windows/Test-WindowsDiskCounterAuditStatic.ps1`**

- Purpose: static guard for approval, delegation, pass tokens, and no direct
  formatting.
- RF / DT: RF-1, RF-2, RF-3.
- Required tests: itself.
- Cover target: N/A — static harness.

**MODIFY — `docs/reliability/GAP-REGISTER.md`**

- Record the harness as partial evidence; do not close Task Manager UI parity.

## Observability

| Signal | Where | Type |
| --- | --- | --- |
| `audit-summary.json` | audit artifact directory | pass/fail JSON |
| `disk-io.out` | delegated exhaustive artifact | PerfDisk/direct-I/O text |

## Implementation Order

1. ITEM-1: add plan-only/live harness.
2. ITEM-2: add static safety test.
3. ITEM-3: run static tests and docs checks.
4. ITEM-4: update gap register after live evidence exists.

