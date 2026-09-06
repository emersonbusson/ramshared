# ramshared-tier

Multi-tier memory cascade orchestration, priority ordering, and demotion safety nets.

## Scope & Responsibility

`ramshared-tier` encodes the mathematical and logical rules governing the multi-tier swap cascade:
- **Priority Invariants:** Enforces strict `swapon` priority order:
  $$\text{ZRAM (Tier 1, prio 200)} > \text{VRAM (Tier 2, prio 100)} > \text{SSD Origin / Disk (Tier 3, prio } < 100\text{)}$$
- **N3 State Machine:** Manages transitions across `Armed`, `UsingZram`, `UsingVram`, `UsingDisk`, `Demoting`, and `Degraded` states.
- **DEMOTE Safety Net (Invariant A1):** Verifies that lower tiers have sufficient free capacity before allowing VRAM pages to be demoted, preventing out-of-memory cascading stalls.

## Workspace Dependencies

- Pure logic crate; zero workspace crate dependencies.

## Safety Invariants

- **Safe Code Only:** `#![forbid(unsafe_code)]` strictly enforced.
- **Fail-Closed Transitions:** Returns typed [`OrderError`](src/priority.rs) whenever swap configuration violates priority hierarchy.

## Testing

```bash
cargo test -p ramshared-tier
```
