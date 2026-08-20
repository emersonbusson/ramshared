# SPEC — WSL2 control-plane pressure incident hardening

## Decisions

| ID | Decision |
| --- | --- |
| DT-1 | `allow_commit` denial returns an I/O error before CUDA allocation. |
| DT-2 | Product mode preallocates the selected 4/2/1 GiB profile and exposes its actual size through NBD. |
| DT-3 | `status --json` schema v3 retains the v2 phase fields and adds activation identity plus disk baseline/growth attribution. |
| DT-4 | `UsingZram` with at least 80% zram and no active VRAM is `AT_RISK`; unverified capacity is `BLOCKED`. |
| DT-5 | `ramshared run --profile safe -- ...` creates only `ramshared-workloads.slice` with calculated memory limits. |
| DT-6 | The host watchdog sends TERM, waits ten seconds, then sends KILL only to that slice. It has no WSL lifecycle action. |
| DT-7 | The attended installer atomically deploys the sealed health and workload units without enabling them; their runtime paths resolve through `/opt/ramshared/current`. |
| DT-8 | The sealed uninstaller stops and removes only byte-identical service definitions; it removes the static workload-slice definition without stopping managed workloads. |
| DT-9 | The Windows incident snapshot is `PLAN` by default; explicit capture bounds each WSL client query, records a SHA-256 manifest plus sanitized `.wslconfig` kernel/swap state, and never downloads or runs the Microsoft collector. |
| DT-10 | `ramshared monitor` is read-only: a two-second default sample interval, five-minute history, two-second GPU query deadline, and no lifecycle or pressure controls. |
| DT-11 | The typed monitor is the sole health sampler; JSONL rotates at 50 MiB and the Windows-visible heartbeat is atomically replaced. |
| DT-12 | Disk swap is `UsingDisk` only when activation-scoped growth reaches the active threshold. No baseline means no attribution and an active legacy receipt is `BLOCKED`. |
| DT-13 | The Windows status viewer reads the heartbeat only; btop tuning is plan-first and produces an exact rollback artifact. |

## Public contracts

- `ramshared --version` and `ramsharedd --version`.
- `ramshared run --profile safe -- <command> [args...]`.
- `status --json` includes `schema_version: 3` and
  `protection_state: OFF|READY|ACTIVE|AT_RISK|BLOCKED` plus a stable
  `protection_reason`.
- `ramshared monitor` emits schema v3 JSONL plus an atomically replaced
  Windows-visible heartbeat; `cascade-health.sh` is only a flag-translation
  wrapper.
- The sealed uninstaller preserves missing, symlinked, non-regular, and
  non-matching systemd definitions.
- `Capture-WslIncidentSnapshot.ps1` emits the official collector URL in
  `PLAN`; its `-Run` path is read-only with respect to WSL lifecycle and
  pressure. Its configuration record never includes the local config or
  kernel path.

## Named tests

| Test | Evidence |
| --- | --- |
| `host_budget_denial_prevents_cuda_allocation` | Refusal returns before provider allocation. |
| `adaptive_profile_falls_back_4_2_1_before_swapon` | Profile selection preserves the VRAM reserve. |
| `product_daemon_command_forces_guaranteed_capacity` | Product CLI cannot start the sparse backend. |
| `status_blocks_unverified_capacity` | Missing capacity receipt is `BLOCKED`. |
| `status_blocks_ineffective_tier_before_control_plane_exhaustion` | Saturating zram is not healthy topology. |
| `managed_scope_preserves_control_plane_reserve` | Workload limit retains guest control-plane memory. |
| `ramshared_wsl_watchdog_is_bounded_and_scope_only` | Host script contains no VM termination path. |
| `status_off_does_not_attribute_preexisting_disk_pages_to_ramshared` | Disk pages visible before activation remain unattributed. |
| `disk_growth_after_activation_is_at_risk` | Activation-scoped disk growth selects `UsingDisk`. |
| `monitor_defaults_are_read_only_and_bounded` | Default sampling and history bounds have no mutation surface. |
| `cascade_health_delegates_to_typed_monitor` | Shell contains no duplicate telemetry heuristic. |
| `ramshared_wsl_status_reads_heartbeat_only` | Windows viewer starts no guest or lifecycle process. |
| `configure_btop_is_plan_first_and_reversible` | btop tuning preserves a byte-restorable backup. |
| `auxiliary_unit_conflict_refuses_and_rolls_back` | A foreign auxiliary unit prevents publication and preserves the previous selector. |
| `uninstaller_preserves_foreign_unit_definitions` | Removal proves unit identity and does not stop the workload slice. |
| `wsl_incident_snapshot_is_bounded_read_only` | The snapshot collector defaults to plan-only and forbids WSL lifecycle or pressure operations. |

Rust slices require at least 80% coverage. Live WSL evidence is
`before → action → after`, exact `BINARY_MATCH`, no ghost state, and
environment-bound until the approved host campaign runs.

## Qualification matrix

1. Disposable Windows VM: WSL 2.7.12 versus an official build containing
   microsoft/WSL#41252.
2. Microsoft stock kernel versus the same official tree with only the
   microsoft/WSL#41054 config deltas.
3. RamShared 512/1024 MiB zram cells; select the largest cell that starts NBD
   I/O while control-plane reserve and order-7 availability remain positive.
4. Physical GPU validation only through the Windows watchdog harness and a
   fresh explicit shared-host approval.

Rollback trigger: any stale-green sample, capacity without a receipt, swap
write refusal after activation, HCS timeout in the qualified cell, ghost swap,
or teardown that cannot prove swapoff-first.
