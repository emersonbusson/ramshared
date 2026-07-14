# SPEC — cascade-lifecycle-observability

> Implements [`PRD.md`](PRD.md). Single SPEC file (no SPECvN).  
> **Does not** change swap priorities (200 > 100 > −2), DEMOTE algorithms, or pressure-probe host policy.  
> **Does** add pure phase derivation, richer `ramshared status`, health JSON fields, unit tests ≥80% on phase module.  
> Parents: `wsl2-cascade-swap`, `cascade-vram-ondemand`, `wsl2-cascade-boot`, `broker-telemetry-reconciliation`, `cascade-desktop-app`.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1, RF-5..RF-8 | DT-1 phase enum + DT-2 priority + DT-3 thresholds |
| RF-2, RF-3 | ITEM-2 CLI status text/JSON |
| RF-4 | DT-4 demote snapshot; ITEM-3 optional daemon wire |
| RF-9 | Required tests matrix |
| RF-10 | ITEM-5 docs |
| NFR-1 | DT-5 read-only status path |
| NFR-2 | cover gate ITEM-1 module |
| NFR-3..NFR-5 | ITEM-2 constraints + validation |

## Day-0

One primary path: **pure function in CLI crate** is the source of truth for phase.  
`cascade-health.sh` **must** either:

1. Prefer `ramshared status --json` when `RAMSHARED_BIN` / `target/release/ramshared` exists and exits 0, then merge/pass through `phase` + `demote`, **or**  
2. Call the same rules only via that binary (no independent shell reimplementation of the phase table).

No dual-path heuristics in shell. No kernel uAPI.

## Files

| Path | Action |
| --- | --- |
| `crates/ramshared-cli/src/cascade/lifecycle.rs` | **create** — pure types + `derive_phase` + tests |
| `crates/ramshared-cli/src/cascade/mod.rs` | **modify** — `mod lifecycle`; replace thin `status()`; parse `--json` |
| `crates/ramshared-cli/src/main.rs` | **modify** — pass argv for `status` (`status` / `status --json`) |
| `scripts/safety/cascade-health.sh` | **modify** — emit `phase`, `phase_reason`, `demote`, `thresholds_kib` from CLI JSON when available; keep existing fields |
| `docs/FAQ.md` | **modify** — Armed vs Using VRAM |
| `README.md` | **modify** — one status line pointer |
| `docs/ARCHITECTURE.md` | **modify** — short pointer to lifecycle module |
| `validation.md` | **append** — live sample after IMPL |
| `docs/specs/no-milestone/cascade-lifecycle-observability/IMPL.md` | **create** at Step 3 |
| `docs/INDEX.md` | regenerate (SPEC status when this file present) |

**Out of files:** `ramshared-wsl2d` demote algorithm; optional ITEM-3 only if a zero-risk status field already exists on a local socket — otherwise demote stays `unknown` in Day-0.

## DT-1 — Phase enum

Stable string tags (JSON + logs):

| Tag | Meaning |
| --- | --- |
| `Off` | No product cascade: daemon dead **and** no zram+vram product shape (see inputs) |
| `Armed` | Healthy cushion: VRAM tier present (nbd/ublk product path), daemon alive, VRAM used &lt; active threshold, not degraded |
| `UsingZram` | zram used ≥ threshold, VRAM used &lt; threshold, not degraded |
| `UsingVram` | VRAM used ≥ threshold |
| `UsingDisk` | disk/vhdx used ≥ threshold |
| `Demoting` | Daemon reports demote in progress (`demote.in_progress == true`) only |
| `Degraded` | ghost **or** `order_ok == false` **or** (VRAM tier in swaps with used ≥ threshold **and** daemon not alive) **or** half-state reasons listed in DT-2 |

Rust: `#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum CascadePhase` with `as_str()` matching tags exactly.

## DT-2 — Phase selection priority

Evaluate in order; first match wins:

1. **Degraded** if any of:  
   - `ghost == true`  
   - `order_ok == false` (zram prio must be &gt; vram prio &gt; disk prio when those tiers present)  
   - VRAM-class device in swaps (`*nbd*`, `*ublk*`, `*ublkb*`) **and** daemon not alive **and** (used_vram ≥ threshold **or** half-state: run records require daemon — if CLI can detect `/run/ramshared` without pid, count as degraded; else only used_vram≥threshold without daemon)  
2. **Demoting** if `demote.in_progress == true`  
3. **UsingDisk** if disk/vhdx used ≥ threshold  
4. **UsingVram** if vram used ≥ threshold  
5. **UsingZram** if zram used ≥ threshold  
6. **Armed** if daemon alive **and** vram tier present **and** order_ok **and** !ghost  
7. **Off** otherwise  

`phase_reason`: short snake_case string for the winning rule (e.g. `vram_used_ge_threshold`, `ghost`, `daemon_dead_hot_vram`, `armed_low_vram_used`).

### Tier classification (match health.sh)

| Class | Path name match |
| --- | --- |
| zram | `*zram*` |
| vram | `*nbd*` or `*ublk*` or `*ublkb*` |
| disk | remaining swap entries that are not zram/vram (typically VHDX/`sd*`) |

Priorities: for `order_ok`, when both zram and vram present: `p_zram > p_vram`; when vram and disk present: `p_vram > p_disk` (same spirit as health).

## DT-3 — Thresholds

| Name | Default | Env override |
| --- | --- | --- |
| `active_kib` | **1024** (1 MiB) | `RAMSHARED_STATUS_ACTIVE_KIB` (parse u64; invalid → default) |

Used for: residual nbd after pressure stays **Armed** when used &lt; 1024 KiB (matches live residual ~176 KiB).

Document: residual below threshold is intentional, not a bug.

## DT-4 — Demote snapshot (optional input)

```rust
pub struct DemoteSnapshot {
    pub total: Option<u64>,
    pub last_reason: Option<String>,
    pub in_progress: bool,
}
```

Day-0 default when daemon does not expose status:  
`DemoteSnapshot { total: None, last_reason: None, in_progress: false }`.

**Never invent Demoting** without `in_progress == true` from an injectable source (test double or future daemon field).

ITEM-3 (optional): if a cheap read of demote counters exists without new protocol, wire it; else leave None and document gap in IMPL.

## DT-5 — Read-only status path

`status` / phase derivation **must not**:

- call `swapoff`, `swapon`, `nbd-client -d`, `pkill`, CUDA alloc  
- start pressure probe  

May: read `/proc/swaps`, `pgrep`/`/proc/*/cmdline`, optional env, optional existing status socket.

## Pure API (ITEM-1)

```rust
// crates/ramshared-cli/src/cascade/lifecycle.rs

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierSample {
    pub present: bool,
    pub prio: Option<i32>,
    pub size_kib: u64,
    pub used_kib: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CascadeSnapshot {
    pub zram: TierSample,
    pub vram: TierSample,
    pub disk: TierSample,
    pub ghost: bool,
    pub order_ok: bool,
    pub daemon_alive: bool,
    pub daemon_pid: Option<u32>,
    pub demote: DemoteSnapshot,
    pub active_kib: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleView {
    pub phase: CascadePhase,
    pub phase_reason: &'static str, // or String if dynamic; prefer &'static for table reasons
    pub ok: bool,                   // !matches Degraded and basic health
    pub reasons: Vec<String>,     // degraded reasons
}

pub fn derive_lifecycle(s: &CascadeSnapshot) -> LifecycleView;
```

Build `CascadeSnapshot` from existing `read_swaps()` / ghost helpers in `mod.rs` (reuse `SwapEntry`, `is_ghost`, tier name match). Do **not** duplicate swap parsing.

`ok` for view: `false` iff phase is `Degraded` **or** (optional) daemon dead while Armed was expected — align with health `ok` where practical: at least ghost/order_ok/hot-vram-no-daemon map to reasons[].

## ITEM-2 — CLI `status`

### Args

```text
ramshared status           # human text
ramshared status --json    # one JSON object, stdout only (no swapon table unless also useful on stderr — prefer JSON-only on stdout)
```

Invalid flags → usage exit non-zero.

### Human text (minimum)

```text
phase: Armed (armed_low_vram_used)
ok: true
zram: present prio=200 size_kib=… used_kib=…
vram: present prio=100 size_kib=… used_kib=…
disk: present prio=-2 size_kib=… used_kib=…
daemon: alive pid=…
demote: total=? last_reason=? in_progress=false
ghost: false order_ok: true
```

Plus existing ghost warning block if ghosts present.

### JSON schema (frozen)

Required keys:

| Key | Type |
| --- | --- |
| `phase` | string (DT-1 tags) |
| `phase_reason` | string |
| `ok` | bool |
| `reasons` | array of string |
| `tiers.zram` / `vram` / `disk` | object `{present, prio|null, size_kib, used_kib}` |
| `order_ok` | bool |
| `ghost` | bool |
| `daemon` | `{alive, pid|null}` |
| `demote` | `{total|null, last_reason|null, in_progress}` |
| `thresholds_kib` | `{active: u64}` |
| `ts` | string ISO-8601 local or RFC3339 |

`prio` may be JSON `null` if tier absent.

Serialize with `serde_json` if already a dependency of CLI; else minimal hand-rolled JSON **must** match golden fixtures (prefer serde if available in workspace).

### main.rs

```text
Some("status") => {
  let json = args.iter().any(|a| a == "--json");
  to_exit(cascade::status(json))
}
```

## ITEM-3 — Daemon demote counters (optional)

| If | Then |
| --- | --- |
| Existing local status already exposes demote total / in_progress without new protocol | Wire into `DemoteSnapshot` |
| Else | Leave null/false; IMPL notes gap; do not block Done on RF-4 partial |

No new wire protocol in this SPEC.

## ITEM-4 — cascade-health.sh

After building existing sample fields:

1. If `RAMSHARED_STATUS_JSON_CMD` set, use it; else try in order:  
   `$RAMSHARED_BIN status --json`,  
   `./target/release/ramshared status --json` (from repo root if present),  
   `command -v ramshared`  
2. On success, parse with `python3 -c` to extract `phase`, `phase_reason`, `demote`, `thresholds_kib` and **embed** into health JSON.  
3. On failure, emit `"phase":null,"phase_reason":null,"demote":null` and do not invent phase in shell.  
4. Keep all existing keys for backward compatibility.

## ITEM-5 — Docs

- `docs/FAQ.md`: short Q “Is RamShared using my VRAM?” → Armed vs UsingVram vs Demoting.  
- `README.md`: under control/status, mention `ramshared status` phases.  
- `ARCHITECTURE.md`: one paragraph + link to this SPEC.

## Kahneman (critical steps)

| Step | Question | Min evidence | Abort |
| --- | --- | --- | --- |
| DT-2 Degraded rules | Can false Degraded spam users on residual used? | threshold 1024 KiB; unit residual 176 | Raise threshold only via SPEC revise |
| Demoting without daemon | Would we lie about demote? | only `in_progress` true | Never guess Demoting |
| Health dual path | Two heuristics diverge? | health uses CLI JSON only | No shell phase table |
| Status IO | Could status thrash host? | read-only checklist DT-5 | No swapoff/probe in status |

## Rollback trigger (IMPL commits)

- If healthy live cascade (`health ok=true`, prios 200&gt;100&gt;−2, daemon alive, vram used &lt; 1 MiB) reports phase other than `Armed` or `UsingZram` for ≥3 consecutive status samples → revert phase logic.  
- If `status --json` p99 &gt; 5 s idle → revert expensive work.  
- Env kill-switch: `RAMSHARED_STATUS_LEGACY=1` restores old `swapon --show` only status (Day-0 exception with removal after validation).

## Required tests matrix

| Production | Named test | Type | Cover |
| --- | --- | --- | --- |
| `lifecycle.rs` | `phase_off_when_no_tiers_no_daemon` | #9 | ≥80% file |
| `lifecycle.rs` | `phase_armed_low_vram_used` | #9 | |
| `lifecycle.rs` | `phase_using_zram` | #9 | |
| `lifecycle.rs` | `phase_using_vram` | #9 | |
| `lifecycle.rs` | `phase_using_disk` | #9 | |
| `lifecycle.rs` | `phase_degraded_ghost` | #13 | |
| `lifecycle.rs` | `phase_degraded_order` | #13 | |
| `lifecycle.rs` | `phase_degraded_hot_vram_no_daemon` | #13/#16 | |
| `lifecycle.rs` | `phase_demoting_only_when_flag` | #13 | |
| `lifecycle.rs` | `priority_degraded_beats_using_vram` | #9 | |
| `lifecycle.rs` | `active_threshold_from_env_invalid_defaults` | #9 | |
| `lifecycle.rs` | `json_shape_golden_or_roundtrip` | #9 | |
| `mod.rs` / status | `status_json_flag_smoke` (if mock swaps injectable) | #9 | |
| package | `cargo test -p ramshared-cli` | all pass | |
| cover | `cargo llvm-cov -p ramshared-cli --summary-only` focus lifecycle.rs ≥80% | NFR-2 | |

E2E (IMPL, not unit):

| Check | Expect |
| --- | --- |
| Live `ramshared status --json` after healthy up | `phase` ∈ {`Armed`,`UsingZram`}, `ok` true, `order_ok` true |
| `cascade-health.sh` | includes `phase` non-null when CLI available |
| BINARY_MATCH | not required for status-only if daemon untouched; if rebuild daemon, yes |

## Implementation order (ITEM list)

| ITEM | Work |
| --- | --- |
| ITEM-1 | `lifecycle.rs` + full unit table |
| ITEM-2 | wire `status` / `--json` / main |
| ITEM-3 | optional demote wire or explicit skip in IMPL |
| ITEM-4 | health.sh CLI JSON merge |
| ITEM-5 | FAQ + README + ARCHITECTURE |
| ITEM-6 | validation.md live sample + IMPL.md + INDEX |

## Out of SPEC

- Changing demote latency constants or free-floor.  
- Forcing fill into VRAM.  
- Pressure probe changes.  
- Desktop app GUI beyond optional one-line phase (may call CLI later).  
- Prometheus.  
- Windows guest lifecycle.

## Security

- No new privileged ioctl.  
- Status readable without root if `/proc/swaps` is (typical).  
- No kernel pointers in JSON.

---

**End of Step 2 SPEC.** Next: Step 3 IMPL (or optional 2.5 — low risk read-only; can skip if team accepts go).
