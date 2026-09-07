# Priority Tier Weight Rebalancing Under Asymmetric Tier Pressure

## Objective
Dynamically shift priority weights when GPU VRAM fills up faster than system host RAM.

## Finding
This objective contradicts the strict cascade priority hierarchy enforced by the RamShared architecture.

Phase 0 findings (§9.5) explicitly demonstrated that configuring VRAM as a dynamic or maximum-priority hot swap tier is latency-unsafe under memory pressure. The architecture enforces a static priority cascade: `zram > VRAM > VHDX`. The `zram` tier (compressed RAM) must absorb the hot working set, and `VRAM` must act only as a cold tier for overflows, ensuring system responsiveness. Dynamically adjusting priority weights based on tier fill rates violates this core stability principle.

Therefore, this request is an architectural scope trap. The system must retain its fixed priority weights (`ZRAM_PRIO = 200`, `VRAM_PRIO = 100`, `vhdx = -2`) and reject dynamically shifted weights to avoid degrading host performance.

No code modifications were made. This is a FINDING_ONLY report.
