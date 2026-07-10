---
slug: cascade-desktop-app
title: Desktop control app for WSL2/Linux cascade (zenity + CLI)
milestone: —
issues: []
---

# PRD — App de controle da cascata (linda e usável)

## 1. Summary

O path shippable é a cascata zram→VRAM→disco. Falta uma **superfície de app**: ligar, desligar, ver status, ligar boot — sem digitar uma dúzia de comandos e sem Electron.

**Confirmed in codebase:** `ramshared check|up|down|status`, `install-cascade-boot.sh`, preflight fail-closed.  
**Confirmed in lab (2026-07-10):** WSL2 + WSLg + `zenity` + `notify-send`.  
**Inference:** menu zenity + fallback CLI cobre 90% do “parecer app” com risco baixo.

## 2. Technical context

- Root ainda é necessário para `up`/`down`/boot (swap). App pede `pkexec` ou documenta `sudo`.  
- Não toca LKM. Não host-real Windows.  
- Kernel-true track blocked neste lab (`kernel-vram-as-memory/PASSO0-INVENTORY.md`).

## 3. Recommended option

**`scripts/safety/cascade-app.sh`** + `.desktop` opcional:

- GUI: zenity list (Start / Stop / Status / Check / Enable boot / Disable boot / Quit)  
- CLI: mesmos verbos `start|stop|status|check|enable-boot|disable-boot`  
- Notificações: `notify-send` em sucesso/falha  
- Defaults conservadores: conf `/etc/ramshared/cascade.conf` ou 1024/1024  

**Rejected:** Electron/Tauri neste ciclo (peso + supply chain).  
**Rejected:** system tray nativo Windows-only no MVP (WSL first).

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

1. User opens “RamShared Cushion” → Start → polkit/sudo → notify OK / fail.  
2. Game on Windows → demote still daemon-side; app Status shows if VRAM tier gone.  
3. Quit app does **not** stop cascade (service stays up).

## 7–8. Data / API

No new ABI. Calls existing binaries and scripts. Env: `RAMSHARED_BIN_DIR`, `RAMSHARED_REPO`.

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

- [ ] `cascade-app.sh status|check` sem root  
- [ ] `start|stop` com sudo/pkexec  
- [ ] zenity menu se DISPLAY+zenity  
- [ ] `.desktop` install helper  
- [ ] plain README section  

## 14. Validation

Manual on this WSL lab; `bash -n`; docs-check.
