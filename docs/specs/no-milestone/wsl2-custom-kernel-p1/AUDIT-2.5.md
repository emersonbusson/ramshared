# AUDIT-2.5 — wsl2-custom-kernel-p1

> **Date:** 2026-07-10  
> **Object:** [`SPEC.md`](SPEC.md) before first live `apply`  
> **Verdict:** **GO** for: build artifacts, qemu stamp, CLI status/enable/arm/disarm, **arm** on host.  
> **GO with human gate** for: `apply` (stops all WSL).  
> **NO-GO** as automatic agent action: unsolicited `apply` while user offline.

---

## Findings by severity

### CRITICAL

None open for non-apply path.

### HIGH

| ID | Finding | Disposition |
| --- | --- | --- |
| H1 | `apply` kills every WSL session | Accept with flag `--i-know-this-stops-all-wsl` + auto-revert (boot-kernel-safe) |
| H2 | Module insmod failed in qemu busybox initramfs | **Accepted residual** — qemu-validate documents best-effort modules; authoritative modprobe is post-boot (SPEC ITEM-3/8). Stamp only gates boot uname match. |
| H3 | `uname -r` of custom equals stock-style string `…microsoft-standard-WSL2+` | Detection uses **exact REL from release.txt** after boot; before reboot NEED_REBOOT when armed. Risk of false READY if stock REL ever equals custom — low; document in IMPL. |

### MEDIUM

| ID | Finding | Disposition |
| --- | --- | --- |
| M1 | Interop flaky (binfmt) | CLI fail-fast + WSL_CONFIG/WIN_USER env; arm uses /mnt/c path |
| M2 | modules_install may still be finishing in lab | .ko copied from build tree; post-apply modprobe may need full modules_install — IMPL notes |

### LOW

| ID | Finding | Disposition |
| --- | --- | --- |
| L1 | enable on stock with ublk=loaded (host already has something) | State machine still NEED_BUILD until custom bzImage + match |

---

## Kahneman map (apply path)

| # | Check | Status |
| --- | --- | --- |
| #2 | Rollback: timeout → disarm | Present in boot-kernel-safe + SPEC 60s |
| #13 | Boot PASS ≠ modules PASS | Explicit |
| #16 | Default enable safe | Verified (no shutdown in enable) |
| #17 | arm/enable idempotent | Implemented |
| #15 | No blind retry modprobe | Implemented |

---

## Open questions

1. After first human `apply`, confirm `modprobe ublk_drv` on live WSL (ITEM-8).  
2. If modules missing, finish `modules_install` + modules VHDX per MS README.

---

## go / no-go

| Path | Decision |
| --- | --- |
| status / enable / arm / disarm | **go** |
| qemu-pass stamp as apply gate | **go** (boot only) |
| Agent runs `apply` now | **no-go** (user away / session kill) |
| Human runs `apply` after training | **go** (flag + stamp present) |

**Blockers fixed in SPEC:** none new.  
**Stamp present:** `R:\WSL\RamShared-Kernel-build\qemu-pass.stamp` (2026-07-10).  
**Live (2026-07-10 evening):** kernel + `kernelModules` VHDX + `ublk_drv` + cascade NBD smoke GREEN. See `IMPL.md` + `validation.md`.
