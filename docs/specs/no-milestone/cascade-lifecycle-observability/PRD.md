---
slug: cascade-lifecycle-observability
title: Cascade lifecycle observability — state machine + fill/demote counters
milestone: —
issues: []
parents:
  - wsl2-cascade-swap
  - cascade-vram-ondemand
  - wsl2-cascade-boot
  - broker-telemetry-reconciliation
  - cascade-desktop-app
---

# PRD — Cascade lifecycle observability (state + counters)

> **Status:** PRD + SPEC + IMPL (SSDV3 Step 1–3). Live status phase validated 2026-07-14.  
> **Does not change** swap priority policy (200 > 100 > -2) or DEMOTE safety rules.  
> **Adds** explicit lifecycle state, fill/demote counters, and human-readable status so “is VRAM in use?” is answerable without re-running pressure probes.  
> Kahneman **#1** (WYSIATI — full state, not “ok” alone), **#3** (number + unit + time), **#9** (test types with numbers).

## 1. Summary

### User intent (product language)

1. See at a glance whether the cascade is **armed**, **filling**, **using VRAM**, **demoting**, or **broken**.  
2. After pressure or a game reclaim, see **numbers**: when zram/VRAM first took pages, how much is used per tier, demote count/reason.  
3. Keep today's safety model: cgroup-bounded probes; shared-host pressure requires explicit approval and the Windows watchdog, with no unsupervised thrash.

### Problem today

| Layer | Behaviour now | Gap |
| --- | --- | --- |
| Kernel swap order | zram 200 → nbd 100 → disk -2 | **OK** — proven live |
| `ramshared status` | prints `swapon --show` + ghost warning only | No phase, no rates, no demote story |
| `cascade-health.sh` | JSON: ok, prios, used_kib, daemon | Snapshot only; no state enum; no fill/demote history |
| Daemon canary / DEMOTE | latency + free-floor → swapoff nbd; logs to stderr | Counters not exported to CLI/health |
| User mental model | “Is RamShared using my VRAM?” | Ambiguous when used≈0 but nbd is swapon |

**Confirmed in codebase:**

- `crates/ramshared-cli/src/cascade/mod.rs` — `status()` ≈ `swapon --show` + ghost warn (lines ~800–815).  
- `scripts/safety/cascade-health.sh` — append-only JSON sample of swaps/daemon/gpu; **does not** invent demote policy (header comment).  
- `crates/ramshared-wsl2d/src/residency.rs` — canary demote decision pure logic.  
- `crates/ramshared-wsl2d/src/main.rs` — DEMOTE path logs + swapoff nbd.  
- `crates/ramshared-wsl2d/src/telemetry.rs` — `demotes_delta` / reconcile (broker path).  
- `scripts/safety/cascade-pressure-probe.sh` — cgroup worker proves fill order (lab).

**Confirmed live (2026-07-14):**

- Pressure probe: `zram_first=2s`, `nbd_first=8s`, `disk_first=none`.  
- Idle after: zram ~42 MiB used, nbd ~176 KiB residual, health ok.

**Inference:** A pure userspace **derived state** from `/proc/swaps` + daemon liveness + optional daemon status socket is enough for Day-0 without kernel changes.

## 2. Technical context

### 2.1 Two axes (must not conflate)

| Axis | Driver | Observable |
| --- | --- | --- |
| **Fill** (guest memory pressure) | Linux mm + swap prios | `used_kib` growth order zram → vram → disk |
| **Demote** (host GPU pressure) | Canary / WDDM free floor | demote count, reason, nbd used → 0, free VRAM up |

### 2.2 Existing surfaces to extend (not replace)

| Surface | Role after this feature |
| --- | --- |
| `ramshared status` | Primary human + machine status (text + optional JSON) |
| `cascade-health.sh` | Add `phase` + counters fields; keep JSONL loop |
| `cascade-app` status | Show phase string (optional thin follow-up) |
| Daemon stderr / broker Status | Source for demote counters when available |

### 2.3 Day-0 constraint

- **One** derivation of phase (shared pure function), used by CLI and optionally by health script via `ramshared status --json` or duplicated pure rules documented 1:1.  
- Prefer **CLI pure module** + thin shell consumer over two divergent heuristics.

## 3. Recommended option

**Option A (recommended): pure lifecycle module in `ramshared-cli` (or small `ramshared-tier` helper) + richer `status` + health fields.**

1. Define enum **cascade phase** (see Data model).  
2. Pure function: inputs (swap entries, daemon alive, optional demote snapshot) → phase + counters snapshot.  
3. `ramshared status [--json]`: print phase, used per tier, priorities, ghost flag, demote counters if known.  
4. `cascade-health.sh`: include `phase` + last known counters (call CLI JSON or reimplement rules only if SPEC allows single source).  
5. Daemon: expose demote counters on existing status path if cheap; else CLI reports `demote: unknown` until socket available.

**Rejected alternatives:**

| Option | Why not |
| --- | --- |
| B — Only shell heuristics in health.sh | Diverges from CLI; hard to unit-test at ≥80% |
| C — Kernel/sysfs new uAPI | Out of scope; not needed for observability |
| D — Change swap priorities or force VRAM fill | Product policy change; not this PRD |

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-1 | Derive a single **phase** from observable state (table in SPEC). At minimum: `Off`, `Armed`, `UsingZram`, `UsingVram`, `UsingDisk`, `Demoting`, `Degraded` (ghost / order_ok false / daemon dead with nbd hot). |
| RF-2 | `ramshared status` shows phase in **human English** one-liner + tier used/size/prio (numbers). |
| RF-3 | `ramshared status --json` emits machine-readable object compatible with health consumers (schema in SPEC). |
| RF-4 | Counters (monotonic where applicable): `fill_events` optional; at least **last sample**: `used_zram_kib`, `used_vram_kib`, `used_disk_kib`; demote: `demotes_total` and `last_demote_reason` when daemon provides them, else null/unknown. |
| RF-5 | Phase **Armed** when nbd (or product VRAM tier) is in `/proc/swaps` with used below a small threshold **and** daemon alive (default threshold e.g. 1 MiB — SPEC). |
| RF-6 | Phase **UsingVram** when VRAM-tier used ≥ threshold. |
| RF-7 | Phase **Demoting** when daemon signals demote in progress **or** (if only CLI) nbd disappearing / swapoff race — SPEC defines fail-safe: prefer `Degraded` over false Demoting. |
| RF-8 | Ghost / wrong priority order → `Degraded` with reasons[] (reuse health semantics). |
| RF-9 | Unit tests for pure phase function: table-driven (#9 numbers; #13 refusal/degraded pairs). |
| RF-10 | Docs: FAQ/README short “how to read status” (armed vs using). |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-1 | Status path is **read-only**: no alloc thrash, no swapoff, no GPU stress. |
| NFR-2 | Pure phase logic cover ≥80% on its module/file (llvm-cov). |
| NFR-3 | `status` / health sample completes in &lt; 2 s on idle WSL (typical). |
| NFR-4 | English user-facing phase labels; optional PT summary only in README details if already patterned. |
| NFR-5 | No secrets / no kernel addresses in status output. |

## 6. Flows

1. **Idle cushion:** `up` done → phase **Armed** → user sees “VRAM tier ready, unused”.  
2. **Guest pressure:** probe or real load → **UsingZram** then **UsingVram** as used crosses thresholds; counters show used_kib.  
3. **Windows game:** canary demote → phase **Demoting** then **Armed** or **UsingZram**/disk; demotes_total increments.  
4. **Ghost / hung backend:** phase **Degraded**; status prints recovery hint (`wsl --shutdown` / `down`).  
5. **Off:** no daemon / no product swaps → **Off**.

## 7. Data model

### 7.1 Phase (conceptual)

```text
Off
Armed          # tiers present, VRAM used < threshold, daemon OK
UsingZram      # zram used ≥ threshold, vram below threshold
UsingVram      # vram used ≥ threshold (may also use zram)
UsingDisk      # disk/vhdx used ≥ threshold (pressure spilled past VRAM)
Demoting       # demote in progress (daemon-authoritative when available)
Degraded       # ghost, order_ok=false, nbd hot without daemon, etc.
```

Priority if multiple match: **Degraded > Demoting > UsingDisk > UsingVram > UsingZram > Armed > Off** (SPEC may refine).

### 7.2 Status JSON (sketch — SPEC freezes schema)

```json
{
  "phase": "Armed",
  "phase_reason": "vram_tier_swapon_low_used",
  "ok": true,
  "reasons": [],
  "tiers": {
    "zram": { "present": true, "prio": 200, "size_kib": 2097148, "used_kib": 43316 },
    "vram": { "present": true, "prio": 100, "size_kib": 2097148, "used_kib": 176 },
    "disk": { "present": true, "prio": -2, "size_kib": 8388608, "used_kib": 0 }
  },
  "order_ok": true,
  "ghost": false,
  "daemon": { "alive": true, "pid": 123 },
  "demote": { "total": 0, "last_reason": null, "in_progress": false },
  "thresholds_kib": { "active": 1024 },
  "ts": "2026-07-14T10:47:35-03:00"
}
```

### 7.3 Persistence

- **Default:** no new on-disk store; counters from live sample + daemon memory.  
- **Optional later:** JSONL already in health `--loop`; do not require new DB for Day-0.

## 8. Interfaces

| Interface | Change |
| --- | --- |
| CLI | `ramshared status` enhanced; `ramshared status --json` |
| Health | fields `phase`, `demote`, aligned thresholds |
| Daemon | optional status fields for demote counters (if already on broker/socket — wire through; else “unknown”) |
| App | optional: display phase string from CLI |
| Env | optional `RAMSHARED_STATUS_ACTIVE_KIB` threshold override |

**Not** a new ioctl/uAPI.

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| Dual heuristics CLI vs shell | Single pure function; health calls CLI JSON when binary present |
| False Demoting without daemon signal | Prefer not to guess; only Demoting when daemon says so |
| Residual nbd used after pressure | Threshold + document “residual &lt; 1 MiB still Armed” |
| Pressure probe still host-dangerous if misused | Keep probe lab-only; status remains read-only |

## 10. Implementation strategy

1. **Step 2 SPEC** — freeze phase table, thresholds, JSON schema, test names, file list.  
2. **Step 2.5** — optional (low Ring0 risk; still hang-class *read* only → optional audit).  
3. **Step 3 IMPL** — pure module + tests ≥80%; wire CLI; extend health; validation entry with live `status --json` vs health.  
4. Docs: FAQ “Armed vs Using VRAM”; pointer from README status line.

## 11. Documents to update

| Doc | Why |
| --- | --- |
| This folder `SPEC.md` / `IMPL.md` | SSDV3 |
| `docs/FAQ.md` | Human “is VRAM used?” |
| `README.md` | One line under control/status |
| `validation.md` | Live sample after IMPL |
| `ARCHITECTURE.md` | Short pointer to phase enum |
| `docs/INDEX.md` | regenerate |

## 12. Out of scope

- Changing swap priorities or forcing fill into VRAM.  
- Full autotier promote policy redesign.  
- ublk product path on WSL2.  
- Windows StorPort lifecycle UI.  
- Live multi-tenant pressure on daily host as a required gate.  
- Prometheus/OpenTelemetry export (may follow later).  
- Changing DEMOTE algorithms (latency constants) — observability only.

## 13. Acceptance criteria

1. Pure phase function unit-tested with named cases covering Off/Armed/Using*/Degraded (and Demoting if daemon flag injectable).  
2. `ramshared status` shows phase + per-tier used without root if possible (read `/proc/swaps`).  
3. `ramshared status --json` validates against SPEC schema (or golden fixture).  
4. Cover ≥80% on phase module.  
5. Live: after normal `up`, phase is Armed or UsingZram (not Degraded) when health ok.  
6. Live or fixture: injected high nbd used → UsingVram.  
7. FAQ documents Armed vs Using for end users.

## 14. Validation plan

| Check | Method |
| --- | --- |
| Unit | `cargo test -p ramshared-cli` (phase module tests named in SPEC) |
| Cover | `cargo llvm-cov -p ramshared-cli --summary-only` on phase file ≥80% |
| Live idle | `./target/release/ramshared status --json` + `cascade-health.sh` phase agreement |
| Live pressure | optional lab-only probe; record first-use times if health loop running — **not** required on daily host for PRD close |
| Docs | FAQ paragraph + INDEX regenerate |

### Rollback trigger (for later IMPL commits)

- If `status --json` disagrees with `cascade-health` phase on healthy cascade for &gt; 5 consecutive samples → revert phase heuristic.  
- If status path takes &gt; 5 s p99 idle → revert expensive probes.

## 15. Traceability notes (for Step 2)

| PRD | Expected SPEC items |
| --- | --- |
| RF-1..RF-8 | DT phase table + thresholds |
| RF-9 | ITEM tests list |
| RF-10 | docs ITEM |
| NFR-2 | cover gate in SPEC matrix |

---

**End of Step 1 PRD.** Next: Step 2 `SPEC.md` in this folder (user confirm when ready).
