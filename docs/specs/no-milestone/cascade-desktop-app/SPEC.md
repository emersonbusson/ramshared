# SPEC — cascade-desktop-app

## Traceability

RF-1..7 → ITEM-1..5 · NFR-1..4 embutidos.

## Files

| Path | Action |
| --- | --- |
| `scripts/safety/cascade-app.sh` | create — main entry |
| `scripts/safety/install-cascade-app.sh` | create — install .desktop |
| `scripts/safety/ramshared-cushion.desktop.in` | create — template |
| `docs/specs/.../IMPL.md` | create |
| `README.md`, `docs/FAQ.md` | short “control app” section |

## ITEM-1 — cascade-app.sh

```text
Usage: cascade-app.sh [--gui|--cli] [command]
  commands: start stop status check enable-boot disable-boot menu
  default: menu if zenity+DISPLAY else print usage / status
```

Behavior:

1. Resolve `REPO`, `BIN_DIR` (`target/release` preferred, else `debug`).  
2. `need_root` for start/stop/enable/disable: re-exec via `pkexec` if available and not root, else print `sudo ...` and exit 2 if not root.  
3. **start:** run `cascade-preflight.sh` if executable; then `"$CLI" up` with conf sizes.  
4. **stop:** `"$CLI" down` only.  
5. **status:** `swapon --show`; if ghost keywords, warn in red/plain.  
6. **check:** `"$CLI" check`.  
7. **enable-boot / disable-boot:** call install/uninstall scripts (enable with `--enable`).  
8. **menu:** zenity radiolist or list; loop until Quit.  
9. **notify:** `notify-send "RamShared" "..."` if present (non-fatal).

## ITEM-2 — no new kill paths

App must not `pkill -9 ramsharedd`. Only `ramshared down`.

## ITEM-3 — desktop file

`Name=RamShared Cushion`  
`Exec=@SCRIPTS@/cascade-app.sh --gui menu`  
`Terminal=false`  
`Categories=System;`

`install-cascade-app.sh` installs to `~/.local/share/applications/` (user) or `/usr/share/applications` if root.

## ITEM-4 — security

- No world-writable scripts.  
- No embedding passwords.  
- Preflight refuse messages surfaced to zenity `--error`.

## ITEM-5 — docs

README: 5 lines “Prefer the control app”. FAQ: one Q.

## Kahneman

| # | Note |
| --- | --- |
| #16 | preflight before start |
| #13 | GUI open ≠ cascade up — status tells truth |
| #17 | start twice = idempotent up |

## Rollback

Remove .desktop + stop using script; cascade CLI unchanged.

## Tests

`bash -n` on scripts. Manual menu smoke if DISPLAY set.
