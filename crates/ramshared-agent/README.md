# ramshared-agent

In-guest background service and telemetry monitor for the RamShared multi-tier memory cascade.

## Scope & Responsibility

`ramshared-agent` runs within the virtualized guest environment (Linux / WSL2) to oversee active swap interfaces, monitor kernel `/proc/swaps` tables, track PSI (Pressure Stall Information), and enforce fail-closed teardown invariants.

Key capabilities:
- **Swap Lifecycle Management:** Coordinates clean, ordered `swapoff` execution prior to device detachment to prevent kernel page-fault deadlocks.
- **Watchdog Supervision:** Continuous liveness probes and heartbeat reporting to the memory broker.
- **Fail-Closed Teardown:** Immediate refusal of detach sequences while NBD or ublk devices are registered in the active swap hierarchy.

## Workspace Dependencies

- [`ramshared-broker`](../ramshared-broker/README.md) — Lease arbitration and client-broker handshake protocol.

## Safety Invariants

- **Zero Unchecked Panics:** Strictly compiles under `#![deny(clippy::unwrap_used, clippy::expect_used)]`.
- **Swapoff-First:** Never stops block service daemons while blocks are active in `/proc/swaps`.

## Testing

```bash
cargo test -p ramshared-agent
```
