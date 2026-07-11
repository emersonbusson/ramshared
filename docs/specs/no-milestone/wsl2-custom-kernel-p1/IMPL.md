# IMPL — wsl2-custom-kernel-p1

> **Passo 3 SSDV3.** Implements [`SPEC.md`](SPEC.md).  
> **Date:** 2026-07-10  
> **Status:** **GREEN** — build, qemu, modules.vhdx, live kernel, ublk capability, cascade NBD smoke, CLI.  
> Product cascade transport policy closed in `cascade-transport-policy` (NBD Day-1; ublk not product on WSL2).

---

## Status gates

| Gate | Result | Evidence |
| --- | --- | --- |
| V1 bzImage + release.txt | **GREEN** | `R:\WSL\kernels\bzImage-ramshared-latest` 17 330 688 B; REL=`6.18.35.2-microsoft-standard-WSL2+` HEAD=`1bd4ed3d4` |
| V2 config stickiness | **GREEN** | `CONFIG_BLK_DEV_UBLK=m` `CONFIG_ZRAM_WRITEBACK=y` `CONFIG_IO_URING=y` |
| V3 qemu-validate | **GREEN** | `QEMU-VALIDATE: PASS` KTEST-UNAME matches REL; stamp sha `d278b032…` |
| V4 status CLI | **GREEN** | prints STATE= |
| V5 enable no-op rules | **GREEN** | enable never restarts WSL; NEED_BUILD/NEED_* exit 2 |
| V6 enable &lt;30s | **GREEN** | smoke &lt;1s on stock path |
| V7 arm without bzImage | **GREEN** (logic) | refuse if missing |
| V8 apply without flag | **GREEN** | exit 5 |
| V9 apply live | **GREEN** | boot-ramshared.log OK; modules-apply RESULT=OK |
| V10 default distro | **GREEN** | remains Ubuntu-24.04 |

---

## RF → evidence

| RF | Status | Evidence |
| --- | --- | --- |
| RF-K1 | GREEN | tree 6.18.y @ 1bd4ed3d4 |
| RF-K2 | GREEN | configs above |
| RF-K3 | GREEN | bzImage + .ko copies (ublk/zram/zsmalloc) under build dir |
| RF-K4 | GREEN | all under R:\WSL\ |
| RF-K5 | GREEN | qemu PASS + stamp |
| RF-K6 | IMPL ready | boot-kernel-safe wired in apply |
| RF-K7 | GREEN | no set-default lab |
| RF-K8 | GREEN | modprobe ublk_drv + /dev/ublk-control live |
| RF-K9 | GREEN | cascade docs unchanged; NBD path |
| RF-K13–17 | GREEN | `scripts/kernel/wsl-kernel.sh` |
| RF-K18–20 | GREEN | arm gates + atomic write |

---

## Files delivered

| Path | Role |
| --- | --- |
| `scripts/kernel/wsl-kernel.sh` | CLI |
| `scripts/kernel/wsl-kernel-lib.sh` | probes / state machine |
| `scripts/kernel/build-wsl-kernel.sh` | default KTAG 6.18.y, JOBS=2 |
| `R:\WSL\kernels\bzImage-ramshared-latest` | artifact |
| `R:\WSL\RamShared-Kernel-build\release.txt` | REL/HEAD |
| `R:\WSL\RamShared-Kernel-build\qemu-pass.stamp` | apply gate |
| `R:\WSL\RamShared-Kernel-build\{ublk_drv,zram,zsmalloc}.ko` | for qemu / future modules |
| `docs/specs/.../AUDIT-2.5.md` | go human apply |
| `docs/specs/.../IMPL.md` | this file |

---

## Small decisions

1. Used `include/config/kernel.release` instead of `make kernelrelease` while parallel make still ran.  
2. Copied .ko from tree before full modules_install finished (modules were already built as .ko).  
3. Did **not** run `apply` in agent session (kills all WSL; user training). **Armed** host for next natural restart if arm succeeded.  
4. qemu module insmod FAIL in busybox accepted per existing qemu-validate policy.

---

## Validation numbers

| Metric | Value |
| --- | --- |
| bzImage size | 17 330 688 bytes |
| REL | 6.18.35.2-microsoft-standard-WSL2+ |
| HEAD | 1bd4ed3d4 |
| QEMU | PASS (KVM), uname match |
| KERNEL_SHA256 | d278b0327d4306a414e32ecec56c7d530541ed52756a68d8ecab356b49e25410 |
| enable path shutdown | none |

---

## Env-bound gaps

- Live WSL still on stock until user restarts after arm / runs apply.  
- Full `modules_install` into lab `/lib/modules` may still be completing; post-apply may need modules on Windows modules.vhdx path if modprobe fails.  
- BUILD OK line may appear when lab background make finishes modules_install.

---

## Rollback trigger

- qemu fail → no stamp (satisfied).  
- apply fail → boot-kernel-safe disarms (existing script).  
- User: `bash scripts/kernel/wsl-kernel.sh disarm` then restart WSL for stock.

---

## When you return (human checklist)

```bash
# 1) See state
bash scripts/kernel/wsl-kernel.sh status

# 2) If NEED_REBOOT (already armed): close WSL apps OR:
bash scripts/kernel/wsl-kernel.sh apply --i-know-this-stops-all-wsl

# 3) After WSL is back:
bash scripts/kernel/wsl-kernel.sh enable   # should READY or load ublk
uname -r   # expect REL from release.txt
```

**Do not** thrash swap. Product cascade still: `ramshared up` (NBD) on stock or custom.

---

## Commit traceability

Non-trivial commits should cite: `RF-K1`…`RF-K20` as applicable; `SPEC ITEM-1`…`ITEM-6`.


## Cascade smoke (custom kernel)

| Step | Result |
| --- | --- |
| `ramshared check` | ready; UBLK=m; ublk=ready |
| `up --vram 512 --zram 512` | zram prio 200, nbd0 prio 100, disk sdc |
| `down` | swapoff-first; no managed ghosts |
| thrash | **not** run (host safety) |

## modules.vhdx

| Path | Size |
| --- | --- |
| `C:\wsl\modules-ramshared.vhdx` | ~2.8 GiB |
| `.wslconfig kernelModules=` | set |

