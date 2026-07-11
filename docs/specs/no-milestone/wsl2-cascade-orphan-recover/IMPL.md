# IMPL — wsl2-cascade-orphan-recover

> Passo 3 SSDV3. Implements [`SPEC.md`](SPEC.md). AUDIT-2.5: **GO**.  
> **Date:** 2026-07-10  
> **Status:** **GREEN**

## Status gates

| Gate | Result | Evidence |
| --- | --- | --- |
| cargo test -p ramshared-cli | **GREEN** | 23 passed |
| Live orphan sim used=0 → up | **GREEN** | recover log + healthy cascade |
| Disk sdc never removed | **GREEN** | still prio -2 after recover |
| used>0 refuse (unit) | **GREEN** | `orphan_plan_dirty_nbd_is_refuse` |
| Ghost still refused | **GREEN** | `refuse_ghost_swap_state` unchanged |
| Kill-switch env | **GREEN** | `RAMSHARED_NO_ORPHAN_RECOVER=1` in code |

## RF / ITEM → files

| ID | Files |
| --- | --- |
| ITEM-1 | `canonicalize_swap_path`, `SwapEntry::canonical_path`, allowlist |
| ITEM-2 | `plan_orphan_action`, `try_recover_zero_used_orphans`, `up()` order |
| ITEM-3 | `swapoff_try`, `down()` nbd/zram canonical |
| ITEM-4 | `[up] orphan recover:` logs; single pass |
| ITEM-5 | refuse matrix + env kill-switch |

## Live numbers (orphan sim)

| Metric | Value |
| --- | --- |
| Before | zram0+nbd0 prio 200/100; `/run` wiped; daemon none |
| Recover | swapoff `/dev/zram0`, `/dev/nbd0`; nbd disconnect |
| After | zram1 prio 200, nbd0 prio 100, sdc -2; daemon pid alive; unit active |
| up exit | 0 |

## Small decisions

1. Log lines reuse `[down] swapoff ok` from shared `swapoff_all` (acceptable noise).  
2. zram may renumber (`zram0` → `zram1`) after recover — expected.  
3. No systemd unit change; boot inherits via `cascade-up.sh` → `up`.

## Rollback trigger

WSL hard freeze or swapoff hang > 30s after recover → set `RAMSHARED_NO_ORPHAN_RECOVER=1` in unit Environment and revert commit; note validation.md.

## Traceability

PRD RF-R1..R8 → SPEC ITEM-1..5 → this IMPL.
