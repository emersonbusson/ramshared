---
slug: wsl2-cascade-orphan-recover
title: "WSL2 cascade orphan recover — auto-heal after terminate without stacking"
milestone: —
issues: []
---

# PRD — Cascade orphan recover (WSL terminate / half-state)

> **Status: proposed → audit before IMPL.**  
> Parent features: `wsl2-cascade-boot`, `cascade-transport-policy`.  
> Incident: 2026-07-10 soak — `wsl --terminate` left nbd/zram in `/proc/swaps` with empty `/run/ramshared` and dead daemon; boot unit failed fail-closed.

## 1. Summary

When the user opens WSL2, cascade must become **healthy** (daemon + zram prio 200 + VRAM/nbd prio 100 + disk lower), even if a previous `wsl --terminate` left **orphan managed swap** in the shared kernel VM.

Today `ramshared up` **refuses** that state (correct anti-stack). Product boot then stays **failed** forever until manual cleanup.

## 2. Technical context

| Fact | Class |
| --- | --- |
| WSL2 shares one kernel VM across distros | Confirmed docs / MS |
| `wsl --terminate` kills userspace (`/run` tmpfs cleared) but may keep swap in kernel if VM does not fully die | Confirmed soak 2026-07-10 |
| Orphan signature: live nbd/zram in `/proc/swaps`, no `/run/ramshared`, no `ramsharedd` | Confirmed codebase + journal |
| `up` error: `ha swap nbd/ublk ativo sem estado /run/ramshared (orfao)` | Confirmed codebase |
| swapoff-first before daemon kill is Day-0 anti-hang | Confirmed cascade.rs |
| Dead nbd + `used_kb > 0` → swapoff can stall / I/O error | Confirmed dmesg 2026-07-10 |
| Paths may appear as `/nbd0` not `/dev/nbd0` | Confirmed soak log |
| Product VRAM transport on WSL2 is NBD only | Confirmed cascade-transport-policy |

## 3. Recommended option

| Option | Verdict |
| --- | --- |
| A. Manual only (`down` + docs) | **NO-GO** for product boot UX |
| B. Auto-recover orphan **only if all managed orphans have `used_kb == 0`**, then normal `up` | **GO** |
| C. Auto-recover even when `used_kb > 0` (force swapoff dead nbd) | **NO-GO** without isolate drill — freeze/stall class |
| D. Full ublk product path | **Out of scope** |

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-R1 | Detect orphan managed swap: live nbd/ublk/zram managed tier, no healthy cascade (no run record **or** no live daemon/socket), not ghost |
| RF-R2 | If any orphan managed entry has `used_kb > 0` **and** is nbd/ublk → refuse with clear message (`wsl --shutdown` or manual), no auto path |
| RF-R3 | If all orphan managed entries have `used_kb == 0` → auto: swapoff (path-normalized) → nbd disconnect best-effort → clear `/run/ramshared` → continue `up` |
| RF-R4 | Never swapoff non-managed disk tiers (e.g. `/dev/sdc`) |
| RF-R5 | Never `kill -9` daemon; never kill daemon while nbd/ublk still in `/proc/swaps` |
| RF-R6 | Path normalize: `/nbd0` ↔ `/dev/nbd0`, `/zram0` ↔ `/dev/zram0` for swapoff |
| RF-R7 | Idempotent: healthy cascade still early-return; 2× recover = clean once then up |
| RF-R8 | Boot unit (`cascade-up.sh` → `ramshared up`) inherits recover without new flags |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-R1 | Recover attempt bounded (single pass, no retry loop on swapoff hang) — Kahneman #15 |
| NFR-R2 | TimeoutStartSec of unit remains ≤ 120s; recover must finish well under that when used=0 |
| NFR-R3 | Log clearly: `[up] orphan recover: …` for journal diagnosis |
| NFR-R4 | No host thrash / pressure tests as part of recover |

## 6. Flows

### Happy path (post-terminate, used=0)

1. Boot → preflight OK  
2. `up` sees orphan nbd+zram used=0  
3. Recover: swapoff → disconnect → clear run  
4. Normal up: zram + daemon + nbd  
5. Unit active  

### Refuse path (used>0 on nbd)

1. `up` sees orphan nbd used>0  
2. Error: refuse auto-recover; instruct full `wsl --shutdown` or careful manual  
3. Unit failed (honest)  

### Ghost path (unchanged)

1. Ghost in `/proc/swaps` → refuse (existing)  

## 7. Data model

No new on-disk schema. Uses `/proc/swaps`, `/run/ramshared/*` only.

## 8. API / Interfaces

CLI: no new subcommand required. Behavior change inside `ramshared up` (and `down` path normalization).

Optional later (out of scope): `ramshared recover` dry-run.

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| Auto swapoff on wrong device | Allowlist: nbd/ublk/zram only |
| Hang on dead nbd with pages | used_kb>0 refuse |
| Race two `up` concurrent | Existing half-state refuse after recover; root oneshot boot |
| Mask real bugs by always recovering | Log + only used=0 signature |

## 10. Implementation strategy

1. SSDV3 SPEC + security AUDIT-2.5  
2. Path normalize helper + unit tests  
3. `recover_zero_used_orphans` in cascade.rs before setup  
4. Live sim: manufacture used=0 orphan → `up`  
5. validation.md  

## 11. Documents to update

- This folder PRD/SPEC/AUDIT-2.5/IMPL  
- `wsl2-cascade-boot/IMPL.md` gap note  
- `validation.md`  
- `docs/INDEX.md`  

## 12. Out of scope

- Product ublk on WSL2  
- Recover of ghost swap without Windows shutdown  
- Auto-recover when nbd/ublk `used_kb > 0`  
- Changing swap priorities  

## 13. Acceptance criteria

- [ ] After manufactured used=0 orphan, `sudo ramshared up` exits 0 with healthy cascade  
- [ ] used>0 nbd orphan still refused  
- [ ] Disk swap never removed by recover  
- [ ] `cargo test -p ramshared-cli` green  
- [ ] Ghost still refused  

## 14. Validation

Live: orphan sim used=0; refuse path unit test or dry sim; no `wsl --shutdown` required for used=0 path.
