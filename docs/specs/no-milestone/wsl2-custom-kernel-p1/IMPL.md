# IMPL — wsl2-custom-kernel-p1

> **Passo 3 SSDV3.** Implements [`SPEC.md`](SPEC.md).  
> **Date:** 2026-07-10  
> **Status:** **HISTORICAL CAPABILITY GREEN / PRODUCT UBLK DEFERRED** — build,
> qemu, modules.vhdx, live kernel, ublk capability, cascade NBD smoke, and CLI
> were green for the recorded 2026-07-10 lab state. This does **not** close
> current product ublk transport readiness. Product cascade transport policy is
> closed in `cascade-transport-policy` as NBD Day-1; ublk remains deferred to
> `custom-kernel-ublk-product-transport`.

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

- The recorded GREEN state is historical and must be revalidated before use.
- Current product transport remains NBD Day-1.
- ublk product transport requires the dedicated
  `custom-kernel-ublk-product-transport` lifecycle campaign.

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


## 2026-07-14 — NBD vs ublk product decision (issue #30)

**Context:** Daily host runs stock/inbox WSL kernel (`6.18.33.2-microsoft-standard-WSL2`); `/dev/ublk-control` **absent**. Product cascade policy already fails closed on ublk for WSL2 (freeze risk on teardown; transport=auto → nbd).

| Criterion | NBD (Day-1) | ublk (lab/custom kernel) |
| --- | --- | --- |
| Available now on daily WSL | **YES** | **NO** without custom kernel + modules VHDX |
| Host safety | Proven cascade path | Historical freeze class on WSL teardown |
| 15% latency win claim | Not re-measured this session | **Blocked** until custom kernel READY + non-daily lab |
| Product ship | **Ship NBD** | Optional Phase B if kernel earns keep |

**Acceptance re-scope (honest):**
1. ~~Compile custom kernel with CONFIG_BLK_DEV_UBLK~~ — capability existed on armed custom kernel earlier; **not** the running product kernel today.
2. Latency suite under pressure on daily host — **refuse** (benchmarks.md / thrash policy).
3. ublk ≥15% better than NBD — **OPEN** only in isolated lab (qemu already has ublk smoke PASS; not apples-to-apples swap latency).

**Recommendation:** keep product on **NBD**; close #30 as **wontfix on daily WSL / deferred to dedicated kernel lab** unless custom kernel is re-armed and measured off the daily host.
