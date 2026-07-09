# Runbook — Step 0 / Virtual Disk Pagefile Drill (Windows VM)

> **Objective:** Empirically measure how Windows behaves when the **backend of a virtual disk hosting an active secondary pagefile disappears** — verifying if it is gracefully contained (only the affected user processes die, analogous to Linux's `SIGBUS`) or triggers a BSOD `KERNEL_DATA_INPAGE_ERROR` (0x7a). This evaluates risk R7 from `PRD.md` (the largest risk of this feature) **without writing a driver from scratch** and **without risking the physical host machine** — by using an off-the-shelf virtual disk (ImDisk/VHDX) inside a **disposable Windows VM**.
>
> **Context:** Research dated 2026-07-03 indicated a high risk of BSOD. This drill **confirms or refutes** it empirically, and — crucially — tests if a **driver-mediated I/O error** (where the backend returns I/O errors but the disk is NOT physically unplugged) is more recoverable than the raw "disk unplugged" scenario covered by legacy research.

## ⚠️ Strict Safety Rule

*   **ONLY execute inside a disposable Windows VM, NEVER on the physical host.** A blue screen is the expected outcome of some test scenarios — hence the VM isolation. Take a VM snapshot BEFORE each destructive test run.
*   This is the exact Windows equivalent of our Linux rule: crash drills run only in QEMU/VM, never on the active WSL2 host (`benchmarks.md`).

## Prerequisites (Host — Requires Admin Privileges)

1.  **Full Hyper-V Support** enabled (WSL2 uses a subset; this drill requires full VM management): PowerShell admin -> `Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All` (may require reboot). Host resources verified: C:\ ~136 GB free, 32 GB RAM.
2.  **Windows ISO** (Free Evaluation): "Windows 11 Enterprise Evaluation" (90-day trial) from the Microsoft Evaluation Center (~5-6 GB). No paid license required.
3.  **Hyper-V VM Configuration**: 4 GB RAM, 40 GB disk, 2 vCPUs. Install Windows normally. **NO** GPU/CUDA passthrough is required in the VM — this drill evaluates PAGEFILE BEHAVIOR, not VRAM execution (VRAM only enters the loop in the real product; here, a standard SCSI virtual disk acts as the volatile backend).
4.  Inside the VM: Install **ImDisk Toolkit** (or use native VHDX hot-plugging) to provide a controllable virtual disk interface.

## Scenarios (Run each starting from a clean snapshot)

### Scenario A — Does Windows accept a secondary pagefile on a custom virtual disk?
1.  In the VM: Initialize a SCSI virtual disk of ~2 GB, formatted in NTFS, assigned a drive letter (e.g., `E:`).
2.  Control Panel -> System -> Advanced System Settings -> Performance -> Virtual Memory -> Add a system-managed pagefile on `E:` (e.g., 1024–2048 MB). Apply.
3.  Execute `Get-CimInstance Win32_PageFileUsage` (or inspect Virtual Memory settings) to confirm that `E:\pagefile.sys` is **active**.
    *   **PASS-A:** Windows accepted the pagefile on the custom virtual disk.
    *   **FAIL-A:** Windows rejected the disk -> the "secondary pagefile on custom disk" approach is unviable. Record the exact error message.

### Scenario B (Decisive) — What happens when the backend disk disappears with an active pagefile?
> **Take a snapshot before proceeding.** This scenario may trigger a BSOD.
1.  With the pagefile on `E:` active (Scenario A), generate **sustained memory pressure** in the VM until Windows pages out to `E:` (e.g., run a memory allocator that exceeds the VM's RAM capacity; verify via `Get-Counter '\Paging File(*)\% Usage'` that pagefile usage on `E:` is > 0).
2.  **Simulate backend failure** in two ways (run both from fresh snapshots):
    *   **B1 — Abrupt Disconnect (Hot-Remove):** Abruptly detach the virtual disk from the VM SCSI controller while the pagefile is active and usage is > 0. (Simulates sudden hardware unplugging).
    *   **B2 — Driver-Mediated I/O Error:** Simulate all I/O failing on the device while the disk object itself remains present in the OS (simulates the miniport driver remaining active but the userspace service dying). If ImDisk/VHDX cannot simulate this, mark it as "not testable without our driver".
3.  Observe system behavior:
    *   **Graceful Containment (Safe):** Only the user process(es) that had pages swapped out to `E:` terminate; the VM operating system remains stable and responsive (analogous to Linux `SIGBUS`).
    *   **BSOD (Fatal):** The system crashes with a `KERNEL_DATA_INPAGE_ERROR` (0x7a) or similar BugCheck -> worst-case scenario confirmed; triggers the mitigation logic of PRD §14 #2b.

## Decision Matrix

| Result | Meaning | Action (PRD) |
| --- | --- | --- |
| FAIL-A | Windows rejects pagefile on custom disk | **Abort** transparent pagefile path; fallback to app-opt-in |
| PASS-A + B contained (B1/B2) | Backend failure is gracefully contained | **GO** — Proceed to driver MVP with mitigation policies |
| PASS-A + B1 BSOD, B2 contained | Physical detachment crashes, but driver-mediated I/O error does not | **Conditional GO** — Miniport driver must capture and return I/O errors; never report disk detachment |
| PASS-A + B BSOD on both | Worst-case confirmed, unmitigatable at driver level | **Re-evaluate**: Accept as experimental feature with risk disclosures OR pivot to app-opt-in |

## Results (Executed on 2026-07-03)

**Environment:** Hyper-V VM `win11-drill` (Windows 11 Pro 25H2, 4 GB -> 2 GB RAM, Secure Boot OFF + test-signing), installed headless via `autounattend.xml`. Automation orchestrated via **PowerShell Direct** (no GUI/network). Volatile backend = **5 GB hot-removable VHDX** on SCSI 0:1. Execution scripts stored in `C:\ramshared-drill\`.

| Scenario | Result | Evidence |
| --- | --- | --- |
| **A** — Windows accepts secondary pagefile on SCSI virtual disk | **PASS-A** | `E:\pagefile.sys` alloc=4096 MB **active** after reboot (`Win32_PageFileUsage`) |
| **B1** — Disk disappears (hot-remove) with active pagefile | **Gracefully Contained (3x runs: 194MB@4GB RAM, 178MB@2GB RAM)** | Disk detached from host/guest (`Test-Path E:\` = False); VM remained **responsive** for 120s; **no** `BugCheck 1001` or `MEMORY.DMP` generated |
| **B2** — Driver-mediated I/O error (miniport alive, I/O fails) | **NOT TESTABLE** without custom driver | Deferred to driver MVP stage |

**Verdict:** **"PASS-A + B contained"** -> **GO** (Proceed to driver MVP with mitigation policies) — with the following critical considerations:

*   Active pages on the detached disk were **~150-200 MB of USER space memory**, not kernel-mode pages (paged pool). Research indicates that `KERNEL_DATA_INPAGE_ERROR` crashes are caused by lost **kernel pages**. This vector was not reproducible using a userspace stressor. -> **User-workload = contained (empirical); Kernel-page = unrefuted.**
*   **Methodology Finding:** Windows 11 *Memory Compression* compresses pages in RAM and masks pagefile activity when data is compressible (pagefile remained at ~2 MB even under load). Only **incompressible data** (`[System.Security.Cryptography.RandomNumberGenerator]::GetBytes`) successfully forced pagefile usage on `E:`. Future stressor specifications must use random, incompressible buffers.

**PRD Action:** R7 downgraded to MEDIUM for user-workloads (remains HIGH for kernel-pages); §14/§Step-0 updated with "EMPIRICAL RESULTS"; SPEC must include a test verification that **forces paged-pool/kernel-page pageout** (via our miniport driver) before release.
