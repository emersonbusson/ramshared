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
- **Dry-run validation**: Patch structure verified against Linux 6.6 / 6.18+ Hyper-V trees.
- **Documentation check**: Passed `./scripts/docs-check.sh` (`✓ docs-check OK`).
