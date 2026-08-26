/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _RAMSHARED_H
#define _RAMSHARED_H

#include <linux/types.h>

#define RAMSHARED_DRIVER_NAME		"ramshared"
#define RAMSHARED_DRIVER_VERSION	"0.9.0-beta.2"
#define RAMSHARED_SECTOR_SIZE		512
#define RAMSHARED_SECTOR_SHIFT		9
#define RAMSHARED_DEFAULT_QUEUE_DEPTH	128

/**
 * struct ramshared_dma_region - PCIe DMA memory descriptor
 * @pci_addr: Physical PCIe bus address
 * @cpu_addr: Kernel virtual address (if mapped)
 * @size: Size of the allocated region in bytes
 * @dma_handle: DMA bus address mapping
 */
struct ramshared_dma_region {
	phys_addr_t	pci_addr;
	void __iomem	*cpu_addr;
	size_t		size;
	dma_addr_t	dma_handle;
};

/**
 * struct ramshared_device - Main in-tree block device state
 * @disk: gendisk descriptor
 * @tag_set: blk-mq tag set
 * @dma: DMA memory region
 * @capacity_bytes: Total available VRAM capacity in bytes
 * @dev: Pointer to underlying struct device
 * @lock: Mutex protecting device state transitions
 */
struct ramshared_device {
	struct gendisk			*disk;
	struct blk_mq_tag_set		tag_set;
	struct ramshared_dma_region	dma;
	u64				capacity_bytes;
	struct device			*dev;
	struct mutex			lock;
};

int ramshared_dma_init(struct ramshared_device *rs_dev, size_t size);
void ramshared_dma_cleanup(struct ramshared_device *rs_dev);
int ramshared_queue_init(struct ramshared_device *rs_dev);
void ramshared_queue_cleanup(struct ramshared_device *rs_dev);

#endif /* _RAMSHARED_H */
