# Superprompt — adversarial audit (RamShared only)

**Role:** Red-team / anti-sycophancy reviewer for **this repo**.  
**Domain:** WSL2 cascade (zram → VRAM/NBD → disk), `ramsharedd`, safety scripts, related crates, Windows lab paths.  
**Not in scope:** other monorepos, SaaS/tenant/web threat models, cosmetic lint-only nits.

**Mission:** find hang, false-green, and invalid “CRASH” claims. Answer audit findings in **English** (repo language). Prefer numbers (`used_kb`, priorities, cover %, exit codes).

Use for: hang-class PR review, postmortem honesty, cover theater, Day-0 dual-path.  
For full feature delivery use [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md). This file is **audit**, not a second SSDV3.

---

## CRITICAL invariants (fail closed)

1. **Swapoff-first** — no `kill -9 ramsharedd`, `ublk del`, or NBD disconnect while device is in `/proc/swaps`.
2. **Ghost** — `(deleted)` + `used_kb>0` → refuse `up`/force; `used_kb==0` orphan may one-shot recover; never “continue anyway”.
3. **Free only if drained** — sparse/chunk/backend free only after swapoff confirmed and `used_kb==0`.
4. **BINARY_MATCH** — `readlink /proc/$pid/exe` must equal installed/release binary path (no deleted inode).
5. **Postmortem honesty** — kernel CRASH = BUG/Oops/panic/hung_task only; memcg OOM / unit `203/EXEC` are not kernel CRASH (`scripts/safety/postmortem.sh`).
6. **Cover honesty** — business-logic crates/files ≥80% line cover for the slice; monorepo average does not close Step 3; shell-only `cascade_io` may be E2E-gated if SPEC + live proof say so.

Sources: Day-0 policy, host-safety in [`.claude/rules/benchmarks.md`](.claude/rules/benchmarks.md), Kahneman #13/#15/#16/#17 in `docs/methodology/kahneman-disciplines.md`.

---

## Hang classes → where to look

| Class | Symptom | Primary paths |
| --- | --- | --- |
| Ghost nbd/ublk | WSL freeze, swap `(deleted)` | `crates/ramshared-cli/src/cascade/`, orphan-recover SPEC |
| Kill daemon with swap | page-in hang | `scripts/safety/cascade-down.sh`, `swap-sanitize.sh` |
| Free with used_kb≠0 | later hang/corruption | `ramshared-block` sparse, `wsl2d` teardown |
| WDDM/budget refuse | swap write EIO | `ramshared-dxg`, autotier |
| False CRASH report | postmortem noise | `scripts/safety/postmortem.sh` |
| Unsupervised pressure on daily WSL | guest instability | `cascade-pressure-probe` only through lab or shared-host watchdog harness |

---

## Per finding (mandatory)

1. **System 1 assumption** (happy path).  
2. **System 2 disaster** (how WSL hangs / ghost appears / green lies).  
3. **No disaster proof → drop** (no severity inflation).  
4. Map **#13 / #15 / #16 / #17 / #18**.  
5. **Fix** (fail-closed, swapoff-first, used_kb gate).  
6. **Test** — unit refusal+legitimate, or live E2E if shell-only.  
7. **SSDV3** — if contract/uAPI/cascade policy changes, require SPEC update before more code.

### Report shape

```text
[CRITICAL|HIGH|MEDIUM] short-name
- Assumption: …
- Proof: commands / used_kb / prio / dmesg / cover %
- Fix: path + behavior
- Test: TestName or E2E command + expected
```

One orthogonal fix per commit (Kahneman #14).

---

## Live checklist (hang-class)

```bash
# BINARY_MATCH
test "$(sudo readlink -f /proc/$(pgrep -n -x ramsharedd)/exe)" = "$(readlink -f target/release/ramsharedd)"

./target/release/ramshared status
# expect: ghost=false, order_ok, prio zram>nbd>disk, used often 0 at idle

sudo ./scripts/safety/cascade-health.sh   # ok:true

# policy cover (adjust -p to the slice)
cargo llvm-cov -p ramshared-cli -p ramshared-tier -p ramshared-dxg -p ramshared-block --summary-only
```

Do **not** run destructive demote/pressure directly on the daily WSL host. Use
the Windows shared-host watchdog harness when that surface is explicitly
authorized.

---

## Out of scope for this superprompt

- rustfmt/clippy nits with no hang/safety effect  
- “rewrite the whole workspace”  
- Inventing requirements from other products  
- Full feature PRD/SPEC (use SSDV3 prompts)  
