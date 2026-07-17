# SPEC — cascade-lifecycle-observability

> Implements [`PRD.md`](PRD.md). Single `SPEC.md` (no SPECvN).  
> Parents: `wsl2-cascade-swap`, `cascade-vram-ondemand`, `wsl2-cascade-boot`, `broker-telemetry-reconciliation`, `cascade-desktop-app`.  
> Aligned to [`docs/SSDV3-PROMPTS.md`](../../../SSDV3-PROMPTS.md) section order (2026-07-15).

## Closed scope

**In now**

- Pure phase derivation (`CascadePhase` + `derive_lifecycle`) in CLI crate.  
- Richer `ramshared status` / `status --json`.  
- `cascade-health.sh` consumes CLI JSON for `phase` / `demote` / thresholds (no shell dual heuristic).  
- Unit tests + cover ≥80% on `lifecycle.rs` (and wired `mod.rs` status path).  
- Docs: FAQ, README, ARCHITECTURE pointers.  

**Out now**

- Swap priority changes (200 > 100 > −2).  
- DEMOTE algorithm / free-floor / pressure-probe policy.  
- Forcing fill into VRAM; Prometheus; Windows guest lifecycle.  
- Desktop GUI beyond optional CLI phase read.  
- New broker wire protocol for demote (file-based status only if implemented).  

**Assumed ready**

- Existing cascade swap parsing / ghost helpers in `crates/ramshared-cli/src/cascade/mod.rs`.  
- Product cascade shape from parent SPECs (`wsl2-cascade-swap`, boot, ondemand).  

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

## Technical decisions

### DT-1 — Phase enum

Stable string tags (JSON + logs):

| Tag | Meaning |
| --- | --- |
| `Off` | No product cascade: daemon dead **and** no zram+vram product shape |
| `Armed` | Healthy cushion: VRAM tier present, daemon alive, VRAM used &lt; active threshold, not degraded |
| `UsingZram` | zram used ≥ threshold, VRAM used &lt; threshold, not degraded |
| `UsingVram` | VRAM used ≥ threshold |
| `UsingDisk` | disk/vhdx used ≥ threshold |
| `Demoting` | Daemon reports demote in progress (`demote.in_progress == true`) only |
| `Degraded` | ghost **or** `order_ok == false` **or** (VRAM tier hot **and** daemon not alive) **or** half-state rules in DT-2 |

Rust: `#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum CascadePhase` with `as_str()` matching tags exactly.

### DT-2 — Phase selection priority

First match wins:

1. **Degraded** if: `ghost`; `order_ok == false`; VRAM-class device in swaps (`*nbd*`, `*ublk*`, `*ublkb*`) **and** daemon not alive **and** (used_vram ≥ threshold **or** half-state run records require daemon).  
2. **Demoting** if `demote.in_progress == true`.  
3. **UsingDisk** if disk/vhdx used ≥ threshold.  
4. **UsingVram** if vram used ≥ threshold.  
5. **UsingZram** if zram used ≥ threshold.  
6. **Armed** if daemon alive **and** vram tier present **and** order_ok **and** !ghost.  
7. **Off** otherwise.

`phase_reason`: short snake_case for the winning rule (e.g. `vram_used_ge_threshold`, `ghost`, `armed_low_vram_used`).

#### Tier classification (match health.sh)

| Class | Path name match |
| --- | --- |
| zram | `*zram*` |
| vram | `*nbd*` or `*ublk*` or `*ublkb*` |
| disk | remaining swap entries (typically VHDX/`sd*`) |

`order_ok`: when both zram and vram present → `p_zram > p_vram`; when vram and disk present → `p_vram > p_disk`.

### DT-3 — Thresholds

| Name | Default | Env override |
| --- | --- | --- |
| `active_kib` | **1024** (1 MiB) | `RAMSHARED_STATUS_ACTIVE_KIB` (invalid → default) |

Residual nbd after pressure stays **Armed** when used &lt; 1024 KiB (live residual ~176 KiB is intentional).

### DT-4 — Demote snapshot (optional input)

```rust
pub struct DemoteSnapshot {
    pub total: Option<u64>,
    pub last_reason: Option<String>,
    pub in_progress: bool,
}
```

Default when daemon does not expose status: `{ None, None, false }`.  
**Never invent Demoting** without `in_progress == true` from injectable source.

ITEM-3: if cheap demote counters exist without new protocol, wire them; else leave null and document gap in IMPL.

### DT-5 — Read-only status path

`status` / phase derivation **must not** call `swapoff`/`swapon`/`nbd-client -d`/`pkill`/CUDA alloc or start pressure probe.  
May read `/proc/swaps`, `pgrep`/`/proc/*/cmdline`, optional env, optional existing status socket/file.

## Atomicity and rollback

**Atomicity frontier:** phase derivation is a pure function of a snapshot — no multi-step host mutation in this SPEC. Health script either embeds CLI JSON fields or emits nulls (no partial shell recompute of the phase table).

**Rollback**

| Layer | Policy |
| --- | --- |
| Userspace CLI | Revert phase logic commit; optional env `RAMSHARED_STATUS_LEGACY=1` restores old `swapon --show` status (Day-0 exception: document removal after validation) |
| Kernel/module | N/A — no LKM surface |
| Host/persistent | No swap table changes from status path |

**Numeric rollback triggers (IMPL commits)**

- Healthy live cascade (`health ok=true`, prios 200&gt;100&gt;−2, daemon alive, vram used &lt; 1 MiB) reports phase other than `Armed` or `UsingZram` for ≥3 consecutive status samples → revert phase logic.  
- `status --json` p99 &gt; 5 s idle → revert expensive work.

## Kahneman map (critical)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| DT-2 residual used | #9 | False Degraded spam on residual VRAM? | threshold 1024 KiB; unit residual 176 | Raise threshold only via SPEC revise |
| Demoting | #13 | Lie about demote without daemon flag? | only `in_progress == true` | Never guess Demoting |
| Health dual path | #13/#18 | Shell vs CLI heuristics diverge? | health uses CLI JSON only | No shell phase table |
| Status IO | #16 | Could status thrash host? | DT-5 read-only checklist | No swapoff/probe in status |
| Cover | #9 | “CLI package green” without lifecycle proof? | cover gate on matrix paths | Workspace avg does not count |

## Security checklist (pre-impl)

- [x] Privilege: no new privileged ioctl / device node — N/A elevated surface  
- [x] User/host copy: N/A (reads `/proc`, optional status file)  
- [x] Flags: invalid `status` flags → non-zero exit  
- [x] Info-leak: no kernel pointers in JSON  
- [x] IRQ/atomic: N/A userspace  
- [x] Lifetime: N/A  
- [x] Hot-unplug: degraded/off when daemon/tier missing (stable fields)  
- [x] Host safety: read-only status; no live thrash  
- [x] Replayable ops: `status` idempotent read (#17)

## Files to CREATE

**`crates/ramshared-cli/src/cascade/lifecycle.rs`**

- Purpose: pure types + `derive_lifecycle` + unit tests  
- RF / DT: RF-1, RF-5..RF-8, DT-1..DT-4, NFR-2  
- Types/fns: `CascadePhase`, `TierSample`, `CascadeSnapshot`, `DemoteSnapshot`, `LifecycleView`, `derive_lifecycle`  
- Reference: existing `SwapEntry` / ghost helpers in `mod.rs`  
- Required tests: see matrix (named `phase_*` / threshold / JSON)  
- Cover target: ≥80%  
- Kahneman: #9/#13 rows above  

## Files to MODIFY

| Path | What | RF/DT | Tests / cover |
| --- | --- | --- | --- |
| `crates/ramshared-cli/src/cascade/mod.rs` | `mod lifecycle`; replace thin `status()`; build snapshot from swaps | ITEM-2 | `status_json_flag_smoke` if injectable; cover ≥80% if treated as business logic |
| `crates/ramshared-cli/src/main.rs` | `status` / `status --json` argv | ITEM-2 | dispatch only — N/A boilerplate if pure wiring |
| `scripts/safety/cascade-health.sh` | emit `phase`, `phase_reason`, `demote`, `thresholds_kib` from CLI JSON | ITEM-4 | E2E only |
| `docs/FAQ.md`, `README.md`, `docs/ARCHITECTURE.md` | Armed vs Using VRAM; status pointer | ITEM-5 | N/A |
| `validation.md` | append live sample | ITEM-6 | append-only |
| `docs/specs/.../IMPL.md` | Step 3 close | ITEM-6 | numbers + cover gate cmd |

**Out of files:** demote algorithm in `ramshared-wsl2d` (ITEM-3 may only publish counters via existing file/socket).

## Files to DELETE

None.

## Observability

| Signal | Where | Type |
| --- | --- | --- |
| `phase`, `phase_reason` | `ramshared status --json`, health JSON | string |
| `demote.{total,last_reason,in_progress}` | status JSON | optional |
| `thresholds_kib.active` | status JSON | u64 |
| `ok`, `reasons` | status JSON | bool / string[] |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter — lifecycle module pointer |
| `docs/FAQ.md` | Alter — Armed vs UsingVram |
| `README.md` | Alter — status phases |
| `validation.md` | Append on close |
| ADR | N/A |
| `DEGRADATION-MATRIX.md` | N/A (no new failure class beyond phase tags) |
| cover gate tool | N/A (repo tool already exists) |

## Implementation order

| ITEM | Work |
| --- | --- |
| ITEM-1 | `lifecycle.rs` + full unit table |
| ITEM-2 | wire `status` / `--json` / main |
| ITEM-3 | optional demote wire or explicit skip in IMPL |
| ITEM-4 | health.sh CLI JSON merge |
| ITEM-5 | FAQ + README + ARCHITECTURE |
| ITEM-6 | validation.md live sample + IMPL.md + INDEX |

### Pure API (ITEM-1)

```rust
// crates/ramshared-cli/src/cascade/lifecycle.rs
pub fn derive_lifecycle(s: &CascadeSnapshot) -> LifecycleView;
```

Build `CascadeSnapshot` from existing `read_swaps()` / ghost helpers — do **not** duplicate swap parsing.

### ITEM-2 — CLI `status`

```text
ramshared status           # human text
ramshared status --json    # one JSON object on stdout
```

Human minimum fields: phase, ok, tiers, daemon, demote, ghost, order_ok.  
JSON required keys: `phase`, `phase_reason`, `ok`, `reasons`, `tiers.{zram,vram,disk}`, `order_ok`, `ghost`, `daemon`, `demote`, `thresholds_kib`, `ts`.

### ITEM-3 — Daemon demote counters (optional)

| If | Then |
| --- | --- |
| Existing local status exposes demote without new protocol | Wire into `DemoteSnapshot` |
| Else | Leave null/false; IMPL notes gap; do not block Done on RF-4 partial |

### ITEM-4 — cascade-health.sh

1. Prefer `RAMSHARED_STATUS_JSON_CMD`, else `$RAMSHARED_BIN status --json`, else `./target/release/ramshared status --json`, else `command -v ramshared`.  
2. On success, embed `phase`, `phase_reason`, `demote`, `thresholds_kib`.  
3. On failure, emit nulls — **do not** invent phase in shell.  
4. Keep existing keys for compatibility.

### ITEM-5 — Docs

FAQ “Is RamShared using my VRAM?”; README status phases; ARCHITECTURE paragraph + SPEC link.

## Day-0

One primary path: **pure function in CLI crate** is source of truth for phase.  
`cascade-health.sh` must prefer CLI JSON or call the binary — no independent shell reimplementation of the phase table.  
Exception: `RAMSHARED_STATUS_LEGACY=1` only as temporary kill-switch with removal after validation (reason: rollback; not dual product path).

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-cli/src/cascade/lifecycle.rs` | `lifecycle.rs` :: `phase_off_when_no_tiers_no_daemon` | unit | #9 | ≥80% |
| same | `phase_armed_low_vram_used` | unit | #9 | ≥80% |
| same | `phase_using_zram` | unit | #9 | ≥80% |
| same | `phase_using_vram` | unit | #9 | ≥80% |
| same | `phase_using_disk` | unit | #9 | ≥80% |
| same | `phase_degraded_ghost` | unit | #13 | ≥80% |
| same | `phase_degraded_order` | unit | #13 | ≥80% |
| same | `phase_degraded_hot_vram_no_daemon` | unit | #13/#16 | ≥80% |
| same | `phase_demoting_only_when_flag` | unit | #13 | ≥80% |
| same | `priority_degraded_beats_using_vram` | unit | #9 | ≥80% |
| same | `active_threshold_from_env_invalid_defaults` | unit | #9 | ≥80% |
| same | `json_shape_golden_or_roundtrip` | unit | #9 | ≥80% |
| `crates/ramshared-cli/src/cascade/mod.rs` | status JSON smoke / injected swaps | unit | #9 | ≥80% |
| package | `cargo test -p ramshared-cli` | unit | #9 | all pass |

**Cover gate (canonical):**

```bash
node tools/ci/check-rust-slice-coverage.mjs \
  -p ramshared-cli \
  --files crates/ramshared-cli/src/cascade/lifecycle.rs,crates/ramshared-cli/src/cascade/mod.rs \
  --min 80 \
  --report-json tmp/cascade-lifecycle-cov.json
```

**E2E (cascade surface — not cover script):**

| Check | Expect |
| --- | --- |
| Live `ramshared status --json` after healthy up | `phase` ∈ {`Armed`,`UsingZram`}, `ok` true, `order_ok` true (or Using* when used ≥ threshold) |
| `cascade-health.sh` | `phase` non-null when CLI available |
| BINARY_MATCH | N/A for status-only if daemon binary untouched; required if `ramsharedd` rebuilt |

## Validation checklist

- [x] `cargo fmt` / `clippy -D warnings` / `cargo test -p ramshared-cli`  
- [x] Cover gate script on matrix paths ≥80%  
- [x] Live: `status` + `status --json` + `cascade-health.sh`  
- [x] BINARY_MATCH N/A (status path) unless daemon rebuilt  
- [x] Every matrix test name exists  
- [x] Kahneman critical rows executable  

---

**End of Step 2 SPEC.** Step 3: see [`IMPL.md`](IMPL.md). Optional 2.5: low risk read-only — can skip if team accepts go.
