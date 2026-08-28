# LKML Patch Submission & Upstream Integration Guide

**Canonical Reference:** `docs/upstream/LKML-PATCHSET.md`
**Subsystem:** `linux-block` subsystem, Jens Axboe
**Maintainer:** Emerson Busson

---

## 1. Upstream Patchset Overview

The RamShared in-tree kernel driver is structured for direct mainline submission under `drivers/block/ramshared/`:

| Patch | Target File(s) | Description |
| :--- | :--- | :--- |
| **`[PATCH v1 0/2]`** | Cover Letter | Architectural overview, benchmark evidence, and safety rationale |
| **`[PATCH v1 1/2]`** | `drivers/block/ramshared/` | Core block driver (`main.c`, `dma.c`, `queue.c`, `ramshared.h`, `compat.h`) |
| **`[PATCH v1 2/2]`** | `drivers/block/Kconfig`, `Makefile` | Subsystem integration & Kconfig entries (`CONFIG_BLK_DEV_RAMSHARED`) |

---

## 2. Pre-Flight Verification Protocol

Prior to dispatching patches:

1. **Checkpatch Strict Linting:**
   ```bash
   ./scripts/ci/check-kernel-style.sh
   ```
   *Requirement:* 0 errors, 0 warnings, 0 checks.

2. **Sparse Static Semantic Memory Analysis:**
   ```bash
   ./scripts/ci/check-kernel-sparse.sh
   ```
   *Requirement:* Clean `__iomem` pointer isolation and type safety.

3. **Adversarial Invariant Verification:**
   ```bash
   ./scripts/ci/check-adversarial-invariants.sh
   ```
   *Requirement:* All 6/6 invariant gates passed.

---

## 3. Mailing List Dispatch Protocol

Submit patches using standard `git send-email` to the Linux Block Layer Maintainers:
```bash
git send-email \
    --to="linux-block mailing list" \
    --annotate \
    artifacts/lkml-patchset/*.patch
```
