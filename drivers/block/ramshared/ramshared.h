/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _RAMSHARED_H
#define _RAMSHARED_H

#include <linux/types.h>
#include <linux/blkdev.h>
#include <linux/blk-mq.h>
#include <linux/pci.h>
#include <linux/mutex.h>
#include <linux/atomic.h>

#define RAMSHARED_DRIVER_NAME		"ramshared"
#define RAMSHARED_DRIVER_VERSION	"0.9.0-beta.2"
#define RAMSHARED_SECTOR_SIZE		512
#define RAMSHARED_SECTOR_SHIFT		9
#define RAMSHARED_DEFAULT_QUEUE_DEPTH	256
#define RAMSHARED_MMIO_POISON_VALUE	0xFFFFFFFFU

/**
 * struct ramshared_dma_region - PCIe DMA memory descriptor
 * @pci_addr: Physical PCIe bus address
 * @dma_handle: DMA bus address mapping
 * @cpu_addr: Kernel virtual address mapped with write-combining
 * @size: Size of the allocated region in bytes
 */
struct ramshared_dma_region {
	phys_addr_t	pci_addr;
	dma_addr_t	dma_handle;
	void __iomem	*cpu_addr;
	size_t		size;
};

/**
 * struct ramshared_device - Main in-tree block device state
 * @capacity_bytes: Total available VRAM capacity in bytes
 * @dma_transfers_total: Diagnostic counter for completed transfers
 * @read_bytes: Total bytes read
 * @write_bytes: Total bytes written
 * @disk: gendisk descriptor
 * @dev: Pointer to underlying struct device
 * @dma: DMA memory region
 * @tag_set: blk-mq tag set
 * @lock: Mutex protecting device state transitions
 */
struct ramshared_device {
	u64				capacity_bytes;
	atomic64_t			dma_transfers_total;
	atomic64_t			read_bytes;
	atomic64_t			write_bytes;
	struct gendisk			*disk;
	struct device			*dev;
	struct ramshared_dma_region	dma;
	struct blk_mq_tag_set		tag_set;
	struct mutex			lock;
};

int ramshared_dma_init(struct ramshared_device *rs_dev, struct pci_dev *pdev);
void ramshared_dma_cleanup(struct ramshared_device *rs_dev);
int ramshared_queue_init(struct ramshared_device *rs_dev, struct device *parent_dev, unsigned int q_depth);
void ramshared_queue_cleanup(struct ramshared_device *rs_dev);

#endif /* _RAMSHARED_H */
