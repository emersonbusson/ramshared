# Finding: memory pressure scope
The file `crates/ramshared-block/src/origin_cache.rs` does not receive memory pressure events from the supervisor or monitor PSI metrics. It is designed to act on GPU capacity samples via `observe_gpu` and a programmatic `release_cache` signal. Therefore, the task "emergency cache memory release on system memory pressure signal" is an architectural scope trap.
