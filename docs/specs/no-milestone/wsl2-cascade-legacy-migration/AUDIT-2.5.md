# AUDIT-2.5 — wsl2-cascade-legacy-migration

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Critical | DT-2 | Legacy state has no current sealed lifecycle binding; matching `/dev/nbd0` by name would permit a foreign swap teardown. | Require explicit CLI consent, exact topology, daemon BINARY_MATCH or narrow replaced-binary listener proof, device identity, strict snapshots, and zero initial NBD use. |
| Critical | DT-5/DT-6 | A device can be retargeted or a PID reused between planning and effect. | Bind/revalidate device identity before each effect and signal only a revalidated pidfd. |
| Critical | Atomicity frontier | A failed drain can leave a partial old topology; continuing into `up` could hide the failure. | Stop on first error, retain evidence, and call `up` only after legacy retirement is fully proven. |
| High | DT-3 | Draining ZRAM can move compressed pages into RAM or the NBD fallback. | Require an available-memory budget before the first effect, retain the NBD until ZRAM absence is proven, then drain NBD. |
| High | DT-8 | A compatibility record could become a permanent second authority path. | Keep legacy identity in-memory only; success creates only the existing sealed binding. |
| High | Validation | Source tests cannot prove WSL2 control-plane or host behavior. | Require BINARY_MATCH or the narrow replaced-binary listener proof and the approved watchdog E2E; keep IMPL partial until evidence exists. |

## Open questions

- The watchdog campaign must measure the host guardian's healthy state at the
  moment of migration, not merely confirm that its files exist.
- The exact available-memory hard floor comes from the existing cascade safety
  policy and must be recorded in the live evidence rather than hard-coded in
  documentation.

## Verdict

**go**, conditional on all DT-2 through DT-8 controls and the named refusal
tests. Any attempt to restore zero-used name-only recovery is a hard no-go.
