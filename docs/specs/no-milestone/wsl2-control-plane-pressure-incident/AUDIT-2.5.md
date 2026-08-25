# AUDIT-2.5 — WSL2 control-plane pressure containment

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Critical | DT-11 | A guest-local safe-mode file cannot be created while the guest is inaccessible and can lose the boot race. | Host gate is now durable before terminate; heavy units require an ephemeral boot-bound resume lease. |
| High | DT-2 | Directory lock and PID-only ownership could deadlock after crash or accept PID reuse. | Atomic hard-link ownership includes boot ID, PID start time, and nonce; quarantine/release require exact identity. |
| High | DT-8 | Origin/cache failure precedence was not exact. | Exact worst-state precedence and named failure test are required. |
| Medium | validation | Coverage and forbidden host action gates were vague. | Exact coverage command and explicit no-shutdown/no-restart/sealed-distro tests are named. |
| Critical | DT-14 | A service stop could kill the NBD backend before active swap was proved off. | Split controller/backend ownership; no finite kill escalation; failed proof leaves backend alive with host-recoverable evidence. |
| High | DT-13 | Victim actions used a reusable unit name even though start acknowledgement already observed an InvocationID. | Persist and revalidate exact unit+InvocationID before every freeze/thaw/TERM/KILL. |

## Open questions

The VM-only proof that recovered boot starts no heavy unit remains
environment-bound and prevents DONE, but does not block source implementation.

## Verdict

**go** for source ITEM-1 through ITEM-9. The verdict becomes **no-go** if the
host safe-mode gate is written after terminate, a boot can start a heavy unit
without the resume lease, owner identity omits boot/start time, a stale
InvocationID can receive any action, backend stop can escalate before proved
swapoff, or a terminate target is not the exact sealed distro after all four
proof gates.
