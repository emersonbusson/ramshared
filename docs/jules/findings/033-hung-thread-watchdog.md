# Finding: Hung Thread Watchdog Detection

## Scope Trap Analysis
The task requests the implementation of "hung thread watchdog detection with automatic recovery signal" in `crates/ramshared-agent/src/watchdog.rs` to monitor worker thread heartbeats and trigger thread termination and restart if heartbeat stalls > 10 seconds.

However, `crates/ramshared-agent/src/watchdog.rs` is architecturally defined as:
`//! Watchdog timer: checks whether the supervisor or daemon has signaled heartbeat within the deadline.`
`//! SPEC §14.1, DT-9. Pure time arithmetic, no threads.`

Adding thread monitoring, termination, and restart logic into this module directly violates the architectural invariant of "Pure time arithmetic, no threads." The watchdog is designed to remain a pure state machine/timer, and any thread lifecycle management must be handled by the supervisor or main execution loop, not within the pure watchdog component itself.

Therefore, this request is an architectural scope trap, and no code changes should be made to `watchdog.rs`.
