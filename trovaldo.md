# trovaldo.md — Native Linux & Upstream Mainline Dashboard

> **Document purpose:** Roadmap, architecture, and live progress tracker for
> integrating RamShared natively into upstream Linux (Ubuntu, Debian, Fedora,
> Arch) and the mainline Linux kernel (`torvalds/linux` / WSL subsystem) so
> that the memory tier operates out-of-the-box on booted systems.

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

2. **Zero-Copy Page-Locked Hardware DMA:**
   * Utilizes host pinned memory allocations (`cuMemHostAlloc` / `dma-buf`) to
     achieve direct PCIe DMA bandwidth without intermediate buffer bounces.

3. **Autonomous `udev` & `systemd` Activation:**
   * `/lib/udev/rules.d/99-ramshared.rules` triggers service startup whenever a
     supported GPU device node is initialized.
   * `ramshared-vram-service.sh` automatically reads total VRAM capacity, reserves
     a dynamic floor for desktop/gaming display needs, and establishes the tier.

4. **Fail-Safe Revocation & Origin Fallback:**
   * Write-through persistence ensures that if the GPU is revoked, reset, or
     demoted, 100% of data is safely retrieved from authoritative SSD storage
     without process stalls or kernel panics.

---

## 3. The 3 Upstream Workstreams

| Workstream | Target | Current Status | Next Milestone |
| --- | --- | --- | --- |
| **A. Microsoft WSL2** | `microsoft/WSL` & `WSL2-Linux-Kernel` | Candidate submitted ([#41054](https://github.com/microsoft/WSL/issues/41054)) | Microsoft triage to enable `CONFIG_BLK_DEV_UBLK=m` in standard release |
| **B. Native Linux Distros** | Ubuntu, Debian, Fedora, Arch AUR | `.deb` builder and universal `.tar.gz` bundle operational | `.rpm` builder + Arch PKGBUILD + PPA repository publication |
| **C. Linux Mainline (LKML)** | `torvalds/linux` & open drivers | NVIDIA CUDA + ublk driver qualified (EVD-0039) | Vulkan Memory Allocator (VMA) & `dma-buf` cross-vendor backend (AMD/Intel) |

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
| 2026-08-26 | Packaging | Verified `.deb` generator and universal Linux release bundle | `scripts/package/` |
| 2026-08-26 | Governance | Established `trovaldo.md` upstream dashboard and roadmap | PR in progress |
