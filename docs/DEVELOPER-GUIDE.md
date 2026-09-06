# RamShared — Developer & Contributor Guide

A comprehensive guide for engineers and autonomous agents developing, testing, and qualifying code in the RamShared workspace.

---

## 1. System Requirements & Toolchain

| Tool / Dependency | Version / Gate | Purpose |
| :--- | :--- | :--- |
| **Rust Toolchain** | `1.98.0` (pinned) | Workspace compilation, Clippy, rustfmt, and LLVM coverage. |
| **Node.js** | `>= 20.x` (ESM) | CI contract checkers, documentation governance, and inventory tooling. |
| **Linux Environment** | WSL2 (Kernel 6.x) or Bare-metal Linux | Linux cascade daemons, `io_uring`, `ublk`, and NBD block drivers. |
| **NVIDIA Driver** | `>= 535.xx` (CUDA 12+) | Physical GPU VRAM tiering via direct driver API (`libcuda.so.1` / `nvcuda.dll`). |
| **Windows Toolchain** | Windows 11 / Server 2022 + WDK (optional) | Windows StorPort virtual miniport driver (`drivers/windows/ramshared`). |

---

## 2. Workspace Organization (15 Modular Crates)

The Rust workspace comprises 15 modular crates located in `crates/`:

```text
crates/
├── ramshared-cli/         # Primary operator CLI: doctor, stress, top, cascade, monitor
├── ramshared-wsl2d/       # Linux block daemon (ublk server + NBD engine + telemetry)
├── ramshared-agent/       # In-guest swap table monitor and watchdog agent
├── ramshared-winsvc/      # Windows StorPort virtual disk worker daemon
├── ramshared-winbroker/   # Windows SCM least-privilege broker service
├── ramshared-broker/      # Pure memory broker protocol and slice map arbiter
├── ramshared-config/      # TOML configuration parser and fail-closed limit validator
├── ramshared-tier/        # 3-tier cascade state machine and priority ordering
├── ramshared-vram/        # Hardware-agnostic VRAM traits (VramProvider / VramMemory)
├── ramshared-cuda/        # Direct NVIDIA CUDA driver loader and DMA allocator
├── ramshared-vulkan/      # Cross-vendor Vulkan allocator (AMD Radeon & Intel Arc)
├── ramshared-uring/       # Linux io_uring asynchronous block engine
├── ramshared-block/       # Authoritative SSD origin persistence and NBD wire protocol
├── ramshared-integrity/   # SHA-256 block hashing and torn-read detection
└── ramshared-dxg/         # Linux /dev/dxg WDDM memory budget query interface
```

---

## 3. Daily Development Workflow

### Formatting & Linting

Before opening any commit or pull request, verify that code meets workspace lint requirements:

```bash
# Format check
cargo fmt --check

# Strict workspace linting (zero warnings permitted)
cargo clippy --workspace --all-targets -- -D warnings
```

### Running Unit & Integration Tests

All 15 crates support offline testing without requiring root privileges or physical GPU hardware:

```bash
# Run all offline unit tests across the entire workspace
cargo test --workspace

# Run tests for a specific crate
cargo test -p ramshared-block
cargo test -p ramshared-tier
cargo test -p ramshared-winsvc
```

> [!NOTE]
> Tests requiring physical hardware (e.g. CUDA device allocations or `/dev/nbd` ioctls) are marked with `#[ignore]` and run only during live qualification drills.

---

## 4. Documentation Governance & Verification

Documentation is machine-checked by a battery of 28 automated gates:

```bash
# Run the complete documentation governance battery
./scripts/docs-check.sh

# Verify documentation inventory synchronization
node tools/ci/generate-documentation-inventory.mjs --check

# Check exact Rust slice test coverage
node tools/ci/plan-rust-slice-coverage.mjs --check
```

### Adding New Documentation

When creating new markdown files:
1. Ensure the file path matches an existing route in `docs/governance/document-lifecycle-policy.json`.
2. Regenerate the documentation inventory:
   ```bash
   node tools/ci/generate-documentation-inventory.mjs --write
   ```
3. Run `./scripts/docs-check.sh` to confirm all gates pass.

---

## 5. Live Physical Qualification

When qualifying changes against physical hardware on the WSL2 host:

```bash
# 1. Check system readiness and GPU budget
ramshared doctor

# 2. Run multi-tier cascade stress drill (reaching Tier 3 SSD fallback)
ramshared stress --cascade --battery --tier3-target-pct 45 --max-psi-full 60 --hold-sec 10

# 3. Monitor real-time memory tiering in interactive TUI
ramshared top
```

---

## 6. Key Safety Invariants & Anti-Skynet Rules

1. **Swapoff-First Ordering:** Never stop or detach block service daemons while their devices remain active in `/proc/swaps` or the Windows swap table.
2. **Write-Through Persistence:** All acknowledged writes must be committed to the authoritative SSD origin before cache mutations.
3. **Typed Errors Over Panics:** Never use `unwrap()` or `expect()` in production paths; use domain-specific error enums (ADR-0009).
4. **Volume Lock Isolation:** Windows volume mutations must acquire exclusive `LockedVolume` ownership (ADR-0010).
5. **No Persistent Secrets:** Never commit credentials, private tokens, or unredacted host user paths.
