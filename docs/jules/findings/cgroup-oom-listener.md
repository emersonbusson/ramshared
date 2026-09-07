# FINDING_ONLY: cgroup OOM listener

## Objective
Implement cgroup memory.max threshold alignment and OOM notification listener in `crates/ramshared-agent/src/swap.rs` to preemptively shed swap on `oom_kill` and `oom_group` events.

## Finding
This objective is an architectural scope trap. The agent acts solely as an executor for memory broker commands via NBD and does not govern priority or measure host thresholds. Proactive teardown based on host memory remaining or OOM events should not be implemented in `crates/ramshared-agent/src/swap.rs`.
