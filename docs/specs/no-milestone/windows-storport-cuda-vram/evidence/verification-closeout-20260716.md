# Verification closeout — 2026-07-16

This closeout distinguishes reproducible proof from environment-bound or missing proof. No host
reboot, miniport replacement, destructive WSL2 pressure, commit, or merge was performed.

| Gate | Status | Evidence / reason |
| --- | --- | --- |
| Native Rust compile and tests | PASS | `cargo test -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets`: block 41 passed; CUDA 5 passed/1 live-GPU ignored; winsvc 77 passed/1 live-CUDA ignored. |
| Native clippy | PASS | Same three packages, all targets, `-D warnings`. |
| Formatting | PASS | `cargo fmt -p ramshared-winsvc -p ramshared-cuda -- --check`. |
| Documentation and diff hygiene | PASS | `./scripts/docs-check.sh` and final `git diff --check`. |
| Named Rust slice coverage | PASS | winsvc broker/config/driver-link/evidence/runtime/service 84.3–95.5%; CUDA probe 80.0%; threshold 80%. |
| Guest concurrent injectors, rundown, and Driver Verifier | PASS | Campaign `guest-exhaustive-20260715-214831`; `IOCTL_PASS1=PASS`, `IOCTL_VERIFIER=PASS`, every ITEM-3 verdict 1, no new dump; signed miniport SHA-256 `1E57690EA63E6287D4790A134544DC9F46253BB356D1C2B3B1D65FC812F30CFF`. |
| Physical-host miniport `BINARY_MATCH` | BLOCKED | Installed `E690306F…` differs from guest-proven package `1E57690E…`; backup missing. Online was correctly skipped. |
| Real CUDA through lab GPU-PV | BLOCKED | Guest files became present, but NVIDIA remained `CM_PROB_FAILED_POST_START`; package-copy/install path timed out. Presence of DLL/tools is not CUDA execution proof. |
| Product Online, three SHA rounds, exact cleanup | BLOCKED | Requires real CUDA and corrected physical binary; neither precondition is green on an authorized surface. |
| WSL2 freeze elimination | BLOCKED | No isolated hang campaign with before/action/after, timeout/watchdog, swapoff-first, ghost check, binary match, D-state/hung-task evidence, idempotency twice, and cleanup. The daily host was deliberately not pressured. |

Safe-close evidence (`gpupv-safe-close-20260716T025812Z.txt`):

- `VM_AFTER State=Off`.
- Guest staging existed and was removed; host staging was removed.
- Host NVIDIA status was `OK` before and after; `nvidia-smi -L` found the RTX 2060.
- Guest NVIDIA status remained `Error` / `CM_PROB_FAILED_POST_START`.
- `pnputil` completion was not proven, so no speculative driver uninstall was attempted.

Conclusion: the local code gates and isolated StorPort safety campaign are proven. The product CUDA
Online path and the claim that WSL2 freezes are fixed are not proven and must not be represented as
green.
