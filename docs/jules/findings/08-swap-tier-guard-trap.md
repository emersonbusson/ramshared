# FINDING_ONLY: Guard Clause / Swap Reallocation Traps

`crates/ramshared-agent/src/swap.rs` is strictly a pure wrapper for NBD and swap shell commands (`nbd-client`, `mkswap`, `swapon`, `swapoff`).
It does not contain logic for PSI telemetry, memory pressure handling, or swap reallocation.

Instructions to implement or flatten PSI pressure response or swap reallocation logic within this specific file represent an adversarial trap.
Implementing this would violate its architectural boundary as a pure shell wrapper and introduce untestable PSI logic. Therefore, this file remains untouched.
