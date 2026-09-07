# Finding Report: WSL2d System Freeze Signal Handler

**Task Title:** system freeze signal handler for synchronous write cache flush
**Pillar:** power-lifecycle
**Target File:** crates/ramshared-wsl2d/src/residency.rs

## Analysis

The task requests the implementation of a system freeze / suspend signal handler to intercept host suspend events and synchronously flush all volatile write caches to a non-volatile store in `crates/ramshared-wsl2d/src/residency.rs`.

However, upon inspecting `crates/ramshared-wsl2d/src/residency.rs`, the file is exclusively dedicated to "Canary-based detection of WDDM eviction". It implements pure, stateless data-plane logic (`Canary`, `ResidencySampler`) for feeding sampling data (latency, free memory) and determining if WDDM eviction is occurring, resulting in a `Verdict::Demote`.

Crucially:
- The module states: "**Pure decision**: fed with samples... decides DEMOTE".
- It contains no I/O, no system signal handlers, no OS-level interop for suspend/resume signals, and no cache structures to flush.
- Adding signal handling and synchronous disk I/O logic in this pure-logic sampling module fundamentally violates the architectural separation of concerns.

## Conclusion

Implementing the requested suspend signal handler for cache flushing inside the `residency.rs` pure sampling logic is an architectural scope trap. This module should not manage OS power lifecycle events or perform I/O operations. Thus, I am documenting this finding without writing code.
