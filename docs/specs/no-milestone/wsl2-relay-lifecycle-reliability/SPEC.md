# SPEC — WSL2 Relay lifecycle reliability

> SSDV3 Step 2 · PRD:
> `docs/specs/no-milestone/wsl2-relay-lifecycle-reliability/PRD.md`

## Closed scope

Implement only the exact classifier, bounded attended cleanup, tests, and
evidence contract in this document. The tool is a RamShared safety surface for
an upstream WSL defect; it is not a WSL replacement.

## Traceability

| PRD | ITEM |
| --- | --- |
| RF-1, RF-2, NFR-1..3 | ITEM-1 — pure classifier and JSON contract |
| RF-3, NFR-4 | ITEM-2 — sealed bounded signal plan |
| RF-4 | ITEM-3 — sanitized observability |
| RF-5, NFR-5..6 | ITEM-4 — manufactured and live evidence |
| RF-6 | ITEM-5 — upstream release adoption |

## Technical decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-1 | `--check` is the default and never signals. | Observation must be safe for the daily WSL host. |
| DT-2 | `--reap` operates on one sealed PID/start-tick snapshot. | A second discovery pass could signal a different process after PID reuse. |
| DT-3 | Every `/proc` field is re-read before TERM and before KILL. | Process name alone is insufficient proof of ownership. |
| DT-4 | Age uses kernel start ticks, uptime, and `CLK_TCK`. | Filesystem metadata on `/proc/<pid>` is not process age. |
| DT-5 | Parent 1 or 2 is accepted only with all other orphan predicates. | WSL init topology differs between systemd and non-systemd distros. |
| DT-6 | TERM grace is 10 seconds and the total action deadline is 20 seconds. | The known stranded state ignores TERM; waiting indefinitely is unsafe. |
| DT-7 | Candidate cap is 128. | An unexpectedly large population indicates a different failure class. |
| DT-8 | No automatic `--reap` unit is shipped. | Killing WSL internals requires an attended operator decision. |
| DT-9 | Upstream closure requires a released build plus seven clean days. | A merge commit or installed version string alone is not runtime proof. |

## Files

| Path | Action | Cover |
| --- | --- | --- |
| `scripts/safety/wsl-relay-health.sh` | create | N/A — E2E-only shell orchestration |
| `scripts/safety/test-wsl-relay-health.sh` | create | N/A — manufactured harness |
| `docs/specs/no-milestone/wsl2-relay-lifecycle-reliability/IMPL.md` | create after gates | N/A |
| `validation.md` | append after live evidence | N/A |
| `docs/INDEX.md` | regenerate | N/A |

## ITEM-1 — classifier

The script accepts injectable read-only seams for manufactured tests:

- `WSL_RELAY_PROC_ROOT`, default `/proc`;
- `WSL_RELAY_RUN_ROOT`, default `/run/WSL`;
- `WSL_RELAY_UPTIME_FILE`, default `/proc/uptime`;
- `WSL_RELAY_CLK_TCK`, default from `getconf CLK_TCK`.

Overrides are permitted only when `WSL_RELAY_TEST_MODE=1`; production mode
refuses them. The classifier reads `comm`, NUL-terminated `cmdline`, `status`,
`task/<pid>/children`, and field 22 of `stat`. Any missing or malformed input
is a refusal, not a skipped process.

Exit contract:

| Result | Exit |
| --- | --- |
| clean, zero candidates | 0 |
| valid candidates require attention | 1 |
| usage, malformed state, ambiguity, or internal failure | 2 |
| attended cleanup completed and postcondition is clean | 0 |
| attended cleanup failed or postcondition is not clean | 1 or 2 by class |

## ITEM-2 — bounded cleanup

`--reap` requires an exact `--expect-count N` supplied by the operator. The
observed cardinality must equal `N` before any signal. `N=0`, negative values,
non-integers, and values above 128 are refused.

The signal command is injectable only in test mode. Production uses `kill`
against explicit numeric PIDs. Signal delivery is never expressed through a
name, pattern, process group, wildcard, or command substitution after the
candidate set is sealed.

After TERM, wait in bounded one-second increments for at most 10 seconds.
Before KILL, revalidate each surviving PID and its original start ticks. After
KILL, allow at most 5 seconds for disappearance, then rerun the classifier.

## ITEM-3 — output

Emit exactly one JSON object to stdout. Diagnostics go to stderr and contain
reason codes only. Candidate arrays are sorted numerically. Output is
deterministic for an injected fixture clock.

## ITEM-4 — evidence

### Required tests

| TestName | Kind | Pass condition |
| --- | --- | --- |
| `clean_fixture_returns_zero` | manufactured legitimate | Zero candidates, exit 0, no signal calls. |
| `exact_orphan_fixture_requires_attention` | manufactured legitimate | Exact seven predicates produce one candidate and exit 1. |
| `live_parent_is_refused` | manufactured refusal | Parent outside 1/2 is not actionable. |
| `child_process_is_refused` | manufactured refusal | A nonempty children file blocks classification. |
| `interop_socket_is_refused` | manufactured refusal | Existing `<pid>_interop` blocks classification. |
| `young_process_is_refused` | manufactured refusal | Age below 600 seconds blocks classification. |
| `malformed_proc_state_fails_closed` | manufactured refusal | Missing/malformed fields return exit 2. |
| `pid_reuse_before_term_is_refused` | manufactured refusal | Changed start ticks cause zero signals and exit 2. |
| `pid_reuse_before_kill_is_refused` | manufactured refusal | Changed survivor identity prevents KILL. |
| `candidate_cap_is_enforced` | manufactured refusal | More than 128 exact candidates returns exit 2 without signals. |
| `reap_requires_exact_expected_count` | manufactured refusal | Cardinality mismatch returns exit 2 without signals. |
| `reap_signals_only_sealed_numeric_pids` | manufactured legitimate | TERM/KILL call log equals the original exact set. |
| `reap_deadline_is_bounded` | manufactured hang | TERM and final disappearance waits cannot exceed 15 seconds. |
| `output_is_sanitized_and_deterministic` | manufactured privacy | Output has only the schema fields and stable ordering. |
| `live_check_before_action_after` | live E2E | Baseline → explicit reap → zero candidates; interop remains usable. |
| `live_matching_diagnostic_quiet_window` | live E2E | No new matching diagnostic for at least two historical emission periods (130 seconds). |

### Live evidence protocol

1. **Before:** WSL version, kernel release, boot identity hash, classifier JSON,
   matching diagnostic count for the preceding 10 minutes, and read-only
   interop/GPU/product state.
2. **Action:** `sudo ./scripts/safety/wsl-relay-health.sh --reap
   --expect-count <observed>`.
3. **After:** zero candidates, command-interoperability probe succeeds, existing
   service/GPU probes retain their pre-action status, and the 130-second quiet
   window records zero new matching diagnostics.
4. **Refusals:** run at least cardinality mismatch and one live-process fixture;
   neither may send a signal.
5. **Artifacts:** store sanitized JSON under
   `tmp/wsl2-relay-lifecycle-reliability-e2e/` or the SPEC evidence directory.

The manual cleanup already performed before this SPEC is discovery evidence.
It cannot qualify `live_check_before_action_after` because it did not execute
the final operator script.

## ITEM-5 — upstream adoption

Record the public WSL release tag and source revision. Verify ancestry of merge
commit `66da35b80e6a8fa2ccec29c3918cf94b3aa9a1e1` or source commit
`56e0d6ac04542d7513e932b7ab53495593681a99`. If neither identity is present,
the local guard remains active.

After installation of a released build containing the fix, `--check` remains
observation-only for seven normal-use days. Any candidate or matching
diagnostic resets the observation window and keeps the slice partial.

## Validation commands

```bash
bash -n scripts/safety/wsl-relay-health.sh
bash -n scripts/safety/test-wsl-relay-health.sh
./scripts/safety/test-wsl-relay-health.sh
./scripts/safety/wsl-relay-health.sh --check
./scripts/docs-check.sh
```

Rust slice coverage is `N/A — E2E-only` because the production surface is
shell orchestration. Manufactured fixture coverage and live E2E remain
mandatory.

## Rollback trigger

Disable `--reap` if any signal target is not in the sealed snapshot, any start
identity changes, any post-action interop/service/GPU probe regresses, action
duration exceeds 20 seconds, or output exposes non-schema data. Retain
read-only `--check` and record a partial/red validation entry.

## Out of SPEC

- WSL source modifications or duplicate upstream pull requests.
- Automatic cleanup, WSL/Docker restart, WSL shutdown, Windows reboot, or
  Modern Standby changes.
- RamShared swap, daemon, cascade, GPU allocation, Windows driver, or
  performance changes.
- A claim that this tool fixes new-session transport failure or every WSL hang.
