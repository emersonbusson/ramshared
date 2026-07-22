# RamShared-Kernel isolab probe — 2026-07-17

## Question
Is WSL distro `RamShared-Kernel` a disposable isolated lab for freeze-elimination claim?

## Method (read-only)
1. Restored missing WSL PE binfmt (`WSLInterop` → `/init`) so `powershell.exe` / `wsl.exe` work.
2. `wsl -l -v` → Ubuntu-24.04 (Running, default) + RamShared-Kernel (Stopped→Running).
3. `wsl -d RamShared-Kernel --cd ~ -e bash -lc '…'` read-only env dump.
4. Daily host: `./scripts/safety/wsl2-freeze-campaign.sh --check-gates` → refuse.

## Facts
| Field | Ubuntu-24.04 (daily) | RamShared-Kernel |
| --- | --- | --- |
| WSL_DISTRO_NAME | Ubuntu-24.04 | RamShared-Kernel |
| uname -r | 6.18.35.2-microsoft-standard-WSL2+ | same |
| /mnt/c/Users | yes | yes |
| os-release | Ubuntu 24.04.4 | Ubuntu 24.04.4 |
| hostname | emedev | emedev |
| repo at /home/emdev/codespace/ramshared | yes | no (use /mnt/c/ramshared or Windows path) |

## Verdict
**NOT an isolab.** Second WSL distro on the **same Windows desktop host**. Thrash would still freeze the daily machine.

Heuristic gap closed in `wsl2-freeze-campaign.sh`: any `/mnt/c/Users` ⇒ `shared_windows_desktop` + `daily_host` refuse for `--run-isolated` (unless explicit `RAMSHARED_FORCE_ISOLATED_LAB=1` on a true disposable VM).

## Claim status
`claim=NOT_CLAIMED` unchanged. No thrash executed.

## Required for true claim
Disposable isolated environment (separate Hyper-V/qemu guest or machine **without** shared desktop role), then:
`RAMSHARED_ISOLATED_LAB=1` (+ `FORCE` only if `/mnt/c` remains) and 2× before→action→after protocol.
