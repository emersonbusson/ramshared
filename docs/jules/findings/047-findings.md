# FINDING_ONLY: fine-grained locking in slice manager to reduce thread contention

## Issue
The task requested to "Replace global broker mutex with partitioned concurrent slice map to avoid thread contention under heavy load" in `crates/ramshared-broker/src/slices.rs`.

## Finding
`ramshared-broker` is a pure domain logic library. The file `crates/ramshared-broker/src/slices.rs` states: `/// Map of VRAM slices (sole owner of truth about the state; no locks — ITEM-8 is single-threaded).` The `SliceMap` does not implement locks nor contains threading primitives like `Mutex` or `RwLock`.

According to the Memory instructions:
* "The `crates/ramshared-broker/src/lib.rs` file acts only as a pure library module root. It does not contain the core broker loop logic; the actual plumbing and broker loop reside in the `ramsharedd` daemon (crate `ramshared-wsl2d`)."
* "When an adversarial audit task requests code changes that are architecturally impossible or unsafe within the strictly confined scope (e.g., adding locks to a pure logic file), produce a FINDING_ONLY markdown report with evidence in the docs/jules/findings/ directory instead of modifying code."

I confirmed the entire file up to line 212 does not have any `Mutex` and explicitly documents the lack of locks and threads in this logic.

## Conclusion
It is architecturally invalid to add locks or thread partitions to `crates/ramshared-broker/src/slices.rs`, since it is purely a domain logic model. I am producing this FINDING_ONLY report to document the impossibility of implementing this task.
