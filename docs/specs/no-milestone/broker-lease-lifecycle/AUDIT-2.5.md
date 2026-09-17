# AUDIT-2.5 — broker-lease-lifecycle

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| HIGH | DT-3 | Immediate active-lease release on disconnect can reuse capacity before a transiently disconnected native consumer has renewed or unwound. | Retain only active lease state until its monotonic deadline. |
| HIGH | DT-4 | An expiry helper not called by the real tick has no lifecycle effect. | Expire at the beginning of `BrokerCore::on_tick()`. |
| HIGH | Atomicity | A timer thread would race `SliceMap` state transitions and external I/O dispatch. | Keep expiry in the existing single-thread core. |
| MED | DT-2 | A new renew frame would require protocol versioning and every consumer change. | Renew through existing holder PSI heartbeat. |
| MED | RF-1 | A zero or implicit TTL would leak or immediately revoke capacity. | Carry an explicit non-zero duration and refuse invalid grant input before leasing slices. |
| LOW | Observability | Logs can accidentally expose peer/process information. | Emit only broker IDs and lease IDs. |

## Open questions

1. Whether future Windows/DCC consumers maintain the PSI heartbeat cadence while
   rendering must be proven in the isolated lifecycle drill.
2. Whether the documented 30-second TTL needs recalibration for a remote
   tenant must be decided from that drill, not from shared-host behavior.

## Verdict

### go — source and in-process validation

The design uses one existing state owner and one existing heartbeat, introduces
no protocol compatibility path, and has explicit pre-deadline and post-deadline
tests. It is a **no-go** for product promotion until the named isolated
broker/agent before→action→after drill proves reconnect, expiry, and post-expiry
allocation without NBD swap activation or shared-host pressure.
