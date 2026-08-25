# SPEC — Orphan Detection and Fail-Closed Lifecycle

This revision invalidates the legacy auto-recover plan triggered by `used_kb == 0`.
Enumeration is detection-only; ownership derives exclusively from sealed and fresh bindings.

## Technical Decisions

| ID | Decision | Rationale |
| --- | --- | --- |
| DT-R1 | `parse_proc_swaps` returns `Result`, requires a complete schema, and rejects duplicates. | Uncertainty never implies absence. |
| DT-R2 | `OrphanPlan::DetectedUnboundZeroUsed` returns a refusal without side effects. | Zero usage is still an active swap. |
| DT-R3 | `LifecycleBinding` schema 1 contains boot, daemon PID+start, InvocationID, socket, origin, and exact devices; requires 1 NBD, 0 ublk, and at most 1 zram. | Device name or shape does not prove ownership. |
| DT-R4 | Enumeration of `/sys/class/block` produces observations only. Any foreign/duplicate/mismatch blocks the entire operation. | Do not touch third-party devices. |
| DT-R5 | Each executor re-reads swaps, enumerates, authorizes, and revalidates the device immediately before action. Reset/disconnect and delete require strict fresh proof of absence. | Closes TOCTOU between planning and mutation. |
| DT-R6 | `swapoff` failure is treated as absence only when a fresh strict read proves absence; otherwise returns `UnsafeContainment`. | Command output is ambiguous. |
| DT-R7 | Standalone ublk has no WSL2 override; on isolated Linux, TERM/INT calls swapoff-first and only then STOP/DELETE. | Prevents dead backend under active swap. |
| DT-R8 | The live set must match the expected set exactly at each stage: full binding before swapoff/reset, NBD-only after reset, and empty after disconnect. Missing bound device also blocks. | Closes the gap where partial cardinality could skip detach and still stop the daemon. |
| DT-R9 | zram is created solely via `zramctl --find`; identity is sealed and revalidated before `mkswap`. Rollback without exact record and sysfs fallback on `zram0` are refused. | Prevents formatting or resetting foreign or retargeted devices. |

## Mandatory Order

1. Strict snapshot;
2. Sealed binding and cardinality check;
3. Daemon / InvocationID / socket / origin / records validation;
4. Detection-only enumeration and exact equality;
5. Swapoff of all active devices;
6. Fresh proof of absence per device;
7. Reset zram;
8. Disconnect NBD;
9. Proof of device disappearance;
10. Stop daemon;
11. Remove runtime records.

The first error aborts the sequence and preserves the recoverable state.

## Required Tests

- `down_refuses_foreign_live_device_without_running_a_command`
- `down_refuses_missing_bound_live_device_without_running_a_command`
- `down_refuses_ambiguous_live_identity_without_running_a_command`
- `down_refuses_foreign_managed_swap_without_running_a_command`
- `down_refuses_mismatched_runtime_record_without_running_a_command`
- `down_refuses_unreadable_or_malformed_swap_snapshot_before_mutation`
- `active_zero_use_swapoff_failure_preserves_backend_and_evidence`
- `uncertain_swapoff_absence_proof_preserves_backend_and_evidence`
- `lifecycle_binding_rejects_ambiguous_device_cardinality`
- `swapoff_completes_before_nbd_disconnect`
- `zram_rollback_requires_recorded_identity_before_any_command`
- `zram_setup_never_mutates_unbound_sysfs_fallback`
- `zram_reset_stage_mismatch_stops_before_nbd_disconnect`
- `nbd_startup_disconnect_postcheck_preserves_daemon_and_evidence`
- standalone ublk: active-zero, used swap, parser failure, and `swapoff` failure
  preserve STOP/DELETE/backend.

All tests are unit and hermetic. No test executes swapoff, NBD, ublk, zram, or real devices.

## Rollback Trigger

Any command issued to a foreign/ambiguous device, any reset/disconnect/delete without a fresh snapshot, or any removal of evidence after an uncertain outcome keeps activation blocked and requires re-audit.
