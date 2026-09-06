# ramshared-cli

Command-line frontend, interactive terminal dashboard, and system lifecycle supervisor.

## Scope & Responsibility

`ramshared-cli` produces the primary binary (`ramshared`) used by operators and automated agents:
- **Diagnostic Probes (`doctor`):** Inspects host virtualization, NVIDIA GPU driver versions, CUDA toolkits, and kernel swap readiness.
- **Multi-Tier Stress Governor (`stress`):** Generates closed-loop synthetic memory thrash across ZRAM (Tier 1), VRAM (Tier 2), and SSD Origin (Tier 3) with strict thread limits and PSI safety valves.
- **Interactive TUI Dashboard (`top`):** High-refresh terminal user interface powered by `ratatui` for real-time cascade telemetry.
- **Cascade Lifecycle Controls (`cascade`):** Provides safe `up`, `down`, `demote`, and status controls with fail-closed swapoff-first enforcement.

## Workspace Dependencies

- [`ramshared-tier`](../ramshared-tier/README.md) — Multi-tier cascade state machine and priority ordering.
- [`ramshared-cuda`](../ramshared-cuda/README.md) — Direct CUDA driver queries and allocation probes.

## Safety Invariants

- **Zero Panic Policy:** `#![deny(clippy::unwrap_used, clippy::expect_used)]` enforced.
- **PSI Protection Valve:** Halts workload allocation immediately if Linux pressure stall metrics exceed safe operational ceilings.

## Testing & Execution

```bash
cargo test -p ramshared-cli
cargo run --bin ramshared -- doctor
```
