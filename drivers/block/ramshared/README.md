# RamShared Linux Kernel Block Driver (`drivers/block/ramshared`)

Native in-tree Linux block driver designed for hardware-accelerated, ultra-low latency memory tiering and swap paging over discrete GPU video memory (VRAM) apertures via PCIe DMA.

## Architecture

1. **`blk-mq` Multi-Queue Dispatch:**
   - Atomic `gendisk` allocation via `blk_mq_alloc_disk()`.
   - Hardware multi-queue request processing without global queue lock contention.
   - Tag set configuration with hardware-enforced request sizing.

2. **Synchronous `.rw_page` Fast-Path:**
   - Implements direct `.rw_page` execution in `block_device_operations`.
   - Allows Linux kernel page-reclaim and swap subsystem to read and write anonymous 4KB pages directly to/from GPU VRAM MMIO aperture with zero intermediate buffer copies.

3. **PCIe Aperture Mapping & DMA:**
   - Maps GPU VRAM physical BAR0 address space directly into kernel virtual address space via `pci_iomap_wc()`.
   - Write-combining memory barriers (`wmb()`) to enforce store ordering over PCIe bus transactions.

4. **Checked Arithmetic & Boundary Hardening:**
   - Safe 64-bit capacity calculation via `check_mul_overflow()` to eliminate integer overflow during device initialization.
   - Bio and request bounds checking: verifies sector start and total byte length against physical PCIe aperture bounds, returning semantic `-ERANGE` on violations.
   - BAR0 `PAGE_SIZE` alignment validation on device probe.
   - Queue depth parameter clamping: strictly bounds `queue_depth` within `[16..1024]` (default: 128) to prevent kernel allocation failures.
   - Clean probe unwinding: linear error cleanup with `pci_clear_master()` during device initialization failure and module teardown.

## Kernel Configuration

To build RamShared as a Linux kernel module:

```kconfig
CONFIG_BLK_DEV_RAMSHARED=m
```

Associated subsystem requirements:
```kconfig
CONFIG_BLOCK=y
CONFIG_PCI=y
CONFIG_SWAP=y
CONFIG_IO_URING=y
```

## Module Parameters

| Parameter | Type | Default | Valid Range | Description |
| :--- | :---: | :---: | :---: | :--- |
| `queue_depth` | `uint` | `128` | `16..1024` | Maximum hardware request queue depth per blk-mq queue |
| `max_sectors` | `uint` | `256` | `8..2048` | Maximum 512-byte sectors per bio transfer (default: 128 KiB) |

## Upstream & Fork Status

- **LKML RFC v2:** Dispatched to Jens Axboe and the `linux-block` mailing list (`RFC v2 / artifacts/lkml-patchset/`).
- **WSL2 Kernel Fork:** Synchronized on [`emersonbusson/WSL2-Linux-Kernel`](https://github.com/emersonbusson/WSL2-Linux-Kernel) on branch `feature/ramshared-driver-6.18` (commit `5b95fb1cf`).
- **Canonical Kernel Documentation:** Synchronized with `Documentation/block/ramshared.rst` in the kernel tree.
- **Code Quality Gates:** Validated with 0 errors, 0 warnings, 0 checks on `checkpatch.pl --strict`.

## Relationship to WSL2 Userspace Engine

In live WSL2 production environments, memory tiering is primarily driven through userspace block devices (`/usr/local/bin/ramshared` and `ramsharedd`) via the native kernel `ublk` (`io_uring`) driver interface. The in-tree driver (`drivers/block/ramshared`) represents the bare-metal kernel module path intended for upstream Linux distribution and monolithic kernel deployments.
