# FINDING_ONLY: Watchdog timer drift correction under high CPU starvation

**Objective:** Implement hung thread watchdog detection with automatic recovery signal, or use monotonic hardware timers to prevent false-positive hung thread alerts.

**Finding:**
Treated as an architectural scope trap. `crates/ramshared-agent/src/watchdog.rs` explicitly mandates "Pure time arithmetic, no threads" (SPEC §14.1, DT-9). The file already uses `std::time::Instant`, which provides monotonic hardware timers. Implementing an active hung thread detection with automatic recovery signals or adding threads violates the explicit constraints.
Therefore, no code changes are made.
