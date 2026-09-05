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
| 2026-09-05 | Kernel Hardening | Consolidated checked 64-bit capacity multiplication (`check_mul_overflow`), `PAGE_SIZE` PCIe BAR0 alignment, [16, 1024] queue depth clamping, bio sector bounds checks, and semantic errnos (`-ERANGE`, `-EBUSY`) across 162 Jules PR audit | PRs #887–#922 / `drivers/block/ramshared/` |

