# Acceptance Validation — zram→VRAM→VHDX Cascade (SPECv3 §14)

End-to-end empirical evidence on the live system (RTX 2060, WSL2/GPU-PV), with the actual
Rust stack (`ramshared up`/`down` + `ramshared-wsl2d` daemon serving `/dev/nbd0`).
Pressure **confined by cgroup v2** (blast radius limited to the hog). Harness and RAW
in `~/fase0/` (outside the repo, like the Phase 0 smoke tests):
`cascade-validate.sh`, `cascade-demote.sh`, `cascade-hog.c`.

## §14.3 — Spill Under Pressure (the cascade absorbs)

`cascade-validate.sh` (2026-06-05): `up --vram 512 --zram 256`; hog of 1300 MiB
(random data, pattern by page index) in a cgroup with `memory.max=768M`.

| Metric | Measured |
|---|---|
| Mounted Cascade | `zram0` prio **200** › `nbd0` prio **100** › `sdc` prio **-2** ✔ |
| Peak in `/dev/nbd0` (VRAM) | **511 MiB** |
| Post round-trip integrity | **332,800 pages intact, 0 corruption** |
| Canary false-positive | **none** (server latency normal under load) |
| Teardown | clean `down` |

Verdict: pages exceeding RAM+zram spilled into VRAM and **returned intact**.

## §14.4 — DEMOTE: Safe Live-Tier Migration

`cascade-demote.sh` (2026-06-05): hog of 1500 MiB in *hold* mode (holds active pages
in VRAM), then `swapoff /dev/nbd0` — the DEMOTE **action** — with the daemon
serving the read-back. (The canary *trigger* — latency spike — is unit-tested
in `crates/ramshared-wsl2d/src/residency.rs`: the 1.18 s spike from Phase 0 triggers
`Demote(Latency)`.)

| Metric | Measured |
|---|---|
| Active VRAM pages before | **481 MiB** |
| `swapoff /dev/nbd0` (DEMOTE) | **OK in 6 s** |
| `nbd0` after | **absent** from `/proc/swaps` |
| VHDX absorbed | **1277 → 2058 MiB** |
| Post-migration integrity | **384,000 pages intact, 0 corruption** |

Verdict: with active pages in VRAM, DEMOTE **migrates to the lower tier (VHDX) without
loss or corruption** while the daemon serves the read-back — validating the central mitigation for
*latency-unsafe* (§9) at runtime.

### Re-run 2026-07-09 (live 3 GiB cascade)

In-repo harness: [`scripts/p0/measure-cascade-demote.sh`](../../scripts/p0/measure-cascade-demote.sh)
on the live cushion (`zram 1G p200` / `nbd0 3G p100` / `sdb 8G p-2`), hog 2200 MiB /
cgroup `memory.max=512M`, then RESTORE `swapon -p 100 /dev/nbd0`.

| Metric | Measured |
|---|---|
| Active VRAM pages before | **648 MiB** |
| `swapoff /dev/nbd0` (DEMOTE) | **OK in 14768 ms** |
| `nbd0` after | **absent** from `/proc/swaps` |
| VHDX absorbed | **5 → 648 MiB** |
| Post-migration integrity | **563,200 pages intact, 0 corruption** |
| Canary trigger unit tests | **12/12** (`residency` + ublk residency) |
| Restore | **swapon -p 100 OK** (cascade back) |

RAW: `<legacy-private-artifact-root>/CASCADE-DEMOTE-20260709-163527.txt` · also [`validation.md`](../../validation.md).

## Coverage §14

- §14.1 device round-trip — `wiring-smoke.sh` (write/readback 1 MiB in VRAM) ✔
- §14.2 cascade mounting/unmounting — `up`/`down` (above) ✔
- §14.3 confined spill — ✔ (above)
- §14.4 DEMOTE — ✔ (above + 2026-07-09 re-run)
