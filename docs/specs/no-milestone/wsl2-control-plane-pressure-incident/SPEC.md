# SPEC — WSL2 control-plane pressure containment

## Closed scope

In now: aggregate admission, supervisor policy, schema v4, protected systemd
units/drop-ins, safe-mode recovery, persistent guardian source, sanitized
telemetry, and temporal postmortem classification. Out now: host activation,
Docker restart, live terminate/pressure, VHDX mutation, and publication.

Assumed ready: systemd cgroup v2, `systemd-run --scope`, Docker's supported
systemd cgroup parent, Windows Task Scheduler, WSL 2.7.12, and the existing
sealed installer identity checks.

DT-5 and DT-6 from the initial revision are invalidated. Per-process
`MemAvailable` is not aggregate admission, and a guest-only TERM/KILL watchdog
is not an independent curator.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1, RF-2 | ITEM-1, ITEM-2 |
| RF-3, RF-4 | ITEM-2, ITEM-3 |
| RF-5 | ITEM-4 |
| RF-6, RF-7 | ITEM-5, ITEM-6 |
| RF-8, RF-9, RF-10 | ITEM-7, ITEM-8 |
| RF-11 | ITEM-6 |
| RF-12, RF-15 | ITEM-9 |
| NFR-1 | ITEM-4, ITEM-6 |
| NFR-2, NFR-3 | ITEM-1, ITEM-3 |
| NFR-4 | ITEM-6 |
| NFR-5, NFR-6 | ITEM-7, ITEM-8, ITEM-9 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | `reserve=max(4GiB,ceil(MemTotal/4))`; `max=MemTotal-reserve`; `high=max-max(1GiB,ceil(MemTotal/10))`. | One deterministic aggregate budget. |
| DT-2 | Admission opens the persistent canonical ledger directory component-by-component with `O_NOFOLLOW`, validates directory type/effective owner/safe mode, and acquires a nonblocking exclusive advisory lock on that directory inode for the entire ledger transaction. The transition owner is a separate singly-linked regular record; the quarantine is a separately opened directory. Their `(dev, ino)`, type, owner, mode, and canonical FD-relative names are revalidated before and after every load/store or recovery step. First creation and every file-sync→rename publication sync the containing directory. Recovery copies verified-dead/prior-boot evidence into a `create_new` append-only snapshot and never uses a hard-link/rename lock trick. | One persistent directory inode is the lock authority across child-path replacement, unsafe aliases fail closed, stale evidence is retained, and PID reuse cannot claim ownership. |
| DT-3 | Default class ceilings are interactive 2 GiB, build 6 GiB, browser-test 4 GiB, batch 8 GiB; explicit values are 256 MiB through aggregate max. | Bounded generic classes with override. |
| DT-4 | Discard rank is batch 0, browser-test 1, build 2, interactive 3; lower rank is discarded first, then larger reservation, then the smaller durable `issued_ordinal`. Ledger schema v3 stores a monotonic `next_ordinal`, refuses zero/duplicate/out-of-range ordinals and unsupported older schemas, and assigns the ordinal while holding DT-2's lock. PID, unit spelling, and wall clock never stand in for token age. | Stable deterministic victim selection with a true durable older-token tie-break. |
| DT-5 | `GUARDED`: MemAvailable below reserve or PSI full avg10 ≥2% for 3 samples. `CRITICAL`: MemAvailable below 75% reserve or PSI full avg10 ≥5% for 5 samples. `EMERGENCY`: MemAvailable below 50% reserve, PSI full avg10 ≥10% for 5 samples, or sample delay ≥3s. | Exact preventive thresholds. |
| DT-6 | Return to `HEALTHY` only after 60 consecutive samples with MemAvailable ≥ reserve+1GiB and PSI full avg10 <1%. | Hysteresis prevents churn. |
| DT-7 | `CRITICAL` writes cache target zero, durably records one exact unit+InvocationID as freeze `pending` before the effect, changes it to `applied` only after confirmed success, and records reclaim. Recovery/thaw uses only that durable identity. `EMERGENCY` durably binds one target before TERM, records the successful TERM time only from the execution result, and after 5s may KILL only that same identity; a failed TERM, restart, or changed ledger never advances or retargets escalation. `CloseAdmission` executes the canonical publication once and reports its real lock-busy/result. Supervisor state schema v3 publishes every attempted action in execution order with typed `succeeded`/`failed` status and a bounded, single-line, control-free optional error; `last_action` is absent. | Destructive state derives from confirmed outcomes, exact recovery identity survives restart/write failure, and no earlier failure is concealed. |
| DT-8 | Status schema v4 computes `overall_state` as the worst of topology, pressure/control, origin, cache, guardian, and measurement. Origin `FAILED`, cache `STUCK`, stale mandatory telemetry, and unknown GPU measurement are never green. Exact precedence is `EMERGENCY > SAFE_MODE > BLOCKED > CRITICAL > GUARDED > HEALTHY`. | Prevent false healthy state. |
| DT-9 | Guardian proof order is stale heartbeat ≥15s → two distinct 5s guest probes → independent bounded `wsl --status`/HCS probe → capture close → exactly one targeted terminate. | Independent curator and evidence frontier. |
| DT-10 | Guardian install uses the current user SID, highest run level, restart count, `PT0S` execution limit, and exact XML/config backup. | Persistent and reversible. |
| DT-11 | Before terminate, Windows atomically writes `C:\ProgramData\RamShared\safe-mode\<sealed-distro>.json`. Heavy guest units require ephemeral `/run/ramshared/host-resume-lease.json`, which is absent at boot; only monitor/control start without it. After a new boot ID is proven, guardian writes `/var/lib/ramshared/safe-mode.json`. Resume after 60 healthy samples removes only matching incident gates and issues a boot-ID-bound lease. | A dead guest cannot race safe-mode creation and replay remains idempotent. |
| DT-12 | Postmortem warning correlation requires event time within the incident window; boot-time dxg `[ cut here ]` is `kernel_warning_at_boot`, not causal crash evidence. | Temporal honesty. |
| DT-13 | Reservation ledger schema v3 persists a canonical 32-hex systemd `InvocationID` and durable `issued_ordinal` after start acknowledgement while admission remains serialized. Victim selection carries unit+InvocationID, and immediately before freeze, thaw, TERM, or KILL a bounded `systemctl show` must return the same pair. Mismatch/unavailable identity refuses without action. | A recycled unit name cannot receive a stale destructive action, and lexical PID/unit ordering cannot impersonate age. |
| DT-14 | The systemd unit exposes only a long-lived controller as `MainPID`. `KillMode=process`, `SendSIGKILL=no`, and an infinite stop deadline prevent systemd from signaling the backend child. The controller retains an exclusive lifecycle lock, retries the sealed down path until managed swap absence, NBD detach, and daemon exit are proven, and removes its durable Windows marker only after that terminal proof. A plan-first host recovery command may invoke only the same recovery controller and never terminates WSL. | The swap backend cannot be killed under active swap, including after controller restart or host-side recovery. |
| DT-15 | A temporally matching NTFS Event ID 137 is classified `host_volume_exhausted` only when the same evidence contains `0xC000007F` or `STATUS_DISK_FULL`. The classification records host storage failure without assigning the writer or product cause. Absence of Resource Exhaustion Detector events is retained as negative evidence, not proof of absent memory pressure. | Separates an observed trigger from unsupported RamShared, WSL, kernel, or process attribution. |
| DT-16 | Every trusted, non-daemonizing short-lived guest helper starts as leader of an invocation-private process group. Linux exit is observed without reaping via `waitid(WEXITED|WNOHANG|WNOWAIT)`, keeping the zombie leader's PID/PGID pinned while residual exact-group SIGKILL closes descendant-inherited pipes; finite concurrent captures are then joined before the leader is finally reaped. Timeout, output overflow/disconnect, wait error, and callback panic all retain RAII custody. An injected fatal seam proves that failure to observe/reap after SIGKILL selects controller exit 125 instead of normal control flow. Helpers that deliberately `setsid`, change group, or daemonize are outside this trusted-helper boundary. | No stale trusted helper, descendant-held pipe, capture worker, or unaccounted direct child can outlive a decision deadline, and no negative PGID signal occurs after reap. |

## Atomicity and rollback

- Userspace: persistent canonical inode → nonblocking advisory lock → identity
  revalidation → optional append-only quarantine snapshot → owner rewrite;
  scope spawn failure releases the token.
- Daemon/control: supervisor state and cache target use temp+rename; destructive
  actions are replay-idempotent by exact unit+InvocationID. Controller teardown
  is forward-only after a durable swapoff proof.
- Host/persistent: guardian install backs up prior XML/config before registering
  the task; uninstall restores only its exact backup. Safe mode is host-owned
  and atomically durable before terminate; a guest boot has no resume lease.
- Forward-only: once a real guest is proven inaccessible and termination starts,
  do not issue a second terminate. Preserve evidence on timeout.

## Kahneman map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 admission | #13/#17 | Can two processes exceed one budget or double-release? | `aggregate_reservations_share_one_limit`; `reservation_release_is_idempotent` | Sum exceeds max. |
| ITEM-2 crashed owner | #13/#16/#17 | Can a crashed lock, child-path replacement, unsafe alias, or reused PID leak/release authority? | `verified_dead_stale_lock_is_recovered_exactly_once`; `ledger_authority_refuses_symlink_hardlink_unsafe_mode_and_quarantine_alias`; `ledger_directory_lock_prevents_replacement_owner_split_authority`; `reservation_ledger_refuses_nofollow_violation_without_rewriting_target`; `pid_reuse_cannot_own_or_release_reservation` | Directory authority splits, an unsafe alias is accepted, or a mismatched owner is reaped/released. |
| ITEM-4 supervisor | #16 | Does the curator act before its own progress disappears? | `supervisor_transitions_and_hysteresis_are_exact`; delay fixture | Threshold mismatch. |
| ITEM-4 outcomes | #13/#17 | Can intent, a later success, or an unbounded hostile error conceal an earlier action failure? | `supervisor_publishes_every_action_outcome_in_execution_order`; `supervisor_state_advances_only_from_successful_action_results`; `supervisor_action_errors_are_bounded_single_line_and_control_free` | Missing, reordered, lossy, intent-derived, or unbounded outcome. |
| ITEM-4 subprocess | #15/#16 | Can a timed-out helper, inherited output pipe, or spawn-callback panic outlive the decision? | `bounded_systemctl_adapter_reaps_its_owned_timeout_fixture`; `capture_runner_reaps_successful_leader_and_all_stdio_redirected_descendant`; `capture_runner_on_spawn_panic_cannot_strand_owned_child`; `gpu_query_contains_descendant_inherited_pipe_and_keeps_success_valid`; `unreaped_group_selects_fatal_controller_containment` | Unreaped child, surviving owned descendant, or normal continuation after failed reap. |
| ITEM-4 victim identity | #13/#16 | Can a stale reservation, restart, or failed durable write retarget freeze/TERM/KILL? | `stale_invocation_id_refuses_all_systemctl_actions`; `freeze_applied_write_failure_recovers_exact_pending_identity_after_restart`; `reconstructed_supervisor_kills_only_durable_termed_identity_after_grace` | Any action after mismatch or against an identity other than the durable pending/applied target. |
| ITEM-4 backend stop | #13/#16 | Can systemd kill the backend before swapoff proof? | `backend_lifecycle_has_no_pre_swapoff_kill_path` | Finite kill escalation or stop before proof. |
| ITEM-7 guardian | #13/#15/#16 | Can stale monitor alone terminate a healthy guest? | `healthy_guest_with_stale_monitor_never_terminates`; inaccessible fixture | Any early terminate. |
| ITEM-8 recovery | #16/#17 | Does the host gate predate terminate and survive task/guest restart without heavy boot? | `guardian_terminates_exactly_once_and_enters_safe_mode`; `host_safe_mode_gate_survives_guardian_and_guest_restart` | Gate order wrong or heavy unit starts. |
| ITEM-9 postmortem | #7/#13 | Is an old warning mislabeled causal? | `old_boot_warning_is_not_incident_kernel_crash` | Old warning classified crash. |
| ITEM-9 host storage | #7/#13 | Does the event prove disk-full status without inventing a writer? | `ntfs_137_disk_full_is_host_volume_exhausted_without_product_attribution` | Missing status-code binding or product attribution. |

## Security checklist (pre-impl)

- [x] Privilege: system slice and task install require explicit root/elevation.
- [x] User/host copy: bounded JSON sizes and owned parsing.
- [x] Flags: unknown CLI/guardian actions refuse.
- [x] Info-leak: no argv/private paths in telemetry.
- [x] IRQ/IRQL: N/A — userspace.
- [x] Lifetime: token release and stale-PID reaping are explicit.
- [x] Device-gone: N/A — no device mutation in this SPEC.
- [x] Host safety: live pressure and terminate are VM-only until approval.
- [x] Replayable ops: release, task install/uninstall, terminate marker, and resume are idempotent.

## Files to CREATE / MODIFY / DELETE

**CREATE `crates/ramshared-cli/src/supervisor.rs`** — pure states, sample
transition, victim selection, one-second loop, state/cache-target publication,
and schema-v3 ordered typed action outcomes.
Tests: `supervisor_transitions_and_hysteresis_are_exact`,
`supervisor_delay_enters_emergency`, `discard_priority_is_deterministic`,
`frozen_scope_is_thawed_before_emergency_termination`, and
`action_executor_covers_cache_freeze_reclaim_thaw_and_kill`, and
`supervisor_publishes_every_action_outcome_in_execution_order`,
`supervisor_state_advances_only_from_successful_action_results`,
`discard_priority_uses_durable_older_reservation_ordinal_tie_break`,
`freeze_applied_write_failure_recovers_exact_pending_identity_after_restart`,
`reconstructed_supervisor_kills_only_durable_termed_identity_after_grace`, and
`supervisor_action_errors_are_bounded_single_line_and_control_free`.
Cover target: ≥80%.

**MODIFY `crates/ramshared-cli/src/workload.rs`** — aggregate budget, class
parsing, serialized ledger, run/session reservation/release. Tests:
`aggregate_reservations_share_one_limit`, `reservation_release_is_idempotent`,
`verified_dead_stale_lock_is_recovered_exactly_once`,
`stale_ledger_recovery_refuses_aba_replacement`,
`ledger_authority_refuses_symlink_hardlink_unsafe_mode_and_quarantine_alias`,
`ledger_directory_lock_prevents_replacement_owner_split_authority`,
`reservation_ledger_refuses_nofollow_violation_without_rewriting_target`,
`pid_reuse_cannot_own_or_release_reservation`,
`managed_scope_uses_aggregate_ceiling`, `scope_invocation_id_is_persisted`,
`safe_mode_marker_blocks_admission_even_when_a_lease_exists`, and
`scope_ack_timeout_reaps_group_and_owned_descendant`. Cover ≥80%.
Recovery tests additionally prove a durable and replayable matching-incident
transaction across host-gate removal.

**MODIFY `crates/ramshared-cli/src/main.rs`** — public run/session/supervise and
recover dispatch. Tests: `public_control_commands_parse_exactly`. Cover ≥80%
through the canonical `cascade-lifecycle-observability` owner.

**MODIFY `crates/ramshared-cli/src/cascade/lifecycle.rs`** — schema v4 planes,
worst-state precedence, origin/cache fields. Tests:
`schema_v4_worst_plane_controls_ok`, `using_vram_never_masks_critical_pressure`,
`origin_failure_and_stuck_cache_are_never_green`.
Cover ≥80% through the canonical `cascade-lifecycle-observability` owner.

**MODIFY `crates/ramshared-cli/src/monitor.rs`** — full PSI, vmstat, memory
events, sanitized processes, explicit nvidia-smi candidates, compact view.
Tests: `monitor_v4_records_full_pressure_and_sanitized_topn`,
`gpu_measurement_failure_is_explicit_and_not_green`, and
`gpu_query_contains_descendant_inherited_pipe_and_keeps_success_valid`.
Cover ≥80%.

**CREATE `crates/ramshared-cli/src/bounded_process.rs`** — shared private-group
spawn, bounded concurrent capture, exact group termination, bounded reap, and
fatal-controller seam. Tests: `capture_runner_keeps_legitimate_success_and_nonzero_status_typed`,
`capture_runner_rejects_bounded_output_overflow`, and
`capture_runner_reaps_successful_leader_and_all_stdio_redirected_descendant`,
`capture_runner_on_spawn_panic_cannot_strand_owned_child`, and
`unreaped_group_selects_fatal_controller_containment`. Cover ≥80% through the
canonical `cascade-transport-orchestration` owner.

**CREATE `scripts/safety/systemd/ramshared-control.slice`**,
`ramshared-supervisor.service`, `scripts/safety/cascade-controller.sh`,
`scripts/safety/lifecycle-recovery-status.sh`, and
`scripts/windows/Recover-RamSharedWslLifecycle.ps1`; **MODIFY**
workload/health/lifecycle units and installer.
Required test: `scripts/safety/test-control-plane-units.sh` ::
`control_and_workload_limits_are_sealed` and
`backend_lifecycle_has_no_pre_swapoff_kill_path`. E2E-only for shell deployment.

**MODIFY `scripts/windows/Watch-RamSharedWsl.ps1`** — action dispatcher,
scheduled task, four proof gates, capture, exactly-one terminate, safe boot.
**MODIFY `scripts/windows/Test-RamSharedWslWatchdogStatic.ps1`** with named
manufactured cases. Cover: N/A — PowerShell static/manufactured.
Required names: `healthy_guest_with_stale_monitor_never_terminates`,
`guardian_terminates_exactly_once_and_enters_safe_mode`,
`host_safe_mode_gate_survives_guardian_and_guest_restart`,
`guardian_never_uses_wsl_shutdown_or_host_restart`, and
`guardian_only_terminates_the_sealed_distro_after_all_gates`.

**MODIFY `scripts/safety/postmortem.sh`** and create
`scripts/safety/test-postmortem-classification.sh`. Named test:
`old_boot_warning_is_not_incident_kernel_crash`. Cover: N/A — fixture E2E.

**MODIFY structural docs** listed in the PRD and append `validation.md` only
after source gates.

No production file is deleted.

## Observability

| Signal | Where | Type |
| --- | --- | --- |
| control/cache/guardian/overall | status and monitor | enum |
| PSI some/full avg10/60/300 | monitor JSONL | percent |
| MemAvailable/reserve | monitor JSONL | KiB |
| pswpin/pswpout | monitor JSONL | pages |
| memory.high/max/oom/oom_kill | monitor JSONL | counters |
| managed reservations | monitor JSONL | count/bytes/class |
| unmanaged pressure | monitor JSONL | sanitized top-N |
| supervisor action results | state/status | ordered action+status+error records |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alter |
| `docs/reliability/GAP-REGISTER.md` | Alter |
| `validation.md` | Append source-only partial evidence |
| `docs/BENCHMARKS.md` | N/A — no performance claim |
| `.claude/rules/*` | N/A — no convention change |

## Implementation order

1. ITEM-1 budget formulas and systemd contract tests.
2. ITEM-2 aggregate reservation ledger tests and implementation.
3. ITEM-3 run/session class CLI and reversible launchers/drop-ins.
4. ITEM-4 supervisor state/victim policy.
5. ITEM-5 schema v4 status precedence.
6. ITEM-6 monitor telemetry and sanitized top-N.
7. ITEM-7 guardian scheduled-task and proof gates.
8. ITEM-8 safe-mode recovery and idempotency.
9. ITEM-9 temporal postmortem classifier and docs.

## Required tests matrix

| Production path | Test | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `workload.rs` | `aggregate_reservations_share_one_limit` | unit | #13 | ≥80% |
| `workload.rs` | `reservation_release_is_idempotent` | unit | #17 | ≥80% |
| `workload.rs` | `verified_dead_stale_lock_is_recovered_exactly_once` | unit | #16/#17 | ≥80% |
| `workload.rs` | `stale_ledger_recovery_refuses_aba_replacement` | unit | #13/#16 | ≥80% |
| `workload.rs` | `ledger_authority_refuses_symlink_hardlink_unsafe_mode_and_quarantine_alias` | unit/security | #13/#16 | ≥80% |
| `workload.rs` | `ledger_directory_lock_prevents_replacement_owner_split_authority` | unit/concurrency | #13/#16/#17 | ≥80% |
| `workload.rs` | `reservation_ledger_refuses_nofollow_violation_without_rewriting_target` | unit/security | #13/#17 | ≥80% |
| `workload.rs` | `pid_reuse_cannot_own_or_release_reservation` | unit | #13/#17 | ≥80% |
| `workload.rs` | `managed_scope_uses_aggregate_ceiling` | unit | #9 | ≥80% |
| `workload.rs` | `scope_invocation_id_is_persisted` | unit | #13 | ≥80% |
| `workload.rs` | `scope_ack_timeout_reaps_group_and_owned_descendant` | process/timeout | #15/#16 | ≥80% |
| `supervisor.rs` | `supervisor_transitions_and_hysteresis_are_exact` | unit | #16 | ≥80% |
| `supervisor.rs` | `supervisor_delay_enters_emergency` | unit | #16 | ≥80% |
| `supervisor.rs` | `discard_priority_is_deterministic` | unit | #9 | ≥80% |
| `supervisor.rs` | `frozen_scope_is_thawed_before_emergency_termination` | unit | #16 | ≥80% |
| `supervisor.rs` | `stale_invocation_id_refuses_all_systemctl_actions` | unit | #13/#16 | ≥80% |
| `supervisor.rs` | `supervisor_publishes_every_action_outcome_in_execution_order` | unit | #13/#17 | ≥80% |
| `supervisor.rs` | `supervisor_state_advances_only_from_successful_action_results` | unit/fault | #13/#17 | ≥80% |
| `supervisor.rs` | `discard_priority_uses_durable_older_reservation_ordinal_tie_break` | unit/order | #9/#13 | ≥80% |
| `supervisor.rs` | `freeze_applied_write_failure_recovers_exact_pending_identity_after_restart` | unit/restart | #13/#16/#17 | ≥80% |
| `supervisor.rs` | `reconstructed_supervisor_kills_only_durable_termed_identity_after_grace` | unit/restart | #13/#16/#17 | ≥80% |
| `supervisor.rs` | `supervisor_action_errors_are_bounded_single_line_and_control_free` | unit/adversarial | #13/#16 | ≥80% |
| `supervisor.rs` | `bounded_systemctl_adapter_reaps_its_owned_timeout_fixture` | process/timeout | #15/#16 | ≥80% |
| systemd lifecycle | `backend_lifecycle_has_no_pre_swapoff_kill_path` | static | #13/#16 | N/A |
| `workload.rs` | `recovery_releases_gates_only_with_a_current_resume_lease` | unit | #16/#17 | ≥80% |
| `lifecycle.rs` | `schema_v4_worst_plane_controls_ok` | unit | #13 | ≥80% via canonical lifecycle owner |
| `lifecycle.rs` | `using_vram_never_masks_critical_pressure` | unit | #13 | ≥80% via canonical lifecycle owner |
| `lifecycle.rs` | `origin_failure_and_stuck_cache_are_never_green` | unit | #13/#16 | ≥80% via canonical lifecycle owner |
| `monitor.rs` | `monitor_v4_records_full_pressure_and_sanitized_topn` | unit | #9 | ≥80% |
| `monitor.rs` | `gpu_query_contains_descendant_inherited_pipe_and_keeps_success_valid` | process/timeout | #15/#16 | ≥80% |
| `bounded_process.rs` | `unreaped_group_selects_fatal_controller_containment` | injected fatal seam | #15/#16 | ≥80% via canonical transport owner |
| `bounded_process.rs` | `capture_runner_reaps_successful_leader_and_all_stdio_redirected_descendant` | adversarial process/pipe | #15/#16 | ≥80% via canonical transport owner |
| `bounded_process.rs` | `capture_runner_on_spawn_panic_cannot_strand_owned_child` | panic/process | #15/#16 | ≥80% via canonical transport owner |
| Windows guardian | `healthy_guest_with_stale_monitor_never_terminates` | manufactured | #13/#16 | N/A |
| Windows guardian | `guardian_terminates_exactly_once_and_enters_safe_mode` | manufactured | #17 | N/A |
| Windows guardian | `host_safe_mode_gate_survives_guardian_and_guest_restart` | manufactured | #16/#17 | N/A |
| Windows guardian | `guardian_never_uses_wsl_shutdown_or_host_restart` | static | #13 | N/A |
| Windows guardian | `guardian_only_terminates_the_sealed_distro_after_all_gates` | static | #13/#15 | N/A |
| Windows guardian | `guardian_timeout_cleanup_is_bounded` | static | #16 | N/A |
| postmortem | `old_boot_warning_is_not_incident_kernel_crash` | fixture | #7/#13 | N/A |

## Validation checklist

The incident slice owns line coverage only for `workload.rs`, `supervisor.rs`,
and `monitor.rs`. The required `main.rs` and `cascade/lifecycle.rs` tests above
remain incident evidence under the single `cascade-lifecycle-observability`
owner. The `bounded_process.rs` tests likewise remain incident evidence under
`cascade-transport-orchestration`. This avoids counting the same production
source under two active SPECs.

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy -p ramshared-cli --all-targets -- -D warnings`
- [x] `cargo test -p ramshared-cli`
- [x] `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/workload.rs,crates/ramshared-cli/src/supervisor.rs,crates/ramshared-cli/src/monitor.rs,crates/ramshared-cli/src/stress.rs --min 80 --report-json tmp/wsl2-control-plane-pressure-incident-cov.json`
- [x] PowerShell parser and full Windows static suite
- [x] systemd shell static tests and docs-check
- [ ] source-only `/bin/true` before/action/after where authorization permits
- [ ] VM guardian and Docker/cron ancestry remain env-bound, not DONE

Rollback trigger: admitted aggregate above max, threshold deviation, stale
monitor terminating a responsive guest, terminate count other than one, safe
mode starting a heavy unit, or old warning classified as incident crash.
