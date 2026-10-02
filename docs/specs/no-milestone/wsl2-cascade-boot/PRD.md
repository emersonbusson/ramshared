---
slug: wsl2-cascade-boot
title: WSL2 cascade auto-start on boot with fail-closed anti-hang
milestone: —
issues: []
---

# PRD — WSL2 cascade at boot (without hanging)

> **Revision 2 (2026-09-30).** Revision 1 proposed a systemd unit wrapping
> `scripts/safety/*.sh`. That approach is **retained only as a thin trigger**;
> the product requirement is now a **native in-program bootstrap**, because the
> retired script path caused a live outage (see §2.1). SPEC must be rewritten
> before any IMPL change.

## 1. Summary

When WSL2 starts, the user wants the memory cushion (zram → idle VRAM → disk)
**already enabled**, and wants VRAM to **return to the graphics card** when a
Windows game or 3D render needs it — **without killing processes or hanging
WSL**.

Today this works only with a manual four-step attended sequence
(`attach origin` → `Guardian` → `host-gate` → `sudo ramshared up`). The retired
legacy path did auto-start, but unsafely. This PRD closes the gap with a
**native, fail-closed, in-program bootstrap** — not a boot-time binary redeploy
script.

**Confirmed in codebase:** `ramshared up/down` with anti-hang behavior
(swapoff before killing the daemon), a free-floor/latency canary in the daemon,
and measured DEMOTE. `monitor.rs` derives tier/protection display from live
state (`cb39389a`).
**Confirmed in docs:** historical freezes from ghost swap / incorrect kill
(`cascade.rs` contract, `validation.md`); legacy boot auto-deploy caused a dirty
orphan NBD outage on 2026-09-23 (`MEMORY.md`, `GAP-REGISTER.md`).
**Inference (limited):** a systemd unit is the stable *trigger* for "at boot"
in WSL with `systemd=true`; the orchestration logic itself belongs in the CLI.

## 2. Technical context

- Day-1: `ramshared up` → zram priority 200 + NBD/CUDA priority 100 + VHDX priority -2.
- DEMOTE: `swapoff` the VRAM tier; pages fall to disk; processes remain alive.
- WDDM eviction: data-safe, latency-unsafe (~1.18 s for a 4K read under reclaim).
- Real hangs result from killing a daemon with active nbd, ghost `(deleted)` swap,
  host thrash, or ublk without the fix.

### 2.1 Regression lesson (must inform every design decision)

| Date | Event | Evidence |
| --- | --- | --- |
| 2026-09-21 | `455565db fix(packaging): retire unsafe boot auto-deploy` — boot path copied `target/release/*` into `/usr/local/bin` and restarted the VRAM tier service unattended | commit `455565db` |
| 2026-09-23 | Live outage: `/nbd0` active with dead `pid 654`, `/proc/654` absent, `/zram1` ~890 MiB dirty, NBD read errors, disconnect failed; attended `wsl --shutdown` required | `MEMORY.md` 2026-09-23 entries |
| 2026-09-23 | Boot path sanitized: `wsl.conf` boot command removed, `ramshared-vram-tier.service` disabled | `MEMORY.md` |
| 2026-09-27 | `cb39389a fix(cli): report actual RamShared protection state` — TUI previously printed a hardcoded green `Protection: ACTIVE` and `OPERATIONAL` even without a live daemon | commit `cb39389a` |

**Two independent failures were conflated in operator memory as "it used to
come up green":** a real (unsafe) auto-start, **and** a display layer that
reported green regardless of state. Neither may return.

## 3. Recommended option

**Native bootstrap owned by the CLI**, triggered by a thin systemd unit
`ramshared-cascade.service`:

1. **Identity first.** Refuse unless the running CLI and daemon match one
   sealed release (`BINARY_MATCH`). Never redeploy binaries at boot.
2. **Host prerequisites, fail-closed.** Sealed origin attached, Guardian
   `HEALTHY` with a fresh `boot_id`, host-gate `NORMAL_BOOT` within freshness
   window. Missing any → refuse, leave `Off`, emit an actionable reason.
3. **Dirty-state refusal.** Ghost/deleted managed swap, live orphan NBD/zram, or
   unknown half-state → refuse and request attended recovery. Never force.
4. **Ordered activation.** `up` with sizes from `/etc/ramshared/cascade.conf`.
5. **Ordered stop.** `down` (swapoff → nbd disconnect → daemon). Never
   `kill -9` while nbd appears in `/proc/swaps`.
6. **Honest telemetry.** Tier and protection display derive only from live
   state. Hardcoded "green/ACTIVE" strings are forbidden.

The systemd unit is a **trigger and lifecycle boundary** (start/stop/timeout),
not a second orchestrator. One primary path: the CLI command. No boot-time
`cp` of binaries, no version-skewed drop-in approvals, no dual orchestrator.

**Do not** reuse the ublk `ramsharedd.service` as the product path.

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-1 | Opt-in install: enable boot only after `ramshared check` is ready and preflight passes |
| RF-2 | Boot: identity + host prerequisites + preflight → `up`; any failure leaves the system **exactly** `Off` with a recorded reason — never dirty swap |
| RF-3 | Stop: always `down` (swapoff → nbd disconnect → daemon); never `kill -9` while nbd appears in `/proc/swaps` |
| RF-4 | Config: VRAM/ZRAM MiB in `/etc/ramshared/cascade.conf` (conservative default 1024/1024) |
| RF-5 | `up` is idempotent when the cascade is already healthy (unit reboot / duplicate start) |
| RF-6 | Human docs: what to do daily, what not to do, what demotion costs, and why boot may legitimately stay `Off` |
| RF-7 | **Native bootstrap entrypoint** in the CLI (not a free-standing shell redeploy script): performs §3 steps 1–5 with one invocation |
| RF-8 | **No boot-time binary mutation.** Boot never copies, rebuilds, or replaces product binaries; identity is verified, never installed |
| RF-9 | **No stale approval drop-ins.** Version-scoped approval is generated for the running release or refused; a pinned old version is never accepted |
| RF-10 | **Display honesty gate.** No user-visible surface may print `ACTIVE`/`ARMED`/`OPERATIONAL`/`Protection: ACTIVE` unless live state proves it; a stale `ACTIVE` without a live daemon must render `BLOCKED` |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-1 | Prefer **refusing start** over risking a hang |
| NFR-2 | Stop timeout high enough for swapoff with real use (for example, 600 s) |
| NFR-3 | No host thrash; no automatic enable in forensic install |
| NFR-4 | Host-safety RNF: aggressive pressure only in a VM (already a repository rule) |
| NFR-5 | Boot bootstrap completes or refuses within a bounded, observable window; no unbounded retry loop that outlives `TimeoutStartSec` |
| NFR-6 | A refuse at boot is **visible** (journal + `ramshared status` + TUI), never silent and never masked by a green dashboard |

## 6. Flows

1. **First time:** build → seal → `BINARY_MATCH` verify → check → install → enable → restart WSL → `swapon` shows three tiers.
2. **Normal reboot:** systemd start → native bootstrap → host prerequisites verified → `up` → three tiers.
3. **Reboot with missing origin/Guardian:** bootstrap refuses → `Off` + actionable reason → operator runs attended reattach, then bootstrap retries or operator runs `up`.
4. **Windows game:** free VRAM falls → canary → DEMOTE → VRAM tier disappears; WSL apps continue.
5. **WSL shutdown:** systemd stop → ordered `down`.
6. **Dirty state:** preflight/up refuses; message requests `wsl --shutdown` if a ghost exists.

## 7. Data model

- `/etc/ramshared/cascade.conf` — `VRAM_MIB`, `ZRAM_MIB`, `MIN_VRAM_HEADROOM_MIB`.
- `/run/ramshared/*` — runtime state (host-resume lease, armed markers).
- `SANITIZED_PRODUCT_PATH/current` — sealed product path (must not skew from `/usr/local`).

## 8. API / Interfaces

- Unchanged main CLI surface: `check|doctor|up|down|status`.
- **New native entrypoint:** `ramshared boot` (or `up --bootstrap`) — the single
  orchestrator for §3. Exact name fixed in SPEC.
- Scripts remain **operators' attended tools** only
  (`install-cascade-boot.sh`, `uninstall-cascade-boot.sh`); they are not the
  boot-time product path.
- Unit: `ramshared-cascade.service` (thin trigger: `ExecStart` = CLI bootstrap).

## 9. Dependencies and risks

- WSL with `systemd=true` in `/etc/wsl.conf`.
- `nbd-client`, `modprobe nbd/zram`, NVIDIA in the guest.
- Host-side elevated operations (origin attach, Guardian task) cannot be
  eliminated from userspace — the bootstrap **verifies** them and refuses when
  absent; it does not silently substitute.
- Residual risk: a stall during DEMOTE/WDDM — **not** an eternal freeze; document it honestly.

## 10. Implementation strategy

1. Rewrite SPEC against this PRD (rev 1 SPEC is stale — its ITEMs describe the
   script-only design) + AUDIT-2.5.
2. Native bootstrap command + identity/prerequisite gates.
3. Thin systemd unit + version-scoped approval generation.
4. Display honesty regression tests (extend `cb39389a` cases).
5. Human documentation + IMPL + validation entry.

## 11. Documents to update

README, FAQ, ROADMAP, ARCHITECTURE, CONTRIBUTING, validation.md,
`docs/reliability/GAP-REGISTER.md` (Legacy WSL2 handoff row), this folder's
SPEC/IMPL, `docs/INDEX.md`.

## 12. Out of scope

- Real Windows host driver.
- Automatic enable without opt-in (boot integration stays opt-in; the product
  requirement is that **once enabled**, it is native and robust).
- ublk as the boot path.
- A promise of zero latency under reclaim.
- CoCo (SEV-SNP/TDX/Arm CCA) transitions and multi-vendor GPU qualification.

## 13. Acceptance criteria

- [ ] opt-in install documented and scripted
- [ ] preflight refuses a ghost / GPU without headroom / missing binary / missing host prerequisite
- [ ] unit stop calls down
- [ ] up is idempotent with an already active cascade
- [ ] **boot never mutates product binaries** (named test asserting refusal to deploy)
- [ ] **stale version-scoped approval is refused** (named test with an old-version token)
- [ ] **no hardcoded green** — named tests: cascade Off → `STATUS: OFF`/`Protection: OFF`; stale `ACTIVE` without daemon → `BLOCKED`
- [ ] **reboot E2E:** `wsl --shutdown` → start → three swap lines, zero orphan devices, zero ghost swap — three rounds
- [ ] human documentation; README says what happens during a game and when boot stays `Off`
- [ ] unit tests for environment parsing + green workspace suite

## 14. Validation

`cargo test -p ramshared-cli`; dry-run preflight; docs-check; three reboot E2E
rounds with before→action→after captures; entry in `validation.md` with
`BINARY_MATCH` proof of the exact installed release.

## 15. Kahneman map (critical steps)

| Discipline | Question | Min evidence | Abort |
| --- | --- | --- | --- |
| #1 Record the state (WYSIATI) | Is the dashboard describing reality, or a story we prefer? | Named test: Off → `Protection: OFF`; stale ACTIVE → `BLOCKED` | Any hardcoded "green/ACTIVE" string fails CI |
| #3 Number before adjective | Did we measure the boot path, or assume it? | 3 reboot rounds with exact swap lines + orphan count | Orphan NBD/zram or ghost swap > 0 → stop and recover attended |
| #5 Worst case / real load | What happens when origin/Guardian are missing at boot? | Reboot with origin detached → refusal reason recorded, state exactly `Off` | Any dirty swap left behind → `wsl --shutdown` recovery, no retry |
| #13 Refusal + legitimate path | Can the safe path also succeed? | Enabled boot on a prepared host reaches three tiers | Permanent refusal on a prepared host → fix the gate, do not bypass it |
| #16 From exhaustion | What did the 2026-09-23 outage teach that this design must not repeat? | Named test: boot never copies binaries | Any `cp`/write to product binaries at boot → rollback trigger |
