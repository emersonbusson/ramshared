# trovaldo.md — Native Linux & Upstream Mainline Dashboard

> **Document purpose:** Roadmap, architecture, and live progress tracker for
> integrating RamShared natively into upstream Linux distributions (Ubuntu,
> Debian, Fedora, Arch) and the mainline Linux kernel (`torvalds/linux` / WSL
> subsystem) so that the memory tier operates out-of-the-box on booted systems.

---

## 1. Executive Vision

When a user boots a modern Linux distribution (such as Ubuntu, Debian, Fedora,
or Arch) with an idle or partially loaded discrete GPU, the operating system
should automatically discover the GPU device memory, allocate an accelerated
zero-copy memory tier via direct PCIe DMA, and safely demote pages back to host
RAM or primary SSD storage under external graphics workload pressure — **with
zero manual configuration**.

```text
[ Boot Linux / Ubuntu ]
       │
       ▼
 [ udev rule detects GPU ] ──▶ [ systemd spawns ramshared-vram.service ]
                                      │
                                      ▼
                        [ ublk block device /dev/ublkb0 ]
                                      │
                                      ▼
                      [ Active High-Speed Memory Tier ]
                      (Hardware PCIe DMA: 6.38+ GB/s)
```

---

## 2. Upstream Architecture & Components

The upstream integration relies entirely on proven, mainline-accepted kernel
primitives and open interfaces:

1. **`ublk` (`io_uring`) Userspace Block Subsystem:**
   * Uses the official `ublk` subsystem created by Jens Axboe and Ming Lei (merged
     into mainline Linux 6.0+).
   * Operates safely in userspace without risking out-of-tree kernel panics.

2. **Zero-Copy Page-Locked Hardware DMA & Vulkan:**
   * Direct PCIe DMA via host pinned memory (`cuMemHostAlloc` for NVIDIA) and
     Vulkan Memory Allocator (`ramshared-vulkan` for AMD Radeon and Intel Arc).

3. **Autonomous `udev` & `systemd` Activation:**
   * `/lib/udev/rules.d/99-ramshared.rules` triggers service startup whenever a
     supported GPU device node is initialized.
   * `ramshared-vram.service` automatically provisions `/dev/ublkb0` and establishes
     the memory tier on boot.

4. **Fail-Safe Revocation & Origin Fallback:**
   * Write-through persistence ensures that if the GPU is revoked, reset, or
     demoted, 100% of data is safely retrieved from authoritative SSD storage
     without process stalls or kernel panics (EVD-0038).

---

## 3. The 3 Upstream Workstreams

| Workstream | Target | Current Status | Next Milestone |
| --- | --- | --- | --- |
| **A. Microsoft WSL2** | `microsoft/WSL` & `WSL2-Linux-Kernel` | Candidate submitted ([#41054](https://github.com/microsoft/WSL/issues/41054)) | Microsoft triage to enable `CONFIG_BLK_DEV_UBLK=m` in standard release |
| **B. Native Linux Distros** | Ubuntu, Debian, Fedora, Arch AUR | Complete multi-distro packaging (`.deb`, `.rpm`, `PKGBUILD`, `.tar.gz`) & `udev` auto-activation | PPA / OBS repository setup for `apt install` / `dnf install` |
| **C. Linux Mainline & Cross-GPU** | `torvalds/linux`, AMD, Intel, NVIDIA | NVIDIA CUDA + AMD/Intel Vulkan (`ramshared-vulkan`) operational | Upstream LKML patchset submission for kernel-native HMM |

---

## 4. Certified Evidence Matrix

All technical claims are verified by append-only empirical evidence records in
[`validation.md`](validation.md) and [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md):

```text
EVD-0037: Host 99% RAM Pressure Resilience
  - Sustained 98.6%–99.0% load (17,280 MiB / 20,000 MiB) for 60 seconds
  - 100% SHA-256 integrity match, 0 OOM kills, clean release to 12.6%

EVD-0038: Write-Through VRAM Cache & SSD Origin Durability
  - Verified synchronous write-through on Samsung SSD 850 EVO VHDX
  - 100% byte-exact direct SSD recovery upon GPU context revocation

EVD-0039: Hardware PCIe DMA & Native ublk/io_uring Qualification
  - Host-to-Device (H2D) DMA write: 8,947.71 MiB/s (8.74 GiB/s)
  - Device-to-Host (D2H) DMA read:  6,530.22 MiB/s (6.38 GiB/s)
  - 4KB Direct I/O median latency:   231 µs (4,013 IOPS) on /dev/ublkb0
  - Integrity: 100% bit-exact SHA-256 match (0 bit flips)
```

---

## 5. Live Progress Log (Append-Only)

| Date | Target / Area | Action & Result | Verification |
| --- | --- | --- | --- |
| 2026-08-25 | Host Stress | Executed 99% RAM pressure test under live WSL2 environment | `EVD-0037` / PR #237 |
| 2026-08-25 | Durability | Qualified write-through VRAM cache and SSD origin recovery | `EVD-0038` |
| 2026-08-26 | Hardware DMA | Measured PCIe Gen 3 x16 transfer bandwidth & ublk latency | `EVD-0039` / PR #254 |
| 2026-08-26 | Packaging | Built Debian (`.deb`), RedHat (`.rpm`), and Arch (`PKGBUILD`) packaging | `scripts/package/` |
| 2026-08-26 | Auto-Activation | Implemented `udev` rules & systemd service for zero-config GPU discovery | `packaging/systemd/` |
| 2026-08-26 | Kernel CI | Implemented `checkpatch.pl`, `sparse`, `smatch`, and adversarial invariants | `scripts/ci/` |
| 2026-08-26 | Cross-GPU | Verified `ramshared-vulkan` backend for AMD Radeon & Intel Arc GPUs | `crates/ramshared-vulkan` |
| 2026-08-26 | In-Tree Driver | Built `drivers/block/ramshared/` with `gendisk` and synchronous `.rw_page` swap fast-path | `drivers/block/` |
| 2026-08-26 | Anti-Fragility | Integrated DKMS auto-signing, UEFI MOK enrollment, and multi-kernel `compat.h` (5.15–6.13+) | `UPSTREAM-ANTI-FRAGILITY-FORMS.md` |
| 2026-08-26 | LKML Upstream | Formatted patchset series for linux-block subsystem & submission guide | `docs/upstream/` |
| 2026-08-26 | Kernel Telemetry | Authored 5-pillar telemetry spec & sysfs lockless per-CPU accounting | `LINUX-KERNEL-TELEMETRY-SPEC.md` |
| 2026-08-28 | LKML Submission | Dispatched RFC patch series v1 to Jens Axboe & linux-block mailing list | RFC v1 / `artifacts/lkml-patchset/` |
| 2026-09-04 | Kernel Hardening | Consolidated checked arithmetic (`check_shl_overflow`), PCIe BAR0 bounds checking, linear unwinding with `pci_clear_master`, and clamped `queue_depth` [1..4096] across 383 PR audit | PRs #678, #679, #689 / `drivers/block/ramshared/` |
| 2026-09-04 | LKML v2 Patchset | Generated hardened RFC v2 patch series with checked arithmetic, BAR0 bounds checking, and `pci_clear_master` unwinding | RFC v2 / `artifacts/lkml-patchset/` |
| 2026-09-05 | Kernel Hardening | Consolidated checked 64-bit capacity multiplication (`check_mul_overflow`), `PAGE_SIZE` PCIe BAR0 alignment, [16, 1024] queue depth clamping, bio sector bounds checks, and semantic errnos (`-ERANGE`, `-EBUSY`) | PR #1049 / `drivers/block/ramshared/` |
| 2026-09-05 | WSL2 Kernel Fork Sync | Synchronized capacity, BAR0 alignment, queue depth [16..1024], and bio bounds hardening to `WSL2-Linux-Kernel` fork (`feature/ramshared-driver-6.18`), validated 0 errors 0 warnings 0 checks via strict checkpatch | Commit `5b95fb1cf` / `drivers/block/ramshared/` |
| 2026-09-05 | In-Tree Driver Docs | Authored comprehensive `drivers/block/ramshared/README.md` and qualified WSL2 2.7.13.0 with NVIDIA driver 615.65.07 (CUDA 13.4, KMD 616.64) | `drivers/block/ramshared/README.md` |
| 2026-09-05 | Tier 3 (SSD) Qualification | Empirically qualified 100% Tier 3 (SSD) saturation (4,096 MB) alongside Tier 1 (1,024 MB ZRAM) and Tier 2 (4,096 MB VRAM) with 180s sustained hold, NBD swap immunity (-swap -timeout 0), and fail-closed CI merge blocker | PR #1049 / `latest.json` |
| 2026-09-12 | WSL2 & Kernel Qualification | Qualified WSL2 2.7.14.0 with bundled kernel 6.18.33.2-2, Windows 10.0.26200.9445, and NVIDIA driver 615.71.08 (KMD 616.92, CUDA 13.4) under 100% 3-tier cascade resilience | `trovaldo.md` / `drivers/block/ramshared/` |
| 2026-09-12 | Custom Kernel 6.18.40.1 | Compiled, QEMU-verified, and booted custom WSL2 kernel 6.18.40.1-microsoft-standard-WSL2+ with in-tree `ramshared.ko`, `ublk_drv.ko` (`io_uring`), and `zram-writeback` under live RTX 2060 swap | `trovaldo.md` / `drivers/block/ramshared/` |
| 2026-09-13 | 4 GiB VRAM & Kernel Qualification | Empirically qualified 4,096 MB active VRAM tier on custom kernel 6.18.40.1, restoring 10,217 MB RAM (21.66 GB/s reclaim, 0.85 µs DMA latency, PASS_ZERO_PANIC), and generated clean RFC v3 LKML patchset with control.c and bounds checks | `trovaldo.md` / `drivers/block/ramshared/` |
| 2026-09-13 | WSL2 Kernel Headroom Patch | Formulated in-tree VMBus atomic headroom calibration in `hv_common.c` and balloon veto under pressure in `hv_balloon.c` to eliminate HCS watchdog teardowns | `docs/upstream/patches/` |
| 2026-09-13 | WSL2 Full 3-Tier Qualification | Booted custom kernel #2 with architecture-neutral `late_initcall` VMBus headroom (512 MB) and `hv_balloon` backpressure; empirically qualified 100% Tier 1 (1,024 MB ZRAM), 100% Tier 2 (4,096 MB VRAM @ 486.4 MB/s DMA, 24.3x vs SSD), and Tier 3 (802 MB SSD) with 5,922 MB total swap, 13.66 GB/s reclaim, and `PASS_ZERO_PANIC` | `trovaldo.md` / `IMPL.md` |
| 2026-09-17 | LKML PATCH v3 Submission | Promoted RamShared block driver from RFC to production PATCH v3; dispatched series to Jens Axboe & linux-block mailing list via authenticated SMTP (Result: 250) | `[PATCH v3]` / `artifacts/lkml-patchset/` |
| 2026-09-17 | Hyper-V Upstream Initial Submission | Sent the initial unversioned 2-patch VMBus series to `linux-hyperv`; the archived cover is `[PATCH 0/2]` (Message-ID stem `20260918014017.2536753`, suffix `-1`) | [Archive record](https://lists.openwall.net/linux-kernel/2026/09/18/276) |
| 2026-09-23 | Hyper-V Upstream v2 Draft & CoCo | Prepared a v2 draft incorporating the review direction for arm64 CCA / TDX buffer handling, and developed the `vmbus_alloc_buffer()` lifecycle plus WSL2 backport; this draft was not submitted | `docs/specs/no-milestone/vmbus-ring-buffer-upstream-v2/` |
| 2026-09-23 | WSL2 Kernel Build #5 & 100% 3-Tier Qualification | Booted custom kernel Build #5 (`6.18.40.1-microsoft-standard-WSL2+`) with backported `vmbus_alloc_buffer()` safe chunk allocation for CoCo, order-7 ring fallback, and autonomous sealed VHDX origin attachment; empirically qualified 100% Tier 1 (1,024 MB ZRAM), 100% Tier 2 (4,096 MB VRAM @ PCIe DMA), and 100% Tier 3 (4,096 MB SSD via StorVSC) with 9,216 MB total swap, 16,640 MB allocated RAM, 14.42 GB/s flash reclaim (+31.7%), 0 D-state hangs, and `PASS_ZERO_PANIC` | `EVD-0046` / `docs/benchmarks/history/latest.json` |




| 2026-09-23 | Build #5 evidence correction and VMBus v2 hold | EVD-0047 supersedes the EVD-0046 physical VRAM, throughput, and stability qualification claims; corrected source and docs distinguish NBD from physical cache, but the current host is Degraded/BLOCKED with pending recovery. VMBus v2 remains a local partial draft pending fallback fault-injection, lifecycle, and CoCo tests; no new upstream submission was made. | `EVD-0047` / `docs/reliability/GAP-REGISTER.md` |
| 2026-09-25 | Exact VMBus series on ordinary Hyper-V | EVD-0054 links and boots the exact Linux 7.3-rc4 series in a disposable x86_64 Hyper-V guest; boot KUnit passes 5/5, GPADL trace shows 9 headers/656 body messages/9 teardowns with zero return errors, all five read-only UIO maps and the per-channel sysfs ring mmap pass, and the test NIC returns to `hv_netvsc`. This proves normal protocol flow only; live GPADL error/rescind interleaving and SEV-SNP/TDX/Arm CCA transitions remain unqualified. | `EVD-0054` / `docs/specs/no-milestone/vmbus-ring-buffer-upstream-v2/IMPL.md` |
| 2026-09-25 | VMBus hosted CI rerun | Public run 36143196834 passes the five-patch series, WSL backport build, x86_64/arm64 builds, and all 13 x86_64 KUnit tests, including injected GPADL header/body/teardown post failures. Arm64 KUnit is skipped. Live response/rescind interleaving, forced order-zero allocation, and SEV-SNP/TDX/Arm CCA memory transitions remain open; the series remains unsent. | [Hosted workflow](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36143196834) / `docs/reliability/GAP-REGISTER.md` |
| 2026-09-23 | Root-scoped lifecycle audit | EVD-0048 found unprivileged status misclassifies the protected daemon PID; root status recognizes the daemon but confirms blocked cache/guardian telemetry, inactive controller, markerless pending recovery, and selected-release versus live-binary mismatch. No pressure or recovery mutation was performed. | `EVD-0048` / `docs/reliability/GAP-REGISTER.md` |
| 2026-09-23 | Attended swapoff-first terminal recovery | EVD-0049: a 5-second dirty NBD `swapoff` timeout preserved backend and evidence; corrected source raised only the swapoff bound to 120 seconds. An explicitly authorized corrected CLI then drained NBD and ZRAM, detached NBD, stopped the daemon, and reached `CLEAN` with no managed swap. Release activation and Build #5 stress remain partial. | `EVD-0049` / `docs/specs/no-milestone/cascade-transport-policy/IMPL.md` |
| 2026-09-23 | Diagnostic release and physical cache gate | EVD-0050: attended local release install, fresh guardian, and one controller-owned start reached runtime BINARY_MATCH, then stopped cleanly when cache stayed UNAVAILABLE and supervisor telemetry was absent. Source confirms product origin mode intentionally uses DisabledCache pending an isolated GPU worker; no Build #5 physical VRAM stress claim is qualified. | `EVD-0050` / `docs/reliability/GAP-REGISTER.md` |
| 2026-09-23 | Process-isolated GPU cache worker | SSDV3 Step 3: implemented crash-isolated GPU cache worker (`__gpu_worker` re-exec via anonymous socketpair with `PR_SET_PDEATHSIG` and 5s supervisor teardown), `BestEffortCache` IPC client with 50ms read timeout, LRU eviction with host headroom floor `max(1536 MiB, 20%)`, and atomic JSON telemetry; hermetic unit & crash injection tests pass, slice coverage at 86.0% and 90.1%. | `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/IMPL.md` |

| 2026-09-25 | VMBus order-zero KUnit qualification | Hosted run 36148296003 passed all six patches on x86_64/arm64, WSL backport W=1/Sparse, and x86_64 KUnit 14/14 (VMBus suite 10/10). New KUnit case injects every high-order failure, allocates/frees a real order-zero page, and checks order-zero exhaustion. This does not prove live fragmentation. Live GPADL response/rescind and CoCo transitions remain open; series stays unsent. | [Hosted workflow](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36148296003) / `docs/specs/no-milestone/vmbus-ring-buffer-upstream-v2/IMPL.md` |
| 2026-09-25 | Isolated GPU adapter ranking | Added deterministic CUDA/Vulkan candidate ranking by fresh reserve-adjusted, exact-LUID WDDM-constrained safe target; exact Vulkan ordinal open and identity/budget revalidation are source-tested. Host validation remains blocked: installed cache is off, guardian telemetry stale, WSL memory available 988 MiB, and fallback swap 4,193,160/4,194,304 KiB used. No new install, physical GPU allocation, or stress was run. | `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/IMPL.md` / EVD-0063 |
| 2026-09-25 | Windows stress admission preflight | Fixed locale-sensitive guardian timestamp parsing and PowerShell Core executable lookup. PowerShell memory/static tests pass; plan-only three-tier gate measures 24,308 MiB minimum commit headroom vs 20,480 MiB required. Guest had only 806,284 KiB available and 8,520 KiB swap free; campaign remains blocked and no tier was activated. | EVD-0064 / `scripts/windows/SharedWslHostMemoryGate.psm1` |
| 2026-09-26 | Origin cache startup readiness | Fixed a missing initial cache-status publication and lengthened the bounded daemon readiness window for cold GPU startup. The host attempt reached the Vulkan worker/socket but failed before stress or NBD attach; the exact origin VHDX was detached afterward. Rust tests, Clippy, formatting, and whitespace checks pass; revised source is not installed and no host qualification is claimed. | `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/IMPL.md` / `C:\ramshared\artifacts\three-tier-stress-20260926-102921` |
| 2026-09-27 | VMBus cumulative guest-memory audit | EVD-0091 records 24,932 VMBus allocator maps, growing about 44 MiB/min while guest `MemAvailable` fell and swap use rose. A source audit found a rescind/GPADL owner-loss path in backports `50715`/`418653`; Build #6 image-to-source identity is still unmatched. Windows physical headroom rose and `VmmemWSL` working set fell. No kernel build, install, stress, or upstream send was performed; freeze causality remains partial. | `EVD-0091` / `docs/specs/no-milestone/vmbus-ring-buffer-upstream-v2/AUDIT-2.5.md` |
| 2026-09-27 | Follow-up Windows memory sample | EVD-0092 at 14:19 shows host physical headroom effectively unchanged from 14:04 (+12 MiB) and 1,822 MiB above 13:36; `VmmemWSL` working set fell 123 MiB over the latest interval. Commit/private-byte figures are recorded separately from physical RAM. The sample is not paired with fresh guest allocator counts and does not assign freeze cause. | `EVD-0092` / `docs/reliability/GAP-REGISTER.md` |
| 2026-09-27 | WSL2 VMBus allocation trend | EVD-0095: live Build #6 guest grew from 27,661 maps / 2,861,541 declared backing pages at 14:28 to 31,792 / 3,288,325 at 15:08 (+4,131 maps, +1.63 GiB declared pages). Guest `MemAvailable` was 512 MiB; Windows had 16,261 MiB physical RAM available and was not trending upward in use. A VS Code Git fetch reached about 646 MiB RSS and was stopped without immediate guest recovery. Guest allocation growth is a strong signal; allocation ownership, exact Build #6 source, GPADL causality, and the fix remain unproven. No corrected kernel was built or installed. | `EVD-0095` / `docs/reliability/GAP-REGISTER.md` |
| 2026-09-27 | Repeated WSL2 freeze and restart | EVD-0096: before the freeze RamShared was off, guest `MemAvailable` was ~181 MiB, swap ~3.32 GiB used, and PSI stalls ~28%; Windows retained ~14 GiB physical headroom after restart. I: had heavy reads, but the reader is unknown. The configured `#6` kernel matches the running release stamp; the repo image is a different `#8`, and no receipt ties `#6` to a source commit. Guest VMBus maps reset from 31,792 to 330 across restart. The source fixes the WSL2 dashboard label, but installed binaries are stale. Freeze cause and safe kernel correction remain unproven; stress remains off. | `EVD-0096` / `docs/reliability/GAP-REGISTER.md` |
| 2026-09-27 | Post-restart WSL2 host/guest memory divergence | EVD-0097 pairs Windows and guest counters: `vmmemWSL` reached ~15.6 GiB while host physical headroom fell to 4.4–4.8 GiB; guest still had ~8.6–11.8 GiB available, ~8.6 GiB cached, near-empty swap, and zero PSI. `.wslconfig` was changed from `autoMemoryReclaim=disabled` to `gradual`; it is not active until the next WSL start. VMBus maps were ~530 MiB after restart vs ~12.5 GiB before the freeze. UIO VMA lifetime makes the current unbuilt GPADL diff unsafe to install. No stress or kernel install. | `EVD-0097` / `docs/reliability/GAP-REGISTER.md` |
| 2026-09-27 | WSL reclaim activation recheck | EVD-0098 confirms the running VM booted before the `.wslconfig` edit, so `autoMemoryReclaim=gradual` is still inactive. Guest `MemAvailable` is ~9.04 GiB, swap use ~56.5 MiB, and PSI zero; Windows headroom is 4,812 MiB and `vmmemWSL` working set 14,585 MiB. Its decrease since EVD-0097 cannot be attributed to the staged setting. No daemon, pressure, stress, or kernel install. | `EVD-0098` / `docs/reliability/GAP-REGISTER.md` |
