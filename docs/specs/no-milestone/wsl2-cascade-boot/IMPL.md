# IMPL — wsl2-cascade-boot

> Passo 3 SSDV3. Branch: `main`.

## Status

**code complete / boot opt-in** · host-real Windows unchanged (out of scope)

| Gate | Result |
| --- | --- |
| `cargo test -p ramshared-cli` | **17** passed |
| SPEC items 1–6 | landed |
| Auto-enable by default | **no** (opt-in `--enable`) |
| Host thrash | not done (correct) |

## RF / ITEM → files

| ID | Files |
| --- | --- |
| ITEM-1 | `scripts/safety/install-cascade-boot.sh`, `uninstall-cascade-boot.sh` |
| ITEM-2 | `scripts/safety/cascade-preflight.sh` |
| ITEM-3 | `systemd/ramshared-cascade.service`, `cascade-up.sh`, `cascade-down.sh` |
| ITEM-4 | `cascade.conf.example`, env defaults in `cascade.rs` |
| ITEM-5 | `cascade_already_healthy` + half-state refuse in `up` |
| ITEM-6 | README, FAQ, ROADMAP, ARCHITECTURE (human voice) |

## Small decisions

- Wrapper scripts for conf (systemd EnvironmentFile + ExecStart expansion is flaky).  
- Default install does **not** enable — user must pass `--enable`.  
- Preflight does not require ublk/mlockall string (Day-1 is NBD).

## Validation

```bash
cargo test -p ramshared-cli
# optional on a ready machine:
# sudo bash scripts/safety/cascade-preflight.sh
# sudo bash scripts/safety/install-cascade-boot.sh
```

## Gaps

- Full e2e “reboot WSL → three swap lines”: **partial** — unit enabled + live three lines (zram/nbd/sdc) logged 2026-07-10. Orphan-after-`terminate` heal: **GREEN** via `wsl2-cascade-orphan-recover`.  
- Zero-stall under WDDM reclaim: **not claimed**.

## Rollback trigger

Ghost swap or WSL hard freeze after enable → `uninstall-cascade-boot.sh` + disable unit; note in `validation.md`.

## Traceability

PRD RF-1..6 → SPEC ITEM-1..6 → this IMPL.
