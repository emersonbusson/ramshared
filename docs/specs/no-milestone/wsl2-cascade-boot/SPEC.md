# SPEC — wsl2-cascade-boot

> Passo 2 SSDV3. Implementa `PRD.md` na mesma pasta. Zero criatividade fora deste SPEC.

## Traceability

| PRD | ITEM |
| --- | --- |
| RF-1 | ITEM-1 install opt-in |
| RF-2 | ITEM-2 cascade-preflight + unit ExecStartPre |
| RF-3 | ITEM-3 ExecStop = ramshared down; TimeoutStopSec=600 |
| RF-4 | ITEM-4 /etc/ramshared/cascade.conf + env no CLI |
| RF-5 | ITEM-5 up early-return se cascata saudável |
| RF-6 | ITEM-6 docs humanas |
| NFR-1..4 | embutidos nos ITEMs |

## Files

| Path | Action |
| --- | --- |
| `scripts/safety/cascade-preflight.sh` | create — fail-closed NBD/Day-1 |
| `scripts/safety/systemd/ramshared-cascade.service` | create |
| `scripts/safety/install-cascade-boot.sh` | create |
| `scripts/safety/uninstall-cascade-boot.sh` | create |
| `scripts/safety/cascade.conf.example` | create |
| `crates/ramshared-cli/src/cascade.rs` | env defaults + idempotent up |
| `crates/ramshared-cli/src/main.rs` | usage: boot scripts pointer |
| `README.md`, `docs/FAQ.md`, `ROADMAP.md`, `ARCHITECTURE.md` | human voice |
| `docs/specs/…/IMPL.md` | Passo 3 |

## ITEM-1 — install opt-in

`install-cascade-boot.sh`:

1. Require root.  
2. Resolve `REPO`, build release bins if missing (`cargo build -p ramshared-cli -p ramshared-wsl2d --release`) or use `RAMSHARED_BIN_DIR`.  
3. Run `ramshared check` — exit non-zero → abort install (do not enable).  
4. Run `cascade-preflight.sh` — abort on fail.  
5. Write `/etc/ramshared/cascade.conf` from example if absent (do not overwrite user conf).  
6. Install unit with sed path substitution.  
7. `systemctl daemon-reload`.  
8. **Only if** `--enable` flag: `systemctl enable --now ramshared-cascade.service`. Default: install files only and print next step.  
9. Print plain-language summary.

`uninstall-cascade-boot.sh`: stop+disable unit, remove unit file, leave conf (user data).

## ITEM-2 — cascade-preflight.sh

Read-only gates (exit 1 = refuse):

1. Root or capable of reading /proc/swaps.  
2. Binaries `ramshared` + `ramsharedd` executable.  
3. `nvidia-smi` free MiB ≥ `VRAM_MIB + MIN_HEADROOM` (default headroom 256; env `RAMSHARED_MIN_VRAM_FREE_MIB`).  
4. No ghost managed swap in `/proc/swaps` (`(deleted)` on nbd/ublk/zram).  
5. `nbd-client` present; `modprobe nbd` possible (best-effort warn if not root).  
6. Disk/VHDX or MemAvailable safety: if no lower swap and MemAvailable < 2× VRAM → warn+refuse unless `RAMSHARED_FORCE=1` (mirrors A1 spirit).

Does **not** require ublk or mlockall string (that is ublk lab path).

## ITEM-3 — unit

```ini
[Unit]
Description=RamShared memory cushion (zram + idle GPU + disk)
After=local-fs.target network-online.target
# Fail closed: do not block multi-user if we refuse
[Service]
Type=oneshot
RemainAfterExit=yes
User=root
EnvironmentFile=-/etc/ramshared/cascade.conf
ExecStartPre=.../cascade-preflight.sh
ExecStart=.../ramshared up --vram ${VRAM_MIB} --zram ${ZRAM_MIB} --daemon .../ramsharedd
ExecStop=.../ramshared down
TimeoutStartSec=120
TimeoutStopSec=600
Restart=no
[Install]
WantedBy=multi-user.target
```

Note: systemd EnvironmentFile does not expand `${VRAM_MIB}` in ExecStart for all versions — **use a small wrapper** `scripts/safety/cascade-up.sh` that sources conf and execs CLI.

## ITEM-4 — conf + CLI env

Product sizes come from `/etc/ramshared/cascade.conf` (seeded from `cascade.conf.example` on install) or env. Live host (2026-07-13): nbd **4096** MiB / zram **2048** MiB.

`cascade.conf.example` (product defaults):

```
VRAM_MIB=4096
ZRAM_MIB=2048
```

CLI `parse_up_args_from`: if `--vram`/`--zram` not on argv, read `RAMSHARED_VRAM_MIB` / `RAMSHARED_ZRAM_MIB`; hard fallback when neither conf nor env is present remains **1024/1024** (safe CLI-only path, not the installed product default).

## ITEM-5 — idempotent up

Before setup, if:

- no ghosts, and  
- active nbd (or recorded swap) already in `/proc/swaps` with prio ≈ vram, and  
- daemon pid alive or socket exists  

then log `[up] cascata ja ativa — noop` and `return Ok(())` after `status()`.

If half-state (record without swap, or swap without daemon): refuse with message to run `down` first (fail-closed, no second stack).

## ITEM-6 — docs voice

Rewrite user-facing docs to:

- Short sentences.  
- “You / your machine” not “the system orchestrates”.  
- Honest limits (engasgo ≠ freeze; boot needs systemd; Windows lab separate).  
- One “do this / don’t do this” box.

## Kahneman

| Step | Discipline | Abort |
| --- | --- | --- |
| Boot enable | #16 fail-safe | check not ready → no enable |
| Stop | #17 idempotent down | swapoff twice OK |
| Demote under game | #5 / #13 | do not claim zero stall |
| Ghost | #2 counterfactual | refuse force kill |

## Rollback trigger

If after enable, any session shows ghost swap or WSL hard-freeze attributable to cascade boot: `systemctl disable --now ramshared-cascade` + `ramshared down` + entry in validation.md; revert unit default enable.

## Tests

| Test | Expect | Cover / type |
| --- | --- | --- |
| `cascade::tests::default_mb_from_env_and_orphan_kill_switch` | env MiB + orphan kill-switch | #9 |
| `cascade::tests::zram_zero_is_parsed` | `--zram 0` | #9 |
| `cascade::tests::cascade_healthy_requires_vram_swap_record_and_live_daemon_signal` | not healthy without record/daemon | #13 |
| `cascade::tests::ghost_blocks_healthy` | ghost → not healthy | #13 |
| `cascade::tests::refuse_half_cascade_when_vram_live_without_health` | half-state refuse | #13/#16 |
| `cargo test -p ramshared-cli` | all pass | package |
| Manual: `scripts/safety/cascade-preflight.sh` | CASCADE-PREFLIGHT: OK when ready | E2E |
| Manual: install without `--enable` | unit not enabled | E2E |

**Conf vs CLI defaults (2026-07-13 confront):** `cascade.conf.example` may use larger VRAM/ZRAM (e.g. 4096/2048) than CLI hard-fallback 1024/1024 when env/flags absent. Product path = conf/env; CLI 1024 is fallback only — not a dual default to reintroduce.
