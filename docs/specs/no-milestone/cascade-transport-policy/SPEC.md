# SPEC — cascade-transport-policy

> Implements [`PRD.md`](PRD.md). Zero creativity out of scope.

## Traceability

| PRD | ITEM |
| --- | --- |
| RF-T1 | ITEM-1 priorities already in cascade.rs — assert logs |
| RF-T2 | ITEM-2 install-cascade-boot --enable |
| RF-T3, RF-T4 | ITEM-3 transport auto / ublk refuse on WSL2 |
| RF-T3, RF-T4, RF-T6 | ITEM-5 bounded transport orchestration refusal boundary |
| RF-T5 | ITEM-4 existing idempotent up; ITEM-5 preserves its safe setup order |

## ITEM-1 — Priority order (no code change required)

Keep `TierPriorities::default()`: zram=200, vram=100, disk≈−2.  
`up` must log the order once. The disk tier is a pre-existing lower-tier swap
selected outside this cascade transport policy.

## ITEM-2 — Boot enable

```bash
sudo bash scripts/safety/install-cascade-boot.sh --enable
```

Requires: systemd, `target/release/ramshared`+`ramsharedd`, `ramshared check` ready, preflight OK.  
Unit: `ramshared-cascade.service` oneshot RemainAfterExit.

## ITEM-3 — transport auto

Default `--transport` / omit = **auto**:

| Environment | Resolved |
| --- | --- |
| WSL2 | **nbd** + stderr reason (ublk freeze policy) |
| non-WSL2 + `/dev/ublk-control` | ublk (future full wire; may still error until implemented) |
| else | nbd |

Explicit `--transport ublk` on WSL2: fail closed in `up` (do not start half-daemon).

## ITEM-4 — Idempotent up

Unchanged: if cascade healthy, return status without re-setup.

## ITEM-5 — Bounded transport orchestration

`crates/ramshared-cli/src/cascade/cascade_io.rs` owns the direct process and
runtime-file boundary for the NBD transport lifecycle. It must keep all policy
decisions in the parent `cascade` module, but every process it starts is
bounded and every daemon cleanup target is an exact child or a verified PID.

### DT-T1 — Direct child command boundary

`cascade_io` uses the shared direct-argv bounded runner for short-lived commands
(`modprobe`, `zramctl`, `swapon`, `swapoff`, `nbd-client`, and identity probes).
Production does not invoke a shell or select a process by name. Each child is
the leader of a new invocation-private process group. The runner concurrently
captures at most 64 KiB from each output stream, returns trimmed stdout on
success, and returns the command identity plus its exit/timeout reason on
failure.

The production timeout is 5 seconds per short-lived command. Timeout, wait
error, and a pipe kept open by an owned descendant signal exactly the private
group with SIGKILL and bound the direct-child reap and capture-worker close.
The runner never uses `pkill`, `pgrep`, or a name match. If group SIGKILL plus
the fixed grace cannot prove the direct child reaped, production exits the
controller with status 125 rather than returning to normal cascade control.
An injected fatal seam proves that branch without killing the test process.

### DT-T2 — Daemon identity and readiness

`spawn_daemon` starts the retained daemon as the leader of its own exact process
group and retains the `Child` until the NBD attach succeeds.
Readiness is bounded to 6 seconds waiting for the configured socket path. A
failed readiness check kills that exact group and reaps the direct child before any NBD
attach and removes only this invocation's runtime PID record/forensics
marker.

After startup, `down` reads only its runtime PID and cache identity records. It
opens a pidfd for that exact positive PID, revalidates its boot/start-time/
invocation identity immediately before signaling, and sends `TERM` through the
pidfd only after exact managed-swap absence. A missing, malformed, stale, or
changed record is retained as containment evidence. `down` never falls back to
`pgrep`, `pkill`, or a name-wide signal.

### DT-T3 — Attach rollback and primary error

`connect_nbd` is ordered as `nbd-client` → sealed swap signature verification
→ lifecycle/receipt publication → `swapon`. Normal startup never runs
`mkswap`. It preserves
the original failing error and performs each rollback action at most once:

| Failure point | Required rollback | Forbidden action |
| --- | --- | --- |
| post-spawn pre-attach refusal (invalid managed device or connection count) | terminate/reap the exact spawned daemon; disarm invocation marker | NBD detach without attach, broad kill |
| `nbd-client` with two stable no-effect proofs | terminate/reap the exact spawned daemon group; disarm invocation marker | `pkill`, retry loop |
| signature/record failure after a proven attach | one exact `nbd-client -d <allowlisted swap_dev>` after identity/absence proof; terminate/reap exact daemon group | broad detach/kill or formatting |
| uncertain `swapon` | preserve exact backend, daemon, lifecycle binding, runtime records, and forensics | detach, second `swapon`, or daemon name match |

The exact `swap_dev` must satisfy the existing managed-device allowlist before
the attach sequence begins. The rollback itself does not call `swapoff`,
because `swapon` did not report success. A later live `down` path remains the
only place that may perform the documented `swapoff`-before-disconnect flow.
During normal `down`, the recorded device and the current swap scan are
canonicalized and deduplicated before `nbd-client -d`; an allowlisted NBD
device is detached at most once per invocation.

### DT-T3a — Stable timeout fixture execution

The Linux timeout/reaping test closes its generated script and invokes it as
an argument to the immutable `/bin/sh` interpreter. It does not execute the
freshly written temporary inode directly. The script still `exec`s the bounded
sleep so the observed PID is the exact child reaped by the production runner.
This removes an overlay/antivirus `ETXTBSY` race without adding a retry or
weakening the timeout assertion; an interpreter error remains a terminal test
failure.

### DT-T4 — zram output contract

`zramctl --find` output is accepted only as an exact trimmed `/dev/zram<decimal>`
device identity. Empty output, a suffix, a path outside `/dev`, or non-decimal
digits is refused before `mkswap`/`swapon`. The returned block identity is
observed, sealed in the lifecycle runtime record, and freshly revalidated
before `mkswap`. The former direct `zram0` sysfs fallback is prohibited because
it could reset a foreign device before RamShared had recorded ownership. If
`zramctl --find` cannot allocate a device, zram setup fails closed.

### DT-T5 — Test-only isolation

The named tests use only a temporary runtime directory, controlled direct
child fixtures, and scripted command outcomes. They do not call real
`swapon`, `swapoff`, `nbd-client`, `modprobe`, `/sys`, the product
`ramsharedd`, or `/run/ramshared`. A test that exercises the bounded runner
may start a temporary child owned by the test and must reap it before return.
No test pressure, CUDA allocation, daemon deployment, or host mutation is
authorized by this ITEM.

### DT-T6 — Cross-tier setup rollback

Runtime records for a newly created zram or NBD tier are written before that
tier's `swapon`, then removed if activation refuses. Thus a runtime-file write
failure cannot occur after that tier becomes active. If zram activation
succeeds and any later NBD setup step fails before the cascade is reported
active, the original NBD error is preserved while this invocation performs
exact `swapoff <zram_dev>` followed by `zramctl -r <zram_dev>`. It then removes
only its temporary zram/NBD/PID/socket records and forensics marker.

The rollback never detaches NBD before its swap is off, never selects a
device/process by a broad name, and never converts the primary error into a
cleanup error. The rule applies only to the zram tier created by the same `up`
invocation; it does not select an existing zram device from `/proc/swaps`.

If that exact zram `swapoff` or reset refuses, the rollback stops before any
destructive next step: it does not reset the device, it retains the zram
runtime record, and it re-arms the invocation forensics marker for manual
recovery. The original NBD failure remains the returned error. This is
fail-closed rather than a best-effort reset of a possible active swap.

### Atomicity and rollback

**Atomicity frontier:** each short-lived command is an owned child process;
each new tier records its invocation-owned device before `swapon`. A failed
activation removes that pending record. If a later NBD step fails after zram
activation, this ITEM rolls back that exact newly-created zram before returning
the original error. After a successful full cascade activation, `down` keeps
its existing swapoff-first safety protocol and refuses daemon teardown while a
managed block swap remains.

| Layer | Rollback policy |
| --- | --- |
| CLI userspace | Return the original command error; remove only invocation-owned runtime files and exact child processes. |
| Daemon | Readiness/attach failures terminate and reap only the child retained by this `up`; normal `down` signals only a verified recorded PID. |
| Kernel/module | N/A — no kernel or driver change. |
| Host/persistent swap | No unit test mutates it. A live failure after successful `swapon` follows `down` / orphan-recovery safety policy, never a broad kill. |

Rollback trigger: any timeout that leaves an owned child alive after its
bounded cleanup, any command named `pkill`/`pgrep` in `cascade_io`, any
rollback command whose device is not the configured allowlisted `swap_dev`,
or any live verification finding a managed block swap after `down` means
revert this ITEM and keep the cascade disabled pending investigation.

### Kahneman

| Stage | Discipline | Question | Executable evidence | Abort |
| --- | --- | --- | --- | --- |
| bounded runner | #15/#16 | Could a helper, capture worker, or descendant-held pipe outlive the deadline? | `bounded_command_times_out_and_reaps_its_direct_child`; `bounded_command_contains_descendant_that_inherits_output`; `unreaped_group_selects_fatal_controller_containment` | timeout exceeds the deadline, an owned descendant survives, or failed reap returns normally |
| identity/refusal | #13/#16 | Could a malformed PID, invalid zram output, or pre-attach argument select a different process/device? | `daemon_pid_requires_positive_pid_and_exact_identity`, `zram_output_requires_exact_block_identity`, `connect_nbd_refusal_terminates_exact_daemon_without_detach` | any non-exact identity reaches a command or leaves the spawned child alive |
| attach rollback | #16/#17 | Does one failure detach/clean up exactly once and preserve the first error? | `connect_nbd_preserves_primary_error_and_rolls_back_once` | duplicate/broad rollback or changed primary error |
| cross-tier rollback | #13/#16/#17 | Could a failed NBD setup leave its newly-created zram tier active or reset it after swapoff refusal? | `setup_new_cascade_rolls_back_zram_after_nbd_failure`, `setup_new_cascade_keeps_zram_record_on_swapoff_refusal` | zram record/device persists after successful cleanup, or reset follows refusal, or primary error changes |

### Files

| Path | Change | RF / DT | Tests / cover |
| --- | --- | --- | --- |
| `crates/ramshared-cli/src/bounded_process.rs` | private process groups, finite capture, bounded reap, fatal containment seam | RF-T6; DT-T1 | named matrix below; ≥80% line coverage |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | shared bounded runner integration, exact daemon identity/cleanup, strict zram output parse, transactional tier records and rollback, English diagnostics | RF-T3..RF-T6; DT-T1..DT-T6 | named matrix below; ≥80% line coverage |
| `docs/governance/rust-slice-coverage.json` | exact `cascade-transport-orchestration` owner | DT-T1..DT-T6 | canonical command below |
| `tools/ci/plan-rust-slice-coverage.test.mjs` | exact owner and named-test assertion | DT-T5/DT-T6 | Node planner test |
| `docs/specs/no-milestone/comment-language-integrity/SPEC.md` | move this path from residual language-only block to its feature owner once the gate is green | DT-T1..DT-T6 | documentation assertion only |

### Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | `cascade_io.rs` :: `bounded_command_captures_stdout_and_rejects_nonzero` | process/unit | #9 | ≥80% |
| same | `cascade_io.rs` :: `bounded_command_times_out_and_reaps_its_direct_child` | process/timeout | #15 | ≥80% |
| same | `cascade_io.rs` :: `bounded_command_contains_descendant_that_inherits_output` | adversarial process/pipe | #15/#16 | ≥80% |
| same | `cascade_io.rs` :: `zram_output_requires_exact_block_identity` | unit/refusal | #13/#16 | ≥80% |
| same | `cascade_io.rs` :: `daemon_pid_requires_positive_pid_and_exact_identity` | unit/refusal | #13/#16 | ≥80% |
| same | `cascade_io.rs` :: `failed_readiness_terminates_only_spawned_child` | process/cleanup | #15/#16 | ≥80% |
| same | `cascade_io.rs` :: `connect_nbd_preserves_primary_error_and_rolls_back_once` | unit/rollback | #16/#17 | ≥80% |
| same | `cascade_io.rs` :: `connect_nbd_refusal_terminates_exact_daemon_without_detach` | unit/refusal | #13/#16 | ≥80% |
| same | `cascade_io.rs` :: `connect_nbd_uncertain_swapon_preserves_backend_and_daemon` | unit/containment | #16/#17 | ≥80% |
| same | `cascade_io.rs` :: `zram_fallback_refuses_unexpected_device_without_swapon` | unit/refusal | #13/#16 | ≥80% |
| same | `cascade_io.rs` :: `malformed_zram_success_resets_exact_new_device_without_leak` | unit/rollback | #16/#17 | ≥80% |
| same | `cascade_io.rs` :: `zram_setup_never_mutates_unbound_sysfs_fallback` | unit/refusal | #13/#16 | ≥80% |
| same | `cascade_io.rs` :: `runtime_marker_and_pid_record_refuse_unsafe_identity` | unit/identity | #13/#16 | ≥80% |
| same | `cascade_io.rs` :: `setup_new_cascade_uses_only_temp_runtime_and_direct_child_fixture` | process/orchestration | #15/#16 | ≥80% |
| same | `cascade_io.rs` :: `setup_new_cascade_rolls_back_zram_after_nbd_failure` | process/rollback | #16/#17 | ≥80% |
| same | `cascade_io.rs` :: `setup_new_cascade_keeps_zram_record_on_swapoff_refusal` | process/refusal | #13/#16 | ≥80% |
| same | `cascade_io.rs` :: `down_with_runtime_preserves_swapoff_first_and_cleans_temp_state` | unit/cleanup | #16/#17 | ≥80% |
| same | `cascade_io.rs` :: `transport_refusal_is_fail_closed_before_command` | unit/refusal | #13/#16 | ≥80% |
| shared runner | `bounded_process.rs` :: `capture_runner_keeps_legitimate_success_and_nonzero_status_typed` | process/unit | #9 | ≥80% |
| shared runner | `bounded_process.rs` :: `capture_runner_rejects_bounded_output_overflow` | process/bound | #15 | ≥80% |
| shared runner | `bounded_process.rs` :: `unreaped_group_selects_fatal_controller_containment` | injected fatal seam | #15/#16 | ≥80% |
| package | `cargo test -p ramshared-cli` | package | #9 | all pass |

**Canonical cover gate:**

```bash
node tools/ci/check-rust-slice-coverage.mjs \
  -p ramshared-cli \
  --files crates/ramshared-cli/src/bounded_process.rs,crates/ramshared-cli/src/cascade/cascade_io.rs \
  --min 80 \
  --report-json tmp/cascade-transport-orchestration-cov.json
```

### Later live E2E gap (not exercised by this ITEM)

This ITEM supplies deterministic local process/refusal proof only. It does not
close the cascade live surface. Before Step 3 can be marked done, a qualified
environment must record the following on the actual operator surface:

1. **Before:** `ramshared status`, `/proc/swaps`, and the relevant daemon state.
2. **Action:** one legitimate bounded `ramshared up` / `ramshared down` drill
   and the explicit `--transport ublk` refusal.
3. **After:** numeric swap priorities/state, no managed block swap after down,
   and `BINARY_MATCH` from the running `ramsharedd` executable to the named
   release binary.

The evidence belongs under this slug and is appended to `validation.md` only
when that live drill is authorized. Until then this is an E2E/BINARY_MATCH
gap, not a completed deployment claim.

## Validation

| V | Check |
| --- | --- |
| V1 | up logs priority zram > VRAM > VHDX |
| V2 | up creates zram prio 200 and nbd prio 100 |
| V3 | down leaves only non-managed disk swap |
| V4 | transport auto on WSL2 does not attempt ublk product path |
| V5 | systemd unit enabled after install --enable |
| V6 | every `cascade_io` short-lived helper is group-bounded, no timeout uses a broad process name, and failed reap selects fatal containment |
| V7 | failed NBD attach preserves its first error and cleans up only the invocation-owned daemon/device |

## Kahneman

| # | Rule |
| --- | --- |
| #16 | ublk not Day-1 on WSL2 |
| #18 | Fix freeze at daemon teardown layer before enabling product ublk |
| #17 | up/down idempotent |
| #15 | short-lived process commands have a deadline and no retry loop |

## Out of SPEC

Implementing full ublk `up` wire on WSL2; soak thrash tests.
