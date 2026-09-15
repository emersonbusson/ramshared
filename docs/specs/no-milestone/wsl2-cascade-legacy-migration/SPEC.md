# SPEC — Attended migration from a legacy WSL2 cascade

> SSDV3 Step 2. The implementation is **partial** until the watchdog-supervised
> migration drill in the test matrix supplies before → action → after evidence.

## Closed scope

### In now

- One exact CLI command and dispatch action.
- Strict, pure eligibility plan for the legacy ZRAM → NBD topology.
- Runtime binding of the eligible devices and daemon process.
- Swapoff-first retirement and sealed-origin handoff.
- Hermetic refusal, ordering, and idempotency tests.

### Out now

- Automatic detection/recovery, ublk, boot automation, arbitrary devices,
  changes to the WSL fallback disk swap, and unsupervised pressure.

## Traceability

| PRD | SPEC item |
| --- | --- |
| RF-1 | ITEM-1 |
| RF-2 | ITEM-2, ITEM-3 |
| RF-3 | ITEM-3 |
| RF-4 | ITEM-4 |
| RF-5 | ITEM-3, ITEM-4 |
| RF-6 | ITEM-2, ITEM-4 |
| RF-7 | ITEM-4, ITEM-5 |
| NFR-1..5 | ITEM-1..5 |

## Technical decisions

| ID | Decision | Rationale |
| --- | --- | --- |
| DT-1 | Parse only `migrate-cascade --from-legacy`; reject every other option before an action runner is called. | Explicit consent is part of the authority boundary. |
| DT-2 | `LegacyMigrationPlan` is pure and requires exactly one non-ghost NBD, zero ublk, zero or one ZRAM, and no duplicate managed path. | Device names, a broad prefix, and an inactive-looking row do not prove a safe topology. |
| DT-3 | Initial NBD use must be zero; eligible ZRAM may contain pages only when `MemAvailable >= zram.size_kb + hard_floor_kib`. | This bounds the legacy drain without claiming that zero use alone proves ownership. |
| DT-4 | Before the first effect, runtime admission requires a sealed origin, healthy guardian, no current lifecycle binding/records, one exact `ramsharedd`, and a captured PID/start identity. Its executable must canonically equal the invoking release sibling, unless a replaced-binary proof shows the expected root-owned `(deleted)` path, exact legacy arguments (`--slices 1`, expected `--slice-mb`, `--listen-nbd 127.0.0.1:10809`), and ownership of that listener. | A binary replacement may orphan a live executable inode; the narrow proof retains process-role binding without selecting a process only by name. |
| DT-5 | Device effects use a bound block-device identity, fresh strict snapshots, and exact absence proof. Process termination uses a revalidated pidfd TERM and bounded wait. | Closes device retargeting, PID reuse, and ambiguous-command gaps. |
| DT-6 | Order is ZRAM swapoff → proof, NBD swapoff → proof, ZRAM reset → NBD detach → daemon TERM/wait → existing sealed-origin `up`. | ZRAM pages retain the NBD/disk fallback while they drain; no teardown action occurs while either managed swap remains active. |
| DT-7 | A failed stage does not attempt later stages, remove evidence, reformat a device, or invoke normal `up`. | Uncertain mutation is containment, not permission to continue. |
| DT-8 | Once the new cascade is attached, its existing lifecycle binding is the only persistent authority. A repeat migration refuses because no eligible legacy constellation remains. | Satisfies Day-0: no permanent compatibility path. |
| DT-9 | Non-managed disk swap rows are neither selected nor passed to a mutating command. | The host fallback is outside the migration authority. |

## Atomicity frontier and failure semantics

No mutation occurs until all preconditions in DT-2 through DT-5 pass. The
atomicity frontier is the first successful ZRAM `swapoff`: at that point the
old topology may no longer be restorable as a running legacy cascade. Failure
after that frontier is `ContainedPartial`; the command stops, preserves
migration/runtime evidence, and prints the failed stage. It must not fabricate
the new binding or claim a completed migration.

## Files

### Modify

**`crates/ramshared-cli/src/main.rs`**

- RF/DT: RF-1, DT-1.
- Add `CliCommand::MigrateLegacyCascade`, strict parsing, action-runner method,
  dispatch, usage line, and recording-action test coverage.
- Required test: `public_control_commands_parse_exactly`,
  `run_from_args_dispatches_all_command_variants`.

**`crates/ramshared-cli/src/cascade/mod.rs`**

- RF/DT: RF-2, RF-6, DT-2, DT-3, DT-9.
- Add pure typed legacy eligibility/state plan using strict `SwapEntry` values.
- Required tests: `legacy_migration_plan_accepts_single_clean_nbd_and_zram`,
  `legacy_migration_plan_refuses_ghost_dirty_duplicate_or_ublk`.
- Cover target: at least 80% for this business-logic file.

**`crates/ramshared-cli/src/cascade/cascade_io.rs`**

- RF/DT: RF-3..7, DT-4..8.
- Add injected legacy transition executor, runtime identity/device checks, and
  a public handoff entry point invoked by the CLI.
- Required tests: `legacy_migration_executor_preserves_swapoff_first_order`,
  `legacy_migration_executor_stops_on_first_refusal`,
  `legacy_migration_rejects_replaced_daemon_identity`,
  `legacy_replaced_daemon_requires_bound_root_listener`.
- Cover target: N/A — external-device orchestration; required hermetic executor
  tests plus watchdog E2E and daemon BINARY_MATCH or replaced-binary listener
  proof prove the privileged boundary.

**`README.md`**

- RF/DT: RF-1, NFR-1.
- Add one safe-operation line explaining the explicit one-time command and its
  refusal semantics; do not add benchmark claims.

**`docs/reliability/DEGRADATION-MATRIX.md`**

- RF/DT: RF-7, DT-7.
- Add the legacy-handoff partial state and containment response.

### Create

**`docs/specs/no-milestone/wsl2-cascade-legacy-migration/{PRD,SPEC,AUDIT-2.5,IMPL}.md`**

- Purpose: SSDV3 decision, audit, implementation evidence, and residual live
  gates for this privileged cascade surface.

## Observability

| Signal | Where | Meaning |
| --- | --- | --- |
| migration stage | command stderr | Current bounded stage; no host-private path is printed. |
| migration refusal | command stderr / exit 1 | The exact admission or containment rule stopped the operation. |
| topology and binding | `ramshared status --json` | New cascade health after the existing `up` path completes. |
| watchdog result | approved host harness artifact | Host-side decision for the attended campaign. |

## Implementation order

1. **ITEM-1 — CLI contract.** Finish the existing RED parser checkpoint with
   parsing, dispatch, help, and recording-action support.
2. **ITEM-2 — Pure admission plan.** Add RED/green tests for eligible,
   ghost, dirty, duplicate, ublk, and fallback-disk constellations.
3. **ITEM-3 — Runtime identity binding.** Add the unique daemon/executable/PID
   proof and exact device binding before any effect.
4. **ITEM-4 — Transition executor.** Add the ordered, revalidated effects and
   handoff to existing sealed-origin `up`; test successful order, early stop,
   and identity replacement.
5. **ITEM-5 — Documentation and validation.** Update operation/reliability
   docs, regenerate index, then run source gates and the watchdog campaign.

## Required tests matrix

| Production path | Test | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| CLI parser/dispatch | `crates/ramshared-cli/src/main.rs` :: `public_control_commands_parse_exactly` | unit | #9, #13 | existing CLI slice |
| CLI dispatch | `crates/ramshared-cli/src/main.rs` :: `run_from_args_dispatches_all_command_variants` | unit | #13 | existing CLI slice |
| Pure admission | `crates/ramshared-cli/src/cascade/mod.rs` :: `legacy_migration_plan_accepts_single_clean_nbd_and_zram` | unit | #13 | ≥80% |
| Pure refusals | `crates/ramshared-cli/src/cascade/mod.rs` :: `legacy_migration_plan_refuses_ghost_dirty_duplicate_or_ublk` | unit | #13, #16 | ≥80% |
| Effect ordering | `crates/ramshared-cli/src/cascade/cascade_io.rs` :: `legacy_migration_executor_preserves_swapoff_first_order` | hermetic executor | #9, #17 | E2E-gated |
| First error | `crates/ramshared-cli/src/cascade/cascade_io.rs` :: `legacy_migration_executor_stops_on_first_refusal` | hermetic executor | #15, #16 | E2E-gated |
| Identity replacement | `crates/ramshared-cli/src/cascade/cascade_io.rs` :: `legacy_migration_rejects_replaced_daemon_identity` | hermetic | #13, #16 | E2E-gated |
| Attended migration | watchdog harness :: before/action/after | live E2E | #13, #16, #17 | required |

## Kahneman

| Discipline | Question | Evidence / abort |
| --- | --- | --- |
| #9 | Is “safe handoff” measurable? | Exact exit codes, swap `used_kb`, stage order, binding presence. Abort on adjective-only evidence. |
| #13 | Does every refusal retain a legitimate route? | Each refusal fixture pairs with eligible plan and watchdog live legitimate/refusal paths. |
| #15 | Is failure retried blindly? | Deterministic admission/effect errors stop at attempt one. |
| #16 | Does protection survive an exhausted state? | Budgeted drain and host watchdog; no direct daily-host pressure. |
| #17 | Does replay create a second handoff? | A post-success invocation refuses with zero device effects. |
| #18 | Is legacy a permanent shim? | Transition ends with the existing sealed binding only. |

## Validation checklist

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test -p ramshared-cli legacy_migration -- --test-threads=1`
- [ ] `cargo test -p ramshared-cli public_control_commands_parse_exactly --bin ramshared`
- [ ] `cargo clippy -p ramshared-cli --all-targets -- -D warnings`
- [ ] `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/cascade/mod.rs --min 80`
- [ ] `./scripts/docs-check.sh`
- [ ] BINARY_MATCH or replaced-binary listener proof of the deployed daemon
  before live E2E
- [ ] Approved watchdog harness: before → action → after, legitimate and
      refusal evidence, terminal cleanup state

## Rollback trigger

Block the command and revert this feature if a supervised run observes a
foreign-device effect, mismatched daemon signal, ghost swap, watchdog action,
or kernel BUG/Oops/panic/hung task. No retry is authorized for those outcomes.
