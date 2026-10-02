# MiMo audit — `feat/ramshared-v0.15.0-readiness` (PR #2088)

Date: 2026-09-29
Base: `1bcc9806` (`origin/main`)
Head: `b788c17f`
Scope: 176 commits, 217 files, +39,940 / −5,796 lines

This is an independent source audit of every commit reachable on the branch
that is not on `main`. It reviews the actual diff and the surrounding current
source, not the commit subjects. Findings are ordered by severity. Each
finding cites `file:line` against the working tree at `b788c17f`.

> **Revalidation note (2026-09-29):** the snapshot above describes the public
> PR head `b788c17f`, not the current local branch. The local branch is now at
> `cab5c0fc` (183 commits after `origin/main`, seven ahead of the PR head), and
> its worktree also contains uncommitted product/documentation changes. The
> current verdict for every finding is listed in the adversarial revalidation
> section at the end; it supersedes the original severity and action labels.

---

## 1. Critical

### C1 — Docs-labeled commit silently reverted PSI input hardening (fail-open regression)

Commit `286ccc19` is titled `fix(docs): resolve gitignored artifact link and align
slice coverage`, but 47 of its 48 changed lines are production code in
`crates/ramshared-agent/src/psi.rs`. It removed the hardening landed earlier in
the same branch:

| Removed by `286ccc19` | Originally added by |
| --- | --- |
| Duplicate-field rejection (`seen_avg10` / `seen_avg60` / `seen_total`) | `095e101a` fix(core): reject duplicate PSI pressure fields |
| `.filter(\|value\| value.is_finite() && *value >= 0.0)` on `avg10`/`avg60` | `77968105` fix(agent): reject invalid PSI averages |
| Test `parse_psi_rejects_nonfinite_or_negative_pressure` | `77968105` |
| Test `parse_psi_rejects_duplicate_required_fields` | `095e101a` |

Current state at HEAD (`crates/ramshared-agent/src/psi.rs:29-48`):

```rust
for tok in line.split_whitespace() {
    if let Some(v) = tok.strip_prefix("avg10=") {
        avg10 = v.parse::<f32>().ok();
```

Impact: `/proc/pressure/memory` is a trust boundary used for admission and
stress gating. A duplicated `avg10=` key (last write wins) or a `NaN`/`-inf`
value is again accepted as a pressure sample. The branch therefore claims
hardening in its history and PR body that the candidate binary does not
contain. The regression is invisible in `git diff main...HEAD` for that file
(net-zero), which is how it survived the coverage/CI gates.

Secondary damage in the same hunk: the error string was changed to Portuguese
(`"PSI ilegível"`, `psi.rs:17`), which violates the project English-only rule
for source and is not covered by the comment-language allowlist.

Action: restore the `095e101a` + `77968105` parser and their two tests, revert
the Portuguese string, and split any real docs change into its own commit.

### C2 — Validation-schema redaction gate can be used to rewrite historical PID evidence

`tools/ci/check-validation-schema.mjs:250-271` (`isSecurityRedaction`) added
normalizers that run **before** the numeric-invariant check:

```js
.replace(/\bPID\s+`\d+`/gi, 'a process')
.replace(/\bPID\b/gi, 'process')
```

Then (`check-validation-schema.mjs:318-324`):

```js
if (JSON.stringify(numbers(normalizedOld)) !== JSON.stringify(numbers(normalizedNew)) || ...) {
  return false
}
return normalizedOld !== oldLine && normalizedOld === normalizedNew
```

Because `PID \`12345\`` is erased to `a process` on both sides, the PID digits
never enter the `numbers()` comparison. An edit that changes any historical
process identifier in `validation.md` normalizes equal and is classified as an
allowed security redaction. Commit `15e4b70e` (`fix(ci): allow sanitized
historical evidence edits`) is the companion widening. Numeric metrics and
verdict emojis remain protected; process identity does not.

This is a gate-weakening surface in an evidence file that the repository uses
as the qualification record. Either restrict the PID normalizer to
whitelist-inserted redaction pairs, or compare PID tokens separately from the
generic numeric invariant.

Related smell: the banned token is assembled as
`new RegExp(['ad', 'voq'].join(''), 'gi')` (`check-validation-schema.mjs:263`),
i.e. obfuscated against grep. An audit cannot reason about a rule it cannot
search for.

---

## 2. High

### H1 — GPU cache mutations are fire-and-forget but report `Accepted`

`crates/ramshared-block/src/ipc_cache_client.rs:260-283` (`send_mutation_frame`)
performs one nonblocking `write` and returns `CacheMutation::Accepted` when the
frame is fully queued. The worker (`gpu_cache_worker.rs:420-427`,
`MSG_UPDATE` / `MSG_PROMOTE`) applies the mutation and sends **no response**.

Consequences:

- `Accepted` means "queued on the socket", not "stored in VRAM cache". A worker
  that dies or rejects the mutation after the write still leaves the caller
  with a success verdict.
- There is no end-to-end confirmation, no correlation of mutation to
  application, and no retry/rollback story at this layer.
- The mutation path is deliberately bounded (`MAX_MUTATION_FRAME_DATA_BYTES =
  64 KiB`, partial writes fail closed) — that part is sound — but the return
  value overstates the guarantee.

If best-effort cache semantics are intended, rename the variant (e.g.
`Queued`) or document the weaker contract in `isolated_origin.rs`; if durable
cache semantics are intended, add an ACK frame.

### H2 — Worker silently ignores unknown frame types

`crates/ramshared-block/src/gpu_cache_worker.rs:469`:

```rust
_ => {}
```

Any unrecognized `msg_type` is discarded with no error frame and no protocol
version negotiation. A future client and an older worker (or a corrupt
header) will desynchronize without telemetry. Combined with C1-class history
reverts, silent no-ops are the dominant failure style in this IPC layer.

### H3 — Wire header has six unvalidated reserved bytes and no version field

`gpu_cache_worker.rs:56-94`: `FrameHeader::encode` writes bytes `2..8` as
zero and `decode` never inspects them. There is no protocol version, so any
future header change is indistinguishable from a corrupt header. Reserved
bytes should either be rejected when non-zero or carry a version.

---

## 3. Medium — resource and workflow policy

These findings concern hard-coded policy values. Some are deliberately bounded
or have environment overrides; each entry distinguishes those controls from
the defaults and protocol limits that actually apply to production.

### M1 — GPU cache does not meet the any-VRAM-device goal; reserve policy is unqualified across adapter classes (`ramshared-block` / `ramshared-vram`)

The product requirement is broad GPU and capacity support: RamShared should
adapt safely to the VRAM and live budget of GPUs from different vendors, rather
than treating the RTX 2060 as the only target. “Any GPU with VRAM” still needs
an operating-system/driver API that can allocate, copy, identify the adapter,
and report a trustworthy live budget; where no such backend exists, RamShared
must explain that cache admission is unavailable and retain the safe origin
path.

The cache worker is generic over `VramProvider`; production currently
enumerates CUDA and Vulkan adapters and runs the same budget arithmetic for
either. Vulkan can expose devices from multiple vendors, but that API surface
alone does not prove the full lifecycle works across them. Each adapter needs a
stable identity and a fresh driver-reported budget; the Vulkan provider also
needs a usable transfer queue and positive `VK_EXT_memory_budget` data, or
automatic admission fails closed. When several adapters qualify, the selector
runs one chosen adapter; it does not pool VRAM across the GPUs in the host.
Physical AMD/Intel and other-adapter cache lifecycle qualification remains
open. The generic trait and RTX 2060 qualification do not establish the
product requirement.

The source and public docs had stronger claims than the implementation: the
Vulkan module called this "any GPU" support, and the FAQ called the project
"hardware-agnostic" while naming VMA and cross-process external-memory handles
that this provider does not use. Those statements are corrected in this
working tree. The operator guide now states backend prerequisites and keeps
physical multi-vendor support unqualified.

The production origin-cache path does **not** use several values from
`GpuWorkerConfig::default()`: `run_nbd_with_startup` passes the sealed
`physical_cache_cap_mib` as the target (up to the 24 GiB product limit),
`chunk_bytes_from_env()` (128 MiB default, clamped to 16–512 MiB), and
`reserve_floor_bytes_from_env()` (512 MiB default, clamped to 128–4096 MiB).
The struct defaults of 4 GiB, 2 MiB, and 1536 MiB are used only when callers do
not override them. Treating 1536 MiB and 4 GiB as the active production
defaults was incorrect.

There is nevertheless a real contract drift: the active GPU-cache PRD still
lists `max(1536 MiB, 20%)` as the host-contention mitigation, and the SPEC's
completion checklist claims that default as enforced. The production origin
caller passes the 512 MiB-default environment value instead. The dynamic 20%
reserve and independent 640 MiB buffer remain in force, but that does not make
the documented 1536 MiB absolute floor true. Keep the reserve-policy gate
PARTIAL until the product owner either raises the production floor or revises
the PRD/SPEC with capacity- and vendor-specific evidence.

For the origin-cache worker, the actual target is
`min(sealed_cache_cap, capacity - reserve, live_available - reserve - 640 MiB)`,
where `capacity = min(total, budget)` and
`reserve = max(configured_floor, ceil(capacity / 5))`. The 896 MiB recovery
threshold is applied when clearing `pressure_constrained`; with the default
production 512 MiB reserve floor, `required_free` is at least 1152 MiB, so the
896 MiB recovery threshold cannot bind unless that floor is overridden.

The 24 GiB bound is a cache/origin product limit, not a GPU-class limit: a
larger adapter can be discovered, but the current sealed origin cannot cache
more than 24 GiB. At the other end, the 640 MiB runtime buffer plus the
production reserve floor can leave no safe target on small or heavily occupied
adapters; a target smaller than the selected allocation chunk cannot become
active. Capacity adaptation is product work in both directions.

For a fully free 6 GiB driver budget and production defaults, the 20% reserve
rounds up to 1229 MiB; adding the 640 MiB runtime buffer leaves 4275 MiB for
cache. A sealed 4 GiB cache cap is therefore fully admissible in that example.
The earlier calculation using the struct's 1536 MiB reserve default was not
the production path. Conversely, a 1 GiB device with the default 512 MiB floor
has no cache headroom after the 640 MiB runtime buffer, while larger devices
can use more, subject to the live driver budget and sealed origin cap.

Finding: this policy is capacity-aware in part, but its 20% ratio, absolute
buffers, chunk thresholds, and 24 GiB cache ceiling have not been qualified
across the intended range of VRAM capacities, vendors, display workloads, and
budget APIs. Evidence must include low-capacity cards that may correctly yield
a zero cache target and larger cards whose available VRAM exceeds the product
ceiling. The gap is practical: current evidence does not show safe, useful
cache operation across those classes. Origin-only fallback remains the safe
behavior when an adapter cannot satisfy the policy.

### M2 — Stress qualification thresholds (`ramshared-cli/src/stress.rs:24-46`)

`WSL2_MIN_PHYSICAL_HEADROOM_MB = 600`, `TIER1_AND_TIER2_QUALIFICATION_PCT =
95`, `TIER3_HEADROOM_RESERVE_MB = 16`, `TIER3_MAX_STEP_MB = 8`,
`PRE_TIER3_MAX_STEP_MB = 128`, `WSL2_TIER3_STEP_INTERVAL_MS = 500`,
`TIER3_HEAVY_LOAD_PCT = 80`, `CASCADE_RAMP_LIMIT_PCT = 1000`,
`TIER3_QUALIFICATION_RAMP_LIMIT_PCT = 6000`, and ~15 more.

Note the inconsistency: the Windows wrapper requires **20480 MiB** host
physical headroom (`Invoke-RamSharedThreeTierStress.ps1:98-99` via
`PressureAllocGiB 16` + reserve), while the Rust side publishes
`WSL2_MIN_PHYSICAL_HEADROOM_MB = 600`. These are different gates (guest vs
host) but nothing in source cross-references them; a reader can easily treat
600 MiB as the campaign gate.

### M3 — Derived latency model is presented as measured telemetry

`crates/ramshared-cli/src/monitor.rs:1325-1338`:

```rust
cp.disk_io.min_lat_us = 85.0;
cp.disk_io.avg_lat_us = ... 85.0 + (cp.disk_io.avg_mbs / 100.0) * 120.0 ...
cp.disk_io.max_lat_us = ... f64::max(180.0 + (cp.disk_io.max_mbs / 50.0) * 600.0, 1200.0)
```

These values are estimates derived from throughput with fixed coefficients
(85 / 120 / 180 / 600 / 1200 / 50 / 100), not measurements from the active
device. At the audited snapshot, the UI and JSON used the same numeric fields
as real measurements without recording their origin. The source ambiguity was
reproduced with zero disk I/O: the panel formatter returned
`85.00µs..180.00µs..1.2ms` as if it were observed latency.

**Current local fix:** `TierIoStats.latency_source` now serializes
`estimated`/`measured`/`unavailable`; old JSON without the field defaults to
`unavailable`. The dashboard labels the row “I/O Latency” and prefixes derived
values with `estimated:`. The page-fault row also says “Estimated” and no
longer calls the value hardware-accelerated or disk-fallback pressure. The
formulas remain estimates and no physical latency measurement was added.

### M4 — Legacy VRAM service device/port/size literals

`packaging/scripts/ramshared-vram-service.sh`:

| Literal | Line | Note |
| --- | --- | --- |
| `NBD_DEV="/dev/nbd0"` | 8 | device name not parameterized |
| `ZRAM_MIB=${RAMSHARED_ZRAM_MIB:-1024}` | 16 | default 1 GiB, env-overridable (good) |
| `memory.min = 536870912` (512 MiB) | 24 | cgroup floor, raw bytes |
| `memory.low = 1073741824` (1 GiB) | 27 | cgroup low, raw bytes |
| `--listen-nbd 127.0.0.1:10809` / `--arbiter-listen 127.0.0.1:9090` | 336 | hardcoded ports |
| `DAEMON_BIN="/usr/local/bin/ramsharedd"` | 11 | install path |
| `max_slice_cap_mib=4096` | 60 | capacity claim cap |

The script's ownership/idempotency hardening on this branch (symlink rejection,
exact `/proc/swaps` matching, swapoff-first teardown) is real and tested by
`scripts/safety/test-legacy-vram-service.sh` (756 lines); the literals above
are the remaining fixed policy.

### M5 — Windows three-tier stress sealed-equality constants

`scripts/windows/Invoke-RamSharedThreeTierStress.ps1:15-17, 98-104`:

- `TimeoutSec = 1800`, `HostCommitReserveMiB = 4096`, `HostPhysicalReserveMiB = 4096`
- `PressureAllocGiB 16` (twice) → required headroom = 16384 + reserve
- `guestMemAvailableReserveMiB = 1024`, `guestSwapFreeReserveMiB = 1024`
- Manifest must be **exactly** `logical_capacity_mib -ne 4096` and
  `physical_cache_cap_mib -ne 4096` (line 103-104) — any other sealed origin
  size is refused. This is intentionally fail-closed but means the harness is
  welded to one 4096 MiB fixture.

### M6 — Resource configuration paths and limits

`crates/ramshared-cli/src/resource_config.rs:26-36`:
`DISCOVERY_TIMEOUT = 10 s`, `LINUX_INVENTORY_OUTPUT_LIMIT = 1 MiB`,
`WINDOWS_INVENTORY_OUTPUT_LIMIT = 256 KiB`,
`DEFAULT_RESOURCE_PROFILE_PATH = "/etc/ramshared/resource-profile.toml"`,
`STORAGE_SAMPLE_MAX_AGE_MS = 30_000`, `STORAGE_SAMPLE_FUTURE_TOLERANCE_MS =
5_000`, `MAX_DRAFT_LINE_BYTES = 128`, `MAX_DRAFT_TARGETS = 16`, and fixed
draft relative paths `swap/ramshared-fallback.swap` /
`origin/ramshared-origin.img`. Reasonable bounds; the profile path is the one
that will hurt in packaging.

---

## 4. Gaps

### G1 — Tier ceilings collected by the wizard are never enforced

`TierCaps` (`crates/ramshared-config/src/resource_profile.rs:36-50`) is written
by `collect_draft_profile` (`resource_config.rs:2373-2380`) and rendered by
`render_plan_text` (`resource_config.rs:1555-1576`). The only other readers are
the plan warnings. No consumer in `ramshared-wsl2d`, `ramshared-block`, the
stress path, or the cascade path reads `caps.zram_bytes` / `caps.origin_bytes`
/ `caps.vram_bytes`. `apply_enabled` is hard-coded `false` at every plan
construction site (`resource_config.rs:1379, 1543, 2410`).

The wizard text says "Tier ceilings are profile policy only" (`resource_config.rs:2385`)
so this is disclosed — but the field name `ceiling` and the PR wording
("expose safe resource draft ceilings") read as enforcement. Either wire the
caps into admission or rename them to `planned_caps` until an apply path
exists.

### G2 — Production entrypoints still do not start the vsock control plane

`crates/ramshared-winsvc/src/control_plane.rs:4-5` states the AF_HYPERV
transport lives in `ramshared-ipc::vsock` and "neither the" daemon starts it.
`crates/ramshared-ipc/src/vsock.rs` (947 lines) and `host_gate.rs` are
hermetically tested only. The GAP register already tracks this
(`WSL2 control-plane stability` row); this audit confirms no new wiring landed
on the branch. Handshake, lease revocation on disconnect, and a live
host/guest exchange remain unproven.

### G3 — Worker IPC loop has no read deadline of its own

Client side is deadline-bounded (`read_exact_until` / `write_all_until`,
`ipc_cache_client.rs:41-80`), which is good. The worker loop
(`gpu_cache_worker.rs:338-350`) uses bare `socket.read_exact` with no timeout.
A client that stalls mid-frame blocks the worker forever; only process-level
teardown recovers it. If the worker is intended to be supervisable, it needs
the same absolute-deadline discipline.

### G4 — PR body metadata drift

- PR #2088 claims "All 172 commits reachable from `main`"; `git rev-list
  --count 1bcc9806..HEAD` is **176**.
- The template "Why it was done" column is filled with the placeholder
  "Record or enforce behavior." for 140+ rows instead of the per-commit
  rationale the template requires.

### G5 — Slice-coverage exclusion logic is a permanent blind spot to watch

`tools/ci/check-rust-slice-coverage.mjs:8` excludes inline `#[cfg(test)]`
module regions and unit-test symbols from production line metrics. The
classification is unit-tested (`plan-rust-slice-coverage.test.mjs`), which is
the right mitigation, but any future misclassification of production code as a
test region would silently lower the coverage denominator. The error string
"could not match the body of an inline cfg(test) Rust module"
(`check-rust-slice-coverage.mjs:271`) is the correct fail-closed behavior and
should stay.

### G6 — Seven reliability gates remain PARTIAL (pre-existing, still open)

Per `docs/reliability/GAP-REGISTER.md` and the PR description, unchanged by
this audit: resource configuration live E2E; WSL2 freeze / GPADL lifetime;
control-plane production wiring; legacy handoff `BINARY_MATCH` repetition;
physical multi-vendor GPU lifecycle; Windows physical package/recovery;
Windows virtual-disk matrix. Build #5 three-tier stress is `BLOCKED`. This
audit found nothing that closes any of them.

---

## 5. Low

| ID | Finding | Location |
| --- | --- | --- |
| L1 | `DISABLE_RESP` write errors ignored (`let _ = socket.write_all(...)`) — acceptable for teardown, but leaves a silent half-close path | `gpu_cache_worker.rs:426` |
| L2 | `MSG_READ_REQ` overloads `aux` as the read length while responses overload it as KiB cached; no field-name-level contract on the wire | `gpu_cache_worker.rs:378`, `ipc_cache_client.rs:302` |
| L3 | `unix_time_ms()` swallows `SystemTime` errors via `unwrap_or_default()` (epoch 0) — budget telemetry would look ancient, which fails closed via freshness, so impact is low | `gpu_cache_worker.rs:37-41` |
| L4 | `parse_psi` maps kernel `total=` (cumulative stall µs) into `stall_us` and ignores `avg300` and the `full` line; only `some` is sampled. Documented in the function comment, but the field name suggests a per-sample stall | `psi.rs:28-48` |
| L5 | `FrameHeader` payload `data.len() as u32` on the worker read response relies on the prior `aux` length cap; safe today only because both paths share `MAX_IPC_PAYLOAD_BYTES` | `gpu_cache_worker.rs:388` |
| L6 | `isSecurityRedaction`'s `normalize()` list is open-ended (workstation names, storage labels, VM names). Each new normalizer widens the historical-edit allowlist; there is no test asserting the list cannot swallow a metric | `check-validation-schema.mjs:266-317` |
| L7 | At audited HEAD, GPU-cache status cast `cached_bytes >> 10` to `aux: u32`, wrapping to zero at 4 TiB. The current sealed origin-cache path is capped at 24 GiB, so this specific telemetry overflow is unreachable in that production path; it does not affect cache admission or allocation. Exported `run_gpu_worker_loop` accepts generic `GpuWorkerConfig` without that product cap, so the working tree now saturates the legacy telemetry field and adds a boundary test. Counts above `u32::MAX` KiB remain capped, not exact; a future product that needs exact telemetry above that limit must widen/version the wire field. This narrow counter defect has low current production impact; it is not evidence that broad GPU-vendor or VRAM-capacity support is unimportant. M1 records those practical support and qualification gaps separately. | Audited HEAD: `gpu_cache_worker.rs:372, 389, 405, 459`; working-tree fix/test: `gpu_cache_worker.rs:35-36, 510-517`; `ipc_cache_client.rs:181, 236, 329`; `ramshared-wsl2d/src/main.rs:92, 1203` |

---

## 6. Verified safe (checked, not findings)

- **IPC payload bounds.** Both sides enforce `payload_len <= 16 MiB`
  (`gpu_cache_worker.rs:352`, `ipc_cache_client.rs:292`) and the client
  enforces 4 KiB budget payloads (`ipc_cache_client.rs:237`) and 64 KiB
  mutation payloads (`ipc_cache_client.rs:264`) before any allocation.
- **Partial-write handling on mutations.** `try_write_frame`
  (`ipc_cache_client.rs:85-96`) fails closed on partial queue and restores
  blocking mode; regression-covered.
- **Budget admission fail-closed.** `can_admit_at`
  (`ramshared-vram/src/lib.rs:198-208`) requires adapter identity,
  `DriverReported` source, freshness window, and `budget <= total`; stale,
  future, mismatched, or query-error snapshots refuse.
- **`safe_target_bytes` / `required_free_bytes` use saturating arithmetic**
  (`ramshared-vram/src/lib.rs:155-189`) — no overflow path to over-commit.
- **Origin manifest BOM strip and SHA-256 verify** (`host_gate.rs:94-99`) are
  order-correct (hash over raw bytes, BOM stripped only for JSON parse).
- **Legacy service teardown hardening** (symlinked PID/ZRAM records rejected,
  exact NBD swap matching, swapoff failure refuses teardown, kernel-verified
  NBD detach) is present and covered by `test-legacy-vram-service.sh`.
- **Handshake correlation** is checked (`ipc_cache_client.rs:174`) and the
  deadline is absolute across partial frames (`read_exact_until`).
- **Draft wizard input validation** refuses zero, negative, malformed, and
  overflow MiB values before writing any file (`resource_config.rs:2088-2101`,
  `2410`), with regression cases including the invalid-cap loop added in
  `245146e5`.
- **`GpuBudgetSnapshot` conversions** in `dxg` / `vulkan` / `cuda` were
  spot-checked for identity binding (UUID/LUID) before budget use; the
  mismatch path fails closed.

---

## 7. Commit-surface notes

| Observation | Evidence |
| --- | --- |
| One commit is mislabeled and destructive (see C1) | `286ccc19` |
| Test-first pairing is largely respected (fix/test twins) | e.g. `7b5f809e`→`5876e4e6`, `3a1e611e`→`095e101a`, `fadec6b6`→`11f58dc8` |
| `check-agent-orchestration.mjs` (+606 lines) deleted along with its tests | `afa37ec4`; no remaining references found in `tools/` or workflows |
| Evidence volume is large (`validation.md` +3,171 lines, GAP register +314) | consistent with the repo's evidence discipline; not itself a defect |
| Coverage of new IPC/gpu_cache_worker code is hermetic (UnixStream pairs) | no live worker install/allocation on this branch, as the PR states |

---

## 8. Priority actions

1. **Restore the PSI parser hardening and its two tests** (C1); retitle or
   split `286ccc19`'s remaining docs change. Translate `"PSI ilegível"` to
   English.
2. **Close the PID-invariant hole** in `isSecurityRedaction` (C2) and
   de-obfuscate the banned-token regex so it is auditable.
3. **Decide the cache mutation contract** (H1): ACK frame or honest rename;
   also reject unknown `msg_type` (H2) and non-zero reserved bytes (H3).
4. **Label monitor disk latencies as estimates** (M3).
5. **Either enforce or rename `TierCaps`** (G1) before the wizard is described
   as providing ceilings.
6. Keep all seven PARTIAL gates and the BLOCKED stress row closed to
   promotion until their named evidence exists (G6).

---

*Audit method: full commit-range diff review of `crates/**`, `scripts/**`,
`packaging/**`, `tools/ci/**`, and `.github/workflows/**`; targeted reads of
the current source for each cited symbol; cross-check against
`docs/reliability/GAP-REGISTER.md`, PR #2088 body, and the validation-schema
gate implementation. No hardware or live-system claim is made or closed by
this document.*

## Adversarial revalidation against the current local branch

This section checked the original findings against `cab5c0fc`, current tests,
the current worktree, and the live metadata of PR #2088. “Confirmed” below
means the cited source behavior is present; it does not imply hardware proof.

| Finding | Revalidated result | Reproduction or evidence |
| --- | --- | --- |
| C1 — PSI parser regression | **Fixed locally; still present in public PR head `b788c17f`.** Duplicate required fields and non-finite/negative averages are rejected again. | Six `parse_psi` tests pass, including both refusal cases. Fix: `cb9ce87c`; tests: `e8440099`. |
| C2 — historical PID rewrite | **Fixed locally; still present in public PR head.** The gate compares PID tokens before path/provenance normalization. | Node regression test rejects replacing `PID 123456` with another PID; all 26 validation-schema tests pass. Fix/test: `0cbda6ca` / `f236295c`. Redacting the PID entirely remains an allowed privacy redaction. |
| H1 — mutation returns `Accepted` | **Narrowed; not a data-integrity defect.** The cache is explicitly best-effort and the origin remains authoritative. The public enum now says `Accepted` does not prove worker application or VRAM allocation. | Source/API contract comment added in `3474d8d9`; no durable-cache claim is made. |
| H2 — unknown worker frame ignored | **Confirmed and fixed locally.** Unknown types now fail the worker loop so the parent sees a closed transport and disables the cache. | New socket-pair test failed before the fix (worker waited for another frame and timed out), then passed after it. Fix: `cab5c0fc`. |
| H3 — reserved bytes ignored | **Confirmed and fixed locally on both endpoints.** Header decode rejects any nonzero byte in the six reserved positions. | New worker test previously received a valid handshake response for a malformed header; it now observes stream closure. A client-side malformed-response test also passes. Fix: `3474d8d9`. |
| M1 — any-VRAM / vendor coverage | **Real qualification/product gap, not a single parser bug.** CUDA and compatible Vulkan adapters share the provider contract, but this does not prove lifecycle support for every vendor or adapter size. | The local worktree now qualifies the wording and documents the safe no-cache fallback. No AMD/Intel/other-vendor physical lifecycle run was available, so universal GPU support remains unqualified. |
| M2 — stress thresholds | **Mostly a scope/measurement distinction, not one inconsistent limit.** The 600 MiB Rust floor, the Windows 16 GiB allocation plus reserve, and the guest MemAvailable/SwapFree reserves apply at different gates. | Source checks confirm the values are applied at separate guest and host admission points. They must not be added together or described as one physical-RAM measurement. |
| M3 — derived latency shown as telemetry | **Confirmed data-quality bug; fixed in the current local worktree.** Latency origins are explicit in serialized data and UI; estimates remain estimates. | The zero-I/O regression reproduced the unlabeled `85..180..1200 µs` disk values before the fix. The new monitor suite passes 46/46, including estimate labeling and backward-compatible JSON handling. No physical latency measurement was added. |
| M4 — legacy VRAM service literals | **Not a general resource-allocation bug.** These are fixed service defaults/paths, several with explicit environment overrides. | Static source review only; the legacy script is separate from the current origin-cache admission path. |
| M5 — exact 4 GiB stress manifest | **Intentional qualification fixture, not a product-size limit.** | The full-campaign wrapper refuses manifests outside its sealed test profile; it is not the general RamShared allocator. |
| M6 — resource-profile paths/bounds | **Policy/defaults requiring packaging qualification, not reproduced runtime defects.** | The wizard validates and stores a draft profile; native Linux and Windows package path behavior still needs platform E2E. |
| G1 — draft tier caps not enforced | **Wording fixed locally; enforcement still absent by design.** The wizard, plan text, and plan JSON now label the values as unenforced planned draft caps (`PlannedTierCaps`, `[planned_caps]`, `unenforced_planned_caps`); legacy `[caps]` drafts stay readable through a serde alias and a draft carrying both table names is rejected. `apply_enabled` remains hard-coded false and no runtime consumer reads the caps, so the honest fix was the rename plus explicit unenforced labels, not enforcement. | `plan_text_and_json_label_caps_as_unenforced_draft_planned_caps` fails against the old wording (plan text said "Tier ceilings", JSON said `user_caps`) and now asserts the plan text, plan JSON, and wizard never use "ceiling" wording and label values unenforced. `config_draft_wizard_saves_planned_caps_as_unenforced_draft_policy` and `draft_format_accepts_legacy_caps_table_and_writes_planned_caps` cover the wizard and draft format. |
| G2 — production vsock wiring | **Confirmed open product gap.** Transport helpers exist, but production entrypoints do not start the exchange. | Current daemon/service source and GAP register agree; no live host/guest exchange was run. |
| G3 — worker read deadline | **Fixed locally on the worker read path.** Frame header and payload reads now share one absolute deadline per frame (client `read_exact_until` discipline, `WORKER_FRAME_READ_TIMEOUT` with a test-injected budget), so a stalled, silent, or partial-frame peer fails the worker loop closed instead of blocking until process teardown. Clean EOF before any frame byte still exits successfully; unknown-frame and reserved-byte rejections are preserved. Worker writes remain bare `write_all` and stay bounded only by process teardown. | Three socket-pair regression tests inject a 100 ms frame-read budget and each outlived a 5 s watchdog against the old bare `read_exact` (stalled partial header, stalled partial payload, silent peer), then pass with the deadline: `worker_frame_read_deadline_fails_closed_on_a_stalled_partial_header`, `..._on_a_stalled_partial_payload`, `..._on_a_silent_peer`. No third-party access to the anonymous socket is claimed. |
| G4 — PR metadata | **Confirmed on the public PR, snapshot-specific.** PR #2088 still says 172 commits; the public head range is 176, and 158 rows contain the placeholder `Record or enforce behavior.` The current local branch is 183 commits after `origin/main`. | Read-only GitHub CLI metadata plus local `git rev-list`; no PR edit was made. |
| G5 — coverage exclusion | **Not a current defect.** Test-region classification is unit-tested and malformed inline test regions fail closed. | Existing `plan-rust-slice-coverage` tests and error path; keep as a regression watch item. |
| G6 — seven reliability gates | **Still open as stated.** The local protocol fixes do not provide live resource-profile E2E, host/guest exchange, install parity, physical multi-vendor GPU, Windows runtime/package, or VMBus/CoCo evidence. | GAP register and PR body remain explicit; no live install/stress/hardware run was performed in this audit. |
| L1–L6 | **No confirmed correctness defect from these observations.** Disable response errors become client failure; `aux` is message-type-specific; epoch fallback fails freshness; PSI `total` is cumulative by definition; payload cast is bounded; normalization has metric/verdict regression tests. | Static call-path review and current Node/Rust tests. The normalizer remains a maintenance surface, not proof of tampering. |
| L7 — KiB counter wrap | **Fixed locally.** | Counter now saturates at `u32::MAX`; boundary test passes. Fix and test: `cab5c0fc`. |

### Revalidation test results

- `cargo test -p ramshared-block`: **126 passed**, plus **3 protocol integration tests passed**.
- `CARGO_BUILD_JOBS=1 cargo test -p ramshared-agent parse_psi`: **6 passed**.
- `node --test tools/ci/check-validation-schema.test.mjs`: **26 passed**.
- `CARGO_BUILD_JOBS=1 cargo clippy -p ramshared-block --all-targets -- -D warnings`: passed.
- `CARGO_BUILD_JOBS=1 cargo test -p ramshared-cli --bin ramshared monitor::tests`: **46 passed**, including the latency-origin regression.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- No GPU hardware allocation, stress, package install, or live Windows/WSL E2E was run.

The locally fixed source is seven commits ahead of the public PR head. The PR
head and local candidate must not be treated as the same artifact until the
branch update is published and its final CI reruns.
