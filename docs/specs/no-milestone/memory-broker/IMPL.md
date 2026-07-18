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
- `./scripts/kernel/qemu-ublk-daemon.sh`: PASS.
- `scripts/p0/measure-gpu-workload-vram.ps1` PowerShell parser: PASS.
- `scripts/p0/Invoke-GpuWorkloadGate.ps1` PowerShell parser: PASS.
- `cargo test -p ramshared-cli --all-targets`: PASS, including `diagnose` JSONL summaries.
- `git diff --check`: PASS.

## Explicit non-claims

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
