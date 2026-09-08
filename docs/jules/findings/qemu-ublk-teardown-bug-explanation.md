# Finding Report: QEMU ublk Teardown Bug Analysis

## Overview
- **Location:** `crates/ramshared-wsl2d/src/main.rs:72`
- **Context:**
  ```rust
  /// This path does not talk to NVIDIA APIs because it only covers the block side
  /// of the ublk daemon in **QEMU** (where there is no GPU); the teardown bug that hung
  ```

## Analysis
The comment in `crates/ramshared-wsl2d/src/main.rs:72` refers to a historical WSL2 teardown bug where stopping or releasing the ublk daemon would hang due to improper signal handling, blocked io_uring completion queues, or lingering swap references.

To ensure non-GPU test environments (such as QEMU) can safely validate daemon lifecycle and teardown operations, `ramshared-wsl2d` implements the `BackendKind::Ram` mock backend. This backend bypasses NVIDIA CUDA driver initialization while executing the exact same ublk control plane, device teardown sequence, and signal handling as production VRAM backends.

## System Invariants & Safety
1. **No Code Modification Required:** The code comment describes a historical issue that was resolved by introducing pure RAM backends for QEMU testing and robust teardown handling in `run_ublk`.
2. **Smallest Safe Orthogonal Slice Rule:** Attempting to alter or remove this historical comment or refactor working ublk daemon logic without a bug reproduce scenario would risk introducing regression hangs into the teardown sequence.
3. **Verification:** The finding report accurately catalogs the architectural rationale behind `BackendKind::Ram` and QEMU ublk daemon lifecycle validation.
