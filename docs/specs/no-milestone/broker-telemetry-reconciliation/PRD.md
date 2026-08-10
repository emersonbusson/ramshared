---
slug: broker-telemetry-reconciliation
title: Memory broker telemetry collection and reconciliation
milestone: —
issues: []
---

# PRD — Memory broker telemetry collector and reconciliation

> **Feature slug:** `broker-telemetry-reconciliation`. SSDV3 Step 1.
> **Layer:** Userspace (Rust `ramsharedd` daemon + `ramshared-agent` agent) plus the broker protocol.
> Does not touch kernel core, kernel uAPI, IRQ, or DMA (VRAM is already served by the existing data plane).
> **Linked to** [`.claude/rules/benchmarks.md`](../../../../.claude/rules/benchmarks.md) (output at
> `docs/benchmarks/results.jsonl`) and the SSDV3 P0 gate.

## Summary

**What it is.** A lightweight collector samples **three sources** and
**reconciles** them in one unified line per sample, to (a) provide real
observability for cross-tenant VRAM arbitration — RamShared's **only
defensible moat** (vendors do not arbitrate idle VRAM between heterogeneous
tenants) — and (b) **detect when an external consumer (Windows WDDM/graphics)
is squeezing shared VRAM** or when broker accounting diverges (a stuck slice or
an unaccounted consumer).

**Problem solved.** Today the broker knows which *slices* it assigned, but
does **not count served bytes/IO** and emits only unstructured `eprintln`;
there is no way to prove `Σ slices (broker) ≈ Σ SwapUsed (tenants) ≈ Δ VRAM
attributable to the daemon`. Without that reconciliation, we cannot (1) trust
P0-gate/benchmark numbers, (2) distinguish “Windows reclaimed VRAM” from “a
slice became stuck,” or (3) assert that arbitration works.

**RamShared value.** Telemetry makes the moat (revocable arbitration of idle
VRAM) **verifiable**: the reconciliation invariant is itself a continuous
*counterfactual* (divergence = something is wrong), and the residency canary
already detects “someone external is squeezing VRAM.”

## Technical context

**Modules/roles:**
- `ramshared-broker` (`crates/ramshared-broker/`): protocol plus slice
  ledger/arbiter. **Source of truth** (it allocates and serves, so it counts
  exactly).
- `ramshared-wsl2d` (`crates/ramshared-wsl2d/src/broker_srv.rs`, `main.rs`):
  runs the broker, CUDA worker (canary), and VRAM context. It is **where the
  collector lives** (it sees all three sources).
- `ramshared-agent` (`crates/ramshared-agent/`): remote tenant; reports
  pressure/swap to the broker.
- `ramshared-cuda` (`crates/ramshared-cuda/src/driver.rs`): `mem_info()`
  (cuMemGetInfo).

**Current state to reuse/extend:**
- **Confirmed in codebase** — the protocol already has `Msg::Status` /
  `Msg::StatusReply { tenants: Vec<TenantStatus>, slices: Vec<Slice>,
  last_rebalance_secs }` (`ramshared-broker/src/protocol.rs:46,69`), with the
  handler at `broker_srv.rs:214` (`status_reply()` :473–490). **Reuse it as
  the ledger-read RPC.**
- **Confirmed in codebase** — ledger: `Slice { id, offset, len, tenant, state
  }` plus `enum SliceState { Free, Active, Draining, Leased }`
  (`ramshared-broker/src/slices.rs:29`); `TenantState { name, transport,
  present, sid, psi, reconciled }` (`broker_srv.rs:57`).
- **Confirmed in codebase** — the agent already reads `/proc/pressure/memory`
  (`agent/src/psi.rs:15`, `read_psi`) and `/proc/swaps` (`psi.rs:44`,
  `read_swaps` → `SwapEntry { dev, prio, size_kb, used_kb }`) and sends
  `Msg::Psi { sample, swaps }` every **1 s** (`agent/src/main.rs:27`
  `PSI_PERIOD`, loop :273).
- **Confirmed in codebase** — VRAM: `Context::mem_info() -> (free, total)`
  bytes (cuMemGetInfo, `cuda/src/driver.rs:189`).
- **Confirmed in codebase** — canary:
  `Canary::sample(latency_us, content_ok, free_bytes) -> Verdict` plus
  `enum DemoteReason { Latency, Corruption, FreeFloor }`
  (`wsl2d/src/residency.rs:33`); observable counter
  `ServerHandleDt3VramResidency::demote_count()` (atomic,
  `ublk_server.rs:446`); verdict through `demote_tx` → `CoreEvent::Demote`
  (`broker_srv.rs:640,656`).
- **Confirmed in codebase** — logging is textual `eprintln!` through
  `Outbound::Log(s)` (`broker_srv.rs:808`), **without** CSV/JSON/`tracing`/
  metrics. The `Outbound::Log` infrastructure (action vector) is **reusable**
  for emitting a structured line.

**Confirmed in official documentation:**
- Reliable per-PID VRAM and the **DXGI** side require running **on the Windows
  host**; GPU-PV does not expose them from within WSL2 (see
  [`docs/BENCHMARKS.md`](../../../BENCHMARKS.md) and the vendor analysis:
  DXGI `QueryVideoMemoryInfo` LOCAL/NON_LOCAL is the native host budget API).
- `cuMemGetInfo` provides only device free/total (not per-process attribution).

**Proposed (Inference):**
- **Per-slice served bytes/IO** counters in the broker (they do not exist yet).
- Reading `memory.swap.current` (cgroup v2) and `/proc/diskstats` (page IO) in
  the agent.
- Emitting a **unified reconciled line** (JSONL) plus a divergence flag.
- The host `ramshared-nvml` crate for per-PID attribution — **outside the MVP**
  (see §Out of scope).

## Recommended option

**The broker is the collector.** It already receives tenant telemetry (through
`Msg::Psi`), owns the ledger and VRAM context, and hosts the canary; it is
therefore the natural single-writer point where the three sources meet (DT-27,
no race). The MVP attributes “other VRAM” by **subtraction**
(`vram_outros = vram_total_used − vram_alloc_daemon`), without per-PID NVML —
coarse, but sufficient to detect external pressure corroborated by the canary.

Concretely:
1. Extend the ledger with per-slice IO/byte counters (RF-1).
2. Extend tenant telemetry (`Msg::Psi`) with cgroup swap plus diskstats (RF-2).
3. Have the broker sample `mem_info()` and compute attribution by subtraction (RF-3).
4. Have the broker reconcile the invariant and raise a divergence flag (RF-4).
5. Have the broker emit one JSONL line per sample to
   `docs/benchmarks/results.jsonl` (RF-5), reusing `Outbound::Log`.

**Rejected alternatives:**
- A **standalone collector** that polls everything externally duplicates the
  ledger, VRAM context, and canary the broker already has → violates Day-0
  (parallel path). Rejected.
- **Per-PID NVML/DXGI inside WSL2:** unreliable because of GPU-PV. Rejected for
  the MVP (host = future).
- A **Prometheus metrics endpoint/exporter:** overkill for the MVP; the
  `StatusReply` pull path plus the JSONL append line are enough. It can become
  future work without blocking this slice.

**Accepted trade-offs:** subtraction-based “other” attribution (not per PID)
in the MVP; collector on the WSL2 side (host DXGI/NVML deferred); 1 Hz cadence
(matching the existing PSI heartbeat).

## Functional requirements

- **RF-1 — Per-slice IO/byte accounting (source of truth).** Add
  `bytes_served: u64` and `io_count: u64` per slice (and tenant aggregation),
  incremented in the worker serve path (atomic, no hot-path lock), and expose
  them in `StatusReply`.
  - **Acceptance criterion:** after injecting known load (wire N×4 KiB), the
    tenant's `Σ bytes_served` matches wire bytes within ±2%; `io_count` matches
    the number of operations within ±1%.
  - **Isolation:** counters belong to the broker (single writer, DT-27); read
    through `Status` (RPC), without exposing anything beyond the tenant's own
    aggregate to other tenants.
- **RF-2 — Extended tenant telemetry.** In addition to `/proc/swaps`, the
  agent reads **`memory.swap.current`** (swap-scope cgroup v2) and
  **`/proc/diskstats`** (derive `page_io/s` for the swap device), and carries
  them in `Msg::Psi` (new optional `mem` field; graceful degradation if absent).
  - **Acceptance criterion:** the broker receives per-tenant `swap_used`
    (cgroup) and `page_io/s`; when cgroup/diskstats does not exist, the field is
    `None` and the collector uses `/proc/swaps` without breaking.
  - **Isolation:** read-only access to the tenant's own `/proc` and cgroup; no
    cross-tenant access.
- **RF-3 — VRAM attribution by subtraction (host).** The broker samples
  `mem_info()` → `vram_total_used = total − free`; computes
  `vram_alloc_daemon = Σ slice.len (Active|Draining|Leased) + canary region`;
  derives `vram_outros = max(0, vram_total_used − vram_alloc_daemon)`.
  - **Acceptance criterion:** with the daemon serving K MiB,
    `vram_alloc_daemon ≈ K` (±1 page); `vram_outros` rises observably when a
    Windows graphics application opens.
  - **Isolation:** `mem_info` is device-wide (not per PID); subtraction does
    not disclose process identity.
- **RF-4 — Reconciliation invariant plus divergence flag.** For each sample,
  compute `Σ slices(broker) ≈ Σ SwapUsed(tenants) ≈ vram_alloc_daemon`. If
  `|divergence| > tol` for `streak` samples, raise
  `flag ∈ { eviction, stuck_slice, unaccounted }`, disambiguated by the canary:
  `demotes↑` ⇒ `eviction`; a long-lived `Draining` slice ⇒ `stuck_slice`;
  `vram_outros` growth without a corresponding `Status` ⇒ `unaccounted`.
  - **Acceptance criterion:** a synthetic stuck slice (does not reach zero) →
    `stuck_slice`; synthetic graphics pressure on the host → `eviction` (with
    `demotes ≥ 1`); normal convergence → `flag = none` (no false positive in
    the P0-measured idle window).
  - **Isolation:** read-only decision; **does not** alter arbiter state
    (observer, not actor).
- **RF-5 — Unified reconciled line (integrity-preserving output).** Emit one
  JSON record per sample to `docs/benchmarks/results.jsonl` (and a human
  summary in `docs/BENCHMARKS.md` when it is a benchmark), per
  `.claude/rules/benchmarks.md`. Fields: `t, tenant, slice, swap_used,
  page_io_s, vram_alloc_daemon, vram_total_used, vram_outros,
  canario_demotes, demote_reason, reconcile_delta, flag, branch, commit`.
  - **Acceptance criterion:** each line is valid parseable JSON, and
    `reconcile_delta`/`flag` agree with the same-instant `StatusReply`;
    append-only (never rewrite).
  - **Isolation:** local file; no PII (see RNF-LGPD).

## Non-functional requirements

- **Performance:** sample at **1 Hz** (reuse the 1 s `PSI_PERIOD`,
  `agent/main.rs:27`). The serve hot path adds only **two relaxed atomic
  increments** per operation (RF-1) → negligible cost versus the measured
  241 µs serve time (P0 §3); target: <1% overhead in p50 serve latency
  (validate with the canary).
- **Security:** no new exposed attack surface; `Status` already exists in the
  protocol (private network only, parent PRD RNF-2). `/proc`/cgroup reads are
  read-only for the tenant itself. No secrets in the telemetry line.
- **Observability:** **this is the observability feature.** Dual output
  (machine JSONL plus human Markdown) plus the divergence flag. No Prometheus
  dependency in the MVP (`Status` pull plus JSONL append).
- **Scalability:** linear in tenant count (the broker already iterates tenants
  in `StatusReply`); the JSONL line is O(slices+tenants) per sample. Fixed 1 Hz
  cadence → predictable volume (~1 line/tenant/s).
- **LGPD:** **no personal data.** The telemetry is infrastructure data (bytes,
  IO, VRAM, PSI). No per PID in the MVP (attribution is anonymous subtraction).
  Retention = the benchmark file (manual rotation). Reassess if a future RF adds
  per-PID/user data on the host.
- **Resilience:** if a source fails, **degrade gracefully** (`None` field,
  `partial` flag); never abort the broker. Unavailable `Status` (agent down) →
  tenant marked absent (reuse `present`). A cuMemGetInfo failure → `vram_*` =
  `None` (never mask as 0). **Never** introduce thrash/pressure on the live
  host (benchmark rule plus WSL2-freeze risk).

## Flows

**Happy path**
1. The agent (tenant) reads PSI + `/proc/swaps` + `memory.swap.current` +
   `/proc/diskstats` (RF-2) and sends `Msg::Psi { sample, swaps, mem }` to the
   broker (1 Hz) — `agent/src/main.rs` loop.
2. The broker updates `TenantState`/ledger; the worker increments
   `bytes_served/io_count` per slice in serve (RF-1).
3. The collector tick (1 Hz) in the broker reads its ledger, sums `Σ slices`
   and `Σ SwapUsed`, samples `mem_info()` (RF-3), and reads `demote_count()`.
4. It reconciles the invariant (RF-4) → `reconcile_delta` plus `flag`.
5. It emits the JSONL line (RF-5) through `Outbound::Log` (structured dispatcher).

**Alternative flows**
- **No co-located tier / zero active tenants:** the collector still records
  `vram_total_used`/`vram_outros` (useful for the “how much idle VRAM exists”
  angle, BENCHMARKS Q1a).
- **On-demand `Status`:** an external tool sends `Msg::Status` and receives a
  point-in-time `StatusReply` without depending on continuous append.

**Error flows**
| Condition (trigger) | Client result | Log/level plus fields | Consistency impact |
|---|---|---|---|
| Agent without cgroup/diskstats | `mem=None` field in `Psi` | `warn` `tenant`, uses `/proc/swaps` | none (degrade) |
| `cuMemGetInfo` fails | `vram_*=None`, `flag=partial` | `warn` `op=cuMemGetInfo` | partial reconciliation |
| Divergence > tol for streak | `flag ∈ {eviction,stuck_slice,unaccounted}` | `error` `delta`, `reason` | signal (does not corrupt state) |
| `Status` while agent is down | tenant `present=false` | `info` | reconciliation ignores tenant |

## Data model

In-memory structures (no database). **Extends** the existing state:
- `Slice` (**modified**, `ramshared-broker/src/slices.rs`):
  `+ bytes_served: u64, + io_count: u64` (incremented in serve; zeroed in
  `Free`). No kernel ABI-alignment change (it is an internal Rust struct,
  serialized through serde in `StatusReply`).
- `TenantStatus` (**modified**, in `StatusReply`):
  `+ swap_used_cgroup: Option<u64>, + page_io_s: Option<u64>, + bytes_served:
  u64`.
- `Msg::Psi` (**modified**): `+ mem: Option<TenantMem>` where
  `TenantMem { swap_current: u64, diskstats_io: u64 }` (new field,
  backwards-compatible through `Option` — JSON-lines tolerates absence).
- `TelemetrySample` (**new**, serde→JSONL): RF-5 fields. Lifecycle: created by
  tick, serialized, discarded (no retained state beyond the file).
- **Memory regions:** no new VRAM/DMA allocation; counters are `u64` in the
  broker heap. `vram_alloc_daemon` derives from the ledger (Σ `slice.len`) plus
  the canary region (`CANARY_BYTES`).

## API / Interfaces

There is **no new kernel uAPI** (ioctl/sysfs/debugfs/IRQ/DMA) — this is a
userspace daemon. The “API” is the existing **`ramshared-broker` TCP JSON-lines
protocol** plus **JSONL output**.

| Field | Value |
|---|---|
| Operation | Pull RPC `Msg::Status` → `Msg::StatusReply` (**existing, extended**) plus JSONL append stream |
| Path | TCP (private network, `--arbiter-listen`); `docs/benchmarks/results.jsonl` file |
| Permissions | private network only (parent PRD RNF-2); local file 0644 |
| Rate limit | fixed 1 Hz cadence (no amplification) |
| Idempotency | `Status` is idempotent (read-only); JSONL line is append (each sample unique by `t`) |

**Extended `StatusReply` — example:**
```json
{ "type": "status_reply",
  "tenants": [ { "tenant_id": 1, "name": "civm", "present": true,
                 "swap_used_cgroup": 4194304, "page_io_s": 512, "bytes_served": 268435456 } ],
  "slices": [ { "id": 0, "offset": 0, "len": 134217728, "tenant": 1, "state": "active",
                "bytes_served": 134217728, "io_count": 32768 } ],
  "last_rebalance_secs": 12 }
```

**Telemetry line (JSONL, RF-5) — example:**
```json
{ "t": 1718500000, "tenant": "civm", "slice": 0, "swap_used": 4194304, "page_io_s": 512,
  "vram_alloc_daemon": 134217728, "vram_total_used": 1517445120, "vram_outros": 1383227392,
  "canario_demotes": 0, "demote_reason": null, "reconcile_delta": 0.004, "flag": "none",
  "branch": "feat/p1-hardening", "commit": "1fba443" }
```

**Errors (protocol):** reuse existing `Msg::Error { reason }`. No new kernel
error codes.

**ABI impact:** **no** kernel-uAPI layout change. Changes are serde-serialized
Rust structs (JSON) — backwards-compatible through `#[serde(default)]`/
`Option` (old agent ↔ new broker tolerates absent fields; **Day-0:** because
there is no live production, update both in the same release).

**Interrupts/workqueues:** N/A (userspace).

## Dependencies and risks

**Prerequisites:** broker P1 (ready), residency canary (ready), `mem_info`
(ready).

| Risk | Mitigation |
|---|---|
| **Reconciliation-tolerance calibration** (noise versus signal) *(Inference)* | Measure natural divergence in P0 (idle window plus load); define `tol` + `streak` (arbiter-like hysteresis). Kahneman #3 (number) in the SPEC. |
| Counters in the serve **hot path** | relaxed `AtomicU64`, no lock; validate <1% overhead via canary (performance RNF) |
| `memory.swap.current`/cgroup path **varies** by distro/tenant | detect cgroup-v2 path; `Option` plus degradation to `/proc/swaps` |
| **NVML/DXGI unavailable** (per PID) | subtraction MVP (RF-3); per PID = host `ramshared-nvml` crate (out of scope) |
| **GPU-PV** prevents host telemetry inside WSL2 | document it; collector runs in broker (WSL2) with cuMemGetInfo; DXGI/per PID = future host work |
| JSONL volume grows | append-only plus manual rotation (scalability RNF); 1 line/tenant/s |

**Breaking changes:** none for kernel/uAPI. New protocol fields are additive
(`Option`).
**Rollout:** behind the already-merged P1; enabled by flag (for example,
`--telemetry-jsonl <path>`).
**Rollback:** the collector is an observer *overlay* (does not change the
arbiter) → turning the flag off reverts with no data-plane effect. **Rollback
trigger:** if RF-1 counters degrade serve latency by >2× the baseline (P0) →
revert RF-1 (`git revert`).
**Likely Kahneman disciplines in the SPEC:** #3 (tolerance numbers), #1
(record state/load in samples — WYSIATI), #5 (the `eviction` flag detects an
external worst case).

## Implementation strategy

Slices (each compiles and is independently testable):
1. **RF-1** per-slice counters plus exposure in `StatusReply` (test: wire
   injects load, `Status` agrees).
2. **RF-5** JSONL emitter (reuse `Outbound::Log`) with already available fields
   (without cgroup/diskstats yet) — validates the output pipeline early.
3. **RF-3** VRAM subtraction in the broker (cuMemGetInfo plus Σ ledger) —
   validate with a VRAM smoke on the host.
4. **RF-2** extended tenant telemetry (cgroup plus diskstats) in the agent plus
   `Msg::Psi` field.
5. **RF-4** invariant plus divergence flag — last (depends on 1–4); calibrate
   `tol`/`streak` with P0 numbers.

**Testable early:** RF-1 + RF-5 (on the live host, without pressure).
**Environment required:** the end-to-end `eviction` flag (RF-4) needs real
graphics load on the host (live host, bounded) or the civm session.
**No migration/backfill** (Day-0; no live production).

## Out of scope

- **`ramshared-nvml` plus per-PID DXGI** (“other” attribution by process, on
  the Windows host): the MVP uses subtraction (RF-3). Reason: GPU-PV does not
  expose reliable per PID information in WSL2; it is a separate subsystem
  (separate PRD, aligned to P2/P3).
- **Prometheus exporter / dashboard:** the MVP provides JSONL plus `Status`.
  Reason: avoid a dependency; it can arrive later without blocking.
- **Acting on divergence** (auto-correct a stuck slice, and so on): the
  collector is an **observer**. Action (revoke/release) already belongs to the
  arbiter/canary. Reason: separate observation from control (RF-4 isolation).
- **Database persistence/time series:** a JSONL file is sufficient for the MVP.
  Reason: simple Day-0.
- **Render/DCC telemetry (P2):** depends on Alex and the add-on; another PRD.
