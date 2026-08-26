.. SPDX-License-Identifier: GPL-2.0

=======================================================
RamShared: In-Kernel VRAM-Accelerated Block Driver
=======================================================

:Author: Emerson Busson 
:Date: August 2026

Overview
========

RamShared is an in-kernel block device driver that exposes idle GPU Video RAM
(VRAM) over the PCIe bus as a high-throughput, low-latency block storage device
(``/dev/ramshared<N>``).

The primary purpose of RamShared is to act as an accelerated swap and memory
tier, absorbing extreme memory pressure spikes and eliminating I/O stalls
caused by traditional disk and virtualized VHDX swapping.

Key Architectural Characteristics
=================================

1. **PCIe Aperture & Write-Combining Mapping:**
   Directly maps GPU PCIe BAR memory regions using ``pci_iomap_wc`` to
   enable multi-gigabyte-per-second sequential and random I/O throughput.

2. **Native Multi-Queue (blk-mq):**
   Full integration with the Linux kernel ``blk-mq`` architecture, allocating
   dedicated hardware queues per online CPU core to eliminate lock contention
   during heavy memory pressure.

3. **Dual-Tier Write-Through Cache Invariant:**
   All acknowledged write requests can be simultaneously persisted to an
   authoritative disk origin while being staged in VRAM. If the GPU context is
   reclaimed or revoked by the display server (e.g., Wayland, X11, or Windows WDDM),
   read operations seamlessly fallback to the authoritative origin with zero data loss.

4. **Detailed Latency and Hit Accounting:**
   Performance metrics, cache hit/miss ratios, and PCIe transfer throughput are
   exposed via sysfs under ``/sys/block/ramshared<N>/ramshared/``.

Module Parameters
=================

- ``logical_capacity_mib`` (ulong):
  Defines the logical size of the created block device in MiB.
  Default: ``4096`` (4 GiB).

Sysfs Interface
===============

The driver creates the following sysfs attributes under
``/sys/block/ramshared<N>/ramshared/``:

====================  ======================================================
File                  Description
====================  ======================================================
``tier_state``        Current operational state (ONLINE, SSD_ONLY, OFFLINE)
``vram_hits``         Total number of read requests served from VRAM
``vram_misses``       Total number of read requests missed/zero-filled
``vram_read_bytes``   Total bytes read over the PCIe VRAM aperture
``vram_write_bytes``  Total bytes written over the PCIe VRAM aperture
====================  ======================================================

Usage Example
=============

1. Load the module with 4 GiB logical capacity:

   .. code-block:: sh

      modprobe ramshared logical_capacity_mib=4096

2. Format and enable as high-priority swap tier:

   .. code-block:: sh

      mkswap /dev/ramshared0
      swapon -p 100 /dev/ramshared0

3. Monitor real-time VRAM throughput and hit counters:

   .. code-block:: sh

      cat /sys/block/ramshared0/ramshared/vram_hits
      cat /sys/block/ramshared0/ramshared/tier_state
