---
slug: wsl2-cascade-legacy-migration
title: Attended migration from a legacy WSL2 cascade
milestone: —
issues: []
---

# PRD — Attended migration from a legacy WSL2 cascade

## 1. Summary

Provide one attended command, `ramshared migrate-cascade --from-legacy`, that
replaces a demonstrably RamShared-owned legacy ZRAM → NBD cascade with the
sealed-origin cascade. The command is a one-time transition, not an automatic
recovery mechanism and not a second runtime topology.

The migration exists because a release predating the lifecycle binding can be
active while the current sealed origin is ready. Normal `up` correctly refuses
that unbound state; the operator otherwise has no safe, product-owned route to
retire it and attach the current cascade.

## 2. Technical context

- **Confirmed in codebase:** normal `up` parses a root-owned sealed origin
  manifest before lifecycle actions, and binds the resulting NBD device to
  origin, daemon, socket, boot, and device identities.
- **Confirmed in codebase:** normal `down` requires that binding and executes
  `swapoff` before zram reset, NBD detach, or daemon stop.
- **Confirmed in codebase:** unbound NBD, ublk, and zram enumeration is
  detection-only, even when `used_kb == 0`.
- **Confirmed in the installed release metadata:** an installed RamShared
  release has a manifest and source provenance; legacy daemon state can lack
  the current lifecycle binding.
- **Inference:** an operator explicitly requesting the migration can authorize
  a bounded one-time handoff only after the process, swap constellation, and
  sealed origin all satisfy the exact admission criteria below. A device name
  by itself remains insufficient evidence.

## 3. Recommended option

Add an explicit, flag-gated migration command. It captures an ephemeral legacy
identity from the only eligible daemon, drains the legacy ZRAM and NBD devices
with fresh checks, detaches them, terminates that exact pidfd-bound daemon, and
then calls the existing sealed-origin `up` path. The new path seals the normal
lifecycle binding; no legacy record is retained as a compatibility path.

Discarded alternatives:

- Treat every zero-used NBD as recoverable: it can detach a foreign active
  swap and is rejected by the existing lifecycle contract.
- Reuse `down`: it correctly refuses when the legacy binding is absent.
- Require manual `swapoff`, `nbd-client -d`, and process termination: this
  recreates the unsafe, non-atomic sequence outside product checks.
- Keep a permanent legacy fallback: it creates a Day-0 dual path and obscures
  ownership after migration.

## 4. Functional requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | The public command is exactly `migrate-cascade --from-legacy`. | Missing, repeated, or extra options fail before any action. |
| RF-2 | Migration requires an explicit operator request, a sealed origin, exactly one live NBD, at most one live ZRAM, no ublk, and no ghost swap. | Ambiguous, malformed, dirty-NBD, or ghost fixtures execute zero commands. |
| RF-3 | The legacy daemon must be unique and either BINARY_MATCH the invoking release's daemon sibling or prove the narrowly-defined replaced-binary condition. | The replacement condition requires a root-owned `(deleted)` link from the expected path, exact legacy command arguments, the expected local NBD listener, and a captured PID/start identity; every other stale or path-mismatched identity refuses. |
| RF-4 | ZRAM and NBD are drained with fresh swap snapshots before reset/detach. | Recorded effect order is ZRAM `swapoff`, NBD `swapoff`, zram reset, NBD detach, daemon stop. |
| RF-5 | Every mutating effect is bound to the observed device or pid identity and revalidated immediately before use. | Retargeted-device and PID-reuse tests stop before the corresponding command. |
| RF-6 | The pre-existing WSL fallback disk swap is observation-only. | The migration never formats, disables, recreates, or detaches a non-managed disk swap. |
| RF-7 | The final state is the existing sealed-origin cascade or a preserved, diagnosable partial state. | On success `up` writes the normal lifecycle binding; any failure retains the evidence needed for safe follow-up. |

## 5. Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-1 | No automatic migration happens during `up`, `down`, status, recovery, daemon start, or boot. |
| NFR-2 | The operation uses bounded commands and a pidfd for the daemon signal; it never uses name-based kill or `kill -9`. |
| NFR-3 | The pure admission and transition plan reach at least 80% line coverage in the declared Rust slice. |
| NFR-4 | Live validation uses the approved Windows/shared-host watchdog harness; direct daily-WSL pressure is not a migration test. |
| NFR-5 | A second invocation after successful migration is a refusal, not a second mutation path. |

## 6. Flows

### Eligible attended handoff

1. The operator invokes `migrate-cascade --from-legacy` as root.
2. RamShared validates the sealed origin, guardian health, strict swap snapshot,
   legacy topology, available-memory drain budget, and unique daemon identity.
3. It binds the exact ZRAM, NBD, and daemon process identities.
4. It drains ZRAM, proves absence, drains NBD, proves absence, then resets and
   detaches the bound devices.
5. It terminates and waits for the exact pidfd-bound legacy daemon.
6. It starts the existing sealed-origin cascade and verifies the resulting
   lifecycle binding and status.

### Refusal

Any ambiguity before the first `swapoff` returns a non-zero exit and performs
no mutation. A post-drain failure stops immediately, preserves migration
evidence, and does not pretend that the old or new cascade is healthy.

## 7. Data / state model

```text
LegacyDetected
  --exact admission + operator flag--> LegacyBound
  --zram swapoff/proven absent--> ZramDrained
  --nbd swapoff/proven absent--> NbdDrained
  --reset + detach + exact daemon exit--> LegacyRetired
  --sealed-origin up + binding--> CurrentBound

Any state --uncertain outcome--> ContainedPartial
```

`LegacyBound` is held only in process memory for this execution. `CurrentBound`
is the normal sealed lifecycle binding. There is no persistent legacy mode.

## 8. Interfaces

```text
ramshared migrate-cascade --from-legacy
```

The command accepts no capacity, device, daemon, force, or transport override.
Those values must come from the sealed current-origin configuration. It prints
the stage and a non-sensitive reason on failure; `status --json` remains the
machine-readable source for the resulting topology.

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| A foreign NBD is mistaken for the legacy tier. | Explicit command, exact cardinality, daemon BINARY_MATCH or narrow replaced-binary listener proof, device identity, zero initial NBD use, and no name-only authorization. |
| ZRAM drain increases RAM pressure. | Require an available-memory budget at least equal to the eligible ZRAM capacity plus the established hard floor before the first effect. |
| A PID is reused before termination. | Capture start ticks, open pidfd, and revalidate executable and process identity before TERM. |
| A swapoff or detach outcome is uncertain. | Fresh strict snapshot and exact device proof; preserve evidence and stop. |
| Host watchdog/control plane is unavailable. | Refuse before mutations; qualification uses the approved watchdog harness. |

Rollback trigger: revert the feature and block migrations if any migration
issues an effect against a foreign/retargeted device, signals a mismatched PID,
or a supervised validation observes a ghost, kernel BUG/Oops/panic/hung task,
or a watchdog termination.

## 10. Implementation strategy

1. Add RED parser/dispatch and pure admission-plan tests.
2. Add a typed legacy plan and an injected executor that proves ordering and
   first-error containment without real swap or device operations.
3. Add runtime identity binding, fresh revalidation, pidfd termination, and
   handoff to the existing sealed-origin `up` implementation.
4. Add the public command/help documentation and regenerate the documentation
   index.
5. Run Rust tests, formatting, Clippy, slice coverage, docs checks, and the
   controlled watchdog campaign. Record environment-bound live evidence as
   partial until the campaign passes.

## 11. Documents to update

- This SSDV3 folder and generated `docs/INDEX.md`.
- `README.md` safe-operation command reference.
- `docs/reliability/DEGRADATION-MATRIX.md` for legacy-handoff containment.
- `validation.md` after the live watchdog campaign.

## 12. Out of scope

- Automatic legacy recovery, boot-time migration, ublk migration, migration of
  non-RamShared devices, editing the WSL fallback disk swap, disk resizing, or
  direct pressure on the daily WSL environment.
- Reintroduction of an unbound cleanup path or a persistent compatibility shim.

## 13. Acceptance criteria

- [ ] Exact CLI parsing and dispatch are covered, including extra-flag refusal.
- [ ] Every admission refusal is paired with a legitimate eligible plan.
- [ ] The executor proves `swapoff` before reset/detach/daemon stop and stops
      on the first uncertain result.
- [ ] Replaced device and daemon identities produce zero later effects.
- [ ] The normal sealed lifecycle binding exists only after successful handoff.
- [ ] Slice coverage, focused tests, formatting, Clippy, and documentation
      checks pass.
- [ ] A supervised before → action → after campaign is recorded; until then
      the IMPL stays partial.

## 14. Validation plan

- `cargo test -p ramshared-cli public_control_commands_parse_exactly`
- `cargo test -p ramshared-cli legacy_migration -- --test-threads=1`
- `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/cascade/mod.rs --min 80`
- `cargo fmt --all -- --check`, targeted Clippy, `./scripts/docs-check.sh`, and
  `git diff --check`.
- Watchdog-only live evidence: baseline status and swaps → attended migration →
  sealed binding/status/health after, plus refusal evidence. No Tier 3 stress
  claim is made by this migration validation.
