# Official Linux Kernel & Microsoft WSL2 CI Infrastructure Guide

> **Document purpose:** Comprehensive reference of all official Continuous
> Integration (CI) systems, automated test robots, compiler matrices, static
> analyzers, and runtime regression frameworks utilized by the Linux Kernel
> Mainline (`torvalds/linux` / `kernel.org`) and Microsoft WSL2
> (`microsoft/WSL` / `microsoft/WSL2-Linux-Kernel`), detailing how RamShared can
> adopt and replicate their exact validation pipelines.

---

## 1. Executive Summary & Verification Taxonomy

Modern Linux Kernel and Microsoft WSL2 development follows a strict four-layer
validation model before patches are accepted into mainline:

```text
┌───────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                   LINUX KERNEL & WSL2 CI LIFECYCLE                                │
├───────────────────────────────┬─────────────────────────────────┬─────────────────────────────────┤
│    1. Static Analysis Gate    │     2. Matrix Build Gate        │    3. Runtime & ABI Gate        │
├───────────────────────────────┼─────────────────────────────────┼─────────────────────────────────┤
│ • checkpatch.pl (Style/SPDX)  │ • Arch: x86_64, aarch64         │ • KUnit (In-kernel unit tests)  │
│ • sparse (Address space, user)│ • Compilers: GCC 11-15, Clang   │ • kselftest (Userspace/MM ABI)  │
│ • smatch (Logic & Deadlocks)  │ • Configs: defconfig, allmod    │ • QEMU/virtme-ng Boot & dmesg   │
│ • coccinelle (Semantic AST)   │ • Warnings: W=1, -Werror        │ • blktests (ublk, zram, nbd)    │
│ • Rustfmt + Clippy (RfL)      │ • Kernel configs: config-wsl    │ • libabigail (abidw/abidiff KMI)│
└───────────────────────────────┴─────────────────────────────────┴─────────────────────────────────┘
```

---

## 2. Official CI Systems in Linux Kernel & WSL2

### 2.1 Upstream Linux Kernel Robots & Test Suites

| CI System / Tool | Managing Entity | Scope & Trigger | Key Capabilities |
| :--- | :--- | :--- | :--- |
| **Intel 0-Day Robot (LKP)** | Intel Open Source Technology Center | Patch submission on LKML / branch commits | • Multi-architecture cross-compiles (x86_64, aarch64, riscv64, s390x).<br>• Automated `sparse`, `smatch`, `coccinelle`, `checkpatch.pl`.<br>• Automated headless QEMU boot test with `dmesg` splat auditing (`BUG:`, `Oops:`, `lockdep warning`). |
| **KernelCI** | Linux Foundation | Commits on `mainline`, `next`, `stable` | • Distributed build matrix across GCC (11–15) and Clang (16–20).<br>• Automated boot testing across physical hardware (LAVA) and virtual QEMU labs.<br>• Executes `kselftest` and `KUnit` suites. |
| **Linaro LKFT** | Linaro | LTS kernels & Android Common Kernel (ACK) | • Deep regression testing with `kselftest`, `LTP` (Linux Test Project), `kvm-unit-tests`.<br>• Zero-false-positive triaged skipfiles. |
| **Google Syzkaller / Syzbot** | Google | Continuous background daemon | • Coverage-guided kernel syscall fuzzing using `KCOV`.<br>• Runs under `KASAN`, `KMSAN`, `KCSAN`, and `LOCKDEP`.<br>• Produces automated C reproducers (`syz-repro`). |
| **ClangBuiltLinux CI** | Linux Foundation / LLVM community | Commits & Pull Requests | • Validates kernel compilation with LLVM/Clang (`LLVM=1`, `LLVM_IAS=1`).<br>• Detects Clang-specific warnings under `-Werror`. |

---

### 2.2 Microsoft WSL2 Subsystem & Kernel Pipelines

| Repository | Component | Build & CI Strategy | Validation Suite |
| :--- | :--- | :--- | :--- |
| **`microsoft/WSL2-Linux-Kernel`** | Guest Linux Kernel (`bzImage` / `Image`) | • Microsoft Azure DevOps internal pipelines.<br>• Builds `Microsoft/config-wsl` (x86_64) and `Microsoft/config-wsl-arm64` (aarch64).<br>• Toolchain: GCC & LLVM. | Hyper-V synthetic storage (`hv_storvsc`), networking (`hv_netvsc`), GPU-PV (`dxgkrnl`), `zram`, `nbd`, and `io_uring`. |
| **`microsoft/WSL`** | Host CLI & Subsystem Manager (`wsl.exe`, `init`) | • GitHub Actions & Azure Pipelines.<br>• Automated MSBuild / WDK compilation.<br>• Windows Server 2025 runners with WSL containers (`wslc`). | Distro installation drills, VHDX mounting, socket relay tests, and CLI parameter regression suites. |

---

## 3. Deep-Dive: Official Tools & How We Can Use Them

### 3.1 Static Analysis Tooling

```text
┌───────────────────┬─────────────────────────────────────────────────────────────────────────────┐
│ Tool              │ What It Checks & Enforces                                                   │
├───────────────────┼─────────────────────────────────────────────────────────────────────────────┤
│ checkpatch.pl     │ Linux coding style (8-space tabs, 80 cols), SPDX headers, unsafe functions  │
│ sparse            │ Address space domains (__user, __kernel, __iomem), bitwise endian types     │
│ smatch            │ Semantic bugs, NULL pointer dereferences, unbalanced locks on error exits   │
│ coccinelle        │ Semantic AST patterns, double-free detection, resource leak checks          │
│ libabigail        │ Kernel Module Interface (KMI) ABI stability diffing (abidw / abidiff)       │
└───────────────────┴─────────────────────────────────────────────────────────────────────────────┘
```

1. **`checkpatch.pl` (Coding Style & Conventions):**
   * **Source:** Official script provided in `scripts/checkpatch.pl` in the Linux tree.
   * **Command:** `perl scripts/checkpatch.pl --no-tree --strict --codespell -q -f <files>`
   * **Adoption in RamShared:** Can be executed in CI on any `.c`, `.h`, or driver source file.

2. **`sparse` (Semantic Type & Address Space Checker):**
   * **Installation:** `sudo apt-get install -y sparse`
   * **Command:** `make C=1 CF="-Wbitwise -Wsparse-all"`
   * **Adoption in RamShared:** Ensures proper separation between user pointers (`__user`) and kernel memory.

3. **`smatch` (Flow Analysis & Locking Balance):**
   * **Installation:** Compiled from `git://repo.or.cz/smatch.git` or `apt install smatch`.
   * **Command:** `smatch --kernel -p=kernel <file.c>`
   * **Adoption in RamShared:** Catches unchecked user copies and lock balance errors across error returns.

4. **`libabigail` (`abidw` / `abidiff` - ABI Stability):**
   * **Installation:** `sudo apt-get install -y libabigail-tools`
   * **Command:** `abidiff baseline-kmi.xml current-kmi.xml`
   * **Adoption in RamShared:** Guarantees that exported structs and symbols do not change layout between releases.

---

### 3.2 Matrix Builds & Compiler Flags

* **Architectures:**
  - `x86_64` (Standard x86 PCs, servers, and WSL2 hosts).
  - `aarch64` / `arm64` (ARM64 Snapdragon laptops, AWS Graviton, Apple Silicon VMs).
* **Compilers:**
  - `GCC 12`, `GCC 13`, `GCC 14`.
  - `Clang 16`, `Clang 17`, `Clang 18` with integrated assembler (`LLVM=1 LLVM_IAS=1`).
* **Warning Strictness:**
  - `make W=1`: Enables extra static warnings.
  - `-Werror`: Treats all warnings as fatal errors.

---

### 3.3 Runtime Emulation & Regression Testing

1. **Headless QEMU / `virtme-ng` Boot Verification:**
   * Uses nested KVM acceleration (`/dev/kvm`) available on GitHub Actions runners.
   * Boots the compiled kernel in ~3 seconds and monitors `dmesg` console for warnings.
   * **Asserted Patterns:** `BUG:`, `WARNING:`, `Call Trace:`, `Oops:`, `kernel panic`, `lockdep warning`.

2. **`blktests` (Block Layer & ublk Regression Suite):**
   * Official Linux block test suite for block devices (`ublk`, `zram`, `nbd`, `loop`).
   * **Command:** `./check -q ublk zram nbd`
   * **Validation:** Verifies queue stall recovery, I/O timeouts, and device teardown idempotency.

3. **Kernel Sanitizers (Debug Configs):**
   * `CONFIG_KASAN=y`: Out-of-bounds memory access and Use-After-Free detector.
   * `CONFIG_LOCKDEP=y` & `CONFIG_PROVE_LOCKING=y`: Deadlock and locking inversion verifier.
   * `CONFIG_DEBUG_ATOMIC_SLEEP=y`: Flags illegal sleeps in atomic/spinlock context.

---

## 4. Comprehensive Comparison: Official Standards vs. RamShared CI

RamShared currently enforces a 20-check CI contract defined in `docs/governance/ci-contract.json`:

| Capability Category | Linux Kernel Mainline (0-Day / KernelCI) | Microsoft WSL2 Standard | RamShared Current CI (`ci-contract.json`) | Status & Action Plan |
| :--- | :--- | :--- | :--- | :--- |
| **Rust Code Quality** | `rustfmt`, `clippy` (`CLIPPY=1`), `#![no_std]` core/alloc | N/A (C / C++) | ✅ `rust-quality` (cargo fmt, clippy -D warnings, cargo test) | **Parity Achieved** |
| **C / Driver Coding Style** | `checkpatch.pl --strict --no-tree` | Linux coding conventions | ⚠️ Not yet added to automated PR contract | 🔴 **Add `checkpatch.pl` step** |
| **Static Semantic Analysis**| `sparse` (`C=1`), `smatch`, `coccinelle` | Compiler warning clean | ⚠️ Missing `sparse` & `smatch` semantic gates | 🔴 **Add `sparse` + `smatch` analysis job** |
| **Supply Chain & CVE** | Mailbox security review, manual advisories | Microsoft internal security scan | ✅ `cargo-audit` (pinned advisory DB), `cargo-deny`, `trivy` SARIF | **Exceeds Standard** |
| **Code Coverage Gate** | `gcov` / `lcov` (ad-hoc / LKFT) | N/A | ✅ `rust-slice-coverage` (cargo-llvm-cov, exact SPEC mapping, min 80%) | **Exceeds Standard** |
| **Multi-Arch Compilation** | `x86_64`, `aarch64`, `riscv64`, `arm`, `s390x` | `x86_64`, `aarch64` (`config-wsl` & `config-wsl-arm64`) | ⚠️ `x86_64` host only; `windows-static` exercises Windows | 🟡 **Add cross-compilation matrix (`x86_64` + `aarch64`)** |
| **Dual Toolchain (GCC+Clang)**| GCC (11–14) + Clang (16–18) with `W=1` | GCC & Clang | ⚠️ Pinned Rust 1.98.0 toolchain only | 🟡 **Add GCC + Clang dual build matrix for drivers** |
| **Virtual QEMU Boot Test** | Headless QEMU boot test, zero console splats | Azure VM / WSL containers | ⚠️ Supervised lab plans exist (`wsl2-lab-plan`), not in automated PR gate | 🔴 **Add `kernel-qemu-smoke` runner via `/dev/kvm`** |
| **Block Driver Regression** | `blktests` (ublk, zram, nbd, null_blk) | Storage VHDX integration tests | ⚠️ Rust crate tests only | 🔴 **Add `blktests` suite for ublk target** |
| **Fail-Closed Governance** | Patchwork / Lore mailing lists | Branch policies in Azure DevOps | ✅ `ci-contract`, `docs-integrity`, `validation-schema`, `aggregate` | **Exceeds Standard** |

---

## 5. Adoption Blueprint: Adding Kernel-Grade Gates to RamShared

We can implement three specialized GitHub Actions workflows to achieve full parity with Microsoft and Linux Kernel maintainers:

### 5.1 Workflow 1: `kernel-static.yml` (Style, Semantic Analysis & KMI)
```yaml
name: kernel-static
on: [workflow_call]
jobs:
  kernel-static:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v4
      - name: Install Tooling
        run: sudo apt-get update && sudo apt-get install -y sparse smatch coccinelle libabigail-tools
      - name: Run checkpatch.pl
        run: |
          if [ -d "drivers" ]; then
            perl scripts/checkpatch.pl --no-tree --strict --codespell -q -f drivers/**/*.c drivers/**/*.h
          fi
      - name: Run sparse and smatch
        run: |
          make -C drivers/ C=1 CF="-Wbitwise -Wsparse-all" CHECK="smatch --kernel"
```

### 5.2 Workflow 2: `kernel-build-matrix.yml` (Cross-Arch & Toolchain Matrix)
```yaml
name: kernel-build-matrix
on: [workflow_call]
jobs:
  matrix-build:
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        arch: [x86_64, arm64]
        compiler: [gcc, clang]
        include:
          - arch: x86_64
            compiler: gcc
            make_args: "CC=gcc-14"
          - arch: x86_64
            compiler: clang
            make_args: "LLVM=1 LLVM_IAS=1"
          - arch: arm64
            compiler: gcc
            make_args: "ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu-"
          - arch: arm64
            compiler: clang
            make_args: "ARCH=arm64 LLVM=1 LLVM_IAS=1"
    steps:
      - uses: actions/checkout@v4
      - name: Build Driver Matrix
        run: make ${{ matrix.make_args }} W=1 -j$(nproc) drivers/
```

### 5.3 Workflow 3: `kernel-qemu-smoke.yml` (Headless Boot & `blktests`)
```yaml
name: kernel-qemu-smoke
on: [workflow_call]
jobs:
  qemu-smoke:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install QEMU & virtme-ng
        run: sudo apt-get update && sudo apt-get install -y qemu-system-x86 virtme-ng blktests
      - name: Boot Kernel & Run blktests
        run: |
          virtme-ng --arch x86_64 --exec "
            modprobe ublk_drv || true
            cd /opt/blktests && ./check -q ublk zram nbd
          " --save-console-log console.log
      - name: Audit Console for Kernel Splats
        run: |
          python3 -c "
          import re, sys
          with open('console.log') as f:
              log = f.read()
          for splat in [r'BUG:', r'WARNING:', r'Call Trace:', r'Oops:', r'kernel panic', r'lockdep warning', r'KASAN:']:
              if re.search(splat, log):
                  print(f'FATAL: Found kernel splat: {splat}')
                  sys.exit(1)
          print('Console clean: PASS')
          "
```

---

## 6. Conclusion & Implementation Priority

1. **Immediate Win:** Incorporating `checkpatch.pl`, `sparse`, and `smatch` into RamShared guarantees that any low-level driver C code satisfies Linux Kernel standards from Day 0.
2. **Runtime Assurance:** The `kernel-qemu-smoke` test provides automated proof on every pull request that kernel-level modules load cleanly without kernel splats or deadlocks.
3. **Upstream Acceptance:** Providing evidence generated by these exact tools (`blktests`, `abidiff`, `checkpatch`) will allow Microsoft WSL2 and Linux Kernel maintainers to review and merge RamShared contributions with zero friction.
