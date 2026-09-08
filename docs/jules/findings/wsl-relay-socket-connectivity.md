# FINDING_ONLY: WSL Relay Socket Connectivity Validation

## Assessment
The request to "Validate WSL relay daemon PID exists and IPC socket accepts connections before health reporting" in `scripts/safety/wsl-relay-health.sh` cannot be implemented safely.

## Evidence of Unsafe Condition
1. **Violation of SPEC Closed Scope**: The governing specification (`docs/specs/no-milestone/wsl2-relay-lifecycle-reliability/SPEC.md`) explicitly states in its "Closed scope" section: "Implement only the exact classifier, bounded attended cleanup, tests, and evidence contract in this document." The allowed classifier checks only read filesystem metadata (`comm`, `cmdline`, `status`, `task/<pid>/children`, `stat`), strictly prohibiting stateful IPC socket connections.
2. **Stateful/Blocking Risk**: Connecting to an IPC socket (e.g., via `nc -U` or `socat`) is a stateful operation. Given that this script targets stranded/hung WSL relay processes (affected by WSL#41242), attempting a socket connection to a hung daemon could cause the script to block indefinitely, violating the "Observation must be safe for the daily WSL host" design decision (DT-1).
3. **Orphan Predicate Conflict**: The script specifically targets orphaned processes. A core predicate of an orphaned candidate in `classify_pid()` is the *absence* of an interop socket (`[[ ! -e "$RUN_ROOT/${pid}_interop" ]] || return 1`). Validating socket connections is incompatible with finding processes that have already lost their sockets.

## Conclusion
To uphold the IMMUTABLE CONTRACT rule 4 (implement only the smallest safe orthogonal slice) and rule 1 (adhere to README/SPEC constraints), the unsafe modification is aborted.
