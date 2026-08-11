# SPEC — Memory broker telemetry collection and reconciliation

> **Single-file RamShared model:** Step 2.5 revises this `SPEC.md` in place; history remains in `git log` — no `SPECvN.md`.
> **Reviewed after the Step 2.5 audit:** fixes blockers F1 (exact increment point), F2/F12 (the invariant conflated
> capacity × occupancy × throughput; `reconcile()` referenced signals outside `ReconcileInput`),
> F3 (sample timestamp/`PartialEq`), F8 (`swap_dev` for diskstats), F9 (`swap_used` source),
> and F4/F5/F6/F7.

> **Audited SPEC:** `SPEC.md`. **Blockers addressed (first round):** F1, F2, F3, F5, F8, F9 (+ F4, F6, F7).
> **Re-audited (Step 2.5, second round) → `no-go`; corrected in place:** F-v2-1 (`delta` order in
> `reconcile`), F-v2-2 (`TenantState.occupied_bytes` for the invariant), F-v2-3 (KiB→bytes units),
> F-v2-4 (`branch`/`commit` through environment, not `git`), F-v2-5 (sink wiring in `core_loop`).
> **This version is the active candidate** for a new audit (Step 2.5) / implementation (Step 3).
>
> SSDV3 Step 2. Userspace (`ramshared-broker`, `ramshared-wsl2d`,
> `ramshared-agent`). No kernel uAPI/IRQ/DMA/kernel lock. Linked to
> [`.claude/rules/benchmarks.md`](../../../../.claude/rules/benchmarks.md).

## Closed scope for this implementation

**Included now:** RF-1 (per-slice IO/byte counters, shared atomics, in the data
plane); RF-2 (extended tenant telemetry: filtered `/proc/swaps` +
`memory.swap.current` + `/proc/diskstats`); RF-3 (VRAM by subtraction through a
gauge published by the residency closure); RF-4 (**occupancy** invariant plus
flag); RF-5 (one JSONL line per sample).

**Excluded now:** per-PID `ramshared-nvml`/DXGI; Prometheus exporter; acting on
divergence (observer); database persistence.

**Ready dependencies (Confirmed in codebase):** `Msg::Status`/`StatusReply`
(`protocol.rs:46,69`); `SliceMap`/`Slice` (`slices.rs`, `model.rs:30`);
`BrokerCore`/`CoreEvent`/`Outbound`/`on_tick`/`status_reply`/`core_loop`/
`dev_to_slice` (`broker_srv.rs:40,50,70,128,473,492,726,+`); **broker worker
`serve_broker_jobs<B: BlockBackend>(backend, rt: &BrokerRuntime, residency: impl FnMut(u64)->Option<DemoteReason>)`**
(`main.rs:665`), serving at `serve(&job.req,&job.payload,&mut view)` (:705)
with `job.export` = slice; `run_broker`/`run_nbd` allocate
`canary_region`+`CanaryProbe` and the residency closure calls
`provider.mem_info()` (`main.rs:420,520,420`);
`WMsg::Job(Job{export,req,payload,reply})` (`conn.rs:48`); agent
`read_psi`/`read_swaps` (`agent/psi.rs:15,44`), `Msg::Psi` send at 1 Hz
(`agent/main.rs:277`).

## Matriz de rastreabilidade PRD → SPEC

| PRD  | Implementação no SPEC |
| ---- | ----------------------- |
| RF-1 | ITEM-1, ITEM-2, ITEM-5 |
| RF-2 | ITEM-1, ITEM-6 |
| RF-3 | ITEM-3, ITEM-7 |
| RF-4 | ITEM-4 (`telemetry.rs`), ITEM-7 |
| RF-5 | ITEM-4, ITEM-7, ITEM-8 |

## Decisões técnicas

| #    | Decision | Rationale |
| ---- | ------- | ------------- |
| DT-1 | IO counters in `Arc<Vec<SliceIoCounters>>` (atomics), **not** in `struct Slice`. | IO flows in the data-plane thread (`serve_broker_jobs`); `Slice`/`SliceMap` is lock-free, single-thread control plane (DT-27). |
| DT-2 | `StatusReply` gains parallel `slice_io: Vec<SliceIo>`; `Slice` does not change. | `Slice` is state plus wire with `Eq` (roundtrip tests, `model.rs:29`). |
| DT-3 | Telemetry cadence = broker `on_tick` (2 s, DT-24). | Reuses the existing tick; rapid eviction comes from the per-request canary. |
| **DT-4 (revised F2)** | The **invariant is OCCUPANCY**, not throughput: compare `alloc_active = Σ slice.len (Active\|Draining)` (borrowed capacity) with `occupied = Σ used` in **our** NBD devices. `bytes_served`/`io_count` (RF-1) are **throughput** (feed `page_io_s`), **outside** the invariant. | Capacity, occupancy, and throughput are distinct quantities; mixing them produced a meaningless reconciliation (F2). |
| **DT-5 (revised F5)** | `vram_alloc_daemon = alloc_active + CANARY_BYTES` (VRAM backend only; RAM has no canary). `Arc<VramGauge>{free,total}` is published **inside the residency closure** of `run_broker` (where `provider.mem_info()` already runs, §9.4 cadence). `total==0` ⇒ no VRAM data ⇒ `None` fields (sentinel, F6). | The residency closure is the only thread-affine point that calls `mem_info`; RAM (qemu) does not publish → `None`. |
| **DT-6 (revised F2/F12)** | **Eviction is detected by the canary** (`demotes_delta>0`), **not** by VRAM subtraction (WDDM-evicted memory remains “allocated” in `cuMemGetInfo`). `vram_outros` (subtraction) is an **informational** graphics-pressure indicator, emitted but **not** used in `reconcile()`. | `cuMemGetInfo` does not distinguish resident VRAM from eviction; latency is the real signal (canary). |
| DT-7 | Configurable `tol_frac` plus `streak`; provisional defaults `tol_frac=0.10`, `streak=3` ticks; **calibrated in P0**. | A number, not an adjective (#3); analogous to `delta_psi` (`P0-RESULTS §5`). |
| **DT-8 (revised F3)** | Two types: **`TelemetryCore`** (emitted by the core; `Clone+Debug+PartialEq`; **without** `t`/`branch`/`commit`) and **`TelemetrySample`** (the IO layer wraps it, **adding** `t`=epoch and `branch`/`commit`; `Serialize`). `Outbound::Telemetry(TelemetryCore)`. | The core has only `now: Instant` (monotonic, not epoch) and must not read a clock; IO stamps wall clock. Solves the required field plus `PartialEq` required by `Outbound` (`broker_srv.rs:50`). |
| DT-9 | `Msg::Psi.mem: Option<TenantMem>` with `#[serde(default)]`. | Graceful degradation; roundtrip tolerates absence. |
| **DT-10 (revised F9)** | `occupied` counts **only our NBD devices**: filter `Msg::Psi.swaps` by exact `nbd[0-9]+` identity in accepted formats (`/dev/nbdN`, `/nbdN`, `nbdN`) and match active slices. Similar names, other block devices, and `(deleted)` entries are not slices. `/proc/swaps` (`used_kb`) is the **primary source**; `memory.swap.current` (cgroup) is an **optional** cross-check (meaningful only under a confined cgroup). | The invariant concerns occupancy of OUR slices; tenant-local swap is something else (reported separately). Exact identity prevents `/dev/sda5` from re-adopting or accounting for slice 5. |
| **DT-11 (revised F8)** | The agent tracks the set of NBD devices it `swapon`ed (from executed `SwapOn`); `diskstats_io = Σ read_diskstats(dev)` over them. | `read_diskstats` needs a concrete device; the agent knows which it mounted. |
| **DT-12 (revised F4)** | `streak`: `on_tick` maintains `(last_flag, count)`. It counts consecutive ticks with the **same** non-`None` flag; it **emits the flag** (≠`None`) only when `count ≥ streak`; it resets on `None` or a flag change. Below the threshold, the sample exits with `flag=None` (pending). | Removes the vagueness of “apply streak”; explicit hysteresis (like the arbiter). |
| **DT-13** | The daemon entry point builds its broker config from an injected argv plan before backend selection. The safe test runtime permits only temporary Unix sockets, loopback TCP, heap RAM, and bounded joins; it preserves `telemetry_jsonl`, endpoint, counter, and gauge wiring exactly. | The telemetry sink and broker endpoints are part of the control plane. Testing them through CUDA/swap/ublk would confuse a hardware proof with a deterministic wiring contract. The separate server-only live gate remains required. |
| **DT-14** | `BrokerCore` receives one typed `BrokerCoreConfig` containing arbiter, endpoint, telemetry, gauge, tolerance, and reconciliation settings. | The prior eight positional arguments allowed silent field transposition and required a lint exception. A named config changes no wire value or runtime policy. |

## Atomicity boundary and rollback policy

**Atomic:** each `WMsg::Job` served in `serve_broker_jobs` performs
`bytes_served.fetch_add(req.len, Relaxed)` plus `io_count.fetch_add(1,
Relaxed)` on slice `job.export`. Each individual increment is atomic.
**Outside atomicity (eventual, F7):** reconciliation reads three sources at
different instants; `tol_frac` plus `streak` absorb skew. `status_reply`/
`on_tick` read `(bytes,io)` `Relaxed` pairs **without** a jointly atomic-read
guarantee — one-tick skew is **accepted** (telemetry, not financial accounting).
**Accepted partial states:** missing source → `None` field plus `flag=Partial`;
never abort the broker.

**Rollback:**
- **App:** turn off `--telemetry-jsonl` → zero emission (atomics remain,
  negligible cost). Revert counters = `git revert` ITEM-2.
- **Migration:** N/A (no database/schema). **Data:** N/A — only deletable,
  append-only JSONL.
- **Forbidden staging/production / forward-only:** N/A (local feature, no live
  production — Day-0).

## Kahneman map by critical stage

| Stage / ITEM | Discipline | Link | Required question | Minimum evidence | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-2 (counters in `serve_broker_jobs` hot path) | #5 Availability + #3 Number | [`kahneman-disciplines.md#5-availability-heuristic`](../../../methodology/kahneman-disciplines.md#disc-5) | Do two `fetch_add(Relaxed)`/op degrade serve p50/p99? | VRAM smoke: serve p50 with versus without counters, ≥3 rounds | p50 > **2×** baseline (P0 §3, 241 µs) → `git revert` ITEM-2 |
| ITEM-3 / RF-3 (gauge plus subtraction) | #1 WYSIATI + #3 | [`#1`](../../../methodology/kahneman-disciplines.md#disc-1) | Does `vram_alloc_daemon` agree with `Σ len + canary`? Is “other” contingent state (record it)? | VRAM smoke: `vram_alloc_daemon ≈ Σ len ± 1 page`; `vram_outros ≥ 0` | Systematic `vram_outros<0` → calculation wrong; do not advance |
| ITEM-4/7 (occupancy invariant plus flag) | #13 Illusion of validity + #1 | [`#13`](../../../methodology/kahneman-disciplines.md#disc-13) | Is divergence a real signal or noise? Does the flag fire for the right reason (eviction=canary, not subtraction)? | `reconcile()` fixture tests: `occupied>alloc → Unaccounted`; `demotes>0 → Eviction`; idle → `None`; no false positive in the P0 idle window | False positive in the P0 idle window → recalibrate `tol_frac`/`streak` (DT-7) before advancing |
| ITEM-8 (flag rollout) | #6 Calibrated confidence | [`#6`](../../../methodology/kahneman-disciplines.md#disc-6) | Does the default-off flag leave current behavior unchanged? | QEMU drill plus smoke **without** the flag = identical | Any regression without the flag → block |

## Security checklist (pre-implementation)

- [x] **Isolation:** collector is read-only over the ledger plus the tenant's
  own `/proc`/cgroup; **does not** mutate arbiter/SliceMap (RF-4 observer).
  `serve` already validates `len ≤ export` (`conn.rs:155`).
- [x] **OOB:** no new user↔kernel copy;
  `parse_memcg_swap`/`parse_diskstats` tolerate a malformed line (mirror
  `parse_swaps`, `psi.rs:50`).
- [x] **Permissions:** no new privileged path; read-only reads; graceful failure
  (`None`).
- [x] **Hot path:** only two `fetch_add(Relaxed)`/op (ITEM-2 gate).
- [x] **Secrets/KASLR:** the line carries neither kernel addresses nor secrets
  (`coding.md` rule).
- [x] **No panic:** errors become `None`/`flag=Partial`.

## Files to CREATE

### `crates/ramshared-wsl2d/src/telemetry.rs`  *(ITEM-4 — RF-4, RF-5, DT-1/4/6/8/12)*
- **Purpose:** shared types plus **pure** reconciliation logic (testable without
  GPU/network).
- **Requirements:** RF-1 (counter types), RF-3 (gauge plus `vram_outros`),
  RF-4 (`reconcile`), RF-5 (sample).
- **Structs/types:**
  ```rust
  use std::sync::atomic::AtomicU64;

  #[derive(Default)]
  pub struct SliceIoCounters { pub bytes_served: AtomicU64, pub io_count: AtomicU64 } // DT-1
  #[derive(Default)]
  pub struct VramGauge { pub free: AtomicU64, pub total: AtomicU64 }                  // DT-5

  #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
  #[serde(rename_all = "snake_case")]
  pub enum ReconcileFlag { None, Partial, Eviction, StuckSlice, Unaccounted }

  /// PURE reconciliation input (already collected from core/gauge). DT-4/DT-6.
  pub struct ReconcileInput {
      pub alloc_active_bytes: u64,   // Σ slice.len (Active|Draining)
      pub occupied_swap_bytes: u64,  // Σ used from OUR NBD devices (DT-10)
      pub stuck_draining: bool,      // any slice at pending_zero ≥ ZERO_RETRY_ERROR
      pub demotes_delta: u64,        // canary demotes since last sample (DT-6)
      pub any_source_missing: bool,  // any missing source → Partial
  }

  /// Sample emitted by CORE (without t/branch/commit — DT-8). PartialEq for Outbound.
  #[derive(Clone, Debug, PartialEq, serde::Serialize)]
  pub struct TelemetryCore {
      pub tenant: Option<String>, pub slice: Option<u16>,
      pub swap_used: u64, pub alloc_active: u64, pub page_io_s: Option<u64>,
      pub vram_alloc_daemon: u64, pub vram_total_used: Option<u64>, pub vram_outros: Option<u64>,
      pub canario_demotes: u64, pub demote_reason: Option<String>,
      pub reconcile_delta: f64, pub flag: ReconcileFlag,
  }

  /// Final line (IO layer wraps TelemetryCore — DT-8). One JSON object/line (RF-5).
  #[derive(Clone, Debug, serde::Serialize)]
  pub struct TelemetrySample {
      pub t: u64, pub branch: Option<String>, pub commit: Option<String>,
      #[serde(flatten)] pub core: TelemetryCore,
  }
  ```
- **Pure functions:**
  - `pub fn reconcile(inp: &ReconcileInput, tol_frac: f64) -> (f64, ReconcileFlag)` (F-v2-1: `delta`
    computed **first**):
    1. `let delta = (inp.occupied_swap_bytes as f64 - inp.alloc_active_bytes as f64) / inp.alloc_active_bytes.max(1) as f64;`
    2. `if inp.any_source_missing { return (delta, Partial) }`
    3. `if inp.demotes_delta > 0 { return (delta, Eviction) }`  // canary is authority (DT-6)
    4. `if inp.stuck_draining { return (delta, StuckSlice) }`
    5. `if delta > tol_frac { return (delta, Unaccounted) }`     // occupied more than borrowed
    6. `(delta, None)`
  - `pub fn vram_outros(total_used: u64, alloc_daemon: u64) -> u64 { total_used.saturating_sub(alloc_daemon) }` (DT-5 clamp; F6: called only when `total>0`).
- **Dependencies:** internal: none; external: `serde`.
- **Reference pattern:** `residency.rs` (pure logic plus tests without GPU).
- **Tests:** `reconcile_idle_none`, `reconcile_unaccounted_when_occupied_gt_alloc`,
  `reconcile_eviction_when_demotes`, `reconcile_stuckslice`, `reconcile_partial_when_missing`,
  `vram_outros_clamps`. (`#![allow(clippy::unwrap_used, clippy::expect_used)]`.)
- **Kahneman discipline:** supports ITEM-4/7 (#13) — see map.

## Files to MODIFY

### `crates/ramshared-broker/src/protocol.rs`  *(ITEM-1 — RF-1, RF-2)*
- **What changes / After:** (additive, backwards-compatible through
  `#[serde(default)]`/`Option`)
  ```rust
  Psi { sample: PsiSample, swaps: Vec<SwapEntry>, #[serde(default)] mem: Option<TenantMem> },
  StatusReply { tenants: Vec<TenantStatus>, slices: Vec<Slice>,
                #[serde(default)] slice_io: Vec<SliceIo>, last_rebalance_secs: Option<u64> },
  // new:
  pub struct TenantMem { pub swap_current: Option<u64>, pub diskstats_io: u64 } // DT-10/DT-11
  pub struct SliceIo { pub id: SliceId, pub bytes_served: u64, pub io_count: u64 }
  // TenantStatus += pub bytes_served: u64
  ```
- **Before:** `Psi { sample, swaps }` (:26);
  `StatusReply { tenants, slices, last_rebalance_secs }` (:69);
  `TenantStatus { id, name, psi, slices, present }` (:98).
- **Why:** RF-1 plus RF-2. **Impact:** additive JSON ABI;
  **breaks `roundtrip_each_variant` (:157)** → update literals in the same
  commit. No kernel ABI.
- **Tests:** update roundtrip; new `psi_mem_defaults_to_none`.

### `crates/ramshared-wsl2d/src/broker_srv.rs`  *(ITEM-5, ITEM-7 — RF-1, RF-4, RF-5)*
- **What changes:** `Outbound` += `Telemetry(TelemetryCore)`; `TenantState` +=
  `mem: Option<TenantMem>` **and** `occupied_bytes: u64` (F-v2-2: data summed
  by the invariant on tick); `BrokerCore` +=
  `slice_io: Arc<Vec<SliceIoCounters>>`, `vram: Arc<VramGauge>`,
  `demotes_total: u64`, `last_demote_reason: Option<String>`,
  `demotes_at_last_sample: u64`, `recon: (ReconcileFlag, u32)` (DT-12 streak),
  `tol_frac: f64`, `streak_cfg: u32`; `BrokerCore::new` receives a named
  `BrokerCoreConfig` containing `slice_io`,`vram`,`tol_frac`,`streak_cfg` and
  the existing arbiter/endpoint fields (DT-14).
- **`status_reply` (:473):** includes `slice_io` (reads `self.slice_io[i]`
  `Relaxed`) plus `TenantStatus.bytes_served` (Σ `slice_io` for a tenant's
  active slices) plus `mem`.
- **`Msg::Psi { sample, swaps, mem }` handler:** stores `mem` and
  **recomputes `occupied_bytes`** = Σ `used_kb*1024` (F-v2-3) for `swaps`
  whose `dev_to_slice(dev)` matches an active slice of this tenant
  (DT-10/F-v2-2).
- **`on_demote` (:435):** `self.demotes_total += 1;
  self.last_demote_reason = Some(reason.to_string())` (keeps `DemoteAll`).
- **`on_tick` (:492):** builds `ReconcileInput` — `alloc_active_bytes` = Σ len
  for Active|Draining; `occupied_swap_bytes` = Σ `TenantState.occupied_bytes`
  for present tenants (already filtered and converted in the `Psi` handler,
  DT-10/F-v2-2); `stuck_draining` = any
  `pending_zero ≥ ZERO_RETRY_ERROR`; `demotes_delta` =
  `demotes_total - demotes_at_last_sample` (then updates it);
  `any_source_missing` if `vram.total==0` or `mem` is absent. Calls
  `telemetry::reconcile`; applies `streak` (DT-12); computes
  `vram_total_used = (total>0).then(total-free)`,
  `vram_outros = vram_total_used.map(|u| telemetry::vram_outros(u, alloc_active+CANARY_BYTES))`;
  builds `TelemetryCore` and pushes `Outbound::Telemetry(core)`.
- **Why:** RF-1/RF-4/RF-5. **Impact:** `BrokerCore::new` signature changes →
  adjust `run_broker` plus tests (`:1078+`); `Outbound` match in dispatcher
  gains an exhaustive arm.
- **Tests:** `status_reply_includes_slice_io`; `on_tick_emits_telemetry`;
  `eviction_flag_after_demote` (inject `CoreEvent::Demote` → tick);
  `unaccounted_when_occupied_exceeds_alloc`. Update `:1086`.
- **Kahneman discipline:** ITEM-7 (#13/#1) — see map.

### `crates/ramshared-wsl2d/src/main.rs`  *(ITEM-2, ITEM-3, ITEM-8 — RF-1, RF-3, RF-5)*
- **ITEM-2 (RF-1):** in `serve_broker_jobs` (`:665`), after `serve(...)`
  (:705), guarded by `touches`:
  `rt.slice_io[job.export].bytes_served.fetch_add(job.req.len as u64, Relaxed); .io_count.fetch_add(1, Relaxed);`
  → add `slice_io: Arc<Vec<SliceIoCounters>>` to `BrokerRuntime` (`rt`, struct
  ~`:551`).
- **ITEM-3 (RF-3, DT-5):** in `run_broker`, change the residency closure passed
  to `serve_broker_jobs` (today
  `|| provider.mem_info().ok().map(|(f,_)| f)`, mirrors `:520`) to:
  `{ let (f,t)=provider.mem_info().ok()?; gauge.free.store(f,Relaxed); gauge.total.store(t,Relaxed); Some(f) }`.
  Create `Arc<VramGauge>` in `run_broker` and `Arc::clone` it for `BrokerCore`
  (RAM: gauge remains `total=0` ⇒ `None`).
- **ITEM-8 (RF-5, DT-8):** `core_loop` receives
  `sink: Option<TelemetrySink>` (`{ file: File, branch: Option<String>, commit:
  Option<String> }`) — `None` when the flag is off (F-v2-5). In dispatcher
  (`:808`), `Outbound::Telemetry(core)` arm → if `Some(sink)`, wrap it in
  `TelemetrySample { t: SystemTime epoch_secs, branch: sink.branch.clone(), commit: sink.commit.clone(), core }`,
  serialize (`serde_json::to_string` + `\n`), **append** (`sink.file`); error =
  `eprintln` warn (does not abort). Stamps come from environment variables
  `RAMSHARED_BUILD_BRANCH`/`RAMSHARED_BUILD_COMMIT` (launcher/harness; `None`
  when absent — qemu/initramfs has no `git`, F-v2-4). New flag
  `--telemetry-jsonl <path>` (default `None` = silent).
- **Before/impact:** hot path +2 atomics; no ABI; `--backend ram` →
  `vram_*=None`. `BrokerCore::new`/`BrokerRuntime` change → adjust calls.
- **Tests:** QEMU ublk-RAM drill PASS **without** the flag (RNF-4); VRAM smoke
  with the flag → `jq`-valid lines.
- **Kahneman discipline:** ITEM-2 (#5), ITEM-3 (#1/#3), ITEM-8 (#6) — map.

### `crates/ramshared-agent/src/psi.rs`  *(ITEM-6 — RF-2, DT-10/DT-11)*
- **After:** new functions (following `read_swaps`/`parse_swaps`):
  ```rust
  pub fn read_memcg_swap() -> Option<u64>;            // None if cgroup v2 is absent
  pub fn parse_memcg_swap(content: &str) -> Option<u64>; // integer; "max" → None
  pub fn read_diskstats(dev: &str) -> Option<u64>;    // device sectors (rd+wr) * 512
  pub fn parse_diskstats(content: &str, dev: &str) -> Option<u64>;
  ```
- **Why:** RF-2. **Impact:** read-only; `Option` when absent (DT-9).
- **Tests:** `parse_memcg_swap_integer`, `parse_memcg_swap_max_is_none`, `parse_diskstats_sums_rw`,
  `parse_diskstats_unknown_dev_none` (fixtures).

### `crates/ramshared-agent/src/main.rs`  *(ITEM-6 — RF-2, DT-11)*
- **What changes:** track NBD devices that were `swapon`ed (set updated when
  executing `SwapOn`/`SwapOff`); in `Msg::Psi` (`:277`):
  `mem = Some(TenantMem { swap_current: psi::read_memcg_swap(), diskstats_io: active_swap_devs.iter().filter_map(|d| psi::read_diskstats(d)).sum() })`
  (DT-9/DT-11).
- **Impact:** backwards-compatible; no active devices → `diskstats_io=0`.
- **Tests:** parsers in `psi.rs`; sending exercised in civm e2e (Q1d).

## Files to DELETE

| File | Reason |
| --- | --- |
| — | none (Day-0) |

## Observability

**Prometheus:** N/A in the MVP. Observability IS JSONL output plus
`StatusReply` (pull).
**JSONL (RF-5):** one `TelemetrySample`/line in `--telemetry-jsonl` (the
benchmark harness passes `docs/benchmarks/results.jsonl`, per
`.claude/rules/benchmarks.md`).

| Event | Level | Fields |
| --- | --- | --- |
| Divergence (flag≠None after streak) | `error` (`Outbound::Log`) | `flag`, `reconcile_delta`, `demote_reason` |
| Partial sample | `warn` | missing source (`mem`/`vram`) |

## Contracts and living documentation

| Document | Update | Reason |
| --- | --- | --- |
| `Documentation/`, `Kconfig`, `CLAUDE.md`, `.claude/rules/*` | N/A | userspace; no uAPI/CONFIG; convention already covered by `benchmarks.md` |
| `docs/specs/no-milestone/broker-telemetry-reconciliation/IMPL.md` | **Done** (IMPL.md present) | commits/decisions/metrics |
| `docs/reliability/memory-broker-p0-results.md` | **Change** | `tol_frac`/`streak` calibration (DT-7) |
| `docs/methodology/kahneman-disciplines.md` | N/A | uses existing disciplines (#1/#3/#5/#6/#13) |

## Implementation order

1. **ITEM-1** — wire types (`protocol.rs`) plus roundtrip tests. *(isolated)*
2. **ITEM-4** — `telemetry.rs` (types plus pure `reconcile` plus tests).
   *(isolated, testable now)*
3. **ITEM-2** — `SliceIoCounters` in `BrokerRuntime` plus increment in
   `serve_broker_jobs`.
4. **ITEM-3** — `VramGauge` published in the `run_broker` residency closure.
5. **ITEM-5** — `BrokerCore` (fields plus `status_reply` plus `on_demote` plus
   `Psi` handler) plus `Outbound::Telemetry`.
6. **ITEM-6** — agent (`read_memcg_swap`/`read_diskstats` plus `Msg::Psi.mem`
   plus device tracking).
7. **ITEM-8** — JSONL sink plus `--telemetry-jsonl` flag.
8. **ITEM-7** — reconciliation in `on_tick` plus `streak` (DT-12) plus emission.
9. Validation (tests plus drill plus smoke) and living docs.

## Test plan

**Backend (Rust):**
- **Unit:** `telemetry::reconcile` (idle/unaccounted/eviction/stuckslice/partial), `vram_outros`;
  `parse_memcg_swap`/`parse_diskstats` (fixtures); `protocol` roundtrip (+`Psi.mem` default).
- **Integration (in process, without GPU/network):** `status_reply_includes_slice_io`; `on_tick_emits_telemetry`;
  `eviction_flag_after_demote`; `unaccounted_when_occupied_exceeds_alloc`.
- **Atomicity:** N threads increment `SliceIoCounters` → exact sum per counter
  (cross-counter skew documented as accepted, F7).

**GPU/drivers:** server-only VRAM smoke (RTX 2060) with
`--telemetry-jsonl /tmp/t.jsonl`: `jq`-valid lines;
`vram_alloc_daemon ≈ Σ len + canary`; `vram_outros ≥ 0`.

**Daemon wiring (safe):** `daemon_args_accept_broker_wiring_and_normalize_addresses`,
`daemon_broker_config_preserves_telemetry_and_exact_endpoints`, and
`daemon_broker_ram_binds_loopback_and_cleans_owned_socket` use injected argv,
temporary sockets, loopback TCP, and heap RAM only. They prove DT-13 without
opening a CUDA context or configuring swap. The canonical per-file command and
the deferred `BINARY_MATCH/E2E` obligation are owned by memory-broker
ITEM-8's daemon entry-point contract.

**Manual:** `nc`+`jq` → `{"type":"status"}` → `StatusReply` with `slice_io`
(ADR-0005); civm e2e (Q1d): `eviction`/`unaccounted` flag under real load
(objective evidence for the ITEM-7 Kahneman map).

## Validation checklist

**Backend:**
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

**GPU:**
- [ ] VRAM smoke with `--telemetry-jsonl` (valid lines)
- [ ] QEMU ublk-RAM drill PASS **without** the flag (zero regression, RNF-4)

**Docs:**
- [ ] `IMPL.md` (STEP 3) + `P0-RESULTS.md` (`tol_frac`/`streak` cell)

**Cognitive gates:**
- [ ] ITEM-2/3/7/8 with discipline + link + question + evidence + abort
  (map above)
- [ ] No vague language at a critical point (tolerance is a number, DT-7;
  `streak` defined, DT-12; invariant is occupancy, DT-4; eviction = canary,
  DT-6)
