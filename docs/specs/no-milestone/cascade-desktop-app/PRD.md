---
slug: cascade-desktop-app
title: Desktop control app for WSL2/Linux cascade (zenity + CLI)
milestone: —
issues: []
---

# PRD — Cascade control app (polished and usable)

## 1. Summary

The shippable path is the zram→VRAM→disk cascade. What is missing is an
**application surface**: start, stop, view status, and enable boot without
typing a dozen commands or adding Electron.

**Confirmed in codebase:** `ramshared check|up|down|status`,
`install-cascade-boot.sh`, and fail-closed preflight.
**Confirmed in lab (2026-07-10):** WSL2 + WSLg + `zenity` + `notify-send`.
**Inference:** a zenity menu plus CLI fallback covers 90% of the “feels like an
app” experience at low risk.

## 2. Technical context

- Root is still required for `up`/`down`/boot (swap). The app requests `pkexec`
  or documents `sudo`.
- Does not touch the LKM or the real Windows host.
- The kernel-true track is blocked in this lab
  (`kernel-vram-as-memory/PASSO0-INVENTORY.md`).

## 3. Recommended option

**`scripts/safety/cascade-app.sh`** plus an optional `.desktop` file:

- GUI: zenity list (Start / Stop / Status / Check / Enable boot / Disable boot / Quit)
- CLI: the same verbs, `start|stop|status|check|enable-boot|disable-boot`
- Notifications: `notify-send` on success/failure
- Conservative defaults: `/etc/ramshared/cascade.conf` or 1024/1024

**Rejected:** Electron/Tauri in this cycle (weight + supply chain).
**Rejected:** a native Windows-only system tray in the MVP (WSL first).

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-1 | Start = preflight (if present) + `ramshared up` |
| RF-2 | Stop = `ramshared down` only (swapoff-first path) |
| RF-3 | Status = `swapon --show` + short human summary |
| RF-4 | Check = `ramshared check` / doctor on fail |
| RF-5 | Enable/disable boot wraps install scripts |
| RF-6 | Works headless (CLI) when no DISPLAY/zenity |
| RF-7 | Never kill daemon with active nbd (relies on CLI down) |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-1 | Fail-closed: show refuse reason, no force-kill |
| NFR-2 | No thrash; no pressure tests from the app |
| NFR-3 | Idempotent start (CLI already supports) |
| NFR-4 | Docs in plain language |

## 6. Flows

1. The user opens “RamShared Cushion” → Start → polkit/sudo → notify success or failure.
2. A game on Windows starts → demote remains daemon-side; app Status shows whether the VRAM tier is gone.
3. Quitting the app does **not** stop the cascade (the service stays up).

## 7–8. Data / API

No new ABI. It calls existing binaries and scripts. Environment:
`RAMSHARED_BIN_DIR`, `RAMSHARED_REPO`.

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| User closes WSL without stop | boot unit ExecStop / teach down; status warns ghosts |
| zenity missing | CLI mode |
| sudo password fatigue | pkexec once per action |

## 10. Strategy

PRD → SPEC → IMPL script + desktop file + README pointer. No SSDV3 LKM.

## 11. Docs to update

README, FAQ, IMPL, validation, INDEX.

## 12. Out of scope

Windows tray EXE, auto-demote UI graphing, Electron.

## 13. Acceptance

- [ ] `cascade-app.sh status|check` without root
- [ ] `start|stop` with sudo/pkexec
- [ ] zenity menu when DISPLAY+zenity is available
- [ ] `.desktop` install helper
- [ ] plain-language README section

## 14. Validation

Manual on this WSL lab; `bash -n`; docs-check.
