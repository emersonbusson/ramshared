# ADR-0003 — Page-state model: inherit Linux swap; the daemon guarantees durability, DEMOTE, and atomicity

**Status:** Accepted (2026-06-05).

## Context

VRAM is a swap tier (ADR-0001). Every memory platform has a page-state
machine — on Windows: `In use → Modified (dirty, must be written before reuse)
→ Standby (clean cache) → Free`. In this case, the failure modes exposed by
that machine are:

- **Dirty-page loss:** a "modified" frame reused before being written = lost
  data.
- **Freeze:** when the Windows host needs VRAM, the contents of our allocation
  are "modified" and WDDM **writes them to the Windows pagefile** before reuse
  — this is the **1.18 s** spike measured in Phase 0.
- **Torn read:** reading a block while a write to that same block is in flight.

Question: should the daemon reimplement this state machine?

## Decision

**Do not reimplement it.** Inherit the page-state machine of the **Linux swap
subsystem** (dirty → writeback → swap-in); we only provide the tiers
(zram→VRAM→VHDX). The daemon guarantees **three invariants** for the VRAM tier:

1. **Durability before ACK (§8):** complete block-layer I/O **only** after
   data is durable in VRAM. The kernel releases the dirty page's RAM frame only
   after that.
2. **Latency-driven DEMOTE (§9):** under WDDM eviction, `swapoff` only the
   VRAM tier (pages migrate to VHDX) **without killing processes**.
3. **Per-block atomicity for in-flight operations (§8.1):** a request to a
   block with an operation in flight on that same block serializes behind it —
   no torn read.

Also: run `swapoff` **before** disconnecting NBD (disconnecting with live swap
pages = panic).

## Consequences

- (+) Does not duplicate what the kernel already does; safety is concentrated
  in three testable invariants.
- (+) Each invariant maps to a real failure mode (not a hypothetical
  scenario).
- (−) Synchronous durability (`cuMemcpy*_v2`) costs latency per operation —
  accepted because VRAM is a **cold** tier (ADR-0001), not hot swap.

## Alternatives considered

- **Reimplement modified/standby in userspace:** rejected — the kernel already
  does it; it would be bug surface and noise.
- **Trust that VRAM is never evicted:** rejected — Phase 0 measured the
  eviction (1.18 s).

## Kahneman

- #5 worst-case (measured eviction) · #13 illusion of validity (durability and
  atomicity require an integration test of the real failure mode, not a mock).

## Rollback trigger

Block the VRAM tier if an integration test shows **(a)** a WRITE already ACKed
that does not survive a later swap-in (dirty-page loss), **or** **(b)** a torn
read at QD>1 on the same block.

Links: [ADR-0001](ADR-0001-vram-cascade-tiering.md) ·
[`../specs/no-milestone/wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §8, §9 ·
[`../reliability/wsl2-fase0-final.md`](../reliability/wsl2-fase0-final.md) ·
[`../reliability/DEGRADATION-MATRIX.md`](../reliability/DEGRADATION-MATRIX.md).
