# SPEC — wsl2-cascade-orphan-recover

> Implements [`PRD.md`](PRD.md). Zero creativity out of scope.  
> Revises behavior of `crates/ramshared-cli/src/cascade.rs` only (plus docs/tests).  
> Parent: `wsl2-cascade-boot` ITEM-5 half-state; does **not** lift ublk product NO-GO.

## Traceability

| PRD | ITEM |
| --- | --- |
| RF-R6 | ITEM-1 path normalize |
| RF-R1, RF-R2, RF-R3, RF-R4, RF-R5, RF-R7, RF-R8 | ITEM-2 orphan recover in `up` |
| RF-R6 | ITEM-3 `down` uses normalize (same helper) |
| NFR-R1..R3 | ITEM-4 logging + single pass |
| RF-R2, ghost | ITEM-5 refuse matrix unchanged for dangerous cases |

## Files

| Path | Action |
| --- | --- |
| `crates/ramshared-cli/src/cascade.rs` | modify — normalize, recover, tests |
| `docs/specs/…/wsl2-cascade-orphan-recover/*` | create |
| `docs/specs/…/wsl2-cascade-boot/IMPL.md` | note recover landed |
| `validation.md` | append |
| `docs/INDEX.md` | regenerate |

No change to systemd unit file required if recover is inside `up` (boot already calls `cascade-up.sh` → `up`).

## ITEM-1 — Path normalize

```rust
/// Canonical device path for swapoff/swapon helpers.
/// `/nbd0` → `/dev/nbd0`; `/dev/nbd0` unchanged; `nbd0` → `/dev/nbd0`.
fn canonicalize_swap_path(p: &str) -> String
```

Use in `swapoff_candidates` (push canonical for non-ghost) and when matching.

Unit tests: table of inputs → outputs.

## ITEM-2 — Orphan recover before setup

In `up()`, **after** ublk fail-closed and **after** `refuse_dirty_swap_state` is restructured:

Order:

1. Parse args; ublk fail-closed (existing).  
2. `refuse_ghosts()` — ghosts → error (existing message). **No auto-recover.**  
3. If `cascade_already_healthy` → idempotent return (existing).  
4. **NEW:** `try_recover_zero_used_orphans()`  
5. `refuse_half_cascade` / remaining dirty checks  
6. A1 safety net + setup (existing)

### `try_recover_zero_used_orphans`

Detect managed orphans:

```text
managed = entry is nbd|ublk|zram (is_managed_or_orphan_vram_tier)
live = !ghost
orphan_context = !cascade_already_healthy(entries)
  AND (no SWAP_DEV/ZRAM/PID record OR daemon not alive)
  AND any live managed nbd|ublk|zram in swaps
```

If no orphan_context → Ok(()) no-op.

If any live managed **nbd or ublk** with `used_kb > 0` →  
`Err(Precondition("orphan nbd/ublk com used_kb>0 — recusa auto-recover; wsl --shutdown …"))`.

If any live managed **zram** with `used_kb > 0` **and** no nbd/ublk orphan →  
attempt swapoff zram only (local); if still present after → Err.

If all live managed orphans have `used_kb == 0` **or** only zero-used after zram attempt:

1. Log `[up] orphan recover: zero-used managed swap — swapoff + disconnect`  
2. Build candidates via `swapoff_candidates` (canonical paths)  
3. `swapoff_all` — single pass  
4. For each live nbd (not ghost): `nbd-client -d <canonical>` best-effort  
5. Do **not** pkill daemon if any nbd/ublk still in swaps (existing `daemon_kill_allowed`)  
6. If daemon still running and kill allowed → TERM only (same as down)  
7. `remove_dir_all`/`remove_file` on `/run/ramshared` contents best-effort  
8. Re-read swaps: if any live nbd/ublk remain → Err (recover failed)  
9. Ok(()) then continue normal up  

**Allowlist:** only paths whose bare name matches `nbd*`, `ublk*`, `zram*`. Never touch other partitions.

## ITEM-3 — down path normalize

`swapoff_all` / candidates already use canonical paths so `/nbd0` in `/proc/swaps` is swapoff'd as `/dev/nbd0` (try both if needed: first canonical, on No such file try bare).

## ITEM-4 — Logging + single pass

- One recover attempt per `up` invocation.  
- No sleep-retry loop on swapoff failure (#15).  
- stderr lines start with `[up] orphan recover:` or `[down]`.

## ITEM-5 — Refuse matrix

| State | Action |
| --- | --- |
| Ghost managed | refuse (no recover) |
| Healthy cascade | noop |
| Orphan nbd/ublk used>0 | refuse |
| Orphan managed used=0 | recover then up |
| Half-state with records but dead daemon and live nbd used=0 | recover (records absent or present — treat as recover if not healthy) |
| Explicit ublk transport | still fail-closed before recover (existing) |

## Kahneman

| # | Application |
| --- | --- |
| #15 | No retry loop on failed swapoff |
| #16 | Default refuse when used>0 on dead backend; safe auto only used=0 |
| #17 | Recover 2×: second sees clean or healthy |
| #18 | Fix at cascade orchestration layer (owner of swap lifecycle), not kernel |

## Rollback trigger

If after deploy, any boot causes **WSL hard freeze** or swapoff hang > 30s attributable to orphan recover → disable recover behind env `RAMSHARED_NO_ORPHAN_RECOVER=1` (fail-closed to old refuse) and revert commit; log in validation.md.

Implementation: if `RAMSHARED_NO_ORPHAN_RECOVER=1`, skip recover and keep old orphan error.

## Tests

| Test | Expect | Type |
| --- | --- | --- |
| `cascade::tests::canonicalize_swap_path_table` | `/nbd0`→`/dev/nbd0`, etc. | #9 |
| `cascade::tests::orphan_plan_dirty_nbd_is_refuse` | used>0 → Refuse | #13/#16 |
| `cascade::tests::orphan_plan_zero_used_is_recover` | used=0 → Recover | #13 |
| `cascade::tests::allowlist_rejects_disk_paths` | no swapoff of `/dev/sdc` | #13 |
| `cascade::tests::daemon_kill_forbidden_with_active_ublk_or_ghost` | no kill with live/ghost block swap | #16 |
| `cascade::tests::try_recover_refuses_dirty_backend` | recover path refuse used>0 | #13 |
| `cascade::tests::try_recover_kill_switch_on_zero_used` | `RAMSHARED_NO_ORPHAN_RECOVER` | #16 |
| `cascade::tests::swapoff_try_prefers_canonical_then_bare` | path normalize on swapoff | #9 |
| `cargo test -p ramshared-cli` | all pass | package |

Confronted 2026-07-13: symbols + tests present; full `wsl --terminate` E2E still out of unit scope.

## Out of SPEC

- Manufacturing full `wsl --terminate` inside unit tests  
- ublk product wire  
- preflight auto-clean (recover lives in `up` only)
