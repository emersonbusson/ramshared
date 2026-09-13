# IMPL — Native Linux Kernel VMBus Atomic Headroom and Hyper-V Balloon Protection

## 1. Summary of Changes

Authored an upstream-ready Linux kernel patch for `microsoft/WSL2-Linux-Kernel` targeting `arch/x86/kernel/cpu/mshyperv.c` and `drivers/hv/hv_balloon.c`. The patch introduces automated late-init calibration of `min_free_kbytes` based on Hyper-V guest RAM topology and implements cooperative balloon backpressure during active direct reclaim.

## 2. Artifacts Produced

- **Patch file**: `docs/upstream/patches/0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch`
- **PRD**: `docs/specs/no-milestone/wsl2-kernel-vmbus-headroom/PRD.md`
- **SPEC**: `docs/specs/no-milestone/wsl2-kernel-vmbus-headroom/SPEC.md`
- **AUDIT-2.5**: `docs/specs/no-milestone/wsl2-kernel-vmbus-headroom/AUDIT-2.5.md` (Verdict: `go`)

## 3. Evidence & Verification

- **Syntax & Patch Format**: Conforms to standard `git format-patch` with DCO sign-off (`Signed-off-by: Emerson Busson`).
- **Compilation**: Compiled `vmlinux` (190 MiB) and `arch/x86/boot/bzImage` (15 MiB) under `6.18.40.1-microsoft-standard-WSL2+` with zero errors.
- **Binary Symbol Verification**: Verified kernel strings in `vmlinux` and `bzImage`:
  - `Hyper-V: Calibrating min_free_kbytes from %d kB to %lu kB for VMBus resilience`
  - `hv_balloon: balloon inflation deferred; guest memory constrained`
- **Host Staging**: Deployed `arch/x86/boot/bzImage` to `C:\wsl\kernel-ramshared-new` (with `C:\wsl\kernel-ramshared.bak` preserved).
- **WSL Configuration**: Configured `%USERPROFILE%\.wslconfig` to boot `kernel=C:\\wsl\\kernel-ramshared-new` on next WSL restart.
- **Documentation check**: Passed `./scripts/docs-check.sh` (`✓ docs-check OK`).
