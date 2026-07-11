---
slug: cascade-transport-policy
title: "Cascade transport policy — NBD Day-1 on WSL2; ublk prefer off-WSL2 only"
milestone: —
issues: []
---

# PRD — Cascade transport + boot (priority VRAM before SSD)

> **Status: GO** for product behaviour already designed; **NO-GO** for ublk as Day-1 VRAM transport **on WSL2**.  
> Kahneman **#16** (fail-safe): ublk teardown on WSL2 can **freeze** the host (incident 2026-06-09, `guard_not_wsl2` in `ramsharedd`).

## 1. Summary

User goal when opening WSL2:

1. Cascade **already on** if configured (boot).  
2. Under memory pressure, **VRAM tier before SSD** (faster cold tier than disk).  
3. If VRAM tier fills / demotes, **then** disk (VHDX).

This is **already** the cascade architecture (`zram prio 200 > VRAM prio 100 > VHDX −2`).  
Transport for VRAM tier on WSL2 remains **NBD** (Day-1). **ublk** is available in the custom kernel but **not** product-on by default on WSL2.

## 2. Technical context

| Fact | Class |
| --- | --- |
| Priorities in `ramshared_tier` / `up` | Confirmed codebase |
| `ramsharedd --transport ublk` exists | Confirmed codebase |
| `guard_not_wsl2()` blocks ublk on WSL2 unless `RAMSHARED_ALLOW_UBLK_ON_WSL2=1` | Confirmed codebase |
| Freeze risk on bad ublk teardown | Confirmed docs / incident |
| Custom kernel has UBLK=m + modules.vhdx live | Confirmed environment |
| Boot unit `install-cascade-boot.sh --enable` | Confirmed codebase |

## 3. Recommended option

| Option | Verdict |
| --- | --- |
| Product VRAM tier = **NBD** on WSL2 | **GO** |
| `transport=auto` → NBD on WSL2, log why | **GO** |
| Wire full ublk into `ramshared up` on WSL2 now | **NO-GO** without new AUDIT-2.5 proving teardown |
| Enable cascade on boot (systemd) | **GO** |
| Prefer ublk on bare-metal/non-WSL when control present | **Future** (SPEC when implementing) |

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-T1 | Default cascade order always zram > VRAM > disk (swapon priorities) |
| RF-T2 | Boot: opt-in unit starts cascade after preflight |
| RF-T3 | `up --transport auto` (default): WSL2 → nbd; never silent ublk on WSL2 |
| RF-T4 | Explicit `--transport ublk` on WSL2 fails closed with clear message (no freeze) |
| RF-T5 | `up` idempotent if cascade already healthy |

## 5. Out of scope

- Lifting `guard_not_wsl2` without dedicated AUDIT-2.5 + drill  
- HMM / VRAM-as-RAM  
- Changing MS stock kernel  

## 6. Acceptance

- Boot install enable → after reboot cascade active (or refuse with preflight, never hang)  
- Manual up: VRAM nbd prio 100, zram 200, disk lower  
- down: clean managed swaps  
- auto transport message on WSL2 mentions nbd + ublk policy  
