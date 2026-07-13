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

---

## D. `cascade-vram-ondemand`

| ITEM | SPEC claim | Code | Tests | Verdict |
| --- | --- | --- | --- | --- |
| ITEM-1 SparseVramBackend | chunk map + first-write alloc; empty read = zeros | `crates/ramshared-block/src/sparse_vram.rs` (`SparseVramBackend`, `DEFAULT_CHUNK_MIB=128`) | `read_empty_is_zeros_without_alloc`, `write_then_read_roundtrip_one_chunk`, `cross_chunk_write_two_allocs`, `alloc_fail_returns_io_error` (**15** sparse tests pass) | ✅ |
| ITEM-1 shape | `ChunkState { Empty, Live }` | Code uses `mem: Option` + `written`/`last_write` (semantically Empty/Live) | — | 🟡 **structural drift**, behavior matches |
| ITEM-2 reclaim | free Live only when `used_kb==0` (+ floor/idle); worker thread | `try_reclaim`; wired in `wsl2d/main.rs` worker + `recv_timeout` idle tick | `reclaim_blocked_when_used_kb_nonzero`, `reclaim_frees_when_used_zero_and_below_floor` | ✅ |
| ITEM-2b mid-flight spill | out of MVP | not present | — | ✅ correctly absent |
| ITEM-3 telemetry | capacity/committed/live/total/frees/fails/mode | stderr: `VRAM mode=sparse…committed=0`; reclaim logs `freed … live=`; counters `alloc_fails`/`reclaim_frees`; optional `--telemetry-jsonl` is broader residency, not the exact sparse field list | unit counters + live IMPL (2026-07-11) | 🟡 **hygiene**: JSONL field list not 1:1 with SPEC table; product logs + counters prove gates |
| ITEM-4 flags + preflight | PREALLOC / CHUNK_MIB / IDLE_FREE; sparse gate | env helpers in `sparse_vram.rs`; `cascade-preflight.sh` sparse NEED=headroom+1+chunk; `cascade.conf.example` documents env | `env_helpers_have_sane_defaults` | ✅ |
| ITEM-5 safety + live | idle Δ≪VRAM_MIB; pressure order; kill-switch PREALLOC | IMPL + validation 2026-07-11: idle Δ≈212 MiB; free reclaim 4067→4408; pressure zram→nbd | unit + recorded live | ✅ (live not re-drilled this session) |

**Unit re-run (2026-07-13):** `cargo test -p ramshared-block sparse` → **15 passed**.

---

## E. `cascade-transport-policy`

| ITEM | SPEC claim | Code | Tests | Verdict |
| --- | --- | --- | --- | --- |
| ITEM-1 priorities | zram=200, vram=100, disk≈−2; log once on up | `ramshared-tier` `ZRAM_PRIO`/`VRAM_PRIO`; `cascade_io` log `[up] prioridade: zram({}) > VRAM/nbd({}) > VHDX` | `priority::tests::default_priorities_follow_spec_order` (+ order rejects) | ✅ |
| ITEM-2 boot enable | `install-cascade-boot.sh --enable` | `scripts/safety/install-cascade-boot.sh` present | manual (boot confront §A) | ✅ file+prior live unit |
| ITEM-3 transport auto | Auto→Nbd on WSL2; explicit ublk fail-closed | `Transport::{Auto,Nbd,Ublk}`, `is_wsl2`, `resolve_transport`; refuse before half-setup in `cascade_io` | `defaults_to_auto…`, `auto_transport_flag…`, `resolve_transport_explicit_and_auto_on_wsl`, `up_refuses_explicit_ublk_on_wsl` | ✅ |
| ITEM-4 idempotent up | healthy early return | `cascade_already_healthy` | `cascade_healthy_*` (boot confront) | ✅ |

**Doc drift:** IMPL maps ITEM-1/3 to `cascade.rs` — tree is `cascade/mod.rs` + `cascade_io.rs` (split hang cover). Fixed in IMPL hygiene this turn.

**Unit re-run:** cascade filter **41 passed**; tier **8 passed**.

---

## F. `wsl2-cascade-swap` (foundational / historical)

Umbrella SPEC for the cascade pivot (NBD Day-1, VRAM cold priority 100, demote not abort). Product paths live under later slices (boot, orphan, transport, sparse, autotier).

| RF / area | SPEC claim | Code today | Verdict |
| --- | --- | --- | --- |
| RF-2 | ublk revised → nbd Phase A | transport auto → nbd on WSL2; ublk refuse | ✅ |
| RF-3 | VRAM cold prio 100 behind zram 200 | `TierPriorities::default` | ✅ |
| §6 up/down/status | CLI contract | `ramshared up/down/status/check` + cascade package | ✅ product |
| §9 demote | graceful demote | residency + demote drills; sparse reclaim is separate | ✅ core; pressure env-bound |
| §14 acceptance | cgroup pressure, integrity, recover | scripts + validation history; not all re-run daily | 🟡 umbrella — detail in child SPECs |

**Overall:** **go as architecture source**; operational proof is child SPECs + validation log. Do not treat this file as the only Day-0 gate.

---

## G. `wsl2-native-vram-autotier` (Phase 1)

| ITEM | SPEC claim | Code | Tests | Verdict |
| --- | --- | --- | --- | --- |
| ITEM-1 dxg | ENUMADAPTERS2 twice, max 64, select 1 adapter / explicit LUID | `crates/ramshared-dxg/src/lib.rs` | **10** tests (layouts, ambiguity, cuda-fallback only on unavailable) | ✅ |
| ITEM-2 policy | external_usage / usable_budget; stale blocks; hysteresis | `crates/ramshared-wsl2d/src/autotier.rs` | **7** pure policy tests | ✅ |
| ITEM-3 allocation gate | CommitBudgetGate before first-write; constrained write no EIO | `CommitBudgetGate` on sparse; daemon WDDM path | `host_budget_gate_blocks_before_cuda_allocation`; constrained tests in validation | ✅ |
| ITEM-4 lifecycle | available→…→recovering; no free if used_kb>0 | daemon demote/swapoff + `backend_release_requires_zero_used…` | unit green; **live host-budget demote OPEN** (IMPL) | 🟡 code green / lab gate open |
| ITEM-5 regression | zram>vram>disk | tier + cascade health | green | ✅ |
| ITEM-6 docs | SSDV3 + IMPL | IMPL present, honest OPEN pressure | — | ✅ |

**Unit re-run:** dxg **10**, autotier **7** passed. IMPL: *PHASE 1 CODE GREEN; HARDWARE PRESSURE GATE OPEN*.

---

## H. Sample — `memory-broker` + `windows-swap-driver`

Not a full ITEM matrix (SPECs are multi-phase / multi-ITEM). Spot-check that named crates and unit proof exist.

### memory-broker (P1 core sample)

| Claim area | Code | Tests (this session) | Verdict |
| --- | --- | --- | --- |
| Protocol / model / arbiter / slices | `crates/ramshared-broker/{protocol,model,arbiter,slices}.rs` | **32** package tests pass | ✅ P1 library surface |
| Agent tenant | `crates/ramshared-agent` | not re-run full agent suite this session | ⬜ sample only |
| Daemon broker mode | `wsl2d/broker_srv.rs` | not re-run | ⬜ sample only |
| P2 Windows DCC / Blender | out of this spot-check | — | ⬜ not claimed |

### windows-swap-driver (sample)

| Claim area | Code | Tests (this session) | Verdict |
| --- | --- | --- | --- |
| Userspace WinDrive service | `crates/ramshared-winsvc/**` (broker_tenant, driver_link, ntpagefile, service, smoke) | **25** lib tests pass | ✅ userspace |
| TransportKind::WinDrive | broker model + winsvc register | `register_win_drive`, coresidence fail-closed | ✅ |
| Kernel StorPort + ABI | `drivers/windows/ramshared/{protocol.h,ramshared.vcxproj}` | not built/loaded this Linux host | 🟡 source present; **no host-load claim** |
| Real host pagefile-VRAM | gated EV/Partner + VM drills (SPEC header) | — | ⬜ env / R9 |

---

## I. Unit suite summary (this confrontation)

| Package / filter | Result |
| --- | --- |
| `ramshared-block` sparse | 15 pass |
| `ramshared-dxg` | 10 pass |
| `ramshared-tier` | 8 pass |
| `ramshared-wsl2d` autotier | 7 pass |
| `ramshared-cli` cascade | 41 pass |
| `ramshared-broker` | 32 pass |
| `ramshared-winsvc --lib` | 25 pass |

Live cascade healthy path was already proven in §A/B (preflight/health/BINARY_MATCH); no destructive demote/pressure on daily host this session (superprompt).

---

## Gaps (actionable) — multi-SPEC

| Sev | Gap | Proposed fix |
| --- | --- | --- |
| LOW | Sparse SPEC ITEM-3 JSONL field names vs stderr/counters | Align SPEC to “stderr + counters; optional residency JSONL” **or** emit explicit sparse JSON line once per canary — prefer SPEC follow product |
| LOW | transport IMPL still said `cascade.rs` | **Fixed** this turn → `cascade/mod.rs` + `cascade_io.rs` |
| INFO | Autotier live WDDM pressure demote | Lab-only; keep IMPL OPEN gate |
| INFO | windows-swap-driver kernel on real host | R9 / VM only — do not claim Day-0 host load |
| INFO | memory-broker full P1 e2e drill | Existing qemu drill scripts; not re-run here |

---

## Verdict (all confronted SPECs)

| Spec | Code implements ITEMs? | Unit proof? | Live / env | Overall |
| --- | --- | --- | --- | --- |
| wsl2-cascade-boot | ✅ | 🟡 SPEC matrix was weak (hygiene) | ✅ healthy | **go** |
| wsl2-cascade-orphan-recover | ✅ | ✅ | ✅ healthy path | **go** |
| cascade-vram-ondemand | ✅ (+ shape drift) | ✅ 15 sparse | recorded live; not re-drilled | **go** |
| cascade-transport-policy | ✅ | ✅ | prior + unit | **go** |
| wsl2-cascade-swap | ✅ architecture | child SPECs | historical | **go (umbrella)** |
| wsl2-native-vram-autotier | ✅ Phase 1 | ✅ dxg+policy | pressure OPEN | **go w/ lab gate** |
| memory-broker (sample) | ✅ P1 crates | ✅ 32 broker | e2e not re-run | **go P1 library** |
| windows-swap-driver (sample) | ✅ userspace+ABI src | ✅ 25 winsvc | kernel host not claimed | **go userspace / kernel gated** |

**Honesty line:** existence of IMPL.md/DONE ≠ live pressure. This confront re-ran **units** and reused **prior live** evidence for healthy cascade; it does **not** re-prove demote under host GPU reclaim or StorPort load.
