# AUDIT-2.5 — Security & fail-safe audit — wsl2-cascade-orphan-recover

> Passo 2.5 SSDV3 + **security general** on privileged cascade surface.  
> Date: 2026-07-10  
> Scope: proposed SPEC + existing `cascade.rs` / boot unit / NBD path.

## Decision

| Path | Verdict |
| --- | --- |
| Auto-recover **used_kb == 0** managed orphans | **GO** |
| Auto-recover **used_kb > 0** nbd/ublk | **NO-GO** |
| Kill -9 / kill daemon before swapoff | **NO-GO** (existing, keep) |
| Touch non-managed swap (disk VHDX) | **NO-GO** |
| Ship without env kill-switch | Accept **GO** only with `RAMSHARED_NO_ORPHAN_RECOVER=1` kill-switch in SPEC |

**Overall: GO** for IMPL as SPEC written.

---

## 1. Threat / abuse model (privileged surface)

| ID | Abuse / failure | Risk | Control in SPEC |
| --- | --- | --- | --- |
| A1 | swapoff wrong device → data loss on disk swap | **CRITICAL** | Allowlist nbd/ublk/zram only; never sdc |
| A2 | Auto swapoff dead nbd with dirty pages → host hang | **CRITICAL** | Refuse if nbd/ublk used_kb > 0 |
| A3 | Stack second cascade on half-state | **HIGH** | Recover clears then single up; refuse if recover incomplete |
| A4 | kill -9 ramsharedd with live nbd → ghost/freeze | **CRITICAL** | Existing daemon_kill_allowed; recover reuses |
| A5 | Unprivileged caller | **LOW** | `up` already requires root for swapon/modprobe |
| A6 | TOCTOU /proc/swaps vs action | **MED** | Re-read after swapoff; fail if nbd remains |
| A7 | Infinite retry hide root cause | **MED** | Single pass #15 |
| A8 | Log injection / path with spaces | **LOW** | Paths from kernel swap list; ghost handled separately |
| A9 | ublk force product via recover | **MED** | Recover only cleans; up still NBD Day-1 / ublk fail-closed |
| A10 | Env kill-switch missing → no rollback | **MED** | `RAMSHARED_NO_ORPHAN_RECOVER=1` |

## 2. Security checklist (`.claude/rules/security.md` adapted)

| Check | Status |
| --- | --- |
| Capabilities: privileged ops as root only | **OK** (existing) |
| No user-controlled path into swapoff without allowlist | **OK** if allowlist enforced |
| No kernel address leak in new logs | **OK** (device names only) |
| Lifetime: map/unmap N/A; nbd disconnect after swapoff | **OK** order in SPEC |
| Hot-unplug / terminate class | **OK** — this feature |
| Host safety: no thrash | **OK** |
| Secrets | N/A |

## 3. Kahneman map

| # | Finding |
| --- | --- |
| #2 | Rollback: freeze/hang > 30s → env disable + revert |
| #13 | Must test refuse path used>0, not only happy recover |
| #15 | No retry loop |
| #16 | Safe default = refuse dirty backend; auto only zero-used |
| #17 | 2× up after recover = healthy idempotent |
| #18 | Fix in cascade orchestration (owns swap lifecycle) |

## 4. Findings on current code (pre-IMPL)

| Sev | Finding | Disposition |
| --- | --- | --- |
| HIGH | Orphan refuse breaks boot after `wsl --terminate` | Fixed by this feature |
| HIGH | `swapoff` candidates may use `/nbd0` without `/dev/` → false "ausente" | ITEM-1 normalize |
| MED | Soak script accepted swap lines without daemon | Not this SPEC; note in validation |
| MED | `down` skip left orphans | ITEM-3 |
| LOW | journal noise from bash `set` in soak WslBash | soak script only; optional later |

## 5. Open questions

None blocking. Residual risk: zero-used race (page-in between check and swapoff) — accepted; re-read after swapoff fails closed if nbd remains.

## 6. Go / no-go

**GO** — implement SPEC ITEM-1..5.  
Blockers: none.  
Do **not** expand to used>0 nbd auto-recover without new audit + isolated drill.
