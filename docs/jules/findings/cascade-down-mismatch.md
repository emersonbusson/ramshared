# FINDING_ONLY: cascade-down swap drain guard

## Issue
Task requested adding a swap draining guard check before deactivation to ensure no processes are pinning swap pages in `scripts/safety/cascade-down.sh`.

## Findings
`scripts/safety/cascade-down.sh` does not directly perform the swapoff action; it only signals the `$CLI down` process (`exec "$CLI" down`).
The `swapoff` action and the handling of pages is delegated to the daemon lifecycle or other orchestration layers.
Adding an active swap check here is an architectural mismatch because the Rust lifecycle owns swapoff-first as stated in the script's header.

## Action
No code changes were made to `scripts/safety/cascade-down.sh` to avoid hallucinating functionality not present in the current script scope.
