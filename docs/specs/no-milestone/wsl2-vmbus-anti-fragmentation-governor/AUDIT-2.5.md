# AUDIT-2.5 — wsl2-vmbus-anti-fragmentation-governor

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Low | §3 DT-2 | Threshold of 8 chunks might be sensitive if memory is pre-fragmented at start of test. | Verify order-7 chunks before initiating ramp; if $<8$ at start, log compaction warning before halting. |
| Low | §7 | Parsing `/proc/buddyinfo` on non-WSL2 environments might fail if format differs. | Gate buddyinfo order-7 check behind `is_wsl2()`. |

## Open questions

- None. The root cause is empirically proven via the kernel log and buddy allocator counters.

## Verdict

**go**
