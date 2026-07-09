# RamShared

GPU VRAM as a **swap** tier in Linux/WSL2. In non-graphical workloads, GPU VRAM remains ~90% idle while system RAM is exhausted, leading to swapping on slow SSDs — which are dozens of times slower than VRAM. RamShared bridges this gap by inserting VRAM into the swap hierarchy.

## Approach

VRAM is **not** suitable as hot swap space. Under heavy pressure, the WDDM eviction mechanism (WSL2/GPU-PV) preserves data but injects significant latency: a 4KB read can take up to **1.18 s** when VRAM is full — *data-safe, but latency-unsafe*. Therefore, VRAM is configured as a **cold** tier in a priority-ordered cascade using `swapon`:

```text
Memory Pressure ─► zram  (Compressed RAM, lzo-rle)  prio 200  HOT
                ─► VRAM  (CUDA + NBD)               prio 100  COLD
                ─► VHDX  (WSL2 default swap)        prio  -2  LAST
```

zram absorbs the hot working set; VRAM handles the cold overflow (hiding the latency weakness while leveraging its capacity/bandwidth). A **canary** monitor unmounts VRAM (`swapoff`) during latency spikes without interrupting running processes.

## Status

Validated end-to-end in WSL2, with memory pressure confined via cgroup v2:

*   **Spill:** 511 MiB successfully paged out to VRAM, representing **332,800 intact memory pages**.
*   **Demote:** 481 MiB of active pages migrated from VRAM to VHDX via `swapoff` with **0 corruption**.

Evidence log: [`docs/vram-as-ram/VALIDATION-CASCADE.md`](docs/vram-as-ram/VALIDATION-CASCADE.md).

## Usage

> **WSL2 Warning:** Heavy Rust builds can crash the environment — keep `cargo` execution scoped per crate, avoiding global `--release` builds.

```bash
cargo build -p ramshared-cli -p ramshared-wsl2d

sudo ./target/debug/ramshared check        # Preflight: WSL2/CUDA/kernel/tiers
sudo ./target/debug/ramshared up --vram 1024 --zram 1024
swapon --show                              # zram(200) > nbd0(100) > vhdx(-2)
sudo ./target/debug/ramshared down         # swapoff before disconnect (anti-panic)
```

## Structure (7 core crates; userspace-gated)

| Crate | Role |
|---|---|
| `ramshared-tier` | Cascade priority management + DEMOTE safety net |
| `ramshared-cuda` | CUDA Driver API wrapper via dynamic loader (**only unsafe code in the project**) |
| `ramshared-block` | Network Block Device (NBD) fixed-newstyle protocol + I/O library |
| `ramshared-integrity` | Checksum calculations + validation testing patterns |
| `ramshared-uring` | Safe wrapper over `io-uring` |
| `ramshared-wsl2d` | Daemon: state machine, `VramBackend`, canary/DEMOTE triggers |
| `ramshared-cli` | Command-line management: `check`/`doctor`/`up`/`down`/`status` |

*Note on Phase B:* The `ublk` backend has a userspace-gated exception for `io-uring`. It is used via `ramshared-uring` only for the minimal smoke test and is retained only if benchmarks prove a latency benefit over NBD.

## Requirements

*   Rust (Edition 2024).
*   WSL2 with NVIDIA GPU support via GPU-PV (`/dev/dxg` + `libcuda`).
*   Kernel modules: `CONFIG_BLK_DEV_NBD`, `CONFIG_ZRAM`.
*   Userspace utilities: `nbd-client`, `zramctl`.

## Disclaimer

This is a research and development (R&D) project. It interacts with **real swap space** and **hardware GPU layers** — run it in confined environments and with caution.

**Day-0 Policy:** No shims or temporary workarounds; every modification is built to be the definitive production-ready version.
