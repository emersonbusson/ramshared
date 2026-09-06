# ramshared-wsl2d

Dual-tier block device daemon (`ublk` server + NBD engine), autotiering state machine, and residency canary monitor.

## Scope & Responsibility

`ramshared-wsl2d` produces the primary Linux daemon (`ramsharedd`) managing physical block interfaces in WSL2 and native Linux:
- **`ublk` Block Server:** Implements the Linux user-space block driver (`ublk`) interface for zero-copy I/O.
- **NBD Fallback Server:** Maintains an NBD fixed-newstyle server for environments without custom kernel `ublk` support.
- **Canary Residency Probes:** Continuously executes background canary read probes across allocated VRAM chunks to verify data integrity and detect eviction latency spikes.
- **Telemetry Reconciliation:** Gathers cross-tier I/O counters and reconciles swap telemetry with the supervisor.

## Workspace Dependencies

- [`ramshared-block`](../ramshared-block/README.md) — Authoritative SSD origin and cache.
- [`ramshared-cuda`](../ramshared-cuda/README.md) — VRAM allocation and DMA.
- [`ramshared-uring`](../ramshared-uring/README.md) — `io_uring` block queue submission.
- [`ramshared-dxg`](../ramshared-dxg/README.md) — WDDM memory budget query.

## Safety Invariants

- **Safe Code Only:** `#![forbid(unsafe_code)]` enforced across the entire daemon.
- **Ordered Teardown:** Refuses detachment until devices are confirmed removed from `/proc/swaps`.

## Testing & Execution

```bash
cargo test -p ramshared-wsl2d
```
