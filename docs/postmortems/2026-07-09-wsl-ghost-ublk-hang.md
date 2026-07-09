# Postmortem — WSL hang from ghost ublk swap (2026-07-09)

## Outcome

WSL2 distro became unresponsive / restarted repeatedly while attempting to
raise the VRAM cushion to 3 GiB. **Not** a GPU TDR. **Not** Windows pagefile
driver (that path remains VM-only).

## Process quality (Kahneman #7)

| Lens | Assessment |
| --- | --- |
| Process | **Incorrect** — operator path used `kill -9` / forced cleanup while `/proc/swaps` still referenced ublk |
| Outcome | Bad (hang / multi-reboot) |
| Fluke? | No — classic ghost-swap failure mode when block backend dies before `swapoff` |

## Timeline (numbers)

| Time (local) | Event |
| --- | --- |
| Pre-existing | `ramsharedd` **debug** ublk VRAM **512 MiB** since Jul 03; prio **-3** (worse than disk **-2**); **0 B** used initially |
| Activate attempt | `ramshared down` → daemon stall → **kill** of ublk-backed daemon |
| After kill | `/proc/swaps` showed `/dev/ublkb0\040(deleted)` with **used_kb ≈ 117504** (~115 MiB) |
| `swapoff` | Failed: `No such file or directory`; `swapoff -a` ran **~160 s** incomplete |
| `zramctl --algorithm lzo-rle` | Failed: `Invalid argument` on this WSL 6.6 kernel |
| User report | WSL froze and closed |
| After recovery boot | Clean: only `/dev/sdb` swap; ~12 GiB MemAvailable; no ramsharedd |
| Postmortem.service | Multiple boots; `sem crash e sem armed` (no BUG/Oops signature) |

## Root cause (right layer — Kahneman #18)

| Layer | Finding |
| --- | --- |
| Owning | **Swap lifecycle** (`swapoff` before teardown of ublk/NBD/daemon) |
| Not owning | GPU driver, Windows StorPort, zram alone |
| Trigger | Destroying ublk device / killing backend **with pages still mapped as swap** |

## Counterfactual / rollback (Kahneman #2)

- **If** `/proc/swaps` contains `ublk`/`nbd` + `(deleted)` with `used > 0` → **stop**; run `wsl --shutdown` from Windows; do **not** kill more processes.
- **If** `ramshared down` cannot `swapoff` managed devices → **exit non-zero** and **never** `pkill -9` / never disconnect NBD.

## Fix shipped (same incident window)

In `crates/ramshared-cli/src/cascade.rs`:

1. Parse `/proc/swaps` (ghost + normal); unit tests for `\040(deleted)` and space form.
2. **`down`**: swapoff **all** managed/orphan nbd|ublk|zram **before** nbd-client -d / daemon stop.
3. **Daemon kill policy**: no stop if live or ghost vram block swap remains; **no SIGKILL** from CLI.
4. **`up`**: refuse dirty ghost/orphan state; arm forensics marker; zram algo fallback + `--zram 0`; PID file.
5. Explicit reject message for `--transport ublk` until cascade implements it (manual ublk is lab-only).

## Validation

```bash
cargo test -p ramshared-cli
cargo clippy -p ramshared-cli -- -D warnings
```

## Lessons

| # | Lesson |
| --- | --- |
| #5 | Worst case is ghost swap hang, not “daemon won't exit in 5s” |
| #15 | Do not retry kill; first deterministic failure is swapoff |
| #16 | Curator (`down`) must not depend on a live ublk device already deleted |
| #17 | `down` twice must be safe (idempotent swapoff/skip) |
| #13 | Test failure paths: kill forbidden while swap active; ghost parse |

## Open

- Optional: implement first-class `up --transport ublk` with same swapoff-first contract.
- Optional: teach `postmortem.sh --auto` to treat repeated abrupt boots + prior `.armed` more aggressively (already uses armed marker after this fix).
