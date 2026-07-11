# AUDIT-2.5 — cascade-transport-policy

> Passo 2.5 SSDV3. Risk: host freeze (ublk teardown WSL2), wrong swap priority, boot hang.

## Scope under audit

| Path | Risk | Verdict |
| --- | --- | --- |
| Product cascade on WSL2 = **NBD** + priorities zram > VRAM > disk | Low if swapoff-first held | **GO** |
| `transport=auto` → NBD on WSL2 with explicit log | Low | **GO** |
| Boot unit `install-cascade-boot.sh --enable` | Medium (boot loop if up hangs) | **GO** with preflight + oneshot |
| Product `ramshared up --transport ublk` on WSL2 | High (freeze 2026-06-09) | **NO-GO** fail-closed |
| Full ublk wire in `up` on non-WSL2 | Medium (incomplete wire) | **NO-GO** until future SPEC |

## Findings

| Sev | Finding | Disposition |
| --- | --- | --- |
| HIGH | ublk product on WSL2 can freeze host on bad teardown | Keep `guard_not_wsl2` + fail-closed in `up` before idempotent |
| MED | Auto→ublk on bare metal still not implemented in `up` | Honest error; not silent half-state |
| LOW | Soak reboot 2× is hygiene, not new product contract | validation.md only — no extra PRD |

## Kahneman map

| # | Applied |
| --- | --- |
| #16 | Safe default = NBD on WSL2; ublk not Day-1 |
| #18 | Root fix for freeze stays in daemon teardown layer before product ublk |
| #17 | `up`/`down` idempotent; boot oneshot RemainAfterExit |
| #15 | No blind retry of ublk up |

## Open questions

None for Day-1 NBD path. Future ublk product path needs **new** AUDIT-2.5 + drill proving teardown cannot freeze WSL2.

## Decision

**GO** for SPEC as written (NBD Day-1, auto, boot enable, priority order).  
**NO-GO** for shipping ublk as VRAM transport on WSL2 in this feature.

## Blockers fixed in SPEC

None — SPEC already NO-GO ublk Day-1 on WSL2.
