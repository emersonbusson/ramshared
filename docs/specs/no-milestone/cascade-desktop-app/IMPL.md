# IMPL — cascade-desktop-app

## Status

**done** for MVP (zenity + CLI). Not Electron.

| Gate | Result |
| --- | --- |
| `bash -n` scripts | OK |
| SSDV3 PRD/SPEC/AUDIT | go |
| Lab has zenity + DISPLAY | yes (WSLg 2026-07-10) |
| Kernel-true track | blocked (PASSO0 inventory WSL-only) |

## Files

| File | ITEM |
| --- | --- |
| `scripts/safety/cascade-app.sh` | 1–2, 4 |
| `scripts/safety/install-cascade-app.sh` | 3 |
| `scripts/safety/ramshared-cushion.desktop.in` | 3 |
| README / FAQ | 5 |

## Validation

```bash
bash -n scripts/safety/cascade-app.sh
./scripts/safety/cascade-app.sh status
./scripts/safety/cascade-app.sh check   # may need GPU path
bash scripts/safety/install-cascade-app.sh
```

## Rollback

Delete `~/.local/share/applications/ramshared-cushion.desktop`; cascade CLI unchanged.

## Traceability

RF-1..7 covered by cascade-app commands wrapping existing safe CLI.
