# ADR-0007 — Kernel-native RamShared work uses C (mainline style), not app Rust

- **Status:** Accepted  
- **Date:** 2026-07-10  
- **Audit:** [`docs/specs/no-milestone/kernel-native-language/AUDIT-2.5.md`](../specs/no-milestone/kernel-native-language/AUDIT-2.5.md)  
- **Scope PRD:** [`docs/specs/no-milestone/kernel-native-language/PRD.md`](../specs/no-milestone/kernel-native-language/PRD.md)  
- **Related:** ADR-0002 (Rust userspace), `wsl2-native-vram-tier`, `mainline-vram-tiering`, `.claude/rules/kernel.md`

## Context

RamShared’s **shippable** path is **userspace Rust** (cascade: zram → VRAM via CUDA/NBD → disk + DEMOTE) on WSL2.

Separately, the project studies **“native for real in the kernel”**: pages / memory tiers / HMM-style device memory, custom WSL kernel pieces, or mainline. That work must not be written as if it were a Rust app (`ramshared-cli` style), and must not confuse **Rust for Linux** (kernel crate rules) with **userspace Rust**.

Phase 0 evidence (WDDM ~1.18 s latency under reclaim) already forces **cold-tier** design; language choice does not remove that constraint.

## Decision

1. **Any code that runs in Linux kernel context** for RamShared “native” work (LKM, mm hooks, sysfs/debugfs, ublk glue, patches intended for mainline or custom WSL `bzImage`) is written in **C11, Linux kernel coding style**, checkpatch-clean, with kernel idioms (`goto out_err`, documented lock order, no casual `printk`).

2. **Userspace product** (CLI, daemon, CUDA loader boundary, broker) remains **Rust**, per ADR-0002.

3. **Rust for Linux (`rust/` / kernel crates)** is an **exception path**, not the default:
   - only for **new**, bounded modules where RfL is accepted by the target tree;
   - not a rewrite of core mm;
   - not a substitute for understanding C call sites and lockdep;
   - requires explicit SPEC + AUDIT go.

4. **Lab automation** stays **bash / PowerShell**; never elevated to “kernel native implementation language.”

5. Claims of “native VRAM in the kernel” still require the **hardware/test matrix** in `wsl2-native-vram-tier` (WSL GPU-PV ≠ bare-metal device memory). Language choice does not bypass Gate A/B.

## Consequences

### Positive

- Aligns with mainline review culture and existing RamShared kernel rules.  
- Clear split: **Rust = P0 product**, **C = P1/P2/P3 kernel**.  
- Avoids false confidence from “we rewrote it in Rust” without mm correctness.

### Negative / costs

- Contributors must know kernel C for native work.  
- Two languages in the monorepo (already true: Rust + Windows C).  
- RfL exception needs discipline so it does not become a dual default.

## Alternatives considered

| Alternative | Why not default |
| --- | --- |
| Userspace Rust only forever | Fine for **product**; does not satisfy “native in kernel” research |
| Rust for Linux as default for all native work | Ecosystem/review friction; mm hot path still C-dominated; WSL custom kernel RfL support varies |
| C userspace rewrite of cascade | Rejected by ADR-0002; no gain for P0 |
| Python/Go “kernel module” frameworks | Not Linux mainline model; Day-0 reject |

## Rollback trigger

If a **mainline** or **WSL kernel** maintainer path **requires** RfL for a specific interface we must use, open a SPEC amendment and narrow RfL to that interface only — do **not** silently flip the whole native stack to Rust.

If C kernel prototypes show **no** path under GPU-PV after Gate B measurements, **sunset** kernel-native effort for WSL and keep P0 Rust cascade (Kahneman #18 with evidence).

## Links

- Product cascade: ADR-0001, `wsl2-cascade-swap`  
- Userspace Rust: ADR-0002  
- Phases / test matrix: `docs/specs/no-milestone/wsl2-native-vram-tier/PRD.md` §3, §8  
- Mainline arc: `docs/specs/no-milestone/mainline-vram-tiering/PRD.md`  
- Kernel rules: `.claude/rules/kernel.md`, `.claude/rules/coding.md`
