# IMPL — WSL2 Relay lifecycle reliability

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/wsl2-relay-lifecycle-reliability/SPEC.md`

## Status

partial · cover N/A — E2E-only · E2E partial · BINARY_MATCH N/A

The exact classifier and attended cleanup surface are implemented and their
manufactured legitimate/refusal paths pass. Discovery cleanup preceded the
final operator script, so it is retained as useful live evidence but does not
qualify the SPEC's script-level action E2E.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `scripts/safety/wsl-relay-health.sh` | ITEM-1..3 · RF-1..5 | Add read-only classification and an explicit sealed, bounded, identity-revalidated cleanup mode. |
| `scripts/safety/test-wsl-relay-health.sh` | ITEM-4 · RF-1..5 | Add manufactured proc-tree, PID-reuse, cardinality, signal, deadline, and privacy tests. |
| `docs/specs/no-milestone/wsl2-relay-lifecycle-reliability/PRD.md` | Step 1 · RF-1..6 | Separate the upstream defect from RamShared product ownership. |
| `docs/specs/no-milestone/wsl2-relay-lifecycle-reliability/SPEC.md` | Step 2 · RF-1..6 | Freeze exact classification, cleanup, evidence, and release-adoption decisions. |

## Validation (numbers)

- TDD RED: `./scripts/safety/test-wsl-relay-health.sh` → exit 1,
  `production_script_present ... is missing or not executable`.
- Parser: `bash -n scripts/safety/wsl-relay-health.sh` and
  `bash -n scripts/safety/test-wsl-relay-health.sh` → exit 0.
- Manufactured tests: `./scripts/safety/test-wsl-relay-health.sh` → exit 0;
  14 named PASS markers plus final aggregate PASS. The suite covers a clean
  fixture, exact orphan, four non-actionable states, malformed proc data,
  expected cardinality, sealed TERM/KILL targets, a bounded failed cleanup,
  start-tick changes before TERM and KILL, the 128-candidate cap, and
  deterministic sanitized output.
- Cover: N/A — E2E-only shell orchestration per SPEC. Manufactured guard tests
  and live operator evidence are mandatory instead of Rust line coverage.
- Live before: WSL 2.7.11 with custom kernel 6.18.35.2; 12 exact candidates,
  ages 76–192 hours; 112 matching diagnostics in the preceding 10 minutes;
  interop, `/dev/dxg`, `/dev/ublk-control`, NVIDIA GPU, and Docker were usable.
- Live discovery action: every candidate was revalidated as `comm=Relay`,
  exact `/init`, PPID 1, no children, no interop socket, and older than 600
  seconds. All 12 ignored the bounded TERM phase; the same 12 identities were
  revalidated and removed with KILL. No WSL shutdown or Windows reboot ran.
- Live after: zero bare Relays; interop PASS; NVIDIA and Docker probes PASS;
  zero matching diagnostic increments after 12:15 local time for more than
  the required 130-second quiet window.
- Current script check: `./scripts/safety/wsl-relay-health.sh --check` → exit 0,
  `candidate_count=0`, `verdict=CLEAN`.
- Sanitized discovery artifacts:
  `tmp/wsl2-relay-lifecycle-reliability-e2e/after-check.json`
  (`30082e8974dd412b2d3c631238e26c6ba7022150c8d2b03277c50b2c049159b6`),
  `discovery-summary.json`
  (`0ad0fc490c458e1948fcc5a5e67ac2a11d1366234a2ab5c499af104c79ceec2e`),
  and `upstream-adoption.json`
  (`7da6211cbf5932c0f8ed624b37b6e2c91c92ef53c43eddfe830686c9054e8b3f`).
- Upstream: Microsoft merged `microsoft/WSL#41252` after the installed WSL
  2.7.11 release. Runtime evidence confirms the installed release does not yet
  provide the required lifecycle behavior on this host.

## Gaps

- open: if an exact candidate recurs before the fixed WSL release is installed,
  run the final script-level before/action/after command and preserve its
  sanitized JSON; do not manufacture a live orphan merely to close E2E.
- env-bound: install a future released WSL build containing the upstream merge,
  then observe seven normal-use days with zero candidates and zero matching
  diagnostic increments.
- open: integrate the read-only `--check` into the broader health evidence
  surface only after the current CI and documentation gates stabilize. Do not
  schedule `--reap`.

## Rollback trigger

Disable `--reap` and retain only read-only `--check` if a signal target is not
in the sealed snapshot, start identity changes, interop/GPU/service state
regresses, cleanup exceeds 20 seconds, or output exposes non-schema data.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1, RF-2 | ITEM-1 | pending supervised commit |
| RF-3 | ITEM-2 | pending supervised commit |
| RF-4 | ITEM-3 | pending supervised commit |
| RF-5 | ITEM-4 | pending script-level recurrence E2E and supervised commit |
| RF-6 | ITEM-5 | pending released-upstream adoption |
