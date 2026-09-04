# Architectural Findings and Blocked Traps

This directory contains records of adversarial traps, scope integration violations, and architectural constraints identified and rejected during automated Jules agent task qualification.

## Purpose

These findings serve as an immutable record of architectural invariants in RamShared:
1. **Preserve Layer Abstractions**: Abstract crates (such as `ramshared-vram`) must not be polluted with concrete driver dispatchers.
2. **Lock-Free Concurrency**: Components using channel-based messaging (such as `ramshared-broker`) must not receive artificial mutexes.
3. **Virtualization Boundaries**: Drivers operating under WSL2 / Hyper-V (such as `ramshared-dxg`) cannot perform direct physical BAR register probing.
4. **Scheduler Decoupling**: Stream priorities and thread pools remain dynamic and uncoupled from transport protocol logic.

## Catalog of Findings

| ID | Finding Document | Component | Rationale |
| :---: | :--- | :--- | :--- |
| 01 | [`01-vram-dispatcher-scope-trap.md`](01-vram-dispatcher-scope-trap.md) | `ramshared-vram` | Abstract trait crate has no dispatcher implementation. |
| 02 | [`02-diagnose-procfs-trap.md`](02-diagnose-procfs-trap.md) | `ramshared-cli` | `diagnose.rs` does not contain procfs parsing logic. |
| 03 | [`03-n3-state-transition-trap.md`](03-n3-state-transition-trap.md) | `ramshared-tier` | N3 state machine transitions are explicit and linear. |
| 04 | [`04-broker-lock-free-trap.md`](04-broker-lock-free-trap.md) | `ramshared-broker` | Slices are lock-free and message-driven. |
| 05 | [`05-ipc-framing-trap.md`](05-ipc-framing-trap.md) | `ramshared-broker` | IPC framing is already canonical. |
| 06 | [`06-dxg-capabilities-trap.md`](06-dxg-capabilities-trap.md) | `ramshared-dxg` | Adapter capabilities are not queried synchronously. |
| 07 | [`07-agent-watchdog-trap.md`](07-agent-watchdog-trap.md) | `ramshared-agent` | Watchdog timer already has safe duration calculation. |
| 08 | [`08-swap-tier-guard-trap.md`](08-swap-tier-guard-trap.md) | `ramshared-tier` | Swap tier bounds are verified by host agent. |
| 09 | [`09-config-parser-trap.md`](09-config-parser-trap.md) | `ramshared-config` | TOML deserialization is delegated to serde. |
| 10 | [`10-block-alignment-trap.md`](10-block-alignment-trap.md) | `ramshared-block` | Alignment constraints are enforced by block size. |
| 11 | [`11-cuda-vram-impl-trap.md`](11-cuda-vram-impl-trap.md) | `ramshared-cuda` | Concrete provider belongs in backend crate. |
| 12 | [`12-vulkan-driver-guard-trap.md`](12-vulkan-driver-guard-trap.md) | `ramshared-vulkan` | Instance allocation prerequisites already checked. |
| 13 | [`13-nbd-readiness-socket-trap.md`](13-nbd-readiness-socket-trap.md) | `ramshared-tier` | Probe sockets must remain non-blocking. |
| 14 | [`14-config-worker-threads-trap.md`](14-config-worker-threads-trap.md) | `ramshared-config` | Worker thread limits validated at runtime. |
| 15 | [`15-dxg-bar-aperture-trap.md`](15-dxg-bar-aperture-trap.md) | `ramshared-dxg` | Virtualized WSL2 environment cannot probe physical BAR. |
| 16 | [`16-vram-allocation-trap.md`](16-vram-allocation-trap.md) | `ramshared-vram` | Allocation must follow async trait lifecycle. |
| 17 | [`17-diagnose-pipeline-trap.md`](17-diagnose-pipeline-trap.md) | `ramshared-cli` | Diagnostic pipeline follows structured exit code model. |
| 18 | [`18-broker-lease-ttl-trap.md`](18-broker-lease-ttl-trap.md) | `ramshared-broker` | Leases are session-bound, not TTL-bound. |
| 19 | [`19-cuda-stream-priority-trap.md`](19-cuda-stream-priority-trap.md) | `ramshared-cuda` | Stream priorities are managed by dynamic scheduler. |
| 20 | [`20-broker-arbiter-error-trap.md`](20-broker-arbiter-error-trap.md) | `ramshared-broker` | Arbiter is decoupled from slice leases. |
| 21 | [`21-cuda-error-mapping-trap.md`](21-cuda-error-mapping-trap.md) | `ramshared-cuda` | Driver errors map to strongly typed CudaError enum. |
| 22 | [`22-pr482-winsvc-handle-cleanup.md`](22-pr482-winsvc-handle-cleanup.md) | `ramshared-winsvc` | Windows driver link handle cleanup is managed by RAII Drop. |
| 23 | [`23-pr487-wsl2d-dxgkrnl-anti-bug.md`](23-pr487-wsl2d-dxgkrnl-anti-bug.md) | `ramshared-wsl2d` | WSL2 dxgkrnl anti-bug verified with MCL_CURRENT. |
| 24 | [`24-pr489-broker-cross-host-civm-historical-note.md`](24-pr489-broker-cross-host-civm-historical-note.md) | `ramshared-wsl2d` | Cross-host CIVM historical race note verified. |
| 25 | [`25-pr493-cli-read-frozen-target-error-handling.md`](25-pr493-cli-read-frozen-target-error-handling.md) | `ramshared-cli` | `read_frozen_target` uses structured Result propagation. |
| 26 | [`26-pr497-wsl2d-dxgkrnl-mlockall-invariants.md`](26-pr497-wsl2d-dxgkrnl-mlockall-invariants.md) | `ramshared-wsl2d` | Memory locking invariants protect against dxgkrnl collisions. |
| 27 | [`27-kernel-win-ioctl-trap.md`](27-kernel-win-ioctl-trap.md) | `kernel-windows` | Unrecognized IOCTLs already return STATUS_INVALID_DEVICE_REQUEST. |
