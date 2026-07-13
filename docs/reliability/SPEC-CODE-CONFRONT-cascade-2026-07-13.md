# SPEC ↔ code confrontation — cascade boot + orphan recover

**Date:** 2026-07-13  
**Method:** read SPEC/IMPL ITEMs → `test -f` / `rg fn` → `cargo test -p ramshared-cli` → live preflight/health/BINARY_MATCH  
**Specs:** `docs/specs/no-milestone/wsl2-cascade-boot/`, `wsl2-cascade-orphan-recover/`  
**Code:** `crates/ramshared-cli/src/cascade/`, `scripts/safety/*`

This is an empirical check that SSD artifacts still match the tree (Kahneman #13: DONE in index ≠ protection active).

---

## A. `wsl2-cascade-boot`

| ITEM | SPEC claim | Code / deploy | Tests / live | Verdict |
| --- | --- | --- | --- | --- |
| ITEM-1 install opt-in | install/uninstall scripts | `scripts/safety/install-cascade-boot.sh`, `uninstall-cascade-boot.sh` present | Manual path (not re-run install) | ✅ files exist |
| ITEM-2 preflight | fail-closed before up | `cascade-preflight.sh`; unit `ExecStartPre=` | Live: **CASCADE-PREFLIGHT: OK** | ✅ |
| ITEM-3 unit stop=down, TimeoutStop 600 | systemd unit | Live unit: `ExecStop=cascade-down.sh`, `TimeoutStopUSec=10min` | — | ✅ |
| ITEM-4 conf + env defaults | conf + CLI 1024/1024 | `cascade.conf.example` has **VRAM_MIB=4096 ZRAM_MIB=2048**; CLI default still **1024/1024** if flags/env absent | `default_mb_from_env_and_orphan_kill_switch`, `zram_zero_is_parsed` | 🟡 **drift**: example conf ≠ SPEC “conservative 1024/1024” (product may have raised capacity intentionally — SPEC text stale) |
| ITEM-5 idempotent / half-state | healthy early-return; half refuse | `cascade_already_healthy`, `refuse_half_cascade` in `cascade/mod.rs`; wired from `cascade_io::up` | `cascade_healthy_*`, `refuse_half_cascade_*`, `ghost_blocks_healthy` | ✅ code+unit (SPEC test section still vague: “if extracted”) |
| ITEM-6 human docs | README voice | not re-audited line-by-line | — | ⬜ skip (doc voice) |

**Boot SPEC “Tests” section** only names env defaults + manual preflight — **under-specified vs code today** (many unit tests exist that SPEC does not list). Gap is **SPEC hygiene**, not missing code.

---

## B. `wsl2-cascade-orphan-recover`

| ITEM | SPEC claim | Code | Tests | Verdict |
| --- | --- | --- | --- | --- |
| ITEM-1 path normalize | `/nbd0`↔`/dev/nbd0` | `canonicalize_swap_path`, `SwapEntry::canonical_path` | `canonicalize_swap_path_table`, `bare_and_canonical_*` | ✅ |
| ITEM-2 orphan recover in up | used>0 refuse; used=0 recover | `plan_orphan_action`, `try_recover_zero_used_orphans` | `orphan_plan_dirty_*` Refuse; `orphan_plan_zero_*` Recover; `try_recover_refuses_dirty_*`; `try_recover_kill_switch_*`; `try_recover_zero_used_with_mocked_swapoff` | ✅ |
| ITEM-3 down uses normalize | swapoff_try / candidates | `swapoff_try`, `swapoff_candidates_from` | `swapoff_try_*`, `swapoff_candidates_*`, `swapoff_all_*` | ✅ |
| ITEM-4 logging + single pass | `[up] orphan recover:` | present in `try_recover_*` | exercised by recover tests (log side-effect) | ✅ |
| ITEM-5 refuse matrix + kill-switch | ghost refuse; `RAMSHARED_NO_ORPHAN_RECOVER` | `refuse_ghost_swap_state`, `orphan_recover_disabled` | `refuse_ghost_*`, `default_mb_from_env_and_orphan_kill_switch` | ✅ |
| RF-R4 never swapoff disk | allowlist | `is_allowlisted_managed_path` | `allowlist_rejects_disk_paths`, `swapoff_all_skips_disk_*` | ✅ |
| RF-R5 no kill with nbd | daemon_kill_allowed | `daemon_kill_allowed` | `daemon_kill_forbidden_*`, `daemon_kill_allowed_active_nbd` | ✅ |

**Live (not full terminate drill):** cascade healthy, ghost=false, BINARY_MATCH=OK, preflight OK — proves **healthy path**, not manufactured orphan (Out of SPEC for unit-simulated terminate).

---

## C. Cover (policy slice — related honesty)

| Target | Lines (recent llvm-cov) | Gate |
| --- | --- | --- |
| `cascade/mod.rs` | ~89% | ≥80% business logic ✅ |
| `cascade_io.rs` | ~2% unit | E2E (health + unit) — not unit 80% |
| `cargo test -p ramshared-cli` | **48 pass** | ✅ |

---

## Gaps (actionable)

| Sev | Gap | Proposed fix |
| --- | --- | --- |
| MED | Boot SPEC §Tests under-specified vs real tests | Update SPEC Tests table in-place with named `cascade::tests::…` (same turn as this confront) |
| LOW | `cascade.conf.example` 4096/2048 vs SPEC text 1024/1024 | Either revise SPEC ITEM-4 to “defaults in conf; CLI fallback 1024” or reset example to 1024 — **prefer SPEC follow product conf** |
| INFO | Full `wsl --terminate` orphan E2E | Still env-bound / Out of SPEC for automated unit; optional lab drill only |

---

## Verdict

| Spec | Code implements ITEMs? | Tests prove pure policy? | Live healthy path? | Overall |
| --- | --- | --- | --- | --- |
| wsl2-cascade-boot | ✅ (ITEM-1..5 files/unit/live; ITEM-6 skip) | 🟡 weak SPEC matrix, strong actual tests | ✅ | **go with SPEC hygiene debt** |
| wsl2-cascade-orphan-recover | ✅ | ✅ | ✅ healthy path only | **go** |

**SSD prompt quality note:** new SSDV3 prompts require a **named test matrix** — these two SPECs (especially boot) predate that bar. Confrontation shows **code ≥ SPEC**, not the reverse.
