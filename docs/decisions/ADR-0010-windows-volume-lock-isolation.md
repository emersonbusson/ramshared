# ADR-0010 — Windows volume lock isolation and safe handle lifecycle

## Status

Accepted

## Context

On Windows host systems, mounting, formatting, dismounting, or resizing a virtual StorPort disk volume requires issuing `FSCTL_LOCK_VOLUME` and `FSCTL_DISMOUNT_VOLUME` control codes to prevent third-party processes, antivirus indexers, or background filesystem threads from mutating blocks concurrently.

Historically, raw `HANDLE` objects obtained via `CreateFileW` were passed between functions or unclosed upon early returns, leading to volume lock leaks. A leaked volume lock causes subsequent service restarts to fail with `ERROR_ACCESS_DENIED`, leaving the virtual disk in an inaccessible, orphaned state.

## Decision

1. **RAII Volume Guard (`LockedVolume`):** Introduce an explicit RAII wrapper struct `LockedVolume` within `crates/ramshared-winsvc/src/windows_host.rs` that encapsulates the raw Windows volume `HANDLE`.
2. **Deterministic Unlock on Drop:** `LockedVolume` implements `Drop` to automatically issue `FSCTL_UNLOCK_VOLUME` and close the handle via `CloseHandle` whenever the guard goes out of scope.
3. **Debug Bounds & Unwrap Safety:** Implement `#[derive(Debug)]` on `LockedVolume` and its corresponding error types (`LockedVolumeError`), ensuring that volume lock outcomes can be logged, asserted, and matched without compiler trait requirement failures.
4. **Ordered Dismount Protocol:** Any partition format or volume retirement must acquire exclusive `LockedVolume` ownership before proceeding with disk teardown.

## Consequences

### Positive

- Completely eliminates orphaned volume lock handles on the Windows host.
- Guarantees that even if an intermediate formatting step fails, the volume lock is released immediately without requiring a machine reboot.
- Integrates cleanly with standard Rust pattern matching and error propagation.

### Costs and limitations

- Windows-specific implementation requiring `#[cfg(windows)]` and simulated abstractions (`FakeDriver`) on Linux targets.
- Requires caller code to hold the lifetime of `LockedVolume` across the entire critical section.

## Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Manual `CloseHandle` calls in every exit branch | Highly error-prone; any early return with `?` or unexpected error bypasses cleanup and leaks the lock. |
| Global lock mutex in user-space | Does not protect against external Windows processes (e.g., Windows Search or antivirus) accessing the volume directly. |
| Force dismount without locking (`FSCTL_DISMOUNT_VOLUME` only) | Can cause active filesystem buffers to flush onto unmounted backing stores, resulting in filesystem corruption. |

## Kahneman

- **#15 — structural-before-tactical:** Replace manual handle management with an RAII guard type that makes leaks structurally impossible.
- **#16 — fail-safe default:** The drop handler always attempts to release locks and close handles, failing closed to a safe state.
- **#17 — idempotent effects:** Dropping an already unlocked or invalid handle is handled safely without secondary panics.

## Rollback trigger

Roll back this decision if `LockedVolume` RAII semantics prevent legitimate concurrent read-only volume probes or if `CloseHandle` deadlocks inside the Windows kernel I/O manager during sudden device removal drills.
