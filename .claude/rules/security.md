---
name: security
description: Kernel/userspace security checklist for RamShared (ioctl, DMA, capabilities).
paths:
  - "**/*.{c,h,rs}"
  - docs/specs/**
---

# Security rules — RamShared

Kernel-adjacent and privileged userspace paths. Not a web/OWASP checklist.

## Mandatory on privileged surfaces

- [ ] **Capabilities:** privileged `ioctl`/`open` checks `capable(CAP_SYS_ADMIN)` (or documented policy); device nodes not world-writable by default.
- [ ] **User copy:** every user buffer uses `copy_{from,to}_user` (or equivalent) with **size + alignment + max** bounds before use.
- [ ] **No TOCTOU:** do not re-read `__user` pointers after the copy; validate once, operate on kernel copy.
- [ ] **Arg smuggling:** reject unknown flags/bits; do not trust client-supplied offsets/handles without ownership checks.
- [ ] **Info-leak:** no kernel addresses / KASLR material in `dmesg`, sysfs, or uAPI error paths.
- [ ] **IRQ/atomic:** no sleep; `GFP_ATOMIC` or no allocation; document lock order.
- [ ] **Lifetime:** get/put and map/unmap balanced; module exit and device remove free resources in reverse order (`goto out_err`).
- [ ] **Hot-unplug:** device gone → stable errno (`-ENODEV`/`-ENOENT`), not UAF.
- [ ] **Host safety:** never run unsupervised swap/ublk pressure on the live WSL2 host; shared-host pressure requires the approved Windows watchdog harness (see `benchmarks.md`).

## Userspace daemons / broker

- [ ] Protocol frames: length-prefixed or length-checked; reject oversized payloads.
- [ ] Replay: commands behind retry are **idempotent** (Kahneman #17).
- [ ] Retry: only transient signatures (Kahneman #15); deterministic errors fail-fast.
- [ ] Secrets: no hardcoded keys/tokens; env or secret store only.

## SSDV3

Security-sensitive structural work uses SSDV3 Step 2.5 + `AUDIT-2.5.md` when risk is high. SPEC must include the security checklist from `docs/SSDV3-PROMPTS.md` (mark N/A when the surface is absent).

## Don't

- ❌ Log kernel pointers "for debug" on default log levels
- ❌ World-writable `/dev` nodes "for convenience"
- ❌ Dual-path privilege bypass without Day-0 exception
- ❌ Treat "it worked in happy path" as security validation (Kahneman #13)
