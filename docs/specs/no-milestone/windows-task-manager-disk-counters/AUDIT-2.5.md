# AUDIT-2.5 — windows-task-manager-disk-counters

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| H1 | DT-4 / ITEM-5 | Changing the reported bus directly on the physical host could remove the LUN PDO or alter class-driver behavior. | VM-first WDK, property and Driver Verifier gate before any physical deployment. |
| H1 | DT-7/8 | A 2 GiB physical allocation can starve a 6 GiB desktop GPU when other workloads are present. | Require live free VRAM at least requested size plus reserve; refuse rather than pressure through the floor. |
| H1 | DT-6 | The existing PowerShell measurement path can log a terminating-quality conversion error and still exit zero. | Manufactured false-green test plus top-level non-zero failure behavior before using new benchmark numbers. |
| H1 | DT-8 | Reconfiguring size/sector requires repeated storage destruction and formatting. | Consumer-first stop, exact identity, pagefile/foreign-volume refusals, watchdog and zero-residue gate between every cell. |
| H2 | DT-5 | Task Manager may still display a rounded capacity or stale formatted value after the driver is correct. | Treat storage APIs as canonical, refresh Task Manager, retain the screenshot as secondary evidence and document disagreement. |
| H2 | DT-9 | With three samples, p99 can be presented with false precision. | Define p99 as the maximum observed sample and record deviation/range. |

All required fixes are present in the active SPEC.

## Open questions

None blocking. The final active size is selected only from cells that pass the
same physical matrix; automatic start and pagefile use remain out of scope.

## Verdict

go
