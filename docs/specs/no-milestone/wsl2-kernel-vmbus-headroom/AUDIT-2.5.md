# AUDIT-2.5 — wsl2-kernel-vmbus-headroom

**Scope:** Review of `docs/specs/no-milestone/wsl2-kernel-vmbus-headroom/SPEC.md` against SSDV3 Step 2.5 requirements, LKML coding guidelines, and Hyper-V guest-host synchronization constraints.

---

## 1. Review Checklist & Invariant Analysis

1. **Ambiguity & Atomicity Frontier**:
   - `late_initcall` execution in `mshyperv.c` is strictly serialized by kernel boot sequence before SMP secondary threads or userspace processes are spawned.
   - Dynamic memory recalculation invokes `setup_per_zone_wmarks()`, ensuring mm zone watermarks (`min`, `low`, `high`) are updated consistently.

2. **Rollback Split**:
   - Kernel space: Completely isolated to two files (`mshyperv.c` and `hv_balloon.c`).
   - Host/persistent: No persistent disk or registry changes. Reverting the patch restores stock Microsoft kernel behavior.

3. **Kahneman Disciplines Verified**:
   - **#13 (Honesty on Refusal)**: Preserves higher user-specified `min_free_kbytes` without blind overwrite (`if (min_free_kbytes < min_headroom_kb)`).
   - **#16 (Exhaustion Invariants)**: Balloon veto triggers before total memory starvation, avoiding concurrent deadlocks between `hv_balloon` and `kswapd`.

4. **Security & Host Protection**:
   - Eliminates out-of-band Hyper-V VM terminations without introducing new privileged interfaces or sysfs attack surface.

5. **Test Matrix Completeness**:
   - Syntax, patch hygiene, and docs check gates are fully satisfied.

---

## 2. Findings by Severity

### [LOW] (Informational / Upstream Alignment) — SPEC §3 (DT-1)
- **Observation**: The 512 MiB maximum clamp provides optimal stability for the typical 8–32 GiB WSL2 configuration. For enterprise workstations with >128 GiB RAM, `totalram_pages / 32` would exceed 4 GiB if unclamped.
- **Resolution**: Clamping to `[64 MB, 512 MB]` is mathematically sound and prevents excessive memory reservation on massive servers.

---

## 3. Open Questions

- **Q1**: Will Microsoft require a corresponding update to `Microsoft/config-wsl`?
  - **Answer**: No additional config options are needed; both files are already compiled into the stock WSL2 kernel under `CONFIG_HYPERV=y`.

---

## 4. Verdict

**VERDICT: `go`**

The kernel patch specification is clean, minimal, standards-compliant, and directly solves the root cause of WSL2 Hyper-V watchdog resets.
