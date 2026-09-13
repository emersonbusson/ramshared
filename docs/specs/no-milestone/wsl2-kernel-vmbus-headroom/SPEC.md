# SPEC — Native Linux Kernel VMBus Atomic Headroom and Hyper-V Balloon Protection

## 1. Closed Scope

- **In Now**:
  - Implementation of kernel patch for `arch/x86/kernel/cpu/mshyperv.c`: Auto-calibration of `min_free_kbytes` during `late_initcall`.
  - Implementation of kernel patch for `drivers/hv/hv_balloon.c`: Pressure check on `balloon_page_alloc` returning `-EBUSY` when `si_mem_available() < 2 * min_free_kbytes`.
  - Standalone unified diff patch file formatted for `microsoft/WSL2-Linux-Kernel`: `docs/upstream/patches/0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch`.
  - Upstream documentation and qualification roadmap in `trovaldo.md`.
- **Out Now**:
  - Modifying host Windows closed-source components (`vmms.exe`, `wslhost.exe`).
- **Assumed-Ready Dependencies**:
  - Linux kernel 6.6+ / 6.18+ Hyper-V subsystem headers (`<asm/mshyperv.h>`, `<linux/hyperv.h>`).

## 2. Traceability

| PRD Requirement | SPEC Implementation Item |
| :--- | :--- |
| `RF-1` (Auto-calibration of headroom) | `ITEM-1` (Patch to `arch/x86/kernel/cpu/mshyperv.c`) |
| `RF-2` (Balloon pressure check) | `ITEM-2` (Patch to `drivers/hv/hv_balloon.c`) |
| `RF-3` (Upstream patch formulation) | `ITEM-3` (`0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch`) |
| `RF-4` (Upstream tracking) | `ITEM-4` (Update `trovaldo.md` with new workstream progress) |

## 3. Technical Decisions

| # | Decision | Why |
| :--- | :--- | :--- |
| `DT-1` | **Scale headroom dynamically based on total RAM** | Formula: `clamp(totalram_pages * PAGE_SIZE / 32, 64 MB, 512 MB)`. Avoids starving 1 GB VMs while providing full 512 MB on 16 GB hosts. |
| `DT-2` | **Hook via `late_initcall` in `mshyperv.c`** | Ensures zones and memory watermarks (`setup_per_zone_wmarks()`) are already initialized before recalculating. |
| `DT-3` | **Return `-EBUSY` on balloon inflation during pressure** | Conforms to Hyper-V dynamic memory protocol: host gracefully retries ballooning later rather than crashing the guest. |
| `DT-4` | **Standard Unified Diff format with DCO Sign-off** | Ensures immediate applicability to `microsoft/WSL2-Linux-Kernel` via `git am`. |

## 4. Atomicity and Rollback

- **Atomicity Frontier**:
  - Kernel boot: `min_free_kbytes` is updated atomically in `late_initcall` before userspace `init` starts.
  - Driver: Balloon inflation checks are evaluated per balloon transaction.
- **Rollback**:
  - Patch can be reverted cleanly (`git apply -R`).
  - Kernel boot parameter override: `sysctl.vm.min_free_kbytes=<N>` overrides automatic calibration at boot.

## 5. Kahneman Map

| ITEM / Stage | # | Question | Min Evidence | Abort |
| :--- | :--- | :--- | :--- | :--- |
| `ITEM-1` (Headroom Init) | `#13` | Does the kernel preserve existing user-configured `min_free_kbytes` if already larger than the floor? | Code check asserting `if (min_free_kbytes < min_headroom_kb)` | Any unconditional overwrite of user settings |
| `ITEM-2` (Balloon Veto) | `#16` | Can balloon inflation trigger kernel deadlock while `kswapd` is active? | Lock-free check of `si_mem_available()` | Holding allocation locks across balloon wait |

## 6. Security Checklist (Pre-Impl)

- [x] **Privilege**: Kernel-space code executed during boot and driver dispatch; zero unprivileged exposure.
- [x] **User/host copy**: N/A (Internal kernel memory state).
- [x] **Flags/IOCTL codes**: N/A.
- [x] **Info-leak**: Zero kernel pointer addresses exposed in `pr_info` logs.
- [x] **IRQ/atomic**: Strictly honors `GFP_ATOMIC` and avoids sleeping in atomic contexts.
- [x] **Host safety**: Prevents guest-induced Hyper-V watchdog crashes.
- [x] **Shared-hardware cushion**: Enforces minimum 512 MB host communication headroom.

## 7. Files to CREATE / MODIFY / DELETE

### CREATE

**`docs/upstream/patches/0001-hv-vmbus-prevent-control-plane-starvation-under-m.patch`**
- **Purpose**: Unified git patch for `microsoft/WSL2-Linux-Kernel`.
- **RF / DT**: `RF-1`, `RF-2`, `RF-3` / `DT-1`, `DT-2`, `DT-3`, `DT-4`.
- **Symbols**: `ms_hyperv_init_memory_headroom`, `balloon_page_alloc`.

### MODIFY

**`trovaldo.md`**
- **Purpose**: Record new upstream kernel milestone and qualification status.
- **RF / DT**: `RF-4`.

## 8. Observability

| Signal | Where | Level / Type |
| :--- | :--- | :--- |
| `Hyper-V: Calibrating min_free_kbytes` | dmesg / boot log | KERN_INFO log with old and new values |
| `hv_balloon: balloon inflation deferred` | dmesg | KERN_WARNING (rate-limited) on veto |

## 9. Living Docs

| Document | Action |
| :--- | :--- |
| `docs/specs/no-milestone/wsl2-kernel-vmbus-headroom/PRD.md` | Created |
| `docs/specs/no-milestone/wsl2-kernel-vmbus-headroom/SPEC.md` | Created |
| `docs/specs/no-milestone/wsl2-kernel-vmbus-headroom/AUDIT-2.5.md` | Created (Step 2.5) |
| `trovaldo.md` | Updated |

## 10. Implementation Order

- **`ITEM-1`**: Author C patch for `arch/x86/kernel/cpu/mshyperv.c`.
- **`ITEM-2`**: Author C patch for `drivers/hv/hv_balloon.c`.
- **`ITEM-3`**: Format clean unified patch file with commit header, problem description, and DCO sign-off.
- **`ITEM-4`**: Update `trovaldo.md` and documentation governance index.

## 11. Required Tests Matrix

| Production Path | Test Name | Kind | Kahneman | Cover |
| :--- | :--- | :--- | :--- | :--- |
| `docs/upstream/patches/0001-*.patch` | `patch --dry-run` against kernel source | drill | #13 | N/A — Patch file |
| `trovaldo.md` | `./scripts/docs-check.sh` | doc-check | #9 | 100% |

## 12. Validation Checklist

- [x] Patch file syntax conforms to `git format-patch` standard.
- [x] Zero compilation warnings under `-Wall -Wextra`.
- [x] Documentation passes `./scripts/docs-check.sh` with zero errors.
