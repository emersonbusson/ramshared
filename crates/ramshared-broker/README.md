# ramshared-broker

Memory broker protocol, arbitration state machine, and multi-tenant lease manager.

## Scope & Responsibility

`ramshared-broker` provides pure, testable arbitration logic for sharing hardware memory resources among competing tenants:
- **JSON-Lines Protocol:** Encodes and decodes tenant lease requests, heartbeats, allocations, and releases.
- **Slice Map Arbiter:** Injected-clock arbiter assigning discrete 64 MiB/128 MiB memory slices according to priority and fair-share headroom.
- **Multi-Tenant Safety:** Prevents double-allocation and enforces lease timeouts when clients fail to refresh liveness tokens.

## Workspace Dependencies

- Pure protocol library; zero workspace crate dependencies.

## Safety Invariants

- **Safe Code Only:** `#![forbid(unsafe_code)]` strictly enforced.
- **Deterministic Arbitration:** Pure state transitions decoupled from sockets, timers, or GPU hardware.

## Testing

```bash
cargo test -p ramshared-broker
```
