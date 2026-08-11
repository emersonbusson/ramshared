# IMPL — RamShared Memory Broker

> SSDV3 Step 3 implementation record. This slice closes the safe local code
> surface; hardware and host-pressure claims remain environment-bound.

## Status

**CODE GREEN / HARDWARE GATES PARTIAL.** The broker model, JSON-lines protocol,
slice/lease state machine, DCC transport classification, local agent protocol,
Windows memory sampler boundary, and generic local workload bridge are implemented and
covered by local tests. CUDA/Vulkan/root ublk ignored tests were executed on the
current host, and the standalone ublk daemon smoke was executed through the
isolated QEMU drill. A real Windows GPU pressure campaign and a disposable WSL2
freeze campaign are not claimed on the shared desktop.

## Implemented surface

| Area | Result |
| --- | --- |
| Broker slices/arbiter/leases | Existing implementation verified; DCC tenant excluded from swap rotation |
| Shared TOML config | `ramshared-config` with defaults and validation |
| Host agent | `ramshared-host-agent` registers `DccAgent` and forwards lease/status requests |
| Local workload protocol | Bounded JSON-lines bridge, separate from broker protocol |
| Generic workload measurement | Aggregate VRAM/RAM sampler and idle/load/recovery gate for externally launched GPU workloads |
| Windows memory pressure | Locale-neutral CIM parser and Windows sampler boundary |
| Explanations | Deterministic evidence formatter and `ramshared diagnose --events PATH`; no unsupported process attribution |

## Local evidence

- `cargo test --workspace --all-targets`: PASS.
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo test -p ramshared-cuda -- --ignored --test-threads=1`: PASS.
- `cargo test -p ramshared-vulkan -- --ignored --test-threads=1`: PASS.
- `cargo test -p ramshared-winsvc cuda_probe::tests::probe_cuda_allocates_roundtrips_and_restores -- --ignored --test-threads=1`: PASS.
- `ramshared-wsl2d` ignored CUDA backend tests: PASS.
- Root `ublk_control_smoke --ignored --test-threads=1`: PASS.
- Root `ublk_io_smoke --ignored --test-threads=1`: PASS.
- `./scripts/kernel/qemu-broker-drill.sh`: PASS (`KTEST-DAEMON-BINARY-MATCH=ok`, `KTEST-AGENT-BINARY-MATCH=ok`, `KTEST-SWAP-ACTIVE=ok`, `KTEST-TELEMETRY=ok`, `KTEST-SWAPOFF=ok`, `KTEST-DAEMON-TERMINATED=ok`).
- `./scripts/kernel/qemu-ublk-daemon.sh`: PASS (`KTEST-BINARY-MATCH=ok`, `KTEST-SERVED=ok`, `KTEST-TERMINATED=ok`, `KTEST-DEVICE-REMOVED=ok`).
- `scripts/p0/measure-gpu-workload-vram.ps1` PowerShell parser: PASS.
- `scripts/p0/Invoke-GpuWorkloadGate.ps1` PowerShell parser: PASS.
- `scripts/p0/Start-CudaVramWorkload.ps1` PowerShell parser: PASS; live smoke 128 MiB for 3 s: PASS.
- `scripts/p0/Invoke-GpuWorkloadGate.ps1` with generic CUDA workload 1024 MiB for 35 s: PASS on RTX 2060 (idle peak 1525 MiB, loaded peak 2648 MiB, recovery peak 1540 MiB).
- `cargo test -p ramshared-cli --all-targets`: PASS, including `diagnose` JSONL summaries.
- `git diff --check`: PASS.

## Explicit non-claims

- The cross-feature source of truth for environment-bound open gates is
  [`docs/reliability/GAP-REGISTER.md`](../../../reliability/GAP-REGISTER.md).
- No evidence yet that an external GPU workload reduces WDDM budget and causes a
  successful live DEMOTE under load.
- No evidence yet for two `before -> action -> after` freeze rounds on a
  disposable WSL2/GPU lab.
- Aggregate VRAM metrics do not identify a particular application without process
  attribution telemetry; explanations must say “process not attributed”.

## Naming boundary

The reclaim behavior is named **VRAM Reclaim** and remains application
agnostic. No app-specific integration directory is part of this slice; host
adapters must live behind a generic DCC/workload boundary and must not name the
generic broker, host agent, or reclaim policy.

## Rollback

Revert the DCC/config/local-workload slice independently if the local protocol changes
or if a live campaign shows a lease, swap, corruption, or teardown regression.
The existing cascade rollback remains `swapoff` first; no local workload path can issue
swap commands.

**Rollback trigger:** any lease granted without broker confirmation, local
message over 64 KiB accepted, DCC tenant entering swap rotation, or any live
campaign observing corruption, ghost swap, Oops, bugcheck, or forced daemon
termination.

## 2026-08-11 broker shutdown wake closure

### Status

**implemented · cover ✓ · safe RAM-broker E2E ✓ · BINARY_MATCH ✓.** This
closure fixes the hosted CI hang without promoting the separately env-bound
GPU, active-swap, ublk, or Windows lab gates.

### Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-wsl2d/src/conn.rs` | ITEM-7 / DT-50 | Added the internal `WMsg::Shutdown` control wake. |
| `crates/ramshared-wsl2d/src/main.rs` | ITEM-8 / DT-50 | Paired the terminal flag with a nonblocking wake, preserved earlier FIFO I/O, and connected the production signal bridge. |
| `docs/specs/no-milestone/memory-broker/SPEC.md` | ITEM-7/8 | Defined shutdown, full-queue, drain, and refusal semantics plus named tests. |
| `tools/ci/plan-rust-slice-coverage.test.mjs` | ITEM-8 | Bound the new named tests to the canonical coverage owner. |

### Validation (numbers)

- Hosted symptom: release PR #151 job `CI Contract / ci-core / fmt + clippy + test`
  reached its exact 30-minute limit; the independent CI caller passed the same
  release SHA in 32 seconds.
- Root-cause proof: the isolated worker test repeatedly stalled in
  `std::sync::mpsc::Receiver::recv_timeout`; a debugger backtrace placed the
  worker in the receive wait while the test thread was joining it.
- TDD RED: missing explicit wake types failed compilation; the FIFO test then
  failed with a disconnected reply, and the full-queue test timed out waiting
  for the absent terminal marker.
- Tests: final `cargo test --workspace -- --test-threads=1` exited 0;
  `ramsharedd` passed 47/47 tests. The formerly intermittent test passed
  100/100 bounded repetitions.
- Format/clippy: `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` exited 0.
- Cover: `crates/ramshared-wsl2d/src/main.rs` 81.7% (2827/3461), report
  `tmp/memory-broker-wsl2d-daemon-cov.json`; `conn.rs` 96.5% (497/515), report
  `tmp/wsl2-conn-cov.json`.
- SPEC matrix: the planner suite passed 25/25 with 88.85% lines, 81.80%
  branches, and 97.70% functions; `plan-rust-slice-coverage.mjs --all`
  returned `READY`.
- Live E2E: before — no `ramsharedd` process and no test socket; action —
  release RAM broker created one 1 MiB slice, matched
  `target/release/ramsharedd`, and received SIGTERM; after — exit 0 in 1995 ms
  (within the existing 2 s control-plane tick), owned socket absent. Refusal —
  an exact regular-file socket path exited 1 and preserved its SHA-256.
  Artifacts: `tmp/pr151-broker-shutdown-e2e/`.

### Gaps

- Closed for the safe RAM-broker shutdown surface.
- Hosted PR #151 same-revision checks remain the final promotion gate until the
  fix reaches `main` and release-please refreshes the PR.
- GPU pressure, active swap, and physical Windows/VM evidence remain unchanged
  environment-bound gaps; this closure does not claim them.

### Rollback trigger

Worker wake at or above 1 s; full RAM-broker shutdown at or above 3 s; any
queued reply lost; a notifier surviving receiver teardown; BINARY_MATCH
mismatch; or an owned socket remaining after exit.

### Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-B1/RNF-6 | ITEM-7/8 test contract | `74f8f7f`, `dffba74` |
| RF-B1/RNF-6 | ITEM-7/8 implementation | `20eb5bb`, `795a292` |
