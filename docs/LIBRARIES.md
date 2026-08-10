# LIBRARIES — API/subsystem decisions (RamShared)

Anti-halo record (Kahneman #11): no API, subsystem, or dependency enters
without a **measurable criterion**, **alternatives**, and a **"when to
revisit."** An ideal LKM has zero external dependencies — this record covers
**kernel API choices** and the few userspace dependencies. It includes what is
**deliberately NOT used**. The current NBD path remains free of external
dependencies; Phase B/ublk has an explicitly gated userspace exception.

## Active choices

| Choice | Criterion (measurable / hard compatibility) | When to revisit |
| --- | --- | --- |
| **Block backend: NBD** (Phase A) | the only one that works on consumer GeForce (`nvidia_p2p_*` → `EINVAL`); `nbd.ko` present, requiring only `modprobe` | when the WSL2 kernel has `CONFIG_BLK_DEV_UBLK` |
| **Block backend: ublk** (Phase B) | lower latency (io_uring), without a socket round trip | requires a custom kernel; only after Phase B |
| **Userspace ring: `ramshared-uring` + `io-uring` crate** (Phase B, gated) | `ramshared-uring` isolates any SQE `unsafe`; `io-uring 0.7.12` (MIT/Apache-2.0) avoids hand-rolled acquire/release barriers; the lockfile also brings `libc`, `bitflags`, and `cfg-if`; ADR-0004 accepts the exception | remove if the ublk benchmark does not surpass NBD or the supply-chain audit fails |
| **Broker protocol: `serde`/`serde_json` (JSON Lines)** (P1, broker) | control plane at ~1 msg/s/tenant, debuggable with `nc`/`jq`; `serde 1.0.228` + `serde_json 1.0.150` (MIT/Apache-2.0) provide shape validation for free and avoid a fragile hand-rolled parser; confined to the `ramshared-broker` crate (daemon/library retain `#![forbid(unsafe_code)]`); [ADR-0005](decisions/ADR-0005-broker-protocol-jsonl.md) | migrate to length-prefixed (`bincode`) if payload >64 KiB/msg or >100 msg/s/tenant |
| **Hot tier: zram (lzo-rle)** | compressed RAM, low latency; present (`CONFIG_ZRAM=m`) | if `CONFIG_ZRAM_WRITEBACK` is enabled → writeback to VRAM |
| **VRAM: CUDA Driver API via `dlopen` / `LoadLibraryW`** | same `_v2` table in `libcuda.so` (Linux/WSL2) and `nvcuda.dll` (Windows); split `loader_unix`/`loader_win` loader (windows-swap-driver SPEC ITEM-1) | if Microsoft/NVIDIA break stable `_v2` symbols or a coherent path emerges (bare-metal CXL) |
| **Windows block path: StorPort virtual miniport + SPSC rings** (P4) | Day-0 from scratch; userspace `ramshared-winsvc` + CUDA; secondary pagefile; [ADR-0006](decisions/ADR-0006-storport-virtual-miniport.md) | if ITEM-8 proves B2 infeasible (BugCheck 0x7a without mitigation) → park PRD abort #2b |
| **Windows dependencies (`windows-sys` / future `windows-service`)** | `windows-sys` already exists in `ramshared-cuda` (loader); winsvc will add `windows-service` only under `cfg(windows)` | if `cargo deny`/audit fails or the license leaves MIT/Apache |
| **Windows config/evidence codecs: `toml` 1.1.3 and `base64` 0.23.1** | `toml` parses the closed service configuration; `base64` encodes the fixed PowerShell UTF-16LE command. `base64` uses `default-features = false, features = ["std"]`, deliberately excluding its default `simd-unsafe` path because this control-plane encoding is not throughput-sensitive. Both are MIT/Apache-2.0. | revisit on RustSec/cargo-deny failure, an incompatible config grammar, or removal of the bounded PowerShell adapter |
| **VRAM backend Vulkan: `ash` 0.38** (RF-G2) | standard Vulkan binding (battle-tested, maintained); reuse > hand-roll of loader/FFI (hard rule #1); `DEVICE_LOCAL` + staging + transfer queue covers "any GPU" (AMD/Intel/NVIDIA); **validated in lavapipe** (Vulkan on CPU): byte-identical round trip. `unsafe` isolated in the `ramshared-vulkan` crate (`// SAFETY:` per block), with an `unsafe`-free trait boundary. MIT/Apache-2.0 | if `cargo audit`/`deny` on `ash` fails, or if D3D12/`/dev/dxg` (RF-G3) covers non-NVIDIA within WSL2 |
| **Userspace language: Rust (std)** | safety + GPU-resource RAII (see [ADR-0002](decisions/ADR-0002-rust-userspace-port.md)) | if FFI proves unstable (ADR-0002 rollback) |

## Deliberately NOT used

- **ImDisk / WinSpd as product** — only historical instruments for Phase 0;
  the product is Day-0 StorPort (ADR-0006).
- **`nvidia_p2p_get_pages_persistent` / BAR1 `ioremap_wc`** — `EINVAL` on
  consumer GeForce; BAR1 maps only ~16 MiB (framebuffer).
- **zram-writeback** — requires `CONFIG_ZRAM_WRITEBACK` (custom kernel); a
  priority cascade resolves Day-0.
- **MTD/phram (direct MMIO)** — discarded for performance (CPU memcpy).
- **OpenCL** (original PRD-2 proposal) — CUDA selected for the WSL2/GPU-PV
  path.
- **`vulkano` (high-level Vulkan)** — hides the memory/queue control required
  by the tier (`DEVICE_LOCAL` allocation, transfer queue, staging) and adds
  weight; `ash` (thin binding) is selected in RF-G2. Hand-rolled `libvulkan`
  FFI is also discarded: its surface is too large for Day-0 (hard rule #1).
- **`clap` (argument parsing)** — discarded to preserve **zero external
  dependencies** in a Ring-0/Day-0 project (#11). For ~4–9 flags, the
  hand-rolled parser (`std::env::args`) is sufficient; clap would bring ~10
  transitive crates + build cost. The quality of the "polish" (issue #3 LOW)
  came from **typed errors** (`CascadeError`, without a dependency), not clap.
  Revisit if the CLI grows substantially (many subcommands/validations with
  rich `--help`).

## Forward (bare metal — decisions to record when applicable)

`HMM`/`devm_memremap_pages(DEVICE_PRIVATE)` versus NUMA hotplug · `spinlock`
versus `mutex` on the hot path · `workqueue` versus `kthread` — each requires a
measurable criterion and its own ADR.
