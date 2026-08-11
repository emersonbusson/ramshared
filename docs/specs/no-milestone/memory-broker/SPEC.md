# SPEC — RamShared Memory Broker (P0 + P1 + P2)

> SSDV3 STEP 2, generated from `docs/specs/no-milestone/memory-broker/PRD.md`. Slug: `memory-broker`.
> Scope: **P0 (Measurement) + P1 (Linux-to-Linux Broker Core) + P2 (Windows Bridge + generic DCC/workload telemetry MVP)**.
> Disciplines: Mandatory links to `docs/methodology/kahneman-disciplines.md` in every critical step.

## Audit Logs

> **RamShared model:** one `SPEC.md` per feature. Each Step 2.5 pass revises this file in place; prior text remains in `git log` — do **not** create `SPECv2.md` / `SPECvN.md`.

### Phase 0 & Phase 1 Audit (Step 2.5) — changelog deste `SPEC.md`
- **1st Audit (2026-06-09):** Result: no-go. Findings F1–F17 → incorporados in-place.
- **2nd Audit (2026-06-13):** Result: no-go. Findings R1–R6 → incorporados in-place.
- **3rd Audit (2026-06-13):** Result: no-go. Findings R7–R9 → incorporados in-place.
- **4th Audit (2026-06-13):** Result: go. Findings R10–R11 → incorporados in-place. **Candidato ativo = este arquivo.**

| Finding | Severity | Resolution (this SPEC.md) |
| --- | --- | --- |
| F1 — Agent without `mkswap`: SwapOn flow unexecutable | CRITICAL | DT-16; ITEM-9 (`swap.rs::mk_swap`, loop order); drills updated |
| F2 — Slice reassigned without zeroing leaks pages between tenants | CRITICAL | DT-17; `WMsg::ZeroExport` (ITEM-7), core wiring (ITEM-8); atomic boundary includes zero step |
| F3 — Watchdog without mandatory heartbeat source | HIGH | DT-18: Mandatory `Ack` for each `Psi`; e2e asserts cadence |
| F4 — Drill validated happy path only; undefined swapoff behavior | HIGH | ITEM-11 rewritten: 3 phases (graceful / kill idle / kill used), numeric criteria, explicit initramfs |
| F5 — Lease without state: round-robin reassigns leased slices | HIGH | DT-19: `SliceState::Leased` + grant/deny/release rules |
| F6 — Tenant absent with active slices: arbiter stuck in `Draining` | HIGH | DT-20: No Action with absent target; frozen slices visible in Status; reconciliation on re-Register |
| F7 — Orderly shutdown without test evidence | HIGH | e2e scenario (f) + drill phase 0 (graceful SIGTERM) |
| F8 — Single NBD endpoint vs. TransportKind per tenant | MEDIUM | DT-25: Optional Unix and TCP endpoints; transport-based selection |
| F9 — Multiplicative counterfactual without floor: noise trigger | MEDIUM | DT-23: Guard `psi(from) > psi_floor` in addition to 2× factor |
| F10 — Core outbound write block (agent blocks broker) | MEDIUM | DT-24: Per-session writer thread, bounded channel (cap 64), `try_send`, backpressure disconnect |
| F11 — Subspecified protocol details | MEDIUM | DT-21/DT-22 |
| F12 — Incomplete rename of `ramsharedd` | LOW | ITEM-8: Grep verification + file mapping updates |
| F13 — One decision per tick vs. `-> Vec<Action>` | LOW | ITEM-4: Max 1 Move/Revert per tick; Assign/lease coexist |
| F14 — "VM remains responsive" without observable criteria | LOW | ITEM-11: Echo < 2s × 3 + no processes in D-state > 10s |
| F15 — DT-13 (euid == 0) under `forbid(unsafe_code)` | LOW | DT-26: Parse `/proc/self/status`, zero-dep |
| F16 — civm runbook without NBD module persistence | LOW | ITEM-12 |
| F17 — `measure-nbd-tcp.sh` without dependency check | LOW | ITEM-1: Preflight checks with installation suggestions |
| R1 — Agent command execution blocks heartbeat loop | HIGH | DT-27: Commands run on dedicated thread; watchdog measures liveness, not command latency; ITEM-9 rewritten |
| R2 — Lease reservation race under multi-tick revocation | MEDIUM | DT-19: Slices reaching Free under pending lease immediately go to Leased, round-robin suppressed |
| R3 — Slice geometry unavailable to worker | MEDIUM | ITEM-8: Worker maintains `geom` (base, len per export); `block::Export` stays name+size |
| R4 — `ZeroExport` try_send failure without retry | MEDIUM | DT-17: try_send failure keeps slice `Draining`, retries next tick |
| R5 — Drill/runbook: Unix socket path and `comm` parsing bugs | LOW | ITEM-11: `/tmp` socket path, `nbds_max=8` modprobe, parse state after last `)` |
| R6 — Worker blocked during zeroing of large slices | LOW (note) | DT-17: Acceptable (rebalancing is rare, 60s cooldown); noted limit |
| R7 — Worker lifecycle under broker mode | HIGH | DT-28: Worker ignores `LiveCount` break in broker mode; exits only on `jobs` close |
| R8 — Agent two-thread socket writing collision | MEDIUM | DT-27: Execution thread returns results via channel; main loop is sole writer |
| R9 — Rebalancing not suppressed during pending lease | LOW | ITEM-4: Move/Revert supressed under pending lease |
| R10 — Shutdown ambiguity: slice zeroing vs. whole-buffer zero | LOW | DT-17: Shutdown uses whole-buffer teardown zeroing, skipping per-slice `ZeroExport` |
| R11 — Reconciliation assumed NBD base | LOW | DT-21: Broker reconciles by extracting final integer from `SwapEntry.dev` |

### Phase 2 Audit (Step 2.5)
- **Phase 2 Audit:** Result: go after correcting findings C1–C3 and H1–H3:
  - **C1 (DT-36):** DccAgent exclusion moved to `broker_srv::on_tick` (transport-agnostic arbiter).
  - **C2 (model.rs):** Exhaustive matching for `TransportKind` in `endpoint_for` handled for `DccAgent`.
  - **C3 (DT-34):** `AgentRole` trait abstraction introduced to avoid moving `wsl2d`-dependent swap loop to Windows.
  - **H1 (DT-42):** NVML process-specific memory query used instead of device-wide metrics.
  - **H2 (DT-43):** Conditional target compilation configured for Windows dependencies.
  - **H3 (DT-35):** Dedicated monomorphic codec for local communications.

---

## Closed Scope of This Implementation

### Phase 0 & Phase 1 Scope (Active)
- **P0** — Measurement scripts (no product code) + results template acting as the numeric gate for P1: PSI idle/load (WSL2, civm, host), reachability/RTT VM↔WSL2, raw NBD/TCP p50/p99 in virt-switch, VRAM/RAM render measurement (tester script).
- **P1** — RF-B1, RF-B2, RF-B3, RF-B4, RF-L1, RF-L2, RF-L3, RF-L4, RF-P2 (partial: NBD as universal fallback; ublk unchanged), RNF-1..RNF-6:
  - New crate `ramshared-broker` (JSON-lines protocol, model, pure arbiter, slice map);
  - New crate `ramshared-agent` (tenant binary: PSI, nbd-client + mkswap + swapon/swapoff, watchdog);
  - Daemon (`crates/ramshared-wsl2d`) gains `--slices/--slice-mb`, `--listen-nbd tcp://`, `--arbiter-listen`, `--backend ram` NBD support, slice hygiene (zero on release), and binary renamed to `ramsharedd`;
  - Named NBD exports per slice in `ramshared-block::server_handshake`;
  - D-state drill in QEMU (`scripts/kernel/qemu-broker-drill.sh`, 3 phases);
  - Copiable civm runbook (`docs/runbooks/CIVM-TENANT.md`).

### Phase 2 Scope (Active)
- **P2** — `ramshared-nvml` (FFI dlopen, RF-W1/W2); `ramshared-config` (TOML, RF-P3); Windows **DccAgent** (`ramshared-host-agent`, RF-W1); generic local workload↔agent↔broker lease bridge (RF-W3); Windows installer (RF-P1).

### Explicitly Out of Scope
- RF-W4 (Interposer -> Phase 4).
- RF-G3 (D3D12 inside WSL2 -> Phase 3 research).
- Windows-as-swap-consumer (disk driver -> Phase 4).
- Rewriting application engines.
- Custom authentication/encryption (relies on private/Tailscale networks).
- Multi-lease active concurrently (broker enforces 1 active lease at a time).
- State persistence across broker restarts (in-memory only).

---

## Confirmed Codebase and Environment Dependencies

- `ramshared_block::BlockBackend` + `serve()` + NBD fixed-newstyle multi-connection (Unix socket).
- DEMOTE machinery: `Canary`/`ResidencySampler`/`CanaryProbe` (`crates/ramshared-wsl2d/src/residency.rs`) and `spawn_swapoff`.
- CUDA via dlopen (`ramshared-cuda` wrapper for `cuMemGetInfo_v2`).
- Teardown ublk validated in QEMU.
- Custom WSL2 kernel with `CONFIG_PSI=y` and `CONFIG_BLK_DEV_NBD=m`.
- Windows 10/11 system with active CUDA/OptiX drivers and `nvml.dll` available.

---

## Traceability Matrix (PRD → SPEC)

| PRD Ref | Implementation in SPEC | Description |
| --- | --- | --- |
| **P0 (§10)** | ITEM-1 | Measurement scripts and baseline validation |
| **RF-B1** | ITEM-2, ITEM-3, ITEM-8, ITEM-9 | JSON-lines protocol and server/client endpoint routing |
| **RF-B2** | ITEM-4 | Arbiter logic (streak, hysteresis, floor) |
| **RF-B3** | ITEM-4, ITEM-8 | Revocable lease scheduling and worker integration |
| **RF-B4** | ITEM-8, ITEM-9 | Structured logging and status query interface |
| **RF-L1** | ITEM-4, ITEM-5, ITEM-6, ITEM-7, ITEM-8 | Slices, named exports, offset mapping views |
| **RF-L2** | ITEM-7, ITEM-8 | TCP transport sockets and interface binding |
| **RF-L3** | ITEM-9 | Linux PSI & swap automation |
| **RF-L4** | ITEM-12 | copiable VM deployment runbook |
| **RF-W1** | ITEM-13, ITEM-15, ITEM-16, ITEM-17 | Windows native memory pressure and NVML budget tracking |
| **RF-W2** | ITEM-20 | Generic GPU workload telemetry and headroom recommendation |
| **RF-W3** | ITEM-17, ITEM-18, ITEM-19 | Addon to broker lease handshake bridge |
| **RF-P1** | ITEM-21 | Windows service wrapper and packaging |
| **RF-P2** | ITEM-8 | NBD transport fallback mechanics |
| **RF-P3** | ITEM-14 | Central TOML config loader and CLI overrides |

---

## Technical Decisions (DT-1 to DT-50)

| ID | Technical Decision | Rationale & Trade-offs |
| --- | --- | --- |
| **DT-1** | Broker protocol uses UTF-8 **JSON-lines** (`\n` separated) over TCP. | Low rate (1 msg/s/tenant). Easily debuggable using standard tools. Bounded to 64 KiB to prevent DoS. |
| **DT-2** | Broker runs **in-process** within the daemon (`ramsharedd`). | A single controller must arbitrate physical VRAM to avoid blind hardware resource contention. |
| **DT-3** | Slices in P1 map to **named NBD exports** (`s0..s{K-1}`). | WSL2 kernel cannot run ublk due to lockup risks under swap. Named exports cleanly isolate slices. |
| **DT-4** | Single CUDA worker handles whole-buffer; slices use `SliceView`. | Retains CUDA thread affinity and sync guarantees without redesigning the CUDA backend. |
| **DT-5** | Rename daemon binary to **`ramsharedd`**. | Matches PRD conventions and simplifies systemd service management. |
| **DT-6** | Slices allocated **round-robin** to registered present tenants. | Swap is passive; physical consumption occurs under pressure. Avoids complex admission policies. |
| **DT-7** | Remote swap runs with **lowest kernel priority** (no `-p` in `swapon`). | Ensures remote VRAM swap is only targeted after local RAM/swap pools are exhausted. |
| **DT-8** | "Never zero slices under pressure" (RF-B2) applies only to rebalancing. | Lease demands (DCC renders) can completely drain swap slices to prioritize active workflows. |
| **DT-9** | Broker state is fully **in-memory** and rebuilt on registration. | Eliminates state synchronization anomalies. Watchdogs trigger clean re-registration on restarts. |
| **DT-10** | Lease counterfactual (<50% usage in 5 min) deferred to P2. | Requires Windows NVML telemetry (P2) to monitor client usage. P1 only logs lease state. |
| **DT-11** | TOML config loader deferred to P2; P1 uses CLI flags. | TOML is an installable packaging detail. CLI flags map 1:1 to future TOML structures. |
| **DT-12** | No Prometheus exporter in P1; logs and status queries suffice. | Reduced scope. RF-B4 is satisfied by structured logs and status frames. |
| **DT-13** | `ramshared-agent` requires root/euid 0. | `swapon`, `swapoff`, `mkswap`, and `nbd-client` all require admin capabilities. |
| **DT-14** | `nbd-client` uses `-timeout 30` and **never** `-persist`. | Prevents indefinite D-state kernel hangs when NBD server drops. |
| **DT-15** | Arbitration uses the **`some`** PSI line (`avg10`). | `some` detects early thrashing; `full` represents system starvation, which is too late. |
| **DT-16** | **`mkswap`** runs on every `SwapOn` execution. | Newly allocated/assigned slices are blank (zeroed); signature is needed before mount. |
| **DT-17** | **Slice Hygiene:** Daemon zeroes slices before reassignment. | Prevents page leaking/data exposure between tenants. Handled out-of-band by worker. |
| **DT-18** | Broker replies with `Ack` to every `Psi` report. | Ensures the agent's watchdog can reliably detect silent network dropouts. |
| **DT-19** | `SliceState::Leased` tracks active leases (max 1). | Prevents round-robin from reclaiming leased slices during multi-tick allocation processes. |
| **DT-20** | Absent tenants are excluded from arbitration; slices are frozen. | Prevents sending commands to disconnected agents, which would stall state machines. |
| **DT-21** | Reconciliation identifies slices via sufix integer parsing. | Decouples broker from local NBD device paths, making it agnostic to agent naming choices. |
| **DT-22** | Status frames accepted without registration. | Allows CLI health checking without registering as an active swappable tenant. |
| **DT-23** | `RevertMove` requires PSI above `psi_floor`. | Prevents idle noise triggers from causing ping-pong rebalancing loops. |
| **DT-24** | Dedicated writer thread per agent session with bounded channel. | Isolates broker core from slow/hung sockets. Backpressure triggers clean drop. |
| **DT-25** | `BrokerConfig` supports both Unix and TCP socket bindings. | Accommodates local WSL2 tenants (Unix) and remote CI VMs (TCP) simultaneously. |
| **DT-26** | Zero-dependency root check via `/proc/self/status` parsing. | Avoids unsafe FFI binds or heavy external dependencies. |
| **DT-27** | Agent executes mounting commands in a separate thread. | Prevents slow kernel command executions from starving heartbeat loops. |
| **DT-28** | Broker mode worker ignores `LiveCount` disconnect terminations. | Broker daemon must persist even when NBD sessions temporarily drop to zero. |
| **DT-29** | WSL2 acts as server-only in E2E environments. | Eliminates risk of WSL2 kernel D-state lockups by offloading swap mounting to client VMs. |
| **DT-30** | Arbiter ticks use deadline-based intervals. | Prevents frequent incoming telemetry from indefinitely postponing arbitration ticks. |
| **DT-31** | Latency canary trigger limit raised to 64×. | Eliminates false-positive evictions caused by normal heavy paging spikes. |
| **DT-32** | Windows Memory Pressure uses `GlobalMemoryStatusEx`. | Single fast system call tracking physical availability and commit limit. Rejects perfmon. |
| **DT-33** | Hand-rolled NVML dlopen wrapper (`ramshared-nvml`). | Zero-dependency FFI. Loads `nvmlInit_v2` and `nvmlDeviceGetComputeRunningProcesses_v3`. |
| **DT-34** | Core agent loop extracted to generic client framework. | Trait `AgentRole` abstracts away Linux-specific swap interfaces for Windows reuse. |
| **DT-35** | Local DCC/workload bridge uses distinct JSON-lines codec. | Separates local workload queries from broker protocol frames. Bounded to 64 KiB. |
| **DT-36** | DCC agents filtered out in `broker_srv::on_tick`. | Excludes non-swappable DCC tenants from swap rebalancing while preserving lease flows. |
| **DT-37** | TOML config via `ramshared-config` (serde). | Centralizes options. CLI arguments override TOML values. |
| **DT-38** | Graceful NVML fallback. | Missing GPU/driver falls back to CPU/RAM heuristics instead of crashing. |
| **DT-39** | Single active lease limit. | Simplifies P2 core. Multi-lease requires priority queues, deferred. |
| **DT-40** | Windows Service wrapper uses SCM libraries. | Ensures proper integration with Windows Service Control Manager. |
| **DT-41** | Addon communicates only with local agent loopback. | Local agent consolidates session authentication, socket lifecycle, and local metrics. |
| **DT-42** | Agent handles auto-release counterfactual by checking PID VRAM usage. | Releases VRAM back to swap pool if the renderer consumes <50% of lease for 5 min. |
| **DT-43** | Cross-compile target gating. | Conditional dependencies keep workspace tests compilable on Linux hosts. |
| **DT-44** | DCC Agent reports dummy PSI frame to satisfy broker. | Avoids protocol changes. Dummy reports are ignored due to the DCC filter. |
| **DT-45** | `ramshared-agent` has an explicit public CLI termination contract. | `-h`/`--help` print English usage to stdout and exit 0; malformed or incomplete CLI input (including missing `--broker` or agent-mode `--tenant`) writes a diagnostic plus usage to stderr and exits 2; a `--status` transport/runtime refusal, clean EOF, or bounded I/O timeout exits 1. `--status` sets both read and write deadlines to 5 s, accepts one `StatusReply` within at most 50 frames, and reports a timeout without retrying or starting swap work. Broker `Msg` variant names and wire values remain unchanged; a broker-supplied error reason is preserved after an English local diagnostic prefix. |
| **DT-47** | A ublk descriptor is copied into an owned fixed-size array after the CQE; Rust never creates or returns `&[u8]` over the kernel-mutated shared mapping. | The kernel is an external writer. A borrowed Rust slice would express immutability that the mapping cannot guarantee and can become aliasing UB. The copy is bounds-checked and decoding uses only the owned snapshot. |
| **DT-48** | Every ublk composite handle joins both the ring owner and the backend worker before returning; the ring result has deterministic precedence only after both attempts complete. | An early `?` on the ring result can abandon an unjoined worker and leave teardown incomplete. Joining both preserves ownership cleanup without inventing recovery. |
| **DT-49** | Every ublk descriptor, buffer, and completion operation validates `tag < queue_depth` before accessing the rounded shared mapping or buffer vector. | Page-rounded mappings can contain bytes beyond the declared queue. Mapping bounds alone do not prove tag ownership, and unchecked vector indexing can panic on a kernel-supplied tag. An invalid tag is a terminal `InvalidInput` refusal. |
| **DT-50** | Broker worker shutdown uses a dedicated `WMsg::Shutdown` control-plane wake paired with the atomic terminal flag; it never depends only on `recv_timeout` expiring and never abandons earlier FIFO work. | The shutdown requester stores the flag before a nonblocking `try_send`. A queued wake releases an idle receiver immediately. On a full queue, the requester returns after handing a cloned sender to a dedicated notifier, which waits for capacity and appends the wake after already queued I/O; a disconnected queue is already terminal. The worker exits on the control message (with the atomic timeout check retained only as fallback), so shutdown does not block the signal bridge, change NBD wire values, or drop earlier replies. |

---

## Atomicity Boundary and Rollback Policy

### Atomicity Boundary
1. **Slice Allocation:** A slice is never active on two hosts simultaneously. The sequence is strictly enforced: `SwapOff(from)` → `SwapOffDone` → `ZeroExport(slice)` → `ZeroDone` → `SwapOn(to)`.
2. **Lease Management:** Slices leased to a DCC agent are protected from eviction. Slices are zeroed out before lease handover.
3. **Partial Failures:** If a tenant disconnects during transition, the slice is frozen (`Draining`) until reconnection reconciles the state.

### Rollback Policy
- **App Rollback:** SIGTERM on the daemon triggers `DemoteAll` to clean up mounts, followed by whole-buffer zeroing. Reverting codebase is achieved via standard `git revert`.
- **Telemetry/Data Rollback:** Virtual disk swaps are volatile. A daemon crash can cause process crashes on tenants due to EIO, but it will not hang the operating system.

---

## Kahneman Map by Critical Stage

| Stage / ITEM | Discipline Ref | Mandatory Question | Minimal Evidence | Abort Trigger |
| --- | --- | --- | --- | --- |
| **ITEM-1 (P0)** | #3 Number, #1 WYSIATI | Do the PSI/RTT/NBD metrics contain real units and counts? | Filled `docs/reliability/memory-broker-p0-results.md` | Missing metrics |
| **ITEM-4 (Arbiter)** | #2 Counterfactual | What triggers a rebalance undo? | `cargo test` verifying `RevertMove` under counterfactual floor | Missing counterfactual tests |
| **ITEM-8 (Hygiene)** | #5 Worst-case | Does tenant B read garbage from tenant A's slice? | Zero-fill unit tests and E2E validation showing clean blocks | Slices assigned without `ZeroDone` |
| **ITEM-9 (Agent CLI and watchdog)** | #13 Validity Illusion, #15 Retry semantics, #16 Safe defaults | Does the public CLI distinguish help, malformed input, broker refusal, and a bounded silent broker without starting swap work? | Named process tests for status success/refusal/timeout plus the existing isolated QEMU phase-2 drill showing clean EIO recovery under 5s | A status command exceeds its 5 s I/O deadline, retries silently, starts swap work, or a QEMU simulation leaves swapoff stuck |
| **ITEM-11 (Drill)** | #5 Worst-case | Is the drill executed on disposable machines? | Marked logs from E2E simulation running in isolated QEMU | Stalls or hangs exceeding 60s |
| **ITEM-13 (NVML)** | #3 Number | Do NVML metrics align with CUDA memory calls? | Unit test comparing `mem_info` results | Systematic memory metric drifts |
| **ITEM-20 (Addon)** | #2 Counterfactual | Does predictive allocation trigger out-of-core fallback? | Real scene render completing under VRAM limit simulation | Scene crash due to VRAM exhaustion |

---

## Security Checklist

- **Network Isolation:** NBD listeners and broker sockets must bind only to loopback (`127.0.0.1`) or private Tailscale interfaces.
- **Slice Cleansing:** Slices must be fully zero-filled during transition to prevent information leaks.
- **Out-of-Bounds Protection:** `SliceView` wrapper enforces bounds check against slice boundaries, preventing OOB memory reads/writes.
- **Privilege Separation:** Windows DCC agent runs without admin elevation; Linux agent validates root context early.
- **Buffer Safety:** Protocol lines limited to 64 KiB to prevent memory exhaustion attacks.

---

## Files to CREATE

### ITEM-1 — P0 Scripts
- `scripts/p0/measure-psi.sh` (collects `/proc/pressure/memory` metrics).
- `scripts/p0/measure-net.sh` (matrice of VM-to-WSL2 latency).
- `scripts/p0/measure-nbd-tcp.sh` (NBD/TCP raw performance tests).
- `scripts/p0/measure-gpu-workload-vram.ps1` (generic Windows telemetry probe for aggregate VRAM/RAM).
- `scripts/p0/Invoke-GpuWorkloadGate.ps1` (idle/load/recovery aggregate VRAM pressure gate).
- `scripts/p0/Start-CudaVramWorkload.ps1` (generic synthetic CUDA VRAM allocator for pressure campaigns when no external app is pinned to the architecture).

### ITEM-2 — ADR-0005
- `docs/decisions/ADR-0005-broker-protocol-jsonl.md` (Design record for JSON-lines over TCP).

### ITEM-3 — Crate `ramshared-broker` (Protocol & Models)
- `crates/ramshared-broker/Cargo.toml`
- `crates/ramshared-broker/src/lib.rs`
- `crates/ramshared-broker/src/model.rs`
- `crates/ramshared-broker/src/protocol.rs`

### ITEM-4 — Crate `ramshared-broker` (Slices & Arbiter)
- `crates/ramshared-broker/src/slices.rs`
- `crates/ramshared-broker/src/arbiter.rs`

### ITEM-9 — Agent public CLI process tests
- `crates/ramshared-agent/tests/agent_cli.rs` (real child-process diagnostics,
  refusal, exit-code, status reply, and bounded-timeout tests using a local
  TCP listener; no swap command is issued).

### ITEM-13 — Crate `ramshared-nvml`
- `crates/ramshared-nvml/Cargo.toml`
- `crates/ramshared-nvml/src/lib.rs` (hand-rolled NVML dlopen bindings, `VramBudget`, `RenderVram`).

### ITEM-14 — Crate `ramshared-config`
- `crates/ramshared-config/Cargo.toml`
- `crates/ramshared-config/src/lib.rs` (TOML parser mapping options to CLI flags).

### ITEM-15 — `crates/ramshared-agent/src/win_mem.rs`
- Windows-specific memory pressure statistics sampler using `GlobalMemoryStatusEx`.

### ITEM-16 — `crates/ramshared-agent/src/client.rs`
- Generic agent connection and state machine wrapper (`AgentRole` trait).

### ITEM-17 — `crates/ramshared-agent/src/bin/ramshared_host_agent.rs`
- Windows Agent entry point executing `WinDccRole` and local bindings.

### ITEM-18 — `crates/ramshared-agent/src/local.rs`
- Local DCC/workload listener and JSON-lines codec (`LocalMsg`/`LocalReply`).

### ITEM-20 — Generic workload telemetry
- `scripts/p0/measure-gpu-workload-vram.ps1` records aggregate VRAM/RAM for any externally launched GPU workload.
- `scripts/p0/Invoke-GpuWorkloadGate.ps1` wraps the sampler into idle, loaded, and recovery windows. The gate passes only when aggregate VRAM rises by `MinDeltaMib` and later returns near idle.
- `scripts/p0/Start-CudaVramWorkload.ps1` provides an app-agnostic CUDA workload that allocates, touches, holds, and releases a bounded VRAM slice for repeatable WDDM pressure validation.
- Host-specific adapters are explicitly deferred until requested and must not name the generic reclaim path.

---

## Files to MODIFY

### ITEM-9 — `crates/ramshared-agent/src/main.rs`
- Keep the `Msg` wire protocol and agent-mode swap behavior unchanged.
- Make the public CLI termination contract from DT-45 explicit: help is a
  successful stdout response; malformed input is a stderr refusal with exit 2;
  `--status` runtime failures are stderr failures with exit 1.
- Use an English `Usage` diagnostic and English local diagnostics only. Preserve
  option spellings, `Msg` variant names, and broker-provided reason values.
- Bound `--status` socket read and write I/O at 5 s and do not retry a silent
  broker from the one-shot status command.

### ITEM-5 — `crates/ramshared-block/src/handshake.rs`
- Update `server_handshake` to select named exports:
  ```rust
  pub struct Export { pub name: String, pub size: u64 }
  pub fn server_handshake<R: Read, W: Write>(
      r: &mut R, w: &mut W, exports: &[Export], tx_flags: u16,
  ) -> Result<usize, HandshakeError>;
  ```

### ITEM-6 — `crates/ramshared-wsl2d/src/backend.rs`
- Implement `SliceView` (window projection helper). Move `RamBackend` here.

### ITEM-7 — `crates/ramshared-wsl2d/src/conn.rs`
- Make readers/writers generic over stream types. Add `ZeroExport` and the
  internal broker-only `Shutdown` wake to `WMsg`; neither is an NBD wire value.

### ITEM-8 — `crates/ramshared-wsl2d/src/main.rs`
- Wire CLI options (`--slices`, `--slice-mb`, `--listen-nbd`, `--arbiter-listen`). Re-route eviction signals to broker.

### `crates/ramshared-broker/src/model.rs`
- Add `DccAgent` to `TransportKind`.

### `crates/ramshared-wsl2d/src/broker_srv.rs`
- Filter out `DccAgent` from swap rotation inside `on_tick`. Add TCP acceptor framework.

---

## Observability and Logs

Logs are printed to `stderr` in a key-value format prefixed with `[ramsharedd]` or `[agent]`:

| Event | Log Example |
| --- | --- |
| Rebalance Move | `[ramsharedd] arbiter move slice=s1 from=civm(psi10=14.2) to=wsl2(psi10=0.0) streak=5` |
| Lease granted | `[ramsharedd] lease granted id=1 holder=dcc-agent bytes=4294967296 slices=[0, 1]` |
| Watchdog trigger | `[agent] watchdog expired broker=127.0.0.1:7777 cleaning up mounts` |
| Slice sanitization | `[ramsharedd] zeroed slice=0 duration=45ms status=ok` |

### ITEM-9 public CLI process contract and coverage

The following are real `ramshared-agent` child-process tests. Their local TCP
fixtures issue only `Msg::Status`/reply frames; they never invoke `SwapOn`,
`swapon`, `swapoff`, NBD, or a host/VM drill.

| Test name | Path | Assertion |
| --- | --- | --- |
| `cli_help_prints_usage_and_exits_zero` | `crates/ramshared-agent/tests/agent_cli.rs` | English usage on stdout; exit 0. |
| `cli_missing_broker_refuses_with_exit_two` | same | Required option refusal on stderr; exit 2. |
| `cli_invalid_transport_refuses_with_exit_two` | same | Invalid transport refusal on stderr; exit 2. |
| `cli_missing_tenant_refuses_with_exit_two` | same | Agent-mode tenant refusal on stderr; exit 2 before privilege or socket work. |
| `cli_status_reply_prints_public_status_and_exits_zero` | same | Local `StatusReply` is printed and exits 0. |
| `cli_status_broker_refusal_exits_one` | same | Local broker `Msg::Error` reason is surfaced; exit 1. |
| `cli_status_timeout_exits_one_within_six_seconds` | same | A local silent peer produces the DT-45 timeout diagnostic and exits 1 within six seconds. |
| `help_is_a_parse_outcome` | `crates/ramshared-agent/src/main.rs` :: `tests` | Parser distinguishes help from a malformed argument. |
| `usage_diagnostic_adds_usage_once` | same | Every malformed-input diagnostic gains one English usage block. |
| `swap_on_prefers_broker_priority_without_running_swap` | same | `SwapOn` dispatch preserves broker priority without invoking the executor. |
| `demote_all_dispatches_release_without_running_swap` | same | `DemoteAll` queues releases without invoking the executor. |
| `session_registers_dispatches_commands_and_stops_on_refusal` | `crates/ramshared-agent/src/main.rs` :: `tests` | Real local agent session registers, reports PSI, dispatches command messages to its execution channel, and stops on a broker refusal without executing swap commands. |
| `session_reports_execution_results_without_running_swap` | same | Real local session emits queued execution results through its sole writer without running a swap command. |
| `session_watchdog_terminates_silent_broker_without_swap` | same | A real local silent session terminates at the watchdog deadline with no active swap command. |

The canonical per-file coverage owner for this business path is:

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-agent --files crates/ramshared-agent/src/main.rs --min 80 --report-json tmp/memory-broker-agent-cli-cov.json
```

The process tests provide the public-surface validity/refusal evidence (#13),
the timeout test proves the no-retry terminal path (#15), and the no-swap
fixtures prove the safe default (#16). The isolated QEMU drill remains the
separate required live proof for active-swap EIO recovery.

### ITEM-8 daemon entry-point bounded test contract and coverage

**DT-46 — `ramsharedd` parses and plans before any backend, GPU, swap, NBD
client, or ublk action.** The daemon entry point is a business-safety boundary,
not untested wiring. Its parser accepts an injected argv vector; the resulting
plan validates slice geometry, private-only listener addresses, and advertised
endpoint consistency before it can select a backend. A bounded test runtime may
use only a loopback TCP listener, a temporary Unix-socket pathname, heap-backed
`RamBackend`, and an in-process broker. It must have an explicit stop signal
and join deadline; timeout is a refusal and its cleanup removes only the socket
that it created. It must never load CUDA/Vulkan, invoke `swapon`/`swapoff`,
touch `/dev/nbd*` or `/dev/ublk*`, or mutate a host/VM.

The production shell remains responsible for CUDA/Vulkan construction,
`mlockall`, swap lifecycle, and device setup. Tests inject a bounded runtime
only around the safe broker-RAM control plane; they do not claim a GPU, NBD, or
ublk live proof. The daemon's externally visible flag spellings, NBD export
names, JSON wire values, and telemetry JSONL schema remain unchanged.

| Test name | Fixture / assertion | Discipline |
| --- | --- | --- |
| `daemon_args_accept_broker_wiring_and_normalize_addresses` | injected argv; exact slices, private loopback listeners, advertisement, and telemetry path are retained | #13 |
| `daemon_args_refuse_invalid_or_unsafe_combinations_before_backend` | injected malformed/missing/unspecified/oversized/conflicting argv; no runner call | #16/#17 |
| `daemon_args_cover_flag_boundaries_before_backend` | injected argv covers every scalar flag, alignment, missing values, overflow, and mutually dependent endpoint refusal before a runner/backend action | #13/#16 |
| `daemon_process_refusals_exit_before_backend` | real child process with invalid safe argv; English diagnostic and exit 1 before a runner/backend action | #13/#16 |
| `daemon_broker_config_preserves_telemetry_and_exact_endpoints` | pure broker config construction; exact Unix/TCP endpoint and JSONL path | #13 |
| `daemon_broker_ram_binds_loopback_and_cleans_owned_socket` | temporary Unix socket + loopback TCP + bounded stop/join; no GPU, swap, or device | #15/#16 |
| `daemon_broker_setup_stops_bounded_without_backend` | injected shutdown and recording acceptor starter; real temporary Unix/loopback arbiter listeners are released after a bounded worker/broker join, with no NBD client, GPU, or swap | #15/#16 |
| `daemon_broker_acceptor_failure_rolls_back_owned_socket` | injected acceptor-start failure stops the just-started broker and removes only its temporary owned Unix socket | #15/#16 |
| `daemon_broker_vram_and_ram_lifecycles_use_injected_runtime` | heap-backed Vram/RAM backends plus an injected, pre-stopped broker runtime prove allocation, bounded worker exit, broker join, zeroing, and owned-socket cleanup without CUDA, swap, or an NBD client/device | #13/#15/#16 |
| `daemon_broker_setup_failure_zeroes_allocated_vram_before_return` | an injected broker-setup refusal after backend and canary allocation must explicitly zero both allocations and remove only its owned temporary Unix socket before returning the refusal; no CUDA, swap, NBD client, or device | #13/#15/#16 |
| `daemon_broker_lock_refusal_zeroes_allocated_vram_before_return` | an injected memory-lock refusal after backend allocation must explicitly zero that allocation and return before canary allocation or broker setup; no CUDA, swap, NBD client, or device | #13/#15/#16 |
| `daemon_broker_bind_conflict_refuses_and_preserves_existing_listener` | occupied loopback port; no worker is started and the pre-existing listener remains usable | #16/#17 |
| `daemon_worker_serves_job_counts_io_and_stops_on_shutdown` | heap `RamBackend`, channel job, bounded reply/join; exact counters and terminal shutdown | #13/#15 |
| `daemon_worker_shutdown_wake_is_not_timer_dependent` | a 30 s receive interval plus the dedicated nonblocking wake must join within 1 s, proving shutdown does not wait for the timer | #15/#16 |
| `daemon_worker_shutdown_full_queue_is_nonblocking` | a full worker queue returns the explicit full-queue wake result while setting the terminal flag; no shutdown sender can block | #15/#16/#17 |
| `daemon_worker_shutdown_drains_queued_io_before_stop` | a write queued before the shutdown wake still receives its reply and increments exact counters before the worker joins | #13/#15/#17 |
| `daemon_command_timeout_terminates_child_without_hang` | harmless child process; success, nonzero, missing executable, and deadline branches | #15/#16 |
| `daemon_nbd_prealloc_worker_uses_fake_provider_and_injected_acceptor` | heap-backed `VramProvider`, no-op memory lock, and injected worker messages exercise prealloc protocol setup/zero/write/close/teardown without a CUDA context, NBD client/device, swap command, or `/proc` mutation | #13/#15/#16 |
| `daemon_nbd_sparse_floor_refusal_reclaims_without_provider_allocation` | zero-free heap provider plus injected write/flush/close proves sparse free-floor refusal and idle reclaim decisions without allocating a sparse chunk, CUDA, NBD device, swap, or `/proc` access | #3/#15/#16 |
| `daemon_nbd_budget_poll_uses_injected_wddm_snapshot_and_global_probe` | fake fresh WDDM budget and bounded global-free probe exercise sparse budget reconciliation with no `/dev/dxg`, subprocess, CUDA, NBD device, or swap command | #3/#13/#15/#16 |
| `daemon_nbd_budget_constraint_demotes_then_recovers_with_fake_swap` | injected stale/error then healthy WDDM snapshots and fake swapoff/swapon prove constrained DEMOTE plus hysteretic recovery without `/dev/dxg`, CUDA, NBD device, or swap command | #3/#15/#16 |
| `daemon_nbd_teardown_refuses_until_fake_usage_and_swapoff_confirm` | injected nonzero usage followed by zero usage and fake swapoff confirmation prove fail-closed teardown retry without a five-second test sleep, NBD device, or swap command | #15/#16/#17 |
| `daemon_nbd_residency_demote_uses_injected_clock_and_swapoff` | deterministic latency baseline/spike sequence and injected successful swapoff prove DEMOTE state/status/teardown without timing sleeps, CUDA, NBD device, or a real swap command | #3/#15/#16 |
| `daemon_ublk_runtime_orders_lifecycle_and_rolls_back_without_device` | injected ublk runtime proves guard → lock → create → configure → start → bounded stop/join/delete ordering and refusal-before-runtime; it never opens `/dev/ublk-control` or creates a block device | #15/#16/#17 |
| `daemon_ublk_runtime_failures_delete_candidate_before_return` | injected set-params/server/start/wait failures prove cleanup attempts remain ordered and terminal before error return, without a ublk device | #15/#16/#17 |
| `daemon_ublk_vulkan_refuses_before_device_mutation` | unsupported ublk/Vulkan combination returns before `/dev/ublk-control` | #16 |
| `daemon_production_runner_refuses_safe_terminal_actions_before_platform_load` | production runner receives synthetic RAM-NBD and Vulkan-ublk terminal actions plus a broker-RAM regular-file socket conflict; every case refuses before CUDA/Vulkan loading, swap, NBD client/device, or acceptor startup, preserving the existing file | #16/#17 |
| `daemon_ublk_wsl_guard_and_memory_lock_policy_are_pure_and_fail_closed` | pure os-release/override and lock-result matrices prove WSL2 refusal plus protected-memory refusal without inspecting host state, setting OOM score, or opening a device | #16/#17 |
| `mmap_descriptor_snapshot_is_owned_and_bounds_checked` | a regular-file mapping proves descriptor reads return an owned array, preserve an earlier snapshot after mapped bytes change, and reject an out-of-range offset | #13/#16 |
| `regular_file_ublk_adapters_refuse_without_a_device` | page-aligned regular-file and invalid-descriptor fixtures cover mmap, queue construction, queue-depth tag refusal, and every control command; they may observe only deterministic kernel refusal and never open `/dev/ublk*` | #13/#16 |
| `regular_file_descriptor_queue_decodes_owned_snapshot` | a page-sized regular file carries one manufactured io descriptor; queue decoding returns the exact owned values and rejects an out-of-range tag without submitting a device request | #13/#16 |
| `regular_file_fetch_session_drains_refusal_without_a_device` | a regular-file fetch session exercises RAII ownership and bounded CQE draining; a kernel refusal is accepted only as refusal evidence and no ublk device is created | #15/#16 |
| `regular_file_server_handles_join_after_kernel_refusal` | RAM and generic DT-3 servers run against a page-sized regular file, return a bounded kernel refusal, and join every thread without opening `/dev/ublk*` | #15/#16/#17 |
| `fake_queue_runs_dispatch_commit_and_abort_without_a_device` | an injected in-memory queue drives READ dispatch, worker reply, COMMIT, and terminal ABORT while recording exact results without a kernel device | #13/#15/#16 |
| `fake_queue_rejects_unsupported_and_preserves_write_payload` | an injected unsupported descriptor commits `EINVAL`; a separate WRITE descriptor proves the exact copied payload and recycled buffer without a kernel device | #13/#16 |
| `serve_request_refuses_unsupported_commands_and_covers_trim` | pure RAM backend requests prove trim is the documented no-op while unsupported/disconnect commands return `EINVAL` | #13/#16 |
| `server_and_worker_join_outcomes_are_fail_closed` | manufactured success and panic outcomes cover single/composite/residency joins, deterministic error mapping, demote observation, and join-all behavior | #15/#16 |
| `dt3_join_attempts_worker_after_ring_error` | manufactured ring failure plus an observable worker completion proves both join attempts occur and the ring error retains precedence | #15/#16 |
| `dt3_vram_join_attempts_worker_after_ring_error` | the unit-returning VRAM join path has the same manufactured both-attempts contract | #15/#16 |

The canonical coverage owner for this entry point is:

```bash
node tools/ci/check-rust-slice-coverage.mjs \
  -p ramshared-wsl2d \
  --files crates/ramshared-wsl2d/src/main.rs \
  --min 80 \
  --report-json tmp/memory-broker-wsl2d-daemon-cov.json
```

The ublk shared-memory and composite-teardown business paths use this separate
canonical slice gate:

```bash
node tools/ci/check-rust-slice-coverage.mjs \
  -p ramshared-uring,ramshared-wsl2d \
  --files crates/ramshared-uring/src/lib.rs,crates/ramshared-wsl2d/src/ublk_queue.rs,crates/ramshared-wsl2d/src/ublk_server.rs \
  --min 80 \
  --report-json tmp/memory-broker-ublk-safety-cov.json
```

The bounded support-policy tests imported from the PR audit use:

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-agent,ramshared-tier --files crates/ramshared-agent/src/explain.rs,crates/ramshared-agent/src/local.rs,crates/ramshared-agent/src/win_mem.rs,crates/ramshared-tier/src/cascade.rs --min 80 --report-json tmp/memory-broker-support-safety-cov.json
```

The safe suite is necessary but not sufficient for a deployment claim. The
remaining live gate is server-only: a disposable VM/QEMU or approved WSL2
environment must exercise the selected production backend and record
`BINARY_MATCH` plus before/action/after evidence. Until then this entry-point
slice has an explicit `BINARY_MATCH/E2E` gap and is not a live-daemon DONE.

---

## Order of Implementation

1. **P0 Baseline Verification:** Compile measurement scripts and log results in `docs/reliability/memory-broker-p0-results.md`.
2. **Crate Setup:** Create `ramshared-broker`, `ramshared-nvml`, and `ramshared-config`.
3. **Broker Core:** Implement slices, arbiter, TCP listeners, and Named Exports negotiation.
4. **Agent Integration:** Refactor agent main loop, implement Windows target code and metrics.
5. **Generic DCC/workload telemetry:** Implement app-agnostic local bridge and aggregate workload measurement.
6. **E2E Validation:** Run isolated QEMU crash tests and E2E remote VM simulations.
