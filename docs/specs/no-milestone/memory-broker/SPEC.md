# SPEC — RamShared Memory Broker (P0 + P1 + P2)

> SSDV3 STEP 2, generated from `docs/specs/no-milestone/memory-broker/PRD.md`. Slug: `memory-broker`.
> Scope: **P0 (Measurement) + P1 (Linux-to-Linux Broker Core) + P2 (Windows Bridge + generic DCC/workload telemetry MVP)**.
> Disciplines: Mandatory links to `docs/methodology/kahneman-disciplines.md` in every critical step.

## Audit Logs

> **Modelo Advoq/RamShared:** um único `SPEC.md` por feature. Cada rodada do Passo 2.5 **revisa este arquivo in-place**; o histórico de texto antigo vive no `git log` — **não** há `SPECv2.md` / `SPECvN.md`.

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

## Technical Decisions (DT-1 to DT-44)

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
| **ITEM-9 (Watchdog)** | #13 Validity Illusion | Does the watchdog handle dead brokers with active swap? | QEMU drill phase 2 showing clean EIO recovery under 5s | Stuck swapoff during simulation |
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
- Host-specific adapters are explicitly deferred until requested and must not name the generic reclaim path.

---

## Files to MODIFY

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
- Make readers/writers generic over stream types. Add `ZeroExport` to `WMsg`.

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

---

## Order of Implementation

1. **P0 Baseline Verification:** Compile measurement scripts and log results in `docs/reliability/memory-broker-p0-results.md`.
2. **Crate Setup:** Create `ramshared-broker`, `ramshared-nvml`, and `ramshared-config`.
3. **Broker Core:** Implement slices, arbiter, TCP listeners, and Named Exports negotiation.
4. **Agent Integration:** Refactor agent main loop, implement Windows target code and metrics.
5. **Generic DCC/workload telemetry:** Implement app-agnostic local bridge and aggregate workload measurement.
6. **E2E Validation:** Run isolated QEMU crash tests and E2E remote VM simulations.
