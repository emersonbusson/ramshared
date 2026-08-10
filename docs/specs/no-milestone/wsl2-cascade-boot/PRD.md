---
slug: wsl2-cascade-boot
title: WSL2 cascade auto-start on boot with fail-closed anti-hang
milestone: —
issues: []
---

# PRD — WSL2 cascade at boot (without hanging)

## 1. Summary

When WSL2 starts, the user wants the memory cushion (zram → idle VRAM → disk)
**already enabled**, and wants VRAM to **return to the graphics card** when a
Windows game or 3D render needs it — **without killing processes or hanging
WSL**.

Today this works only with a manual `sudo ramshared up`. The old ublk path
(`ramsharedd.service`) is **not** the Day-1 product (NBD + CLI). This PRD
closes the **boot + ordered stop + refusal on dirty state** gap.

**Confirmed in codebase:** `ramshared up/down` with anti-hang behavior
(swapoff before killing the daemon), a free-floor/latency canary in the daemon,
and measured DEMOTE.
**Confirmed in docs:** historical freezes from ghost swap / incorrect kill
(`cascade.rs` contract, `validation.md`).
**Inference (limited):** a systemd unit is the stable way to provide “at boot”
in WSL with `systemd=true`.

## 2. Technical context

- Day-1: `ramshared up` → zram priority 200 + NBD/CUDA priority 100 + VHDX priority -2.
- DEMOTE: `swapoff` the VRAM tier; pages fall to disk; processes remain alive.
- WDDM eviction: data-safe, latency-unsafe (~1.18 s for a 4K read under reclaim).
- Real hangs result from killing a daemon with active nbd, ghost `(deleted)` swap,
  host thrash, or ublk without the fix.

## 3. Recommended option

**Systemd unit `ramshared-cascade.service` (oneshot + RemainAfterExit)** that:

1. Runs **NBD preflight** (fail-closed).
2. Runs `ramshared up` with sizes from `/etc/ramshared/cascade.conf`.
3. On stop (including `wsl --shutdown` when systemd stops units), runs
   `ramshared down` (swapoff-first).

**Do not** reuse the ublk `ramsharedd.service` as the product path.

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-1 | Opt-in install: enable boot only after `ramshared check` is ready and preflight passes |
| RF-2 | Boot: preflight → `up`; if preflight fails, the unit fails **without** leaving dirty swap |
| RF-3 | Stop: always `down` (swapoff → nbd disconnect → daemon); never `kill -9` while nbd appears in `/proc/swaps` |
| RF-4 | Config: VRAM/ZRAM MiB in config (conservative default 1024/1024) |
| RF-5 | `up` is idempotent when the cascade is already healthy (unit reboot / duplicate start) |
| RF-6 | Human docs: what to do daily, what not to do, and what demotion costs |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-1 | Prefer **refusing start** over risking a hang |
| NFR-2 | Stop timeout high enough for swapoff with real use (for example, 600 s) |
| NFR-3 | No host thrash; no automatic enable in forensic install |
| NFR-4 | Host-safety RNF: aggressive pressure only in a VM (already a repository rule) |

## 6. Flows

1. **First time:** build → check → install-cascade-boot → enable → restart WSL → `swapon` shows three tiers.
2. **Windows game:** free VRAM falls → canary → DEMOTE → VRAM tier disappears; WSL apps continue.
3. **WSL shutdown:** systemd stop → ordered `down`.
4. **Dirty state:** preflight/up refuses; message requests `wsl --shutdown` if a ghost exists.

## 7. Data model

- `/etc/ramshared/cascade.conf` — `VRAM_MIB`, `ZRAM_MIB`, binary paths.
- `/run/ramshared/*` — runtime state (already exists).

## 8. API / Interfaces

- Unchanged main CLI surface: `check|doctor|up|down|status`.
- Scripts: `install-cascade-boot.sh`, `uninstall-cascade-boot.sh`,
  `cascade-preflight.sh`.
- Unit: `ramshared-cascade.service`.
- Optional environment: `RAMSHARED_VRAM_MIB`, `RAMSHARED_ZRAM_MIB`.

## 9. Dependencies and risks

- WSL with `systemd=true` in `/etc/wsl.conf`.
- `nbd-client`, `modprobe nbd/zram`, NVIDIA in the guest.
- Residual risk: a stall during DEMOTE/WDDM — **not** an eternal freeze; document it honestly.

## 10. Implementation strategy

1. SPEC + AUDIT-2.5 (go).
2. NBD preflight + unit + install.
3. Environment defaults + idempotent up.
4. Human documentation.
5. IMPL + validation entry.

## 11. Documents to update

README, FAQ, ROADMAP, ARCHITECTURE, CONTRIBUTING, validation.md, this folder's IMPL.

## 12. Out of scope

- Real Windows host driver.
- Automatic enable without opt-in.
- ublk as the boot path.
- A promise of zero latency under reclaim.

## 13. Acceptance criteria

- [ ] opt-in install documented and scripted
- [ ] preflight refuses a ghost / GPU without headroom / missing binary
- [ ] unit stop calls down
- [ ] up is idempotent with an already active cascade
- [ ] human documentation; README says what happens during a game
- [ ] unit tests for environment parsing + green workspace suite

## 14. Validation

`cargo test -p ramshared-cli`; dry-run preflight; docs-check; entry in `validation.md`.
