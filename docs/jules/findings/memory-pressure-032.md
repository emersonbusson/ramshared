# Finding: Safe zram instance teardown before kernel OOM killer triggers

The issue requested to "Proactively detach and swapoff lowest-priority zram swapfiles when host free memory drops below 5% threshold" in `crates/ramshared-agent/src/swap.rs`.

However, the logic for `ramshared-agent` swap operations operates solely as an executor (`SwapOn`/`SwapOff`) responding to the broker's commands via NBD. The agent itself does not evaluate host free memory or determine priority thresholds. ZRAM is also managed outside this crate (e.g. by cascade scripts/lifecycle). Any proactive teardown logic based on memory pressure would belong in the broker or cascade orchestrator, not in the execution boundary of `ramshared-agent/src/swap.rs`. Therefore, modifying `swap.rs` to implement this directly would be an architectural trap and false positive.
